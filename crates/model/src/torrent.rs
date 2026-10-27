//! Torrents as the list and the details pane see them: what a running torrent reports (its peers,
//! trackers and files), the torrent settings, and the pure parts of asking a tracker about a
//! torrent's swarm (a "scrape": BEP 48 over HTTP, BEP 15 over UDP), each written from its
//! specification. Nothing here does I/O.

use serde::{Deserialize, Serialize};

/// What a running torrent is doing, reported about once a second (not saved).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TorrentLive {
    /// The info hash, in hex.
    pub info_hash: String,
    /// Seconds this run has been going, and of them, seconds spent sharing a finished torrent.
    pub active_secs: u64,
    pub seeding_secs: u64,
    /// Bytes received and sent by this run.
    pub downloaded: u64,
    pub uploaded: u64,
    /// Bytes received that failed their piece's check or arrived twice; known once it's all
    /// there (until then, pieces still arriving can't be told from waste).
    pub wasted: Option<u64>,
    pub down_speed: u64,
    pub up_speed: u64,
    /// The bytes of the files picked, and how many of them are on disk.
    pub size: u64,
    pub have: u64,
    pub pieces: u32,
    pub piece_size: u32,
    /// Peers connected now, being connected to, waiting to be tried, and known in all.
    pub connected: u32,
    pub connecting: u32,
    pub queued: u32,
    pub known: u32,
    /// The most peers one torrent connects to (0: no limit).
    pub peer_limit: u32,
    /// The peers connected now, busiest first (at most [`MAX_PEERS_SHOWN`]).
    pub peers: Vec<Peer>,
    /// The torrent's trackers, as last asked.
    pub trackers: Vec<Tracker>,
    /// Its web seeds (BEP 19): web servers that have its files, and how fetching from them went.
    #[serde(default)]
    pub web_seeds: Vec<Tracker>,
    /// Files are being fetched from a web seed before peers are asked.
    #[serde(default)]
    pub from_web: bool,
    /// The files to download first are done, and the rest are being fetched.
    #[serde(default)]
    pub first_done: bool,
    /// Bytes on disk of each file, by its index in the torrent's list.
    pub file_done: Vec<u64>,
    /// The torrent's files: in the first report of a run only (the engine keeps them).
    #[serde(default)]
    pub files: Vec<crate::probe::TorrentFile>,
}

/// The most peers listed in the details.
pub const MAX_PEERS_SHOWN: usize = 200;

/// One peer connected to a torrent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peer {
    /// `address:port`.
    pub address: String,
    /// `TCP`, `µTP` or `SOCKS`.
    pub connection: String,
    /// The program it says it is, when it says.
    pub client: String,
    pub down_speed: u64,
    pub up_speed: u64,
    /// Bytes received from it and sent to it.
    pub downloaded: u64,
    pub uploaded: u64,
}

/// One of a torrent's trackers, and what it last said about the swarm.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tracker {
    pub url: String,
    pub status: TrackerStatus,
    /// Peers with the whole torrent, peers still downloading, and downloads it has seen finish.
    pub seeds: Option<u32>,
    pub leechers: Option<u32>,
    pub completed: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "why", rename_all = "lowercase")]
pub enum TrackerStatus {
    /// Not asked yet.
    #[default]
    Waiting,
    /// It answered.
    Working,
    /// It didn't, or answered with an error.
    Failing(String),
    /// It can't be asked about the swarm (no scrape address, or a web socket tracker).
    Unsupported,
}

/// Where the torrent engine stands, for the status bar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineStatus {
    /// Nodes the DHT knows; `None` when it's off.
    pub dht_nodes: Option<u32>,
    /// The port other peers reach this one on.
    pub port: Option<u16>,
    pub upnp: bool,
    pub proxy: bool,
    /// Ranges in the IP filter, when one is loaded.
    pub filtered: Option<usize>,
    /// Torrents running now.
    pub running: u32,
    /// Settings changed that apply once no torrent is running.
    pub pending: bool,
}

/// How a torrent connects to peers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Tcp,
    /// TCP and µTP (BEP 29).
    Both,
}

/// The torrent settings (Settings › Torrents). Those marked "start" apply from the next time the
/// torrent engine starts: when no torrent is running, or on the next start of DownloadCraft.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentPrefs {
    /// The port for incoming connections; 0 for one the system picks each start (start).
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub protocol: Protocol,
    /// Ask the router to forward the port, through UPnP (start). Off unless chosen.
    #[serde(default)]
    pub upnp: bool,
    /// The most peers one torrent connects to; 0 for no limit (start).
    #[serde(default = "default_peer_limit")]
    pub peer_limit: u32,
    /// Speed limits over all torrents, in KiB/s; 0 for none (at once).
    #[serde(default)]
    pub down_kib: u32,
    #[serde(default)]
    pub up_kib: u32,
    /// A SOCKS5 proxy for peers and HTTP trackers, `host:port`; empty for none (start).
    #[serde(default)]
    pub proxy: String,
    /// An IP filter file (`.dat` or `.p2p`, gzipped or not); empty for none (start).
    #[serde(default)]
    pub ip_filter: String,
}

