//! One HTTP(S) transfer, resumable with byte ranges, on one connection or several (`segmented`).

pub mod segmented;

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use downloadcraft_model::job::PARTIAL_SUFFIX;
use downloadcraft_model::{DownloadId, Event, names};

use crate::{NetError, Sink};

/// How often progress is reported.
pub(crate) const TICK: Duration = Duration::from_millis(250);
/// The read buffer.
pub(crate) const CHUNK: usize = 256 * 1024;
/// The most `name (n)` numbers tried for a taken name.
const MAX_NUMBERED: u32 = 999;
/// A connection that sends nothing for this long is stalled, and is dropped (curl's
/// `--speed-time` with a speed limit of zero).
pub(crate) const STALL: Duration = Duration::from_secs(30);
/// Tries in a row after a stalled or dropped connection before the download fails; any bytes
/// that arrive start the count again.
const RETRIES: u32 = 5;
/// The first wait before trying again; it doubles each time, up to [`RETRY_MAX`].
const RETRY_FIRST: Duration = if cfg!(test) { Duration::from_millis(50) } else { Duration::from_secs(2) };
const RETRY_MAX: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(crate) struct Fetch {
    pub id: DownloadId,
    pub run: u64,
    pub url: String,
    pub folder: PathBuf,
    pub file_name: String,
    pub may_rename: bool,
    /// The most connections for this file.
    pub connections: u8,
}

/// The agent every transfer shares: certificates always checked (rustls with Mozilla's roots),
/// up to 10 redirects, the system's proxy settings from the environment, and HTTP errors as
/// errors.
pub fn agent() -> ureq::Agent {
    agent_with(STALL)
}

/// [`agent`] with a stall time of its own (tests).
pub(crate) fn agent_with(stall: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_per_read(Some(stall))
        .max_redirects(10)
        .user_agent(concat!("DownloadCraft/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(config)
}

/// Run one transfer to its end, reporting through `sink`. A cancelled transfer leaves without a
/// word: the engine already knows it stopped.
///
/// A stalled or dropped connection is tried again by itself, from where the partial file is, with
/// exponential back-off ([`RETRY_FIRST`], doubling, at most [`RETRY_MAX`]); the count starts again
/// whenever bytes arrived since the last try.
pub(crate) fn fetch(agent: &ureq::Agent, f: &Fetch, cancel: &AtomicBool, sink: &Sink) {
    let (id, run) = (f.id, f.run);
    let mut f = f.clone();
    let mut tries = 0u32;
    loop {
        let before = on_disk(&f);
        let result = transfer(agent, &mut f, cancel, sink);
        match result {
            Ok(Some(done)) => return sink.send(Event::Finished { id, run, done }),
            Ok(None) => return,
            Err(e) if cancel.load(Ordering::Relaxed) => return log::debug!("download {} stopped: {e}", id.0),
            Err(NetError::Retry(why)) => {
                if on_disk(&f) > before {
                    tries = 0;
                }
                tries += 1;
                if tries > RETRIES {
                    return sink.send(Event::Failed { id, run, error: format!("{why}; Resume carries on") });
                }
                let wait = RETRY_FIRST.saturating_mul(1 << (tries - 1).min(8)).min(RETRY_MAX);
                log::info!("download {}: {why}; trying again in {}s ({tries} of {RETRIES})", id.0, wait.as_secs());
                if !sleep(wait, cancel) {
                    return;
                }
            }
            Err(e) => return sink.send(Event::Failed { id, run, error: e.to_string() }),
        }
    }
}

/// Bytes on disk for `f` so far (its partial file), to tell whether a try got anywhere.
fn on_disk(f: &Fetch) -> u64 {
    let part = part_of(&f.folder, &names::sanitize(&f.file_name));
    match segmented::load(&part) {
        Some((_, parts)) => parts.iter().map(|p| p.pos - p.start).sum(),
        None => std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0),
    }
}

/// Wait `d`, a tenth of a second at a time: false when the download was stopped meanwhile.
fn sleep(d: Duration, cancel: &AtomicBool) -> bool {
    let until = Instant::now() + d;
    while Instant::now() < until {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100).min(until.saturating_duration_since(Instant::now())));
    }
    !cancel.load(Ordering::Relaxed)
}

fn disk(path: &Path, e: std::io::Error) -> NetError {
    match e.kind() {
        std::io::ErrorKind::StorageFull | std::io::ErrorKind::QuotaExceeded => {
            NetError::Disk(format!("the disk is full ({}); free some space, then Resume carries on", path.display()))
        }
        _ => NetError::Disk(format!("{}: {e}", path.display())),
    }
}

