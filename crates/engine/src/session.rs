//! The session: the download list, the queue and what the network layer is asked to do.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cmd;
use crate::{CmdError, CmdResult, Registry};
use downloadcraft_model::{Download, DownloadId, Event, Job, Prefs, State};

/// Which downloads the list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    #[default]
    All,
    /// Downloading or queued.
    Active,
    Paused,
    Completed,
    Failed,
}

impl Filter {
    pub const ALL: [Filter; 5] = [Filter::All, Filter::Active, Filter::Paused, Filter::Completed, Filter::Failed];
    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All downloads",
            Filter::Active => "Downloading",
            Filter::Paused => "Paused",
            Filter::Completed => "Completed",
            Filter::Failed => "Failed",
        }
    }
    pub fn shows(self, state: State) -> bool {
        match self {
            Filter::All => true,
            Filter::Active => state.active(),
            Filter::Paused => state == State::Paused,
            Filter::Completed => state == State::Completed,
            Filter::Failed => state == State::Failed,
        }
    }
    pub fn parse(s: &str) -> Option<Filter> {
        Filter::ALL.into_iter().find(|f| serde_json::to_value(f).ok().and_then(|v| v.as_str().map(|v| v == s)).unwrap_or(false))
    }
}

/// The most downloads one list holds.
pub const MAX_DOWNLOADS: usize = 100_000;

/// What is saved between runs (`Session::save` / `Session::load`).
#[derive(Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    downloads: Vec<Download>,
    #[serde(default)]
    prefs: Prefs,
    #[serde(default)]
    next_id: u64,
}

