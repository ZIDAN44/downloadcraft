//! DownloadCraft data model.
//!
//! Pure data and algorithms shared by every layer: a download and its state, what kind of link
//! it came from (direct HTTP, FTP, a torrent, a magnet link, MEGA), safe file names, the jobs the
//! engine asks the network layer to run and the events it gets back, and how sizes, speeds and
//! times left are written. Nothing here does I/O, so it builds for wasm.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod format;
pub mod job;
pub mod names;
pub mod probe;
pub mod source;
pub mod torrent;

use serde::{Deserialize, Serialize};

pub use job::{Event, Job, Segment};
pub use source::SourceKind;

/// A download's id, unique in one list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DownloadId(pub u64);

/// Where a download is in its life.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Waiting for a free slot (`Prefs::max_active`).
    #[default]
    Queued,
    /// Transferring now.
    Downloading,
    /// Stopped by the user (or by quitting); resumes from where it was.
    Paused,
    /// The whole file is in its folder.
    Completed,
    /// Stopped by an error (`Download::error` says which); Resume tries again.
    Failed,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Queued => "Queued",
            State::Downloading => "Downloading",
            State::Paused => "Paused",
            State::Completed => "Completed",
            State::Failed => "Failed",
        }
    }
    /// Can it be started (or resumed)?
    pub fn startable(self) -> bool {
        matches!(self, State::Paused | State::Failed)
    }
    /// Is it waiting for or holding a transfer slot?
    pub fn active(self) -> bool {
        matches!(self, State::Queued | State::Downloading)
    }
}

/// One entry in the download list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Download {
    pub id: DownloadId,
    pub url: String,
    pub kind: SourceKind,
    /// The file's name in `folder` (sanitised; the server may rename it on the first start).
    pub file_name: String,
    pub folder: String,
    /// The whole size, when the server said.
    #[serde(default)]
    pub total: Option<u64>,
    /// Bytes on disk so far.
    #[serde(default)]
    pub done: u64,
    #[serde(default)]
    pub state: State,
    #[serde(default)]
    pub error: Option<String>,
    /// Whether the server takes byte ranges, so a pause doesn't start over.
    #[serde(default)]
    pub resumable: bool,
    /// When it was added, in seconds since 1970 (the app's clock; 0 when unknown).
    #[serde(default)]
    pub added: u64,
    /// Bytes per second over the last moments (not saved).
    #[serde(skip)]
    pub speed: u64,
    /// The transfer run whose events count (not saved); see [`Job::Fetch`].
    #[serde(skip)]
    pub run: u64,
    /// The parts being fetched at once, as the last progress report had them (not saved: the
    /// network layer keeps its own record beside the partial file).
    #[serde(skip)]
    pub segments: Vec<Segment>,
    /// A video page downloaded through yt-dlp: the quality chosen.
    #[serde(default)]
    pub video: Option<VideoChoice>,
    /// The finished file is no longer where it was saved (deleted or moved), as the app last
    /// looked (not saved: looked at again each run).
    #[serde(skip)]
    pub missing: bool,
    /// A torrent or magnet link: the files picked from it.
    #[serde(default)]
    pub torrent: Option<TorrentChoice>,
    /// A torrent's bytes sent to others by this run.
    #[serde(skip)]
    pub uploaded: u64,
    /// A torrent's bytes sent to others over every run (its share ratio is this over its size).
    #[serde(default)]
    pub sent: u64,
    /// `sent` when this run started (not saved).
    #[serde(skip)]
    pub sent_before: u64,
    /// What a running torrent last reported: peers, trackers, files (not saved).
    #[serde(skip)]
    pub live: Option<Box<torrent::TorrentLive>>,
    /// A finished torrent still being shared with others (not saved: seeding stops on closing).
    #[serde(skip)]
    pub seeding: bool,
}

/// The files picked from a torrent: indexes into its list ([`probe::TorrentFile`]); empty for all.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TorrentChoice {
    #[serde(default)]
    pub files: Vec<usize>,
    /// The torrent's files, once it has reported them, so they can be picked while it's paused.
    #[serde(default)]
    pub list: Vec<probe::TorrentFile>,
}

/// The quality chosen for a video download.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoChoice {
    /// The yt-dlp format selector.
    pub selector: String,
    /// What the user picked (`1080p`).
    pub label: String,
}

impl Download {
    /// How far along, 0..=1, when the size is known.
    pub fn fraction(&self) -> Option<f32> {
        match self.total {
            Some(0) => Some(1.0),
            Some(t) => Some((self.done as f64 / t as f64).clamp(0.0, 1.0) as f32),
            None if self.state == State::Completed => Some(1.0),
            None => None,
        }
    }
    /// Seconds left at the current speed.
    pub fn eta(&self) -> Option<u64> {
        let left = self.total?.checked_sub(self.done)?;
        if self.state != State::Downloading || self.speed == 0 {
            return None;
        }
        Some(left.div_ceil(self.speed))
    }
}

/// Light or dark.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Whatever the system uses.
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::System, Theme::Light, Theme::Dark];
    pub fn label(self) -> &'static str {
        match self {
            Theme::System => "System",
            Theme::Light => "Light",
            Theme::Dark => "Dark",
        }
    }
}

/// Settings that belong with the list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// Where new downloads go (the app fills in the system's Downloads folder).
    #[serde(default)]
    pub folder: String,
    /// How many transfer at once; the rest wait in the queue.
    #[serde(default = "default_max_active")]
    pub max_active: usize,
    /// How many connections one download may open, each fetching its own part of the file
    /// (when the server takes byte ranges).
    #[serde(default = "default_connections")]
    pub connections: u8,
    #[serde(default)]
    pub theme: Theme,
    /// Whether Remove also deletes the file, as last chosen in its question.
    #[serde(default)]
    pub delete_files_on_remove: bool,
    /// The languages whose subtitles come with a video, as typed (`en, es`); empty for none.
    #[serde(default)]
    pub subtitles: String,
    /// The interface language: `auto` (the system's) or a language code (`de`, `pt-br`).
    #[serde(default = "default_language")]
    pub language: String,
    /// A finished torrent is shared until it has sent this many times its size (0: not at all),
    /// while DownloadCraft is open.
    #[serde(default = "default_seed_ratio")]
    pub seed_ratio: f32,
    /// Ports, limits, proxy and IP filter for torrents.
    #[serde(default)]
    pub torrent: torrent::TorrentPrefs,
}

fn default_seed_ratio() -> f32 {
    1.0
}

/// The highest seeding ratio anyone can ask for.
pub const MAX_SEED_RATIO: f32 = 10.0;

fn default_language() -> String {
    "auto".into()
}

fn default_max_active() -> usize {
    3
}

fn default_connections() -> u8 {
    8
}

/// The most transfers at once anyone can ask for.
pub const MAX_ACTIVE_LIMIT: usize = 16;
/// The most connections one download may open.
pub const MAX_CONNECTIONS: u8 = 16;

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            folder: String::new(),
            max_active: default_max_active(),
            connections: default_connections(),
            theme: Theme::System,
            delete_files_on_remove: false,
            subtitles: String::new(),
            language: default_language(),
            seed_ratio: default_seed_ratio(),
            torrent: torrent::TorrentPrefs::default(),
        }
    }
}

#[cfg(test)]
mod tests;