/// Fails when the disk holding `folder` hasn't `need` bytes free. A disk that can't say how much
/// it has is let through: a full disk still fails clearly when writing ([`disk`]).
pub(crate) fn room_for(folder: &Path, need: u64) -> Result<(), NetError> {
    match fs4::available_space(folder) {
        Ok(free) => check_room(need, free),
        Err(e) => {
            log::debug!("free space of {} unknown: {e}", folder.display());
            Ok(())
        }
    }
}

pub(crate) fn check_room(need: u64, free: u64) -> Result<(), NetError> {
    if need <= free {
        return Ok(());
    }
    let size = downloadcraft_model::format::bytes;
    Err(NetError::Disk(format!("not enough room on the disk: {} needed, {} free", size(need), size(free))))
}

fn part_of(folder: &Path, name: &str) -> PathBuf {
    folder.join(format!("{name}{PARTIAL_SUFFIX}"))
}

/// `name`, or `name (n)` for the first `n` whose file and partial file are both free.
fn free_name(folder: &Path, name: &str) -> String {
    let taken = |n: &str| folder.join(n).exists() || part_of(folder, n).exists();
    if !taken(name) {
        return name.to_string();
    }
    (1..=MAX_NUMBERED).map(|n| names::numbered(name, n)).find(|n| !taken(n)).unwrap_or_else(|| name.to_string())
}

/// `Content-Range: bytes 100-199/1000` → (100, Some(1000)); `/*` is an unknown total.
pub fn content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let rest = v.trim().strip_prefix("bytes")?.trim_start();
    let (range, total) = rest.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    let total = if total.trim() == "*" { None } else { Some(total.trim().parse().ok()?) };
    Some((start, total))
}

fn header<'a>(resp: &'a ureq::http::Response<ureq::Body>, name: &str) -> Option<&'a str> {
    resp.headers().get(name).and_then(|v| v.to_str().ok())
}

fn http_error(e: ureq::Error) -> NetError {
    match e {
        // Worth another try: a busy server, a timeout, a connection that went.
        ureq::Error::StatusCode(code @ (408 | 429)) => NetError::Retry(format!("the server is busy ({code})")),
        ureq::Error::StatusCode(code @ 500..=599) => NetError::Retry(format!("the server had a problem ({code})")),
        ureq::Error::StatusCode(code) => NetError::Http(match code {
            401 | 403 => format!("the server refused ({code}); the link may need a sign-in or have expired"),
            404 | 410 => format!("the file isn't there any more ({code})"),
            _ => format!("the server answered {code}"),
        }),
        ureq::Error::Timeout(_) => NetError::Retry("the server didn't answer in time".into()),
        ureq::Error::Io(e) => NetError::Retry(format!("the connection failed ({e})")),
        other => NetError::Http(other.to_string()),
    }
}

