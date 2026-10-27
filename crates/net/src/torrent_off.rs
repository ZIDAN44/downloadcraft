//! Torrents where librqbit doesn't build: FreeBSD, where one of its crates
//! (`librqbit-dualstack-sockets` 0.7) asks for a socket option the system doesn't have
//! (`SO_BINDTODEVICE`). The same calls as `torrent.rs`, each saying so: a torrent or magnet link
//! fails with that reason, and there's no torrent engine to report on.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;

use downloadcraft_model::probe::Probe;
use downloadcraft_model::torrent::{EngineStatus, TorrentPrefs};
use downloadcraft_model::{DownloadId, Event};

use crate::Sink;

/// Why torrents don't download here.
const WHY: &str = "torrents aren't available on FreeBSD yet";

/// A torrent to download (`Job::FetchTorrent`), as `torrent.rs` takes it.
pub struct TorrentFetch {
    pub id: DownloadId,
    pub run: u64,
    pub url: String,
    pub folder: PathBuf,
    pub file_name: String,
    pub files: Vec<usize>,
    pub first: bool,
    pub seed_ratio: f32,
    pub sent: u64,
    pub priority: Vec<usize>,
    pub down_kib: u32,
    pub up_kib: u32,
    pub connections: u8,
    pub picks: Receiver<(Vec<usize>, Vec<usize>)>,
}

pub fn set_dir(_dir: PathBuf) {}

pub fn configure(_prefs: TorrentPrefs) {}

pub fn status() -> Option<EngineStatus> {
    None
}

pub fn probe_torrent(_url: &str) -> Probe {
    Probe::Failed(WHY.into())
}

pub fn fetch(spec: &TorrentFetch, _cancel: &AtomicBool, sink: &Sink) {
    sink.send(Event::Failed { id: spec.id, run: spec.run, error: WHY.into() });
}
