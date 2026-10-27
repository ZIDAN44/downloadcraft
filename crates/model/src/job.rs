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
    /// used instead, and a taken name gets a number.
    Fetch { id: DownloadId, run: u64, url: String, folder: String, file_name: String, may_rename: bool },
    /// Stop the transfer of `id` (whatever run it is on); the partial file stays for a resume.
    Stop { id: DownloadId },
    /// Delete the partial file of `id`, which is being removed, once its transfer has stopped
    /// (the finished file is never touched).
    DropPartial { id: DownloadId, folder: String, file_name: String },
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

/// The suffix of a file while it downloads.
pub const PARTIAL_SUFFIX: &str = ".dcpart";
