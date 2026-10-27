//! One file fetched on several connections at once, each on a byte range of its own (RFC 9110
//! §14), the way Internet Download Manager does it.
//!
//! The file is split into as many parts as connections. Each connection writes its bytes straight
//! into place in the partial file (sized to the whole file up front). A connection whose part is
//! done takes over the back half of the biggest part still going, so every connection stays busy
//! to the end. Where each part is gets saved beside the partial file (`name.dcpart.parts`) a few
//! times a second, so a pause, a crash or quitting carries on every part from where it was.
//!
//! A connection keeps the bytes it may write reserved under the lock before writing them, and a
//! part is only ever split at least [`MIN_PART`] past where its connection is, so two connections
//! never write the same bytes.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use downloadcraft_model::{Event, Segment};
use serde_json::{Value, json};

use super::{CHUNK, Fetch, TICK, content_range, disk, header, http_error};
use crate::{NetError, Sink};

/// The smallest part worth a connection of its own.
pub(crate) const MIN_PART: u64 = 1 << 20;
/// Tries per part before the download gives up (and waits for Resume), when no other connection
/// is left to take the part over.
const RETRIES: u32 = 3;
/// How long an extra connection waits for the server to start answering. Many servers cap how
/// many connections one client gets and leave the rest waiting; those give their part back.
const PART_ANSWER: Duration = Duration::from_secs(15);
/// How often the parts' record is written.
const SAVE_EVERY: Duration = Duration::from_secs(1);
/// The most parts a record may hold (a hostile or damaged one is ignored).
const MAX_PARTS: usize = 1024;
/// How long a server's connection cap is remembered; after that it gets the full count again, in
/// case the cap was a busy moment.
const CAP_FOR: Duration = Duration::from_secs(30 * 60);

/// Servers that turned connections away: `host:port`, how many connections they took, and when.
static CAPS: Mutex<Vec<(String, usize, Instant)>> = Mutex::new(Vec::new());

/// `host:port` of `url`, the key of [`CAPS`].
fn server(url: &str) -> Option<String> {
    let uri: ureq::http::Uri = url.parse().ok()?;
    let host = uri.host()?.to_ascii_lowercase();
    let port = uri.port_u16().unwrap_or(if uri.scheme_str() == Some("http") { 80 } else { 443 });
    Some(format!("{host}:{port}"))
}

/// How many connections a download from `url` opens: `wanted`, or fewer when its server turned
/// connections away lately.
pub(crate) fn connections_for(url: &str, wanted: usize) -> usize {
    let Some(key) = server(url) else { return wanted };
    let mut caps = CAPS.lock().unwrap_or_else(PoisonError::into_inner);
    caps.retain(|(_, _, at)| at.elapsed() < CAP_FOR);
    caps.iter().find(|(k, _, _)| *k == key).map_or(wanted, |(_, cap, _)| wanted.min(*cap)).max(1)
}

/// Remember that `url`'s server served no more than `n` connections at once.
fn remember_cap(url: &str, n: usize) {
    let Some(key) = server(url) else { return };
    let mut caps = CAPS.lock().unwrap_or_else(PoisonError::into_inner);
    match caps.iter_mut().find(|(k, _, _)| *k == key) {
        Some(c) => *c = (key, c.1.min(n), Instant::now()),
        None => caps.push((key, n, Instant::now())),
    }
}

/// A part of the file: bytes `start..end`, written up to `pos`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Part {
    pub start: u64,
    pub end: u64,
    pub pos: u64,
    /// A connection is on it.
    pub taken: bool,
}

impl Part {
    fn left(&self) -> u64 {
        self.end.saturating_sub(self.pos)
    }
}

/// `total` bytes split into `n` parts of about the same size (at least [`MIN_PART`] each).
pub(crate) fn split(total: u64, n: u8) -> Vec<Part> {
    let n = u64::from(n.max(1)).min(total.div_ceil(MIN_PART).max(1));
    let size = total.div_ceil(n);
    (0..n)
        .map(|i| {
            let start = i.saturating_mul(size).min(total);
            let end = start.saturating_add(size).min(total);
            Part { start, end, pos: start, taken: false }
        })
        .filter(|p| p.end > p.start)
        .collect()
}

/// Where the parts' record of `part` (the partial file) is kept.
pub(crate) fn record_path(part: &Path) -> PathBuf {
    let mut name = part.as_os_str().to_owned();
    name.push(".parts");
    PathBuf::from(name)
}