/// `Ok(Some(bytes))` when the file is complete, `Ok(None)` when cancelled.
fn transfer(agent: &ureq::Agent, f: &mut Fetch, cancel: &AtomicBool, sink: &Sink) -> Result<Option<u64>, NetError> {
    std::fs::create_dir_all(&f.folder).map_err(|e| disk(&f.folder, e))?;
    let mut name = names::sanitize(&f.file_name);
    let mut part = part_of(&f.folder, &name);
    // Parts left by a download on several connections carry on where each one was.
    if !f.may_rename
        && let Some((total, parts)) = segmented::load(&part)
        && std::fs::metadata(&part).is_ok_and(|m| m.len() == total)
    {
        let done = parts.iter().map(|p| p.pos - p.start).sum();
        sink.send(Event::Started { id: f.id, run: f.run, total: Some(total), resumable: true, file_name: name.clone(), done });
        let got = segmented::run(agent, f, &part, total, parts, None, cancel, sink)?;
        return got.map(|done| finish(f, &name, &part, done)).transpose();
    }
    let _ = std::fs::remove_file(segmented::record_path(&part));
    // A first start never carries on from a partial file someone else left under that name.
    let mut offset = if f.may_rename { 0 } else { std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) };

    let mut req = agent.get(&f.url).header("Accept-Encoding", "identity");
    if offset > 0 {
        req = req.header("Range", format!("bytes={offset}-"));
    } else if f.connections > 1 {
        // Asked as a range from the start, the answer says whether the server takes ranges
        // (206), so the file can be fetched on several connections.
        req = req.header("Range", "bytes=0-");
    }
    let mut resp = match req.call() {
        Ok(r) => r,
        // Asked for bytes past the end: the partial file is already whole.
        Err(ureq::Error::StatusCode(416)) if offset > 0 => return finish(f, &name, &part, offset).map(Some),
        Err(e) => return Err(http_error(e)),
    };
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let status = resp.status().as_u16();
    let length: Option<u64> = header(&resp, "content-length").and_then(|v| v.trim().parse().ok());
    let total = if status == 206 {
        let (start, total) = header(&resp, "content-range").and_then(content_range).unwrap_or((u64::MAX, None));
        if start != offset {
            return Err(NetError::Http("the server sent a different part of the file; remove it and add the link again".into()));
        }
        total.or_else(|| length.map(|l| l.saturating_add(offset)))
    } else {
        // A whole file: whatever was there is replaced.
        offset = 0;
        length
    };
    let resumable = status == 206 || header(&resp, "accept-ranges").is_some_and(|v| v.eq_ignore_ascii_case("bytes"));
    if f.may_rename {
        if let Some(n) = header(&resp, "content-disposition").and_then(names::from_disposition) {
            name = n;
        }
        name = free_name(&f.folder, &name);
        part = part_of(&f.folder, &name);
    }
    if let Some(t) = total {
        room_for(&f.folder, t.saturating_sub(offset))?;
    }
    sink.send(Event::Started { id: f.id, run: f.run, total, resumable, file_name: name.clone(), done: offset });
    // Another try carries on with this name and its partial file.
    f.file_name = name.clone();
    f.may_rename = false;

    if status == 206
        && offset == 0
        && f.connections > 1
        && let Some(t) = total.filter(|t| *t >= 2 * segmented::MIN_PART)
    {
        let parts = segmented::split(t, f.connections);
        let got = segmented::run(agent, f, &part, t, parts, Some(resp.into_body()), cancel, sink)?;
        return got.map(|done| finish(f, &name, &part, done)).transpose();
    }

    let mut file = if offset > 0 { OpenOptions::new().append(true).open(&part) } else { File::create(&part) }.map_err(|e| disk(&part, e))?;
    let mut body = resp.body_mut().with_config().limit(u64::MAX).reader();
    let mut buf = vec![0u8; CHUNK];
    let mut done = offset;
    let (mut tick, mut tick_bytes) = (Instant::now(), done);
    loop {
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                let _ = file.flush();
                let broke = format!("the connection broke at {} ({e})", downloadcraft_model::format::bytes(done));
                return Err(if resumable { NetError::Retry(broke) } else { NetError::Http(format!("{broke}; Resume starts over")) });
            }
        };
        if cancel.load(Ordering::Relaxed) {
            let _ = file.flush();
            return Ok(None);
        }
        file.write_all(buf.get(..n).unwrap_or_default()).map_err(|e| disk(&part, e))?;
        done = done.saturating_add(n as u64);
        let elapsed = tick.elapsed();
        if elapsed >= TICK {
            let speed = ((done - tick_bytes) as f64 / elapsed.as_secs_f64()) as u64;
            sink.send(Event::Progress { id: f.id, run: f.run, done, speed, segments: Vec::new() });
            (tick, tick_bytes) = (Instant::now(), done);
        }
    }
    file.flush().map_err(|e| disk(&part, e))?;
    drop(file);
    if let Some(t) = total
        && done < t
    {
        let closed = format!("the connection closed at {} of {}", downloadcraft_model::format::bytes(done), downloadcraft_model::format::bytes(t));
        return Err(if resumable { NetError::Retry(closed) } else { NetError::Http(format!("{closed}; Resume starts over")) });
    }
    finish(f, &name, &part, done).map(Some)
}

/// Move the partial file into place (under a free name, should one have appeared meanwhile).
fn finish(f: &Fetch, name: &str, part: &Path, done: u64) -> Result<u64, NetError> {
    let mut target = f.folder.join(name);
    if target.exists() {
        target = f.folder.join(free_name(&f.folder, name));
    }
    std::fs::rename(part, &target).map_err(|e| disk(&target, e))?;
    Ok(done)
}

/// The record of a download's parts beside its partial file (deleted with it).
pub(crate) fn parts_record(part: &Path) -> PathBuf {
    segmented::record_path(part)
}

#[cfg(test)]
pub(crate) mod parse_tests {
    #[test]
    fn content_ranges() {
        assert_eq!(super::content_range("bytes 100-199/1000"), Some((100, Some(1000))));
        assert_eq!(super::content_range("bytes 0-0/*"), Some((0, None)));
        assert_eq!(super::content_range("bytes */1000"), None);
        assert_eq!(super::content_range("items 1-2/3"), None);
        assert_eq!(super::content_range("bytes 5-9/x"), None);
    }
}
