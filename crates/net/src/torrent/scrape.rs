//! Asking a torrent's trackers about its swarm: how many peers have all of it, how many are
//! still downloading, and how many downloads they've seen finish. A "scrape", over HTTP (BEP 48)
//! or UDP (BEP 15); librqbit announces to the trackers but doesn't say what they answered.
//!
//! One thread per torrent asks each tracker in turn (at most [`MAX_TRACKERS`]), then again every
//! [`EVERY`], until the torrent stops. While a proxy is set nothing is asked: the scrape would go
//! around it.

use std::net::{ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use downloadcraft_model::torrent::{
    Swarm, Tracker, TrackerStatus, http_scrape_url, parse_http_scrape, udp_connect, udp_connected, udp_scrape, udp_scraped,
};

/// The most trackers asked about one torrent.
pub const MAX_TRACKERS: usize = 30;
/// How long a tracker has to answer.
const TIMEOUT: Duration = Duration::from_secs(12);
/// How often the trackers are asked again.
const EVERY: Duration = Duration::from_secs(15 * 60);
/// The largest answer read from an HTTP tracker.
const MAX_ANSWER: u64 = 64 * 1024;

/// The trackers as last asked, shared with the torrent's transfer.
pub type Trackers = Arc<Mutex<Vec<Tracker>>>;

/// Start asking `urls` about `info_hash` in the background, until `stop` is set.
pub fn start(urls: Vec<String>, info_hash: [u8; 20], proxied: bool, stop: Arc<AtomicBool>) -> Trackers {
    let mut urls = urls;
    urls.sort();
    urls.dedup();
    urls.truncate(MAX_TRACKERS);
    let status = |u: &str| {
        if proxied || !(u.starts_with("http://") || u.starts_with("https://") || u.starts_with("udp://")) {
            TrackerStatus::Unsupported
        } else {
            TrackerStatus::Waiting
        }
    };
    let list: Trackers = Arc::new(Mutex::new(urls.iter().map(|u| Tracker { url: u.clone(), status: status(u), ..Tracker::default() }).collect()));
    if proxied || urls.is_empty() {
        return list;
    }
    let shared = list.clone();
    let spawned = std::thread::Builder::new().name("scrape".into()).spawn(move || {
        let mut next = Instant::now() + Duration::from_secs(3);
        loop {
            while Instant::now() < next {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            for (i, url) in urls.iter().enumerate() {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let answer = if url.starts_with("udp://") {
                    udp(url, &info_hash)
                } else if url.starts_with("http") {
                    http(url, &info_hash)
                } else {
                    continue;
                };
                let mut list = shared.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(t) = list.get_mut(i) {
                    match answer {
                        Ok(s) => {
                            *t = Tracker {
                                url: url.clone(),
                                status: TrackerStatus::Working,
                                seeds: Some(s.seeds),
                                leechers: Some(s.leechers),
                                completed: Some(s.completed),
                            }
                        }
                        Err(Ask::Unsupported) => t.status = TrackerStatus::Unsupported,
                        Err(Ask::Failed(why)) => t.status = TrackerStatus::Failing(why),
                    }
                }
            }
            next = Instant::now() + EVERY;
        }
    });
    if let Err(e) = spawned {
        log::warn!("couldn't ask the trackers: {e}");
    }
    list
}

enum Ask {
    Unsupported,
    Failed(String),
}

fn http(announce: &str, hash: &[u8; 20]) -> Result<Swarm, Ask> {
    let url = http_scrape_url(announce, hash).ok_or(Ask::Unsupported)?;
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().into();
    let mut resp = agent.get(&url).call().map_err(|e| Ask::Failed(short(&e.to_string())))?;
    let body = resp.body_mut().with_config().limit(MAX_ANSWER).read_to_vec().map_err(|e| Ask::Failed(short(&e.to_string())))?;
    parse_http_scrape(&body, hash).map_err(Ask::Failed)
}

fn udp(announce: &str, hash: &[u8; 20]) -> Result<Swarm, Ask> {
    let rest = announce.strip_prefix("udp://").ok_or(Ask::Unsupported)?;
    let host_port = rest.split(['/', '?']).next().unwrap_or("");
    let addr = host_port
        .to_socket_addrs()
        .map_err(|_| Ask::Failed("its name doesn't resolve".into()))?
        .next()
        .ok_or_else(|| Ask::Failed("its name doesn't resolve".into()))?;
    let bind = if addr.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let sock = UdpSocket::bind(bind).map_err(|e| Ask::Failed(short(&e.to_string())))?;
    sock.set_read_timeout(Some(TIMEOUT)).map_err(|e| Ask::Failed(short(&e.to_string())))?;
    sock.connect(addr).map_err(|e| Ask::Failed(short(&e.to_string())))?;
    let t1 = transaction();
    let connection = ask(&sock, &udp_connect(t1), |a| udp_connected(a, t1))?;
    let t2 = transaction().wrapping_add(1);
    ask(&sock, &udp_scrape(connection, t2, hash), |a| udp_scraped(a, t2))
}

/// Send `packet` and read the answer, trying twice (UDP may lose either).
fn ask<T>(sock: &UdpSocket, packet: &[u8], read: impl Fn(&[u8]) -> Result<T, String>) -> Result<T, Ask> {
    let mut buf = [0u8; 2048];
    let mut last = "it didn't answer".to_string();
    for _ in 0..2 {
        sock.send(packet).map_err(|e| Ask::Failed(short(&e.to_string())))?;
        match sock.recv(&mut buf) {
            Ok(n) => return read(buf.get(..n).unwrap_or_default()).map_err(Ask::Failed),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(e) => last = short(&e.to_string()),
        }
    }
    Err(Ask::Failed(last))
}

/// A transaction id that differs from one request to the next.
fn transaction() -> u32 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    t ^ std::process::id().rotate_left(16)
}

fn short(why: &str) -> String {
    why.chars().take(120).collect()
}