/// The record beside `part`, when it's whole and fits a file of `total` bytes (or any size when
/// `total` is `None`): the size and the parts.
pub(crate) fn load(part: &Path) -> Option<(u64, Vec<Part>)> {
    let text = std::fs::read_to_string(record_path(part)).ok()?;
    if text.len() > 256 * 1024 {
        return None;
    }
    let v: Value = serde_json::from_str(&text).ok()?;
    let total = v.get("total")?.as_u64()?;
    let list = v.get("parts")?.as_array()?;
    if list.is_empty() || list.len() > MAX_PARTS {
        return None;
    }
    let mut parts = Vec::with_capacity(list.len());
    for p in list {
        let n = |i: usize| p.get(i).and_then(Value::as_u64);
        let (start, end, pos) = (n(0)?, n(1)?, n(2)?);
        if !(start <= pos && pos <= end && end <= total) {
            return None;
        }
        parts.push(Part { start, end, pos, taken: false });
    }
    parts.sort_by_key(|p| p.start);
    // The parts must cover the file exactly once.
    let mut at = 0;
    for p in &parts {
        if p.start != at {
            return None;
        }
        at = p.end;
    }
    (at == total).then_some((total, parts))
}

fn save(part: &Path, total: u64, parts: &[Part]) -> std::io::Result<()> {
    let list: Vec<Value> = parts.iter().map(|p| json!([p.start, p.end, p.pos])).collect();
    let path = record_path(part);
    let tmp = path.with_extension("parts.tmp");
    std::fs::write(&tmp, json!({"total": total, "parts": list}).to_string())?;
    std::fs::rename(&tmp, &path)
}

/// What the connections share.
struct Shared<'a> {
    agent: &'a ureq::Agent,
    url: &'a str,
    file: &'a Path,
    parts: Mutex<Vec<Part>>,
    done: AtomicU64,
    stop: AtomicBool,
    failed: Mutex<Option<String>>,
    busy: AtomicUsize,
}

impl Shared<'_> {
    fn parts(&self) -> std::sync::MutexGuard<'_, Vec<Part>> {
        self.parts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A part for a connection that's free: one nobody is on, else the back half of the biggest
    /// part going (when that's worth it).
    fn claim(&self) -> Option<usize> {
        let mut parts = self.parts();
        if let Some(i) = parts.iter().position(|p| !p.taken && p.left() > 0) {
            if let Some(p) = parts.get_mut(i) {
                p.taken = true;
            }
            return Some(i);
        }
        let (i, biggest) = parts.iter().enumerate().filter(|(_, p)| p.taken).max_by_key(|(_, p)| p.left()).map(|(i, p)| (i, *p))?;
        if biggest.left() < 2 * MIN_PART {
            return None;
        }
        let mid = biggest.pos + biggest.left() / 2;
        if let Some(p) = parts.get_mut(i) {
            p.end = mid;
        }
        parts.push(Part { start: mid, end: biggest.end, pos: mid, taken: true });
        Some(parts.len() - 1)
    }

    fn fail(&self, why: String) {
        let mut failed = self.failed.lock().unwrap_or_else(PoisonError::into_inner);
        failed.get_or_insert(why);
        self.stop.store(true, Ordering::Relaxed);
    }

    fn snapshot(&self) -> Vec<Part> {
        let mut parts = self.parts().clone();
        parts.sort_by_key(|p| p.start);
        parts
    }
}

