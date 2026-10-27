//! DownloadCraft network layer.
//!
//! The [`Worker`] runs the engine's [`Job`]s: each `Fetch` is a thread of its own that streams
//! the file into `name.dcpart` beside where it will end up, carries on from that partial file
//! with an HTTP byte range when the server takes one (RFC 9110 §14), and renames it into place
//! when the whole file is there. Progress comes back as [`Event`]s through a channel.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod http;
pub mod updates;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

use downloadcraft_model::job::PARTIAL_SUFFIX;
use downloadcraft_model::{DownloadId, Event, Job};

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("{0}")]
    Http(String),
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
                        self.transfers.insert(id, Transfer { cancel, handle });
                    }
                    Err(e) => self.sink.send(Event::Failed { id, run, error: format!("couldn't start: {e}") }),
                }
            }
            // It stays listed until its thread is gone, so a resume waits for it.
            Job::Stop { id } => {
                if let Some(t) = self.transfers.get(&id) {
                    t.cancel.store(true, Ordering::Relaxed);
                }
            }
            Job::DropPartial { id, folder, file_name } => {
                let before = self.stop(id);
                let part = PathBuf::from(folder).join(format!("{file_name}{PARTIAL_SUFFIX}"));
                let _ = std::thread::Builder::new().name("drop-partial".into()).spawn(move || {
                    if let Some(h) = before {
                        let _ = h.join();
                    }
                    for f in [http::parts_record(&part), part] {
                        if let Err(e) = std::fs::remove_file(&f)
                            && e.kind() != std::io::ErrorKind::NotFound
                        {
                            log::warn!("couldn't delete {}: {e}", f.display());
                        }
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
