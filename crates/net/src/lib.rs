//! DownloadCraft network layer.
//!
//! The [`Worker`] runs the engine's [`Job`]s: each `Fetch` is a thread of its own that streams
//! the file into `name.dcpart` beside where it will end up, carries on from that partial file
//! with an HTTP byte range when the server takes one (RFC 9110 §14), and renames it into place
//! when the whole file is there. Progress comes back as [`Event`]s through a channel.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod http;
pub mod probe;
pub mod torrent;
pub mod updates;
pub mod video;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

use downloadcraft_model::job::PARTIAL_SUFFIX;
use downloadcraft_model::{DownloadId, Event, Job, SourceKind};

/// Look at a link before it's added (blocking: a request, or yt-dlp reading a video page).
pub fn probe(url: &str) -> downloadcraft_model::probe::Probe {
    match downloadcraft_model::source::classify(url) {
        Ok(SourceKind::Http) => probe::probe_file(url.trim()),
        Ok(SourceKind::VideoPage) => video::probe_video(url.trim()),
        Ok(SourceKind::Torrent | SourceKind::Magnet) => torrent::probe_torrent(&downloadcraft_model::source::normalize(url)),
        Ok(k) => downloadcraft_model::probe::Probe::Failed(k.not_yet().unwrap_or("not supported yet").to_string()),
        Err(e) => downloadcraft_model::probe::Probe::Failed(e.to_string()),
    }
}

/// Put `path` in the Recycle Bin or Trash; where there's none, delete it.
pub fn trash_file(path: &std::path::Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    trash::delete(path).or_else(|e| {
        log::info!("no Recycle Bin for {} ({e}); deleting it", path.display());
        std::fs::remove_file(path).map_err(|e| e.to_string())
    })
}

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("{0}")]
    Http(String),
    /// A failure that may well not happen again (a stalled or dropped connection, a timeout, a
    /// busy server): the transfer tries again by itself.
    #[error("{0}")]
    Retry(String),
    #[error("{0}")]
    Disk(String),
}

/// Where events go: a channel, and a nudge for whoever reads it (the UI repaints).
#[derive(Clone)]
pub struct Sink {
    tx: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Sink {
    pub fn new(tx: Sender<Event>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Sink { tx, wake }
    }
    pub(crate) fn send(&self, ev: Event) {
        if self.tx.send(ev).is_ok() {
            (self.wake)();
        }
    }
}

struct Transfer {
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<()>,
    /// A torrent's new choices of files.
    picks: Option<std::sync::mpsc::Sender<Vec<usize>>>,
}

/// Runs jobs on threads of their own.
pub struct Worker {
    sink: Sink,
    agent: ureq::Agent,
    transfers: HashMap<DownloadId, Transfer>,
}

impl Worker {
    pub fn new(sink: Sink) -> Self {
        Worker { sink, agent: http::agent(), transfers: HashMap::new() }
    }