/// Fetch the file on up to `connections` connections into `part` (`total` bytes), starting from
/// `parts` (fresh, or a record carried on). `first` is the answer already open at byte 0, which
/// serves the first part. `Ok(Some(total))` when every part is whole, `Ok(None)` when cancelled.
pub(super) fn run(
    agent: &ureq::Agent,
    f: &Fetch,
    part: &Path,
    total: u64,
    mut parts: Vec<Part>,
    first: Option<ureq::Body>,
    cancel: &AtomicBool,
    sink: &Sink,
) -> Result<Option<u64>, NetError> {
    // The answer already open at byte 0 is the first part's, and no one else's.
    let first = match (first, parts.first_mut()) {
        (Some(body), Some(p)) if p.start == 0 && p.pos == 0 => {
            p.taken = true;
            Some((0, body))
        }
        _ => None,
    };
    let file = OpenOptions::new().create(true).write(true).truncate(false).open(part).map_err(|e| disk(part, e))?;
    if file.metadata().map(|m| m.len()).unwrap_or(0) != total {
        file.set_len(total).map_err(|e| disk(part, e))?;
    }
    drop(file);
    let done: u64 = parts.iter().map(|p| p.pos - p.start).sum();
    let shared = Shared {
        agent,
        url: &f.url,
        file: part,
        parts: Mutex::new(parts),
        done: AtomicU64::new(done),
        stop: AtomicBool::new(false),
        failed: Mutex::new(None),
        busy: AtomicUsize::new(0),
    };
    if let Err(e) = save(part, total, &shared.snapshot()) {
        return Err(disk(part, e));
    }
    let workers = connections_for(&f.url, usize::from(f.connections.max(1)));
    std::thread::scope(|scope| {
        let mut first = first;
        for _ in 0..workers {
            let body = first.take();
            shared.busy.fetch_add(1, Ordering::SeqCst);
            let sh = &shared;
            scope.spawn(move || {
                if !connection(sh, body) {
                    sh.busy.fetch_sub(1, Ordering::SeqCst);
                }
            });
        }
        let (mut tick, mut tick_bytes, mut saved) = (Instant::now(), shared.done.load(Ordering::Relaxed), Instant::now());
        while shared.busy.load(Ordering::SeqCst) > 0 {
            std::thread::sleep(Duration::from_millis(50));
            if cancel.load(Ordering::Relaxed) {
                shared.stop.store(true, Ordering::Relaxed);
            }
            let elapsed = tick.elapsed();
            if elapsed >= TICK {
                let now = shared.done.load(Ordering::Relaxed);
                let speed = (now.saturating_sub(tick_bytes) as f64 / elapsed.as_secs_f64()) as u64;
                let segments = shared.snapshot().iter().map(|p| Segment { start: p.start, end: p.end, done: p.pos - p.start }).collect();
                if !cancel.load(Ordering::Relaxed) {
                    sink.send(Event::Progress { id: f.id, run: f.run, done: now, speed, segments });
                }
                (tick, tick_bytes) = (Instant::now(), now);
            }
            if saved.elapsed() >= SAVE_EVERY {
                let _ = save(part, total, &shared.snapshot());
                saved = Instant::now();
            }
        }
    });
    let parts = shared.snapshot();
    let whole = parts.iter().all(|p| p.pos >= p.end);
    if whole {
        let _ = std::fs::remove_file(record_path(part));
        return Ok(Some(total));
    }
    save(part, total, &parts).map_err(|e| disk(part, e))?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let why = shared.failed.lock().unwrap_or_else(PoisonError::into_inner).take().unwrap_or_else(|| "a part didn't finish".into());
    // Each part is where its record says: another try (or Resume) carries on from there.
    Err(NetError::Retry(why))
}

/// One connection: parts one after another until none is left (or the download stops). `first`
/// is a part already assigned to it, with its answer open. True when it left the busy count
/// itself (see [`leave_to_others`]).
fn connection(sh: &Shared, mut first: Option<(usize, ureq::Body)>) -> bool {
    let mut handle = match OpenOptions::new().write(true).open(sh.file) {
        Ok(h) => h,
        Err(e) => {
            sh.fail(disk(sh.file, e).to_string());
            return false;
        }
    };
    loop {
        if sh.stop.load(Ordering::Relaxed) {
            return false;
        }
        let (i, mut body) = match first.take() {
            Some((i, b)) => (i, Some(b)),
            None => match sh.claim() {
                Some(i) => (i, None),
                None => return false,
            },
        };
        let mut tries = 0;
        loop {
            let at = sh.parts().get(i).map(|p| p.pos);
            match fetch_part(sh, &mut handle, i, body.take()) {
                Ok(()) => break,
                Err(_) if sh.stop.load(Ordering::Relaxed) => return false,
                Err(e) => {
                    // Other connections are working: this one gives its part back to them and
                    // goes, as a download manager does when a server allows fewer connections.
                    if leave_to_others(sh) {
                        log::info!("part {i}: {e}; left to the other connections");
                        // Turned away before a byte: the server allows only the ones still going.
                        if sh.parts().get(i).map(|p| p.pos) == at {
                            remember_cap(sh.url, sh.busy.load(Ordering::SeqCst).max(1));
                        }
                        if let Some(p) = sh.parts().get_mut(i) {
                            p.taken = false;
                        }
                        return true;
                    }
                    tries += 1;
                    if tries > RETRIES {
                        sh.fail(e.to_string());
                        return false;
                    }
                    log::info!("part {i}: {e}; trying again");
                    std::thread::sleep(Duration::from_millis(500 << tries));
                }
            }
        }
    }
}

