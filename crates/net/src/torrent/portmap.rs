//! Forwarding the torrent port on the router, so peers on the internet can connect in, as other
//! torrent clients do: NAT-PMP first (RFC 6886), then UPnP's Internet Gateway Device, for TCP and
//! UDP, renewed before the lease ends and taken away when the session ends.
//!
//! librqbit has UPnP of its own, but on a computer with more than one network adapter (WSL's and
//! Hyper-V's virtual ones, a VPN) its search for the router can go out the wrong one and find
//! nothing. Here everything is sent from the address that holds the default route.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use downloadcraft_model::portmap::{
    NATPMP_PORT, Proto, igd_service, join_url, natpmp_answer, natpmp_request, ssdp_location, ssdp_search, upnp_add, upnp_delete, upnp_error,
};

/// How long a mapping is asked for; it's renewed at half that.
const LEASE: u32 = 3600;
/// How long to wait before trying again after nothing worked.
const RETRY: Duration = Duration::from_secs(5 * 60);

/// Where forwarding stands, for the status bar: `Ok(how)` once the router forwards the port.
pub type State = Arc<Mutex<Option<Result<String, String>>>>;

/// Keep `port` forwarded until `stop` is set, in the background.
pub fn start(port: u16, stop: Arc<AtomicBool>) -> State {
    let state: State = Arc::new(Mutex::new(None));
    let shared = state.clone();
    let spawned = std::thread::Builder::new().name("port-forwarding".into()).spawn(move || {
        let set = |v: Result<String, String>| *shared.lock().unwrap_or_else(PoisonError::into_inner) = Some(v);
        let mut held: Option<Mapping> = None;
        let mut next = Instant::now();
        while !stop.load(Ordering::Relaxed) {
            if Instant::now() >= next {
                match forward(port) {
                    Ok(m) => {
                        log::info!("torrents: port {port} forwarded by the router ({})", m.how());
                        set(Ok(m.how()));
                        next = Instant::now() + Duration::from_secs(u64::from(m.lifetime().max(120) / 2));
                        held = Some(m);
                    }
                    Err(why) => {
                        log::info!("torrents: port {port} not forwarded: {why}");
                        set(Err(why));
                        next = Instant::now() + RETRY;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if let Some(m) = held {
            m.release(port);
        }
    });
    if let Err(e) = spawned {
        log::warn!("couldn't start port forwarding: {e}");
    }
    state
}

enum Mapping {
    NatPmp { gateway: Ipv4Addr, lifetime: u32 },
    Upnp { control: String, service: String, lease: u32 },
}

impl Mapping {
    fn how(&self) -> String {
        match self {
            Mapping::NatPmp { .. } => "NAT-PMP".into(),
            Mapping::Upnp { .. } => "UPnP".into(),
        }
    }
    fn lifetime(&self) -> u32 {
        match self {
            Mapping::NatPmp { lifetime, .. } => *lifetime,
            Mapping::Upnp { lease: 0, .. } => LEASE,
            Mapping::Upnp { lease, .. } => *lease,
        }
    }
    /// Take the mapping away (best effort: it lapses by itself otherwise).
    fn release(&self, port: u16) {
        match self {
            Mapping::NatPmp { gateway, .. } => {
                for p in [Proto::Tcp, Proto::Udp] {
                    let _ = natpmp(*gateway, p, port, 0);
                }
            }
            Mapping::Upnp { control, service, .. } => {
                for p in [Proto::Tcp, Proto::Udp] {
                    let (body, action) = upnp_delete(service, p, port);
                    let _ = soap(control, &body, &action);
                }
            }
        }
    }
}

/// This computer's address on the network the default route leads to (nothing is sent).
fn local_ip() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    match s.local_addr().ok()? {
        SocketAddr::V4(a) if !a.ip().is_unspecified() => Some(*a.ip()),
        _ => None,
    }
}

/// Forward `port` (TCP and UDP) by whatever the router speaks.
fn forward(port: u16) -> Result<Mapping, String> {
    let local = local_ip().ok_or("this computer isn't on a network")?;
    let found = find_upnp(local);
    // NAT-PMP goes to the gateway: the UPnP router's address when one answered, else the usual
    // `.1` of this network.
    let gateway = found.as_ref().and_then(|(loc, _, _)| host_ip(loc)).unwrap_or_else(|| {
        let o = local.octets();
        Ipv4Addr::new(o[0], o[1], o[2], 1)
    });
    let mut why = Vec::new();
    match (natpmp(gateway, Proto::Tcp, port, LEASE), natpmp(gateway, Proto::Udp, port, LEASE)) {
        (Ok(life), _) => return Ok(Mapping::NatPmp { gateway, lifetime: life }),
        (Err(e), _) => why.push(format!("NAT-PMP: {e}")),
    }
    match found {
        Some((_, control, service)) => match upnp(&control, &service, port, local) {
            Ok(lease) => return Ok(Mapping::Upnp { control, service, lease }),
            Err(e) => why.push(format!("UPnP: {e}")),
        },
        None => why.push("UPnP: no router answered".into()),
    }
    Err(why.join("; "))
}

fn host_ip(url: &str) -> Option<Ipv4Addr> {
    url.strip_prefix("http://")?.split(['/', ':']).next()?.parse().ok()
}

/// One NAT-PMP mapping, tried four times (250 ms, then twice as long each time): its lifetime.
fn natpmp(gateway: Ipv4Addr, proto: Proto, port: u16, lifetime: u32) -> Result<u32, String> {
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    s.connect(SocketAddrV4::new(gateway, NATPMP_PORT)).map_err(|e| e.to_string())?;
    let req = natpmp_request(proto, port, lifetime);
    let mut wait = Duration::from_millis(250);
    let mut buf = [0u8; 64];
    for _ in 0..4 {
        s.send(&req).map_err(|e| e.to_string())?;
        s.set_read_timeout(Some(wait)).map_err(|e| e.to_string())?;
        match s.recv(&mut buf) {
            Ok(n) => return natpmp_answer(buf.get(..n).unwrap_or_default(), proto).map(|(_, life)| life),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => wait *= 2,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("the router didn't answer".into())
}

/// The UPnP router on this network: its description's address, control address and service.
fn find_upnp(local: Ipv4Addr) -> Option<(String, String, String)> {
    let sock = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, Some(socket2::Protocol::UDP)).ok()?;
    sock.set_multicast_if_v4(&local).ok()?;
    sock.bind(&SocketAddrV4::new(local, 0).into()).ok()?;
    let s: UdpSocket = sock.into();
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    s.send_to(ssdp_search(), "239.255.255.250:1900").ok()?;
    let mut buf = [0u8; 2048];
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(3) {
        let Ok((n, _)) = s.recv_from(&mut buf) else { break };
        let Some(location) = ssdp_location(buf.get(..n).unwrap_or_default()) else { continue };
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).build().into();
        let Ok(mut resp) = agent.get(&location).call() else { continue };
        let Ok(desc) = resp.body_mut().with_config().limit(256 * 1024).read_to_string() else { continue };
        if let Some((service, control)) = igd_service(&desc) {
            return Some((location.clone(), join_url(&location, &control), service));
        }
    }
    None
}

/// Forward `port` over UPnP, TCP and UDP: the lease the router took (0: until taken away).
fn upnp(control: &str, service: &str, port: u16, local: Ipv4Addr) -> Result<u32, String> {
    let mut lease = LEASE;
    for proto in [Proto::Tcp, Proto::Udp] {
        let (body, action) = upnp_add(service, proto, port, &local.to_string(), lease);
        match soap(control, &body, &action) {
            Ok(()) => {}
            // Only mappings without a lease: ask for one of those.
            Err(Some(725)) if lease != 0 => {
                lease = 0;
                let (body, action) = upnp_add(service, proto, port, &local.to_string(), 0);
                soap(control, &body, &action).map_err(|c| format!("the router refused ({})", c.unwrap_or(0)))?;
            }
            Err(code) => return Err(code.map_or_else(|| "the router didn't answer".into(), |c| format!("the router refused (error {c})"))),
        }
    }
    Ok(lease)
}

/// Send one SOAP request; `Err(code)` with the router's UPnP error code, if it gave one.
fn soap(control: &str, body: &str, action: &str) -> Result<(), Option<u32>> {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).http_status_as_error(false).build().into();
    let mut resp =
        agent.post(control).header("Content-Type", "text/xml; charset=\"utf-8\"").header("SOAPAction", action).send(body).map_err(|_| None)?;
    if resp.status().is_success() {
        return Ok(());
    }
    let text = resp.body_mut().with_config().limit(64 * 1024).read_to_string().unwrap_or_default();
    Err(upnp_error(&text))
}