pub struct Session {
    pub downloads: Vec<Download>,
    pub prefs: Prefs,
    pub selection: Option<DownloadId>,
    pub filter: Filter,
    /// The app's clock, in seconds since 1970 (the engine never reads one; 0 in tests).
    pub now: u64,
    /// A one-line message for the status bar (the last thing that happened or went wrong).
    pub status: String,
    next_id: u64,
    next_run: u64,
    jobs: Vec<Job>,
    registry: Registry,
    /// Set by every change worth saving; the app clears it once it has saved.
    pub dirty: bool,
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Session {
            downloads: Vec::new(),
            prefs: Prefs::default(),
            selection: None,
            filter: Filter::All,
            now: 0,
            status: String::new(),
            next_id: 1,
            next_run: 1,
            jobs: Vec::new(),
            registry: cmd::registry(),
            dirty: false,
        }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Run the command `id`. A panic inside a command is caught and comes back as an error, so a
    /// bug in one command never takes the list down.
    pub fn run(&mut self, id: &str, params: &Value) -> CmdResult {
        let Some(spec) = self.registry.get(id).cloned() else { return Err(CmdError::Unknown(id.to_string())) };
        if let Some(why) = (spec.enabled)(self) {
            return Err(CmdError::Disabled(why.to_string()));
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (spec.run)(self, params)))
            .unwrap_or_else(|_| Err(CmdError::Failed(format!("`{id}` stopped unexpectedly"))));
        match &result {
            Ok(_) => self.pump(),
            Err(e) => self.status = e.to_string(),
        }
        result
    }

    /// The jobs queued since the last call, for the network layer.
    pub fn take_jobs(&mut self) -> Vec<Job> {
        std::mem::take(&mut self.jobs)
    }

    pub(crate) fn push_job(&mut self, job: Job) {
        self.jobs.push(job);
    }

    pub fn get(&self, id: DownloadId) -> Option<&Download> {
        self.downloads.iter().find(|d| d.id == id)
    }

    pub(crate) fn get_mut(&mut self, id: DownloadId) -> Option<&mut Download> {
        self.downloads.iter_mut().find(|d| d.id == id)
    }

    pub(crate) fn new_id(&mut self) -> DownloadId {
        let id = DownloadId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// The downloads the current filter shows, newest first.
    pub fn visible(&self) -> impl Iterator<Item = &Download> {
        self.downloads.iter().rev().filter(|d| self.filter.shows(d.state))
    }

    pub fn count(&self, filter: Filter) -> usize {
        self.downloads.iter().filter(|d| filter.shows(d.state)).count()
    }

    /// Bytes per second over every transfer.
    pub fn total_speed(&self) -> u64 {
        self.downloads.iter().filter(|d| d.state == State::Downloading).fold(0u64, |a, d| a.saturating_add(d.speed))
    }

    /// Start queued downloads while there are free slots.
    pub fn pump(&mut self) {
        let max = self.prefs.max_active.clamp(1, downloadcraft_model::MAX_ACTIVE_LIMIT);
        let mut running = self.downloads.iter().filter(|d| d.state == State::Downloading).count();
        // Oldest first, as they were queued.
        let waiting: Vec<DownloadId> = self.downloads.iter().filter(|d| d.state == State::Queued).map(|d| d.id).collect();
        for id in waiting {
            if running >= max {
                break;
            }
            let run = self.next_run;
            self.next_run = self.next_run.saturating_add(1);
            let connections = self.prefs.connections.clamp(1, downloadcraft_model::MAX_CONNECTIONS);
            let Some(d) = self.get_mut(id) else { continue };
            d.state = State::Downloading;
            d.run = run;
            d.speed = 0;
            d.error = None;
            let job = match &d.video {
                Some(v) => Job::FetchVideo {
                    id,
                    run,
                    url: d.url.clone(),
                    folder: d.folder.clone(),
                    file_name: d.file_name.clone(),
                    selector: v.selector.clone(),
                    connections,
                },
                None => Job::Fetch {
                    id,
                    run,
                    url: d.url.clone(),
                    folder: d.folder.clone(),
                    file_name: d.file_name.clone(),
                    may_rename: d.done == 0 && d.total.is_none(),
                    connections,
                },
            };
            self.jobs.push(job);
            running += 1;
            self.dirty = true;
        }
    }

    /// Stop a transfer: its run ends, so late events from it are ignored.
    pub(crate) fn stop(&mut self, id: DownloadId, to: State) {
        let Some(d) = self.get_mut(id) else { return };
        let was_running = d.state == State::Downloading;
        d.state = to;
        d.speed = 0;
        d.run = 0;
        if was_running {
            self.jobs.push(Job::Stop { id });
        }
        self.dirty = true;
    }

    /// Queue running transfers again (an update is installing as the app closes): saved as
    /// queued, they start by themselves in the new version and carry on from their partial files.
    pub fn requeue_running(&mut self) {
        for d in self.downloads.iter_mut().filter(|d| d.state == State::Downloading) {
            d.state = State::Queued;
            d.speed = 0;
            d.run = 0;
            self.dirty = true;
        }
    }

    /// Note whether a finished download's file is still where it was saved (the app looks).
    pub fn set_missing(&mut self, id: DownloadId, missing: bool) {
        if let Some(d) = self.get_mut(id)
            && d.state == State::Completed
        {
            d.missing = missing;
        }
    }

    /// Apply what the network layer reports. Events for a run that has ended are dropped.
    pub fn apply(&mut self, ev: Event) {
        let id = ev.id();
        let Some(d) = self.get_mut(id) else { return };
        if d.run == 0 || d.run != ev.run() || d.state != State::Downloading {
            return;
        }
        let mut finished = None;
        match ev {
            Event::Started { total, resumable, file_name, done, .. } => {
                d.total = total;
                d.resumable = resumable;
                d.done = done;
                if !file_name.is_empty() {
                    d.file_name = downloadcraft_model::names::sanitize(&file_name);
                }
            }
            Event::Progress { done, speed, segments, .. } => {
                d.done = done;
                d.speed = speed;
                d.segments = segments;
                return;
            }
            Event::Finished { done, .. } => {
                d.done = done;
                if d.total.is_none_or(|t| t < done) {
                    d.total = Some(done);
                }
                d.state = State::Completed;
                finished = Some(format!("{} finished", d.file_name));
            }
            Event::Failed { error, .. } => {
                d.state = State::Failed;
                finished = Some(format!("{}: {error}", d.file_name));
                d.error = Some(error);
            }
        }
        self.dirty = true;
        if let Some(status) = finished {
            if let Some(d) = self.get_mut(id) {
                d.speed = 0;
                d.run = 0;
                d.segments.clear();
            }
            self.status = status;
            self.pump();
        }
    }

    /// The list and settings as JSON, to keep between runs.
    pub fn save(&self) -> Value {
        let saved = Saved { downloads: self.downloads.clone(), prefs: self.prefs.clone(), next_id: self.next_id };
        serde_json::to_value(saved).unwrap_or(Value::Null)
    }

    /// Restore what [`Session::save`] wrote. Transfers that were running when the app closed come
    /// back paused (nothing restarts by itself); queued ones stay queued.
    pub fn load(&mut self, v: Value) -> Result<(), CmdError> {
        let mut saved: Saved = serde_json::from_value(v).map_err(|e| CmdError::Params(format!("the saved list is damaged: {e}")))?;
        saved.downloads.truncate(MAX_DOWNLOADS);
        let mut top = 0;
        for d in &mut saved.downloads {
            if d.state == State::Downloading {
                d.state = State::Paused;
            }
            d.file_name = downloadcraft_model::names::sanitize(&d.file_name);
            top = top.max(d.id.0);
        }
        self.downloads = saved.downloads;
        self.prefs = saved.prefs;
        self.prefs.max_active = self.prefs.max_active.clamp(1, downloadcraft_model::MAX_ACTIVE_LIMIT);
        self.prefs.connections = self.prefs.connections.clamp(1, downloadcraft_model::MAX_CONNECTIONS);
        self.next_id = saved.next_id.max(top.saturating_add(1));
        self.selection = None;
        self.dirty = false;
        Ok(())
    }
}