/// Leave the busy count when another connection is still busy (true), else stay in it.
fn leave_to_others(sh: &Shared) -> bool {
    if sh.busy.fetch_sub(1, Ordering::SeqCst) > 1 {
        true
    } else {
        sh.busy.fetch_add(1, Ordering::SeqCst);
        false
    }
}

/// Fetch part `i` from where it is to its end (which may move closer as others take some of it).
/// `body` is an answer already open at the part's start.
fn fetch_part(sh: &Shared, handle: &mut File, i: usize, body: Option<ureq::Body>) -> Result<(), NetError> {
    let Some(p) = sh.parts().get(i).copied() else { return Ok(()) };
    if p.left() == 0 {
        return Ok(());
    }
    let mut body = match body {
        Some(b) => b,
        None => {
            let resp = sh
                .agent
                .get(sh.url)
                .header("Accept-Encoding", "identity")
                .header("Range", format!("bytes={}-{}", p.pos, p.end - 1))
                .config()
                .timeout_recv_response(Some(PART_ANSWER))
                .build()
                .call()
                .map_err(http_error)?;
            if resp.status().as_u16() != 206 {
                return Err(NetError::Http("the server stopped sending parts of the file".into()));
            }
            let start = header(&resp, "content-range").and_then(content_range).map(|(s, _)| s);
            if start != Some(p.pos) {
                return Err(NetError::Http("the server sent a different part of the file".into()));
            }
            resp.into_body()
        }
    };
    let mut reader = body.with_config().limit(u64::MAX).reader();
    let mut buf = vec![0u8; CHUNK];
    loop {
        if sh.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => return Err(NetError::Http("a connection closed early".into())),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(NetError::Http(format!("a connection broke ({e})"))),
        };
        // Reserve the bytes this connection may write, then write them outside the lock.
        let (at, take, last) = {
            let mut parts = sh.parts();
            let Some(p) = parts.get_mut(i) else { return Ok(()) };
            let take = (n as u64).min(p.left());
            let at = p.pos;
            p.pos += take;
            (at, take, p.pos >= p.end)
        };
        if take > 0 {
            let written = handle.seek(SeekFrom::Start(at)).and_then(|_| handle.write_all(buf.get(..take as usize).unwrap_or_default()));
            if let Err(e) = written {
                if let Some(p) = sh.parts().get_mut(i) {
                    p.pos = at;
                }
                sh.fail(disk(sh.file, e).to_string());
                return Ok(());
            }
            sh.done.fetch_add(take, Ordering::Relaxed);
        }
        if last {
            if let Some(p) = sh.parts().get_mut(i) {
                p.taken = false;
            }
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_into_even_parts_of_at_least_a_mebibyte() {
        let parts = split(10 * MIN_PART, 4);
        assert_eq!(parts.len(), 4);
        assert_eq!(parts.first().map(|p| p.start), Some(0));
        assert_eq!(parts.last().map(|p| p.end), Some(10 * MIN_PART));
        assert!(parts.windows(2).all(|w| w[0].end == w[1].start));
        assert_eq!(split(3 * MIN_PART / 2, 8).len(), 2, "no part under a mebibyte");
        assert_eq!(split(10, 8).len(), 1);
    }

    #[test]
    fn records_round_trip_and_bad_ones_are_ignored() {
        let dir = std::env::temp_dir().join(format!("downloadcraft-parts-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let part = dir.join("f.dcpart");
        let mut parts = split(4 * MIN_PART, 4);
        parts[1].pos += 1234;
        save(&part, 4 * MIN_PART, &parts).unwrap();
        assert_eq!(load(&part), Some((4 * MIN_PART, parts.clone())));
        for bad in [r#"{"total":10,"parts":[[0,5,5],[6,10,6]]}"#, r#"{"total":10,"parts":[[0,10,11]]}"#, r#"{"total":10,"parts":[]}"#, "junk"] {
            std::fs::write(record_path(&part), bad).unwrap();
            assert_eq!(load(&part), None, "{bad}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
