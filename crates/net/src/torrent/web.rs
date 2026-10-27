//! Web seeds (BEP 19): web servers that hold a torrent's files, as its `url-list` names them.
//! librqbit doesn't fetch from them, so before it starts, DownloadCraft's own HTTP downloader
//! fetches each file picked that isn't on disk yet, on several connections, from the first web
//! seed that has it. librqbit then checks those files piece by piece as it adds the torrent,
//! fetches whatever is missing or damaged from peers, and shares them.
//!
//! A file is only ever written as `name.dcpart` and renamed into place once whole, so a file at
//! its final path was either fetched in full here or written by librqbit; neither is fetched
//! again. While a proxy is set nothing is fetched here (it would go around the proxy), and speed
//! limits don't apply to these fetches yet.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use downloadcraft_model::probe::TorrentFile;
use downloadcraft_model::torrent::{Tracker, TrackerStatus, web_seed_url};
use downloadcraft_model::{Event, names};

use super::TorrentFetch;
use crate::{Sink, http};

/// What fetching from the web seeds came to.
pub(super) struct Outcome {
    /// The web seeds and how each did.
    pub seeds: Vec<Tracker>,
    /// Every file asked for is now on disk.
    pub all: bool,
    /// Something was written (librqbit must be let write over it, to check it).
    pub wrote: bool,
}

/// The files of a torrent as a web seed is asked for them.
pub(super) struct Plan<'a> {
    pub seeds: Vec<String>,
    pub name: &'a str,
    pub single: bool,
    /// Where the torrent's files go (its folder, or the folder holding its one file).
    pub out: &'a Path,
    /// The files to fetch, in order (those to download first, then the rest).
    pub files: Vec<&'a TorrentFile>,
}

/// Fetch `plan`'s files that aren't on disk, reporting `(bytes so far, speed, the seeds)` as it
/// goes. `None` when it was stopped.
pub(super) fn fetch(spec: &TorrentFetch, plan: &Plan, cancel: &AtomicBool, report: &mut dyn FnMut(u64, u64, &[Tracker])) -> Option<Outcome> {
    let mut seeds: Vec<Tracker> =
        plan.seeds.iter().map(|u| Tracker { url: u.clone(), status: TrackerStatus::Waiting, ..Tracker::default() }).collect();
    let mut done = 0u64;
    let mut all = true;
    let mut wrote = false;
    for f in &plan.files {
        let parts: Vec<&str> = f.path.split('/').collect();
        let target: PathBuf = parts.iter().fold(plan.out.to_path_buf(), |p, part| p.join(part));
        if target.exists() {
            done = done.saturating_add(f.size);
            continue;
        }
        // A name the downloader would change (it makes names safe for every system) is left to
        // peers, so the torrent finds its file where it expects it.
        let Some(file_name) = parts.last().filter(|n| names::sanitize(n) == **n) else {
            all = false;
            continue;
        };
        let folder = target.parent().map_or_else(|| plan.out.to_path_buf(), Path::to_path_buf);
        let mut got = false;
        for (i, seed) in plan.seeds.iter().enumerate() {
            if matches!(seeds.get(i).map(|s| &s.status), Some(TrackerStatus::Failing(_))) {
                continue;
            }
            let path_parts = if plan.single { &[][..] } else { &parts[..] };
            let url = web_seed_url(seed, if plan.single { file_name } else { plan.name }, path_parts, plan.single);
            match one(spec, &url, &folder, file_name, f.size, cancel, &mut |n, speed| report(done.saturating_add(n), speed, &seeds)) {
                One::Done => {
                    got = true;
                    wrote = true;
                    if let Some(s) = seeds.get_mut(i) {
                        s.status = TrackerStatus::Working;
                        s.completed = Some(s.completed.unwrap_or(0).saturating_add(1));
                    }
                    break;
                }
                One::Stopped => return None,
                One::Failed(why) => {
                    log::info!("web seed {seed}: {why}");
                    if let Some(s) = seeds.get_mut(i) {
                        s.status = TrackerStatus::Failing(why);
                    }
                }
            }
        }
        if got {
            done = done.saturating_add(f.size);
        } else {
            all = false;
        }
        report(done, 0, &seeds);
        if seeds.iter().all(|s| matches!(s.status, TrackerStatus::Failing(_))) {
            // No web seed answers: the rest is left to peers.
            all = false;
            break;
        }
    }
    Some(Outcome { seeds, all, wrote })
}

enum One {
    Done,
    Stopped,
    Failed(String),
}

/// Fetch one file from `url` into `folder/file_name` with the HTTP downloader, which carries on
/// from a partial file and reports progress; `size` is what the torrent says it is.
fn one(spec: &TorrentFetch, url: &str, folder: &Path, file_name: &str, size: u64, cancel: &AtomicBool, progress: &mut dyn FnMut(u64, u64)) -> One {
    let job = http::Fetch {
        id: spec.id,
        run: spec.run,
        url: url.to_string(),
        folder: folder.to_path_buf(),
        file_name: file_name.to_string(),
        may_rename: false,
        connections: spec.connections.max(1),
    };
    let (tx, rx) = channel();
    let sink = Sink::new(tx, Arc::new(|| {}));
    let agent = http::agent();
    std::thread::scope(|s| {
        // The sink goes with the thread, so the channel closes when it ends.
        s.spawn(move || http::fetch(&agent, &job, cancel, &sink));
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(Event::Progress { done, speed, .. }) => progress(done, speed),
                Ok(Event::Finished { done, .. }) if done == size => return One::Done,
                Ok(Event::Finished { done, .. }) => {
                    // Not the file the torrent describes: away with it, peers will bring the
                    // right one.
                    let _ = std::fs::remove_file(folder.join(file_name));
                    return One::Failed(format!("it served {done} bytes where the torrent has {size}"));
                }
                Ok(Event::Failed { error, .. }) => return One::Failed(error),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return if cancel.load(Ordering::Relaxed) { One::Stopped } else { One::Failed("it stopped answering".into()) };
                }
            }
        }
    })
}