fn default_peer_limit() -> u32 {
    200
}

/// The most peers anyone can ask a torrent to connect to.
pub const MAX_PEER_LIMIT: u32 = 2000;

impl Default for TorrentPrefs {
    fn default() -> Self {
        TorrentPrefs {
            port: 0,
            protocol: Protocol::Tcp,
            upnp: false,
            peer_limit: default_peer_limit(),
            down_kib: 0,
            up_kib: 0,
            proxy: String::new(),
            ip_filter: String::new(),
        }
    }
}

impl TorrentPrefs {
    /// The settings that need the engine started again to apply.
    pub fn start_part(&self) -> (u16, Protocol, bool, u32, &str, &str) {
        (self.port, self.protocol, self.upnp, self.peer_limit, self.proxy.as_str(), self.ip_filter.as_str())
    }
}

/// `host:port` as a SOCKS5 proxy's address (`socks5://host:port`), or why it isn't one. Sign-in
/// isn't taken here: a password would end up in the saved list (it comes with the keychain).
pub fn proxy_url(typed: &str) -> Result<Option<String>, &'static str> {
    let t = typed.trim().trim_start_matches("socks5://").trim_end_matches('/');
    if t.is_empty() {
        return Ok(None);
    }
    if t.contains('@') {
        return Err("a proxy with a password isn't supported yet");
    }
    let (host, port) = t.rsplit_once(':').ok_or("write the proxy as host:port")?;
    let port: u16 = port.parse().ok().filter(|p| *p > 0).ok_or("the proxy's port is a number from 1 to 65535")?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let ok = !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
    if !ok {
        return Err("the proxy's host isn't a name or an address");
    }
    let host = if host.contains(':') { format!("[{host}]") } else { host.to_string() };
    Ok(Some(format!("socks5://{host}:{port}")))
}

// ---------------------------------------------------------------------------------------------
// Bencode (BEP 3), just enough to read a tracker's scrape answer.

/// A bencoded value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bencode {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Bencode>),
    /// Keys in the order they came.
    Dict(Vec<(Vec<u8>, Bencode)>),
}

impl Bencode {
    pub fn get(&self, key: &[u8]) -> Option<&Bencode> {
        match self {
            Bencode::Dict(items) => items.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn int(&self) -> Option<i64> {
        match self {
            Bencode::Int(i) => Some(*i),
            _ => None,
        }
    }
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Bencode::Bytes(b) => Some(b),
            _ => None,
        }
    }
}

/// How deep lists and dictionaries may nest.
const MAX_DEPTH: usize = 32;
/// The most items in one answer.
const MAX_ITEMS: usize = 100_000;

/// Read one bencoded value that is the whole of `input`.
pub fn bdecode(input: &[u8]) -> Option<Bencode> {
    let mut items = 0;
    let (v, rest) = value(input, 0, &mut items)?;
    rest.is_empty().then_some(v)
}

fn value<'a>(b: &'a [u8], depth: usize, items: &mut usize) -> Option<(Bencode, &'a [u8])> {
    *items += 1;
    if depth > MAX_DEPTH || *items > MAX_ITEMS {
        return None;
    }
    match b.first()? {
        b'i' => {
            let end = b.iter().position(|c| *c == b'e')?;
            let digits = std::str::from_utf8(b.get(1..end)?).ok()?;
            let canonical = digits == "0" || (!digits.starts_with('0') && !digits.starts_with("-0") && !digits.is_empty() && digits != "-");
            if !canonical {
                return None;
            }
            Some((Bencode::Int(digits.parse().ok()?), b.get(end + 1..)?))
        }
        b'l' => {
            let mut rest = b.get(1..)?;
            let mut list = Vec::new();
            while rest.first()? != &b'e' {
                let (v, r) = value(rest, depth + 1, items)?;
                list.push(v);
                rest = r;
            }
            Some((Bencode::List(list), rest.get(1..)?))
        }
        b'd' => {
            let mut rest = b.get(1..)?;
            let mut dict = Vec::new();
            while rest.first()? != &b'e' {
                let (k, r) = string(rest)?;
                let (v, r) = value(r, depth + 1, items)?;
                dict.push((k, v));
                rest = r;
            }
            Some((Bencode::Dict(dict), rest.get(1..)?))
        }
        b'0'..=b'9' => string(b).map(|(s, r)| (Bencode::Bytes(s), r)),
        _ => None,
    }
}

