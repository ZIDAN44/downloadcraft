//! What the engine asks the network layer to do, and what it hears back.
//!
//! Every [`Job::Fetch`] carries a run number. Pausing bumps the download's run, so events from
//! a transfer that was told to stop but is still waking up are ignored rather than applied.

use serde::{Deserialize, Serialize};

use crate::DownloadId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "job", rename_all = "camelCase")]
pub enum Job {
    /// Download `url` into `folder`/`file_name`, carrying on from the partial file if there is
    /// one. When `may_rename` is set (a first start), the server's `Content-Disposition` name is
    /// used instead, and a taken name gets a number. Up to `connections` connections fetch parts
    /// of the file at once when the server takes byte ranges.
    Fetch { id: DownloadId, run: u64, url: String, folder: String, file_name: String, may_rename: bool, connections: u8 },
    /// Download a video page through yt-dlp, in the quality `selector` picks, with up to
    /// `connections` parts at once.
    /// `subtitles`: yt-dlp's `--sub-langs`, empty for none.
    FetchVideo { id: DownloadId, run: u64, url: String, folder: String, file_name: String, selector: String, connections: u8, subtitles: String },
    /// Download a torrent (a magnet link, a `.torrent` file's link or a `.torrent` file on this
    /// computer) into `folder`: `files` picks which of its files (empty for all). `first` when
    /// nothing of it has been written yet, so a file already there is never written over. Once
    /// it's all there it's shared until it has sent `seed_ratio` times its size.
    /// `sent`: what it has sent in earlier runs, which counts towards the ratio.
    FetchTorrent {
        id: DownloadId,
        run: u64,
        url: String,
        folder: String,
        file_name: String,
        files: Vec<usize>,
        first: bool,
        seed_ratio: f32,
        #[serde(default)]
        sent: u64,
        /// Files to download first (`TorrentChoice::first`).
        #[serde(default)]
        priority: Vec<usize>,
        /// This torrent's speed limits in KiB/s (0: none), and the connections a web seed's file
        /// may be fetched on.
        #[serde(default)]
        down_kib: u32,
        #[serde(default)]
        up_kib: u32,
        #[serde(default)]
        connections: u8,
    },
    /// Change the files a running torrent downloads (indexes in its list; empty for all), and
    /// those it downloads first.
    PickFiles {
        id: DownloadId,
        files: Vec<usize>,
        #[serde(default)]
        priority: Vec<usize>,
    },
    /// The torrent settings, whenever they change (and once the list is loaded).
    Configure { torrent: crate::torrent::TorrentPrefs },
    /// Stop the transfer of `id` (whatever run it is on); the partial file stays for a resume.
    Stop { id: DownloadId },
    /// Delete the partial file of `id`, which is being removed, once its transfer has stopped;
    /// with `also_file`, the finished file goes to the Recycle Bin or Trash too.
    /// A torrent's files are written in place, so for one (`torrent`) `also_file` is what takes
    /// away the part that's there.
    DropPartial {
        id: DownloadId,
        folder: String,
        file_name: String,
        also_file: bool,
        #[serde(default)]
        torrent: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum Event {
    /// The server answered: the size if it said, whether ranges work, and the name the file got.
    Started {
        id: DownloadId,
        run: u64,
        total: Option<u64>,
        resumable: bool,
        file_name: String,
        done: u64,
    },
    Progress {
        id: DownloadId,
        run: u64,
        done: u64,
        speed: u64,
        /// The parts being fetched (empty for a download on one connection).
        #[serde(default)]
        segments: Vec<Segment>,
    },
    Finished {
        id: DownloadId,
        run: u64,
        done: u64,
    },
    Failed {
        id: DownloadId,
        run: u64,
        error: String,
    },
    /// What a running torrent is doing (its peers, trackers and files), about once a second.
    Details {
        id: DownloadId,
        run: u64,
        live: Box<crate::torrent::TorrentLive>,
    },
    /// A finished torrent's sharing: bytes sent so far, and whether it's still going.
    Uploaded {
        id: DownloadId,
        run: u64,
        uploaded: u64,
        seeding: bool,
    },
}

impl Event {
    pub fn id(&self) -> DownloadId {
        match self {
            Event::Started { id, .. }
            | Event::Progress { id, .. }
            | Event::Finished { id, .. }
            | Event::Failed { id, .. }
            | Event::Uploaded { id, .. }
            | Event::Details { id, .. } => *id,
        }
    }
    pub fn run(&self) -> u64 {
        match self {
            Event::Started { run, .. }
            | Event::Progress { run, .. }
            | Event::Finished { run, .. }
            | Event::Failed { run, .. }
            | Event::Uploaded { run, .. }
            | Event::Details { run, .. } => *run,
        }
    }
}

/// One part of a file fetched on a connection of its own: bytes `start..end`, of which `done`
/// are on disk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub done: u64,
}

impl Segment {
    pub fn finished(&self) -> bool {
        self.start.saturating_add(self.done) >= self.end
    }
}

/// The suffix of a file while it downloads.
pub const PARTIAL_SUFFIX: &str = ".dcpart";
