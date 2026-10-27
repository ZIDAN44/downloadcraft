//! The librqbit session every torrent shares, started from the torrent settings.
//!
//! Speed limits change at once. The rest (port, protocol, UPnP, peers per torrent, proxy, IP
//! filter) need a new session: one is started for the next torrent once none is running, and
//! torrents still running keep the session they began on until they end.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use downloadcraft_model::torrent::{EngineStatus, Protocol, TorrentPrefs, ip_filter_to_p2p, proxy_url};
use librqbit::limits::LimitsConfig;
use librqbit::{ConnectionOptions, ListenerMode, ListenerOptions, Session, SessionOptions};

use crate::NetError;

pub(super) struct Engine {
    pub rt: tokio::runtime::Runtime,
    pub session: Arc<Session>,
    /// The settings it was started with.
    started: TorrentPrefs,
    /// Ranges in the IP filter it loaded, when it loaded one.
    filtered: Option<usize>,
    /// The router's forwarding of the port, kept up while the session lives.
    forwarding: Option<(super::portmap::State, Arc<AtomicBool>)>,
}

impl Drop for Engine {
    /// The forwarding stops (and is taken away) with the session.
    fn drop(&mut self) {
        if let Some((_, stop)) = &self.forwarding {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

impl Engine {
    /// Whether peers are reached through a proxy (so nothing else may go around it).
    pub fn proxied(&self) -> bool {
        !self.started.proxy.trim().is_empty()
    }
    /// The most peers one torrent connects to (0: no limit).
    pub fn peer_limit(&self) -> u32 {
        self.started.peer_limit
    }
}

struct State {
    wanted: TorrentPrefs,
    current: Option<Arc<Engine>>,
    /// Why the last start failed, so the next torrent says so rather than trying again blindly.
    failed: Option<String>,
}

static STATE: Mutex<State> = Mutex::new(State { wanted: default_prefs(), current: None, failed: None });
static DIR: OnceLock<PathBuf> = OnceLock::new();
/// Torrents running now, over every session.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

const fn default_prefs() -> TorrentPrefs {
    TorrentPrefs {
        port: 0,
        protocol: Protocol::Tcp,
        upnp: true,
        peer_limit: 200,
        down_kib: 0,
        up_kib: 0,
        proxy: String::new(),
        ip_filter: String::new(),
        chosen: false,
    }
}

/// The most ranges read from an IP filter.
const MAX_FILTER_RANGES: usize = 2_000_000;
/// The largest IP filter file read.
const MAX_FILTER_BYTES: u64 = 256 * 1024 * 1024;

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `<data>/torrents`: the DHT's nodes, and the IP filter as the engine reads it.
pub fn set_dir(dir: PathBuf) {
    let _ = DIR.set(dir);
}

/// The torrent settings, as `Job::Configure` brings them.
pub fn configure(prefs: TorrentPrefs) {
    let mut st = state();
    if let Some(e) = &st.current {
        e.session.ratelimits.set_download_bps(bps(prefs.down_kib));
        e.session.ratelimits.set_upload_bps(bps(prefs.up_kib));
    }
    if st.wanted.start_part() != prefs.start_part() {
        st.failed = None;
    }
    st.wanted = prefs;
    retire_if_idle(&mut st);
}

/// Let go of a session started with other settings when no torrent uses it.
fn retire_if_idle(st: &mut State) {
    let stale = st.current.as_ref().is_some_and(|e| e.started.start_part() != st.wanted.start_part());
    if stale && RUNNING.load(Ordering::SeqCst) == 0 {
        st.current = None;
    }
}

fn bps(kib: u32) -> Option<NonZeroU32> {
    NonZeroU32::new(kib.saturating_mul(1024))
}

/// The session for a new torrent, started (from the settings) if there is none.
pub(super) fn engine() -> Result<Arc<Engine>, NetError> {
    let mut st = state();
    retire_if_idle(&mut st);
    if let Some(e) = &st.current {
        return Ok(e.clone());
    }
    if let Some(why) = &st.failed {
        return Err(NetError::Http(why.clone()));
    }
    match start(&st.wanted) {
        Ok(e) => {
            let e = Arc::new(e);
            st.current = Some(e.clone());
            Ok(e)
        }
        Err(why) => {
            st.failed = Some(why.clone());
            Err(NetError::Http(why))
        }
    }
}

/// Held while a torrent runs: a session isn't replaced under it.
pub(super) struct Running;

impl Running {
    pub fn new() -> Self {
        RUNNING.fetch_add(1, Ordering::SeqCst);
        Running
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::SeqCst);
        retire_if_idle(&mut state());
    }
}

/// Whether downloads over all torrents have a speed limit now.
pub(super) fn down_limited() -> bool {
    state().wanted.down_kib > 0
}

/// Where the torrent engine stands, for the status bar: `None` before the first torrent.
pub fn status() -> Option<EngineStatus> {
    let e = state().current.clone()?;
    let dht = e.session.get_dht().map(|d| {
        let s = d.stats();
        s.routing_table_size.saturating_add(s.routing_table_size_v6)
    });
    Some(EngineStatus {
        dht_nodes: dht.map(|n| u32::try_from(n).unwrap_or(u32::MAX)),
        port: e.session.announce_port().or_else(|| e.session.listen_addr().map(|a| a.port())),
        upnp: e.started.upnp,
        forwarded: e.forwarding.as_ref().and_then(|(s, _)| s.lock().unwrap_or_else(PoisonError::into_inner).clone()),
        proxy: !e.started.proxy.trim().is_empty(),
        filtered: e.filtered,
        running: u32::try_from(RUNNING.load(Ordering::SeqCst)).unwrap_or(u32::MAX),
        pending: state().wanted.start_part() != e.started.start_part(),
    })
}

fn start(p: &TorrentPrefs) -> Result<Engine, String> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("torrents")
        .enable_all()
        .build()
        .map_err(|e| format!("couldn't start torrents: {e}"))?;
    let dir = DIR.get().filter(|d| std::fs::create_dir_all(d).is_ok());
    // The DHT's nodes are kept in DownloadCraft's data folder, or not at all (never in a folder
    // of librqbit's choosing).
    let persistence = dir.map(|d| librqbit::dht::DhtPersistenceConfig { config_filename: Some(d.join("dht.json")), ..Default::default() });
    let dht = librqbit::DhtSessionConfig { persistence, ..Default::default() };
    let proxy = proxy_url(&p.proxy).map_err(|e| format!("the torrent proxy: {e}"))?;
    // Through a proxy, peers are reached over TCP only: µTP would go around it.
    let mode = if p.protocol == Protocol::Both && proxy.is_none() { ListenerMode::TcpAndUtp } else { ListenerMode::TcpOnly };
    let listen = ListenerOptions {
        mode,
        listen_addr: (std::net::Ipv6Addr::UNSPECIFIED, p.port).into(),
        // DownloadCraft forwards the port itself (`torrent/portmap.rs`): librqbit's UPnP can look
        // for the router on the wrong network adapter.
        enable_upnp_port_forwarding: false,
        ..ListenerOptions::default()
    };
    let (blocklist_url, filtered) = match ip_filter(p, dir.map(PathBuf::as_path))? {
        Some((url, n)) => (Some(url), Some(n)),
        None => (None, None),
    };
    let opts = SessionOptions {
        dht: Some(dht),
        listen: Some(listen),
        connect: Some(ConnectionOptions { proxy_url: proxy, ..ConnectionOptions::default() }),
        peer_limit: usize::try_from(p.peer_limit).ok().filter(|n| *n > 0),
        ratelimits: LimitsConfig { download_bps: bps(p.down_kib), upload_bps: bps(p.up_kib) },
        blocklist_url,
        client_name_and_version: Some(format!("DownloadCraft {}", env!("CARGO_PKG_VERSION"))),
        ..SessionOptions::default()
    };
    let session = rt.block_on(Session::new_with_opts(std::env::temp_dir(), opts)).map_err(|e| format!("couldn't start torrents: {e:#}"))?;
    log::info!(
        "torrents: listening on {:?} ({mode:?}{}), DHT {}, {} filtered ranges",
        session.listen_addr(),
        if p.upnp { ", forwarding the port" } else { "" },
        if session.get_dht().is_some() { "on" } else { "off" },
        filtered.unwrap_or(0)
    );
    // On by default, as in other torrent clients, so peers can connect in; Settings turns it off.
    let forwarding = match (p.upnp, session.listen_addr()) {
        (true, Some(addr)) => {
            let stop = Arc::new(AtomicBool::new(false));
            Some((super::portmap::start(addr.port(), stop.clone()), stop))
        }
        _ => None,
    };
    Ok(Engine { rt, session, started: p.clone(), filtered, forwarding })
}

/// The IP filter in the settings, written as the P2P form librqbit reads (`<dir>/ipfilter.p2p`):
/// its `file://` address and how many ranges it has.
fn ip_filter(p: &TorrentPrefs, dir: Option<&Path>) -> Result<Option<(String, usize)>, String> {
    let path = p.ip_filter.trim();
    if path.is_empty() {
        return Ok(None);
    }
    let dir = dir.ok_or("the IP filter needs DownloadCraft's data folder")?;
    let raw = read_filter(Path::new(path))?;
    let (p2p, n) = ip_filter_to_p2p(&raw, MAX_FILTER_RANGES);
    if n == 0 {
        return Err("the IP filter has no address ranges in it (.dat and .p2p files are read; .p2b isn't)".into());
    }
    let out = dir.join("ipfilter.p2p");
    std::fs::write(&out, p2p).map_err(|e| format!("couldn't keep the IP filter: {e}"))?;
    Ok(Some((file_url(&out), n)))
}

/// An IP filter file's text, unzipped when it's gzipped.
fn read_filter(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let f = std::fs::File::open(path).map_err(|e| format!("couldn't open the IP filter: {e}"))?;
    let mut bytes = Vec::new();
    f.take(MAX_FILTER_BYTES).read_to_end(&mut bytes).map_err(|e| format!("couldn't read the IP filter: {e}"))?;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut text = Vec::new();
        flate2::read::GzDecoder::new(bytes.as_slice())
            .take(MAX_FILTER_BYTES)
            .read_to_end(&mut text)
            .map_err(|e| format!("couldn't unpack the IP filter: {e}"))?;
        bytes = text;
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `path` as a `file://` address, every byte outside the unreserved set, `/` and `:` encoded.
fn file_url(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn filter_paths_become_file_addresses() {
        assert_eq!(super::file_url(std::path::Path::new("/home/me/a b.p2p")), "file:///home/me/a%20b.p2p");
        #[cfg(windows)]
        assert_eq!(super::file_url(std::path::Path::new(r"C:\Users\Me\ipfilter.p2p")), "file:///C:/Users/Me/ipfilter.p2p");
    }
}