    pub fn submit(&mut self, job: Job) {
        self.transfers.retain(|_, t| !t.handle.is_finished());
        match job {
            Job::Fetch { id, run, url, folder, file_name, may_rename, connections } => {
                // A transfer of the same download that is still on its way out finishes first,
                // so two never write one partial file.
                let before = self.stop(id);
                let cancel = Arc::new(AtomicBool::new(false));
                let spec = http::Fetch { id, run, url, folder: PathBuf::from(folder), file_name, may_rename, connections };
                let (agent, sink, flag) = (self.agent.clone(), self.sink.clone(), cancel.clone());
                let spawned = std::thread::Builder::new().name(format!("download-{}", id.0)).spawn(move || {
                    if let Some(h) = before {
                        let _ = h.join();
                    }
                    http::fetch(&agent, &spec, &flag, &sink);
                });
                match spawned {
                    Ok(handle) => {
                        self.transfers.insert(id, Transfer { cancel, handle, picks: None });
                    }
                    Err(e) => self.sink.send(Event::Failed { id, run, error: format!("couldn't start: {e}") }),
                }
            }
            Job::FetchVideo { id, run, url, folder, file_name, selector, connections, subtitles } => {
                let before = self.stop(id);
                let cancel = Arc::new(AtomicBool::new(false));
                let spec = video::VideoFetch { id, run, url, folder: PathBuf::from(folder), file_name, selector, connections, subtitles };
                let (sink, flag) = (self.sink.clone(), cancel.clone());
                let spawned = std::thread::Builder::new().name(format!("video-{}", id.0)).spawn(move || {
                    if let Some(h) = before {
                        let _ = h.join();
                    }
                    video::fetch(&spec, &flag, &sink);
                });
                match spawned {
                    Ok(handle) => {
                        self.transfers.insert(id, Transfer { cancel, handle, picks: None });
                    }
                    Err(e) => self.sink.send(Event::Failed { id, run, error: format!("couldn't start: {e}") }),
                }
            }
            Job::FetchTorrent { id, run, url, folder, file_name, files, first, seed_ratio, sent } => {
                let before = self.stop(id);
                let cancel = Arc::new(AtomicBool::new(false));
                let (tx, picks) = std::sync::mpsc::channel();
                let spec = torrent::TorrentFetch { id, run, url, folder: PathBuf::from(folder), file_name, files, first, seed_ratio, sent, picks };
                let (sink, flag) = (self.sink.clone(), cancel.clone());
                let spawned = std::thread::Builder::new().name(format!("torrent-{}", id.0)).spawn(move || {
                    if let Some(h) = before {
                        let _ = h.join();
                    }
                    torrent::fetch(&spec, &flag, &sink);
                });
                match spawned {
                    Ok(handle) => {
                        self.transfers.insert(id, Transfer { cancel, handle, picks: Some(tx) });
                    }
                    Err(e) => self.sink.send(Event::Failed { id, run, error: format!("couldn't start: {e}") }),
                }
            }
            Job::PickFiles { id, files } => {
                if let Some(tx) = self.transfers.get(&id).and_then(|t| t.picks.as_ref()) {
                    let _ = tx.send(files);
                }
            }
            Job::Configure { torrent } => torrent::configure(torrent),
            // It stays listed until its thread is gone, so a resume waits for it.
            Job::Stop { id } => {
                if let Some(t) = self.transfers.get(&id) {
                    t.cancel.store(true, Ordering::Relaxed);
                }
            }
            Job::DropPartial { id, folder, file_name, also_file, torrent } => {
                let before = self.stop(id);
                let folder = PathBuf::from(folder);
                let part = folder.join(format!("{file_name}{PARTIAL_SUFFIX}"));
                let stem = file_name.rsplit_once('.').map_or(file_name.as_str(), |(s, _)| s).to_string();
                let _ = std::thread::Builder::new().name("drop-partial".into()).spawn(move || {
                    if let Some(h) = before {
                        let _ = h.join();
                    }
                    // Ours, and what yt-dlp leaves part way (`name.f137.mp4.part`, `name.mp4.ytdl`).
                    // A torrent's files are its own, written in place: only `also_file` takes them.
                    let mut gone = if torrent { Vec::new() } else { vec![http::parts_record(&part), part] };
                    if !torrent && let Ok(list) = std::fs::read_dir(&folder) {
                        gone.extend(list.flatten().map(|e| e.path()).filter(|p| {
                            let n = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                            n.starts_with(&format!("{stem}.")) && (n.ends_with(".part") || n.ends_with(".ytdl") || n.contains(".part-Frag"))
                        }));
                    }
                    for f in gone {
                        if let Err(e) = std::fs::remove_file(&f)
                            && e.kind() != std::io::ErrorKind::NotFound
                        {
                            log::warn!("couldn't delete {}: {e}", f.display());
                        }
                    }
                    if also_file && let Err(e) = trash_file(&folder.join(&file_name)) {
                        log::warn!("couldn't delete {file_name}: {e}");
                    }
                });
            }
        }
    }

    /// Tell `id`'s transfer to stop; its thread, to wait on before touching its file again.
    fn stop(&mut self, id: DownloadId) -> Option<JoinHandle<()>> {
        let t = self.transfers.remove(&id)?;
        t.cancel.store(true, Ordering::Relaxed);
        Some(t.handle)
    }

    /// Stop every transfer (quitting); their partial files stay for next time.
    pub fn stop_all(&mut self) {
        for t in self.transfers.values() {
            t.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Transfers still running.
    pub fn running(&self) -> usize {
        self.transfers.values().filter(|t| !t.handle.is_finished()).count()
    }
}

#[cfg(test)]
mod tests;
