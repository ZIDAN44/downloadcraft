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
    FetchVideo { id: DownloadId, run: u64, url: String, folder: String, file_name: String, selector: String, connections: u8 },
    /// Stop the transfer of `id` (whatever run it is on); the partial file stays for a resume.
    Stop { id: DownloadId },
    /// Delete the partial file of `id`, which is being removed, once its transfer has stopped;
    /// with `also_file`, the finished file goes to the Recycle Bin or Trash too.
    DropPartial { id: DownloadId, folder: String, file_name: String, also_file: bool },
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
}

impl Event {
    pub fn id(&self) -> DownloadId {
        match self {
            Event::Started { id, .. } | Event::Progress { id, .. } | Event::Finished { id, .. } | Event::Failed { id, .. } => *id,
        }
    }
    pub fn run(&self) -> u64 {
        match self {
            Event::Started { run, .. } | Event::Progress { run, .. } | Event::Finished { run, .. } | Event::Failed { run, .. } => *run,
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
