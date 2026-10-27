//! A running torrent's details, as the details pane shows them: built from librqbit's numbers
//! about once a second. Speeds of single peers aren't kept by librqbit, so they're worked out
//! here from how much each has sent since the last look.

use std::collections::HashMap;
use std::sync::PoisonError;
use std::time::Instant;

use downloadcraft_model::probe::TorrentFile;
use downloadcraft_model::torrent::{MAX_PEERS_SHOWN, Peer, TorrentLive};
use librqbit::ManagedTorrent;

use super::scrape::Trackers;

/// What the details carry from one look to the next.
pub(super) struct Watch {
    since: Instant,
    seeding_since: Option<Instant>,
    /// Each peer's bytes received and sent at the last look.
    before: HashMap<String, (u64, u64)>,
    last: Instant,
    pub peer_limit: u32,
    /// The files have been sent (in the first report).
    told_files: bool,
    /// What was on disk at the first look, so this run's bytes can be told even once librqbit's
    /// count starts again (it does when a torrent completes).
    have_at_start: Option<u64>,
    /// The web seeds, as fetching from them went.
    pub web: Vec<downloadcraft_model::torrent::Tracker>,
    /// The files to download first are done.
    pub first_done: bool,
}

impl Watch {
    pub fn new(peer_limit: u32) -> Self {
        Watch {
            since: Instant::now(),
            seeding_since: None,
            before: HashMap::new(),
            last: Instant::now(),
            peer_limit,
            told_files: false,
            web: Vec::new(),
            first_done: false,
            have_at_start: None,
        }
    }

    pub fn seeding(&mut self) {
        self.seeding_since.get_or_insert_with(Instant::now);
    }

    /// The details now. `size` is the bytes of the files picked.
    pub fn look(&mut self, t: &ManagedTorrent, files: &[TorrentFile], size: u64, trackers: &Trackers) -> TorrentLive {
        let st = t.stats();
        let now = Instant::now();
        let secs = now.duration_since(self.last).as_secs_f64().max(0.001);
        self.last = now;
        let live = st.live.as_ref();
        let snap = live.map(|l| &l.snapshot);
        let mut peers: Vec<Peer> = Vec::new();
        let mut seen = HashMap::new();
        if let Some(state) = t.live() {
            for (address, p) in state.per_peer_stats_snapshot(Default::default()).peers {
                let (down, up) = (p.counters.fetched_bytes, p.counters.uploaded_bytes);
                let (d0, u0) = self.before.get(&address).copied().unwrap_or((down, up));
                let rate = |now: u64, then: u64| (now.saturating_sub(then) as f64 / secs) as u64;
                let connection =
                    match p.conn_kind.and_then(|k| serde_json::to_value(k).ok()).as_ref().and_then(|v| v.as_str().map(str::to_string)).as_deref() {
                        Some("utp") => "µTP".to_string(),
                        Some("socks") => "SOCKS".to_string(),
                        _ => "TCP".to_string(),
                    };
                peers.push(Peer {
                    address: address.clone(),
                    connection,
                    client: p.client_name.unwrap_or_default().chars().take(60).collect(),
                    down_speed: rate(down, d0),
                    up_speed: rate(up, u0),
                    downloaded: down,
                    uploaded: up,
                });
                seen.insert(address, (down, up));
            }
        }
        self.before = seen;
        peers.sort_by(|a, b| (b.down_speed + b.up_speed).cmp(&(a.down_speed + a.up_speed)).then_with(|| a.address.cmp(&b.address)));
        peers.truncate(MAX_PEERS_SHOWN);
        let (pieces, piece_size) = t.with_metadata(|m| (m.info.lengths().total_pieces(), m.info.lengths().default_piece_length())).unwrap_or((0, 0));
        let mut file_done = vec![0u64; files.iter().map(|f| f.index + 1).max().unwrap_or(0)];
        for f in files {
            if let (Some(slot), Some(done)) = (file_done.get_mut(f.index), st.file_progress.get(f.index)) {
                *slot = (*done).min(f.size);
            }
        }
        let have = files.iter().filter_map(|f| file_done.get(f.index)).sum::<u64>().min(size);
        let trackers = trackers.lock().unwrap_or_else(PoisonError::into_inner).clone();
        // Counted from when it's live (before that, librqbit is still checking what's on disk).
        let this_run = match live {
            Some(_) => st.progress_bytes.saturating_sub(*self.have_at_start.get_or_insert(st.progress_bytes)),
            None => 0,
        };
        TorrentLive {
            info_hash: t.info_hash().as_string(),
            active_secs: now.duration_since(self.since).as_secs(),
            seeding_secs: self.seeding_since.map_or(0, |s| now.duration_since(s).as_secs()),
            downloaded: snap.map_or(0, |s| s.fetched_bytes).max(this_run),
            uploaded: st.uploaded_bytes,
            wasted: st.finished.then(|| snap.map_or(0, |s| s.fetched_bytes.saturating_sub(s.downloaded_and_checked_bytes))),
            down_speed: live.map_or(0, |l| l.download_speed.as_bytes()),
            up_speed: live.map_or(0, |l| l.upload_speed.as_bytes()),
            size,
            have,
            pieces,
            piece_size,
            connected: snap.map_or(0, |s| s.peer_stats.live),
            connecting: snap.map_or(0, |s| s.peer_stats.connecting),
            queued: snap.map_or(0, |s| s.peer_stats.queued),
            known: snap.map_or(0, |s| s.peer_stats.seen),
            peer_limit: self.peer_limit,
            peers,
            trackers,
            file_done,
            web_seeds: self.web.clone(),
            from_web: false,
            first_done: self.first_done,
            files: if std::mem::replace(&mut self.told_files, true) { Vec::new() } else { files.to_vec() },
        }
    }
}