fn string(b: &[u8]) -> Option<(Vec<u8>, &[u8])> {
    let colon = b.iter().take(12).position(|c| *c == b':')?;
    let len: usize = std::str::from_utf8(b.get(..colon)?).ok()?.parse().ok()?;
    let start = colon + 1;
    let s = b.get(start..start.checked_add(len)?)?;
    Some((s.to_vec(), b.get(start + len..)?))
}

// ---------------------------------------------------------------------------------------------
// Scrape (BEP 48 for HTTP trackers, BEP 15 for UDP ones).

/// What a tracker says about one torrent's swarm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Swarm {
    pub seeds: u32,
    pub leechers: u32,
    pub completed: u32,
}

/// An HTTP tracker's scrape address: its announce address with the last `announce` of the path
/// changed to `scrape` (BEP 48), and the info hash added. `None` when it has none.
pub fn http_scrape_url(announce: &str, info_hash: &[u8; 20]) -> Option<String> {
    let (base, query) = announce.split_once('?').map_or((announce, None), |(b, q)| (b, Some(q)));
    let slash = base.rfind('/')?;
    let last = base.get(slash + 1..)?;
    let rest = last.strip_prefix("announce")?;
    let mut url = format!("{}scrape{rest}", base.get(..=slash)?);
    let hash: String = info_hash.iter().map(|b| if b.is_ascii_alphanumeric() { (*b as char).to_string() } else { format!("%{b:02X}") }).collect();
    match query {
        Some(q) if !q.is_empty() => url.push_str(&format!("?{q}&info_hash={hash}")),
        _ => url.push_str(&format!("?info_hash={hash}")),
    }
    Some(url)
}

/// Read an HTTP tracker's scrape answer for `info_hash`.
pub fn parse_http_scrape(body: &[u8], info_hash: &[u8; 20]) -> Result<Swarm, String> {
    let v = bdecode(body).ok_or("the tracker's answer isn't bencoded")?;
    if let Some(why) = v.get(b"failure reason").and_then(Bencode::bytes) {
        return Err(String::from_utf8_lossy(why).chars().take(120).collect());
    }
    let files = v.get(b"files").ok_or("the tracker's answer has no files")?;
    let entry = files.get(info_hash).ok_or("the tracker doesn't know this torrent")?;
    let n = |k: &[u8]| entry.get(k).and_then(Bencode::int).map_or(0, |i| u32::try_from(i.max(0)).unwrap_or(u32::MAX));
    Ok(Swarm { seeds: n(b"complete"), leechers: n(b"incomplete"), completed: n(b"downloaded") })
}

/// BEP 15's protocol id, sent in the first request.
const UDP_MAGIC: u64 = 0x41727101980;

/// A UDP tracker's connect request.
pub fn udp_connect(transaction: u32) -> [u8; 16] {
    let mut p = [0u8; 16];
    p[..8].copy_from_slice(&UDP_MAGIC.to_be_bytes());
    // action 0 (connect) is already zero
    p[12..].copy_from_slice(&transaction.to_be_bytes());
    p
}

/// The connection id in a connect answer, when it's the answer to `transaction`.
pub fn udp_connected(answer: &[u8], transaction: u32) -> Result<u64, String> {
    udp_check(answer, 0, transaction)?;
    let id = answer.get(8..16).ok_or("the tracker's answer is too short")?;
    let mut b = [0u8; 8];
    b.copy_from_slice(id);
    Ok(u64::from_be_bytes(b))
}

/// A UDP tracker's scrape request for one torrent.
pub fn udp_scrape(connection: u64, transaction: u32, info_hash: &[u8; 20]) -> [u8; 36] {
    let mut p = [0u8; 36];
    p[..8].copy_from_slice(&connection.to_be_bytes());
    p[8..12].copy_from_slice(&2u32.to_be_bytes());
    p[12..16].copy_from_slice(&transaction.to_be_bytes());
    p[16..].copy_from_slice(info_hash);
    p
}

/// Read a UDP tracker's scrape answer to `transaction`.
pub fn udp_scraped(answer: &[u8], transaction: u32) -> Result<Swarm, String> {
    udp_check(answer, 2, transaction)?;
    let n = |i: usize| answer.get(i..i + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])).ok_or("the tracker's answer is too short");
    Ok(Swarm { seeds: n(8)?, completed: n(12)?, leechers: n(16)? })
}

