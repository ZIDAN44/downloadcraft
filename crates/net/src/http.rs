//! One HTTP(S) transfer, resumable with byte ranges, on one connection or several (`segmented`).

mod segmented;

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
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .max_redirects(10)
        .user_agent(concat!("DownloadCraft/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(config)
}

/// Run one transfer to its end, reporting through `sink`. A cancelled transfer leaves without a
/// word: the engine already knows it stopped.
pub(crate) fn fetch(agent: &ureq::Agent, f: &Fetch, cancel: &AtomicBool, sink: &Sink) {
    let (id, run) = (f.id, f.run);
    match transfer(agent, f, cancel, sink) {
        Ok(Some(done)) => sink.send(Event::Finished { id, run, done }),
        Ok(None) => {}
        Err(e) if cancel.load(Ordering::Relaxed) => log::debug!("download {} stopped: {e}", id.0),
        Err(e) => sink.send(Event::Failed { id, run, error: e.to_string() }),
    }
}

fn disk(path: &Path, e: std::io::Error) -> NetError {
    NetError::Disk(format!("{}: {e}", path.display()))
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
fn content_range(v: &str) -> Option<(u64, Option<u64>)> {
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
        ureq::Error::StatusCode(code) => NetError::Http(match code {
            401 | 403 => format!("the server refused ({code}); the link may need a sign-in or have expired"),
            404 | 410 => format!("the file isn't there any more ({code})"),
            429 => "the server asks to slow down (429); try again later".into(),
            500..=599 => format!("the server had a problem ({code}); try again later"),
            _ => format!("the server answered {code}"),
        }),
        ureq::Error::Timeout(_) => NetError::Http("the server didn't answer in time".into()),
        other => NetError::Http(other.to_string()),
    }
}

/// `Ok(Some(bytes))` when the file is complete, `Ok(None)` when cancelled.
fn transfer(agent: &ureq::Agent, f: &Fetch, cancel: &AtomicBool, sink: &Sink) -> Result<Option<u64>, NetError> {
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
    sink.send(Event::Started { id: f.id, run: f.run, total, resumable, file_name: name.clone(), done: offset });

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
                let next = if resumable { "Resume carries on" } else { "Resume starts over" };
                return Err(NetError::Http(format!("the connection broke at {} ({e}); {next}", downloadcraft_model::format::bytes(done))));
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
        return Err(NetError::Http(format!(
            "the connection closed at {} of {}; Resume {}",
            downloadcraft_model::format::bytes(done),
            downloadcraft_model::format::bytes(t),
            if resumable { "carries on" } else { "starts over" }
        )));
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