fn udp_check(answer: &[u8], action: u32, transaction: u32) -> Result<(), String> {
    let word = |i: usize| answer.get(i..i + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let (Some(got), Some(t)) = (word(0), word(4)) else { return Err("the tracker's answer is too short".into()) };
    if t != transaction {
        return Err("the tracker answered something else".into());
    }
    if got == 3 {
        let why = String::from_utf8_lossy(answer.get(8..).unwrap_or_default()).chars().take(120).collect::<String>();
        return Err(if why.is_empty() { "the tracker refused".into() } else { why });
    }
    if got != action {
        return Err("the tracker answered something else".into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Web seeds (BEP 19).

/// The most web seeds taken from one torrent.
pub const MAX_WEB_SEEDS: usize = 8;

/// The web seeds a `.torrent` file names (`url-list`, one address or a list): http and https
/// only, each once, at most [`MAX_WEB_SEEDS`].
pub fn url_list(torrent: &[u8]) -> Vec<String> {
    let Some(v) = bdecode(torrent) else { return Vec::new() };
    let items: Vec<&Bencode> = match v.get(b"url-list") {
        Some(Bencode::List(l)) => l.iter().collect(),
        Some(b @ Bencode::Bytes(_)) => vec![b],
        _ => Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    for b in items.into_iter().filter_map(Bencode::bytes) {
        let Ok(url) = std::str::from_utf8(b) else { continue };
        let url = url.trim();
        let web = url.starts_with("https://") || url.starts_with("http://");
        if web && url.len() <= 2048 && !url.chars().any(char::is_whitespace) && !out.iter().any(|u| u == url) {
            out.push(url.to_string());
        }
        if out.len() >= MAX_WEB_SEEDS {
            break;
        }
    }
    out
}

/// Where a web seed serves one file (BEP 19): a torrent of one file at the address itself, or at
/// the address plus its name when the address ends in `/`; a torrent of several files under
/// `address/name/path`. Each part is percent-encoded.
pub fn web_seed_url(base: &str, name: &str, path: &[&str], single: bool) -> String {
    let enc = |part: &str| -> String {
        part.bytes()
            .map(|b| if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') { (b as char).to_string() } else { format!("%{b:02X}") })
            .collect()
    };
    if single {
        return if base.ends_with('/') { format!("{base}{}", enc(name)) } else { base.to_string() };
    }
    let mut url = base.trim_end_matches('/').to_string();
    for part in std::iter::once(name).chain(path.iter().copied()) {
        url.push('/');
        url.push_str(&enc(part));
    }
    url
}

/// The 20 bytes of a hex info hash.
pub fn hash_bytes(hex: &str) -> Option<[u8; 20]> {
    let b = hex.as_bytes();
    if b.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    for (i, o) in out.iter_mut().enumerate() {
        let pair = std::str::from_utf8(b.get(i * 2..i * 2 + 2)?).ok()?;
        *o = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// IP filters.

/// An IP filter's lines in the P2P form (`name:first-last`), which the torrent engine reads, from
/// either that form or eMule's `.dat` form (`first - last , level , name`, where a level of 128
/// or more means allowed and is left out). Comments, blank lines and lines that are neither are
/// skipped; at most `max` ranges are kept.
pub fn ip_filter_to_p2p(text: &str, max: usize) -> (String, usize) {
    let mut out = String::new();
    let mut n = 0;
    for line in text.lines().map(str::trim) {
        if n >= max {
            break;
        }
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        let range = if let Some((range, rest)) = line.split_once(',') {
            // .dat: the level comes next.
            let level = rest.split(',').next().map(str::trim).and_then(|l| l.parse::<u32>().ok()).unwrap_or(0);
            if level >= 128 {
                continue;
            }
            range
        } else if line.matches(':').count() <= 2 {
            line.rsplit_once(':').map_or(line, |(_, r)| r)
        } else {
            line.split_once(':').map_or(line, |(_, r)| r)
        };
        let Some((a, b)) = range.split_once('-') else { continue };
        let (a, b) = (clean_ip(a), clean_ip(b));
        let both = a.parse::<std::net::IpAddr>().ok().zip(b.parse::<std::net::IpAddr>().ok());
        if matches!(both, Some((x, y)) if x.is_ipv4() == y.is_ipv4()) {
            out.push_str(&format!("r{n}:{a}-{b}\n"));
            n += 1;
        }
    }
    (out, n)
}

/// An address as `.dat` files write it, with leading zeros (`001.002.003.004`), without them.
fn clean_ip(s: &str) -> String {
    let s = s.trim();
    if s.contains(':') {
        return s.to_string();
    }
    s.split('.').map(|p| p.trim_start_matches('0')).map(|p| if p.is_empty() { "0" } else { p }).collect::<Vec<_>>().join(".")
}

#[cfg(test)]
mod tests;
