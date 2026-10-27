//! DownloadCraft keeps itself up to date, on Windows, macOS and Linux.
//!
//! Nothing else updates it, so every installed copy does this itself: a signed release follows
//! the published releases, and a build of the unsigned test-build workflow (which sets
//! `DOWNLOADCRAFT_UPDATE_CHANNEL=test` when compiling) follows the test builds too. A developer's
//! build never changes itself: it isn't run from anywhere [`detect`] recognises.
//! `DOWNLOADCRAFT_NO_AUTO_UPDATE=1` turns it off for a run. Ported from MailCraft's updater
//! (docs/updates.md).
//!
//! 1. **Check** a minute after start (ten seconds when a download was left part way), then about
//!    hourly with jitter, asking GitHub with the last `ETag` so an unchanged list costs nothing
//!    against its anonymous quota. A failed check is tried again after 5 minutes, doubling.
//! 2. **Pick** the newest release newer than this one (of this build's channel), and its
//!    installer of the kind this copy was installed from ([`Kind`]).
//! 3. **Download** it into `<data>/updates`, resumably, keeping it only if its SHA-256 and size
//!    are the release's (`downloadcraft_net::updates`).
//! 4. **Say so**: a bar in the window with Restart now (`crates/ui-egui/src/updates.rs`).
//!
//! Settings › About can also ask at once ([`Ask::Check`]): the answer is only shown, nothing is
//! downloaded, until Download and install ([`Ask::Install`]) downloads it the same way and restarts
//! into it once it's checked.
//! 5. **Install** as DownloadCraft closes, or at once with Restart now: a detached helper waits
//!    for this process to end, puts the new version in place and (when restarting) opens it
//!    (`self_update/install.rs`). Transfers that were running are queued again, so they carry on
//!    in the new version.
//!
//! | Installed from | Installed by the helper |
//! |---|---|
//! | Windows `.msi` (per user, `%LOCALAPPDATA%\Programs`) | `msiexec /i … /passive`, no administrator |
//! | Windows portable `.zip` | unzipped over the app's folder; `DownloadCraftData` stays |
//! | macOS `.dmg` | `DownloadCraft.app` copied out of it in place of this one, where this user may write |
//! | Linux `.AppImage` | the new file renamed over this one, where this user may write |
//!
//! Anything else (`.deb`, `.rpm`, Flatpak, FreeBSD) is updated by its package manager.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use downloadcraft_net::updates::{self, Listing, Progress, Release, newer_release};
use downloadcraft_ui_egui::{UpdateCheck, UpdateState};

mod install;
#[cfg(test)]
mod tests;

/// Set to `test` at compile time by the test-build workflow; anything else is a stable build.
const CHANNEL: Option<&str> = option_env!("DOWNLOADCRAFT_UPDATE_CHANNEL");
/// The commit this was built from (the packaging scripts set it).
const BUILD_SHA: Option<&str> = option_env!("DOWNLOADCRAFT_BUILD_SHA");
/// The first check, after start; sooner when a download waits to be carried on.
const FIRST_CHECK: Duration = Duration::from_secs(60);
const FIRST_RESUME: Duration = Duration::from_secs(10);
/// Between checks (give or take [`JITTER_SECS`]).
const EVERY: Duration = Duration::from_secs(60 * 60);
const JITTER_SECS: u64 = 10 * 60;
/// After a check fails: this, doubling each time, up to [`EVERY`].
const RETRY: Duration = Duration::from_secs(5 * 60);
/// How long the version just installed isn't offered again.
const JUST_INSTALLED_FOR: u64 = 24 * 3600;
/// The marker recording it, in the data folder.
const MARKER: &str = "update-installed.txt";
/// Where downloads wait, in the data folder.
const DOWNLOADS: &str = "updates";

/// Whether updating is on for this run.
pub fn enabled() -> bool {
    std::env::var_os("DOWNLOADCRAFT_NO_AUTO_UPDATE").is_none_or(|v| v.is_empty() || v == "0")
}

/// Why this copy doesn't update itself (for Settings › About), when it doesn't.
pub fn why_not() -> &'static str {
    if !enabled() {
        "Updates are turned off for this run."
    } else {
        "This copy is updated by the package manager it came from, or it's a build of its own."
    }
}

/// Whether this is a test build, which takes test builds (pre-releases) as well as releases.
fn test_channel() -> bool {
    CHANNEL == Some("test")
}

/// The releases this build may update to: a stable build never takes a pre-release.
fn for_channel(releases: Vec<Release>, test: bool) -> Vec<Release> {
    releases.into_iter().filter(|r| test || !r.prerelease).collect()
}

/// How this copy was installed, among the kinds the updater can replace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The per-user Windows Installer package, in `%LOCALAPPDATA%\Programs\DownloadCraft`: the exe.
    Msi(PathBuf),
    /// The Windows portable zip: the exe, with `portable.txt` beside it.
    Portable(PathBuf),
    /// `DownloadCraft.app`, somewhere this user may replace it.
    MacApp(PathBuf),
    /// The AppImage file.
    AppImage(PathBuf),
}

impl Kind {
    /// How the release names this kind's file for this machine (the end of its name).
    pub fn suffix(&self) -> String {
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else if cfg!(target_arch = "x86") {
            "x86"
        } else {
            "x64"
        };
        match self {
            Kind::Msi(_) => format!("-windows-{arch}.msi"),
            Kind::Portable(_) => format!("-windows-{arch}-portable.zip"),
            Kind::MacApp(_) => "-macos-universal.dmg".into(),
            Kind::AppImage(_) => format!("-linux-{}.AppImage", if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" }),
        }
    }
}

/// How the running copy was installed, if the updater can replace it.
pub fn detect() -> Option<Kind> {
    let exe = std::env::current_exe().ok()?;
    let exe = plain(exe.canonicalize().unwrap_or(exe));
    let recorded = recorded_install_dir();
    detect_at(&exe, &|k| if k == INSTALL_DIR { recorded.clone() } else { std::env::var_os(k).map(PathBuf::from) })
}

/// The name [`detect_at`] asks for the folder the MSI recorded.
const INSTALL_DIR: &str = "<msi install dir>";

/// The folder the MSI recorded as where it installed DownloadCraft (the user may have chosen
/// another than the default): `HKCU\Software\Learning Machines LLC\DownloadCraft`, `InstallDir`.
fn recorded_install_dir() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let mut cmd = std::process::Command::new("reg.exe");
    cmd.args(["query", r"HKCU\Software\Learning Machines LLC\DownloadCraft", "/v", "InstallDir"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    parse_reg_value(&String::from_utf8_lossy(&out.stdout), "InstallDir").map(PathBuf::from)
}

/// The value of `name` in `reg query` output (`    InstallDir    REG_SZ    C:\…\DownloadCraft\`).
fn parse_reg_value(out: &str, name: &str) -> Option<String> {
    out.lines().find_map(|l| {
        let rest = l.trim_start().strip_prefix(name)?;
        let value = rest.trim_start().strip_prefix("REG_SZ")?.trim();
        (!value.is_empty()).then(|| value.trim_end_matches('\\').to_string())
    })
}

/// A path without Windows' `\\?\` prefix, which `canonicalize` adds and PowerShell's archive
/// commands don't take.
fn plain(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{unc}"));
    }
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

/// [`detect`] for an exe at `exe`, with environment variables from `env`.
fn detect_at(exe: &Path, env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<Kind> {
    let dir = exe.parent()?;
    if cfg!(windows) {
        if dir.join(crate::store::PORTABLE_MARKER).is_file() {
            return Some(Kind::Portable(exe.to_path_buf()));
        }
        // The MSI installs for this user, into the folder it records (packaging/windows/downloadcraft.wxs),
        // by default %LOCALAPPDATA%\Programs\DownloadCraft.
        let same = |a: &Path, b: &Path| {
            let (a, b) = (a.to_string_lossy(), b.to_string_lossy());
            a.trim_end_matches('\\').eq_ignore_ascii_case(b.trim_end_matches('\\'))
        };
        let installed = env(INSTALL_DIR).is_some_and(|p| same(dir, &p))
            || env("LOCALAPPDATA").is_some_and(|p| same(dir, &p.join("Programs").join("DownloadCraft")));
        return installed.then(|| Kind::Msi(exe.to_path_buf()));
    }
    if cfg!(target_os = "macos") {
        let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"))?;
        // Run from where it was downloaded, macOS runs a read-only copy of it elsewhere.
        if bundle.to_string_lossy().contains("/AppTranslocation/") {
            log::info!("updates: macOS runs this DownloadCraft from a read-only copy; moved to Applications, it updates itself");
            return None;
        }
        let parent = bundle.parent()?;
        return writable(parent).then(|| Kind::MacApp(bundle.to_path_buf()));
    }
    if cfg!(target_os = "linux") {
        let image = env("APPIMAGE").filter(|p| p.is_file())?;
        if !image.parent().is_some_and(writable) {
            log::info!("updates: {} is in a folder this user can't write, so it isn't replaced", image.display());
            return None;
        }
        return Some(Kind::AppImage(image));
    }
    None
}

/// Whether a file can be made in `dir` (made and removed again to find out).
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".downloadcraft-update-probe-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Where the updater is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Waiting,
    /// A newer version is downloading: its tag.
    Downloading { tag: String },
    /// A newer version is downloaded and checked: its tag and file.
    Ready { tag: String, file: PathBuf },
}

/// What Settings › About asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Look for a newer version now (and only say so).
    Check,
    /// Download the newer version and restart into it once it's checked.
    Install,
}

/// What looking by hand found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Manual {
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer release: its tag.
    Available(String),
    Failed(String),
}

/// What the window and the updater's thread share.
#[derive(Default)]
struct Shared {
    /// What Settings › About asked for, not yet taken.
    asked: Mutex<Option<Ask>>,
    /// What looking by hand found, and the release it found.
    manual: Mutex<Manual>,
    found: Mutex<Option<Release>>,
    /// Download and install was asked: once ready, the window restarts into it.
    install_when_ready: AtomicBool,
    status: Mutex<Status>,
    progress: Progress,
    /// The helper has been started (once per run).
    started: AtomicBool,
    /// Restart now was chosen: the new version opens once installed.
    restart: AtomicBool,
    /// DownloadCraft is closing: a download stops between reads (what came is kept).
    stop: AtomicBool,
    /// Wakes the thread from its wait between checks as DownloadCraft closes.
    asleep: Mutex<()>,
    wake: Condvar,
    /// Wakes the window when where the updater is changes.
    waker: Mutex<Option<Waker>>,
}

/// What wakes the window.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The updater, shared by the window (to show and start it) and its own thread.
#[derive(Clone)]
pub struct Updater {
    kind: Kind,
    data: PathBuf,
    shared: Arc<Shared>,
}

impl Updater {
    /// Start checking in the background, when this build updates itself and knows how this copy
    /// was installed. `data` is DownloadCraft's data folder.
    pub fn start(data: PathBuf) -> Option<Updater> {
        if !enabled() {
            return None;
        }
        let Some(kind) = detect() else {
            log::info!("updates: this copy wasn't installed in a way DownloadCraft can update by itself");
            return None;
        };
        let u = Updater { kind, data, shared: Arc::default() };
        let worker = u.clone();
        let spawned = std::thread::Builder::new().name("updates".into()).spawn(move || worker.run());
        if let Err(e) = spawned {
            log::warn!("updates: {e}");
            return None;
        }
        Some(u)
    }

    fn status(&self) -> Status {
        self.shared.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    fn set_status(&self, status: Status) {
        *self.shared.status.lock().unwrap_or_else(PoisonError::into_inner) = status;
        self.wake_window();
    }

    fn wake_window(&self) {
        let waker = self.shared.waker.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(wake) = waker {
            wake();
        }
    }

    /// What wakes the window when the update starts downloading, is ready, or goes.
    pub fn set_waker(&self, waker: Waker) {
        *self.shared.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(waker);
    }

    /// Where it is, for the window.
    pub fn state(&self) -> UpdateState {
        let version = |tag: &str| tag.trim_start_matches('v').to_string();
        match self.status() {
            Status::Waiting => UpdateState::None,
            Status::Downloading { tag } => {
                let (done, total) = self.shared.progress.get();
                UpdateState::Downloading { version: version(&tag), done, total }
            }
            Status::Ready { tag, .. } => UpdateState::Ready { version: version(&tag) },
        }
    }

    /// Ask the updater's thread for something now (Settings › About).
    pub fn ask(&self, ask: Ask) {
        if ask == Ask::Install {
            self.shared.install_when_ready.store(true, Ordering::SeqCst);
        }
        if ask == Ask::Check {
            *self.shared.manual.lock().unwrap_or_else(PoisonError::into_inner) = Manual::Checking;
        }
        *self.shared.asked.lock().unwrap_or_else(PoisonError::into_inner) = Some(ask);
        self.shared.wake.notify_all();
        self.wake_window();
    }

    /// What looking by hand found, for Settings › About.
    pub fn check_state(&self) -> UpdateCheck {
        let version = |tag: &str| tag.trim_start_matches('v').to_string();
        match self.shared.manual.lock().unwrap_or_else(PoisonError::into_inner).clone() {
            Manual::Idle => UpdateCheck::Idle,
            Manual::Checking => UpdateCheck::Checking,
            Manual::UpToDate => UpdateCheck::UpToDate,
            Manual::Available(tag) => UpdateCheck::Available { version: version(&tag) },
            Manual::Failed(why) => UpdateCheck::Failed(why),
        }
    }

    /// Download and install was asked and the new version is ready: time to restart into it.
    pub fn install_now(&self) -> bool {
        self.shared.install_when_ready.load(Ordering::SeqCst) && matches!(self.status(), Status::Ready { .. })
    }

    /// Restart now: the waiting version is installed as DownloadCraft closes and then opened.
    /// False when nothing is waiting.
    pub fn restart_after_close(&self) -> bool {
        let ready = matches!(self.status(), Status::Ready { .. });
        if ready {
            self.shared.restart.store(true, Ordering::SeqCst);
        }
        ready
    }

    /// Whether closing now installs a new version.
    pub fn installs_on_close(&self) -> bool {
        matches!(self.status(), Status::Ready { .. })
    }

    /// DownloadCraft is closing: install what's waiting, and stop a download (what came of it is
    /// carried on from next time).
    pub fn closing(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.wake.notify_all();
        if let Err(e) = self.install(self.shared.restart.load(Ordering::SeqCst)) {
            log::warn!("updates: {e}");
        }
    }

    fn run(&self) {
        let dir = self.data.join(DOWNLOADS);
        let first = std::env::var("DOWNLOADCRAFT_UPDATE_FIRST_CHECK").ok().and_then(|v| v.trim().parse::<u64>().ok()).map(Duration::from_secs);
        let first = first.unwrap_or(if has_part(&dir) { FIRST_RESUME } else { FIRST_CHECK });
        if !self.sleep(first) {
            return;
        }
        let mut etag: Option<String> = None;
        let mut failed = 0u32;
        loop {
            let asked = self.shared.asked.lock().unwrap_or_else(PoisonError::into_inner).take();
            match asked {
                Some(Ask::Check) => self.check_by_hand(),
                Some(Ask::Install) => self.install_by_hand(),
                None => {}
            }
            if asked.is_some() {
                if !self.sleep(next_wait(seed())) {
                    return;
                }
                continue;
            }
            match self.check(&mut etag) {
                Ok(()) => failed = 0,
                Err(e) => {
                    log::info!("updates: {e}");
                    failed = failed.saturating_add(1);
                    // Asked whole again next time, or "unchanged" would skip the failed download.
                    etag = None;
                }
            }
            let wait = if failed > 0 { retry_wait(failed) } else { next_wait(seed()) };
            if !self.sleep(wait) {
                return;
            }
        }
    }

    /// Wait `d`: false when DownloadCraft is closing.
    fn sleep(&self, d: Duration) -> bool {
        let deadline = std::time::Instant::now() + d;
        let mut asleep = self.shared.asleep.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if self.shared.stop.load(Ordering::SeqCst) {
                return false;
            }
            // Something asked for from Settings › About: at once.
            if self.shared.asked.lock().unwrap_or_else(PoisonError::into_inner).is_some() {
                return true;
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return true;
            }
            asleep = match self.shared.wake.wait_timeout(asleep, deadline - now) {
                Ok((g, _)) => g,
                Err(e) => e.into_inner().0,
            };
        }
    }

    /// The newest release newer than this one, asked of GitHub afresh.
    fn newest(&self) -> Result<Option<Release>, String> {
        let releases = match updates::releases_since(&self.kind.suffix(), None).map_err(|e| e.to_string())? {
            Listing::Unchanged => Vec::new(),
            Listing::Changed(list, _) => for_channel(list, test_channel()),
        };
        Ok(newer_release(&current_version(), BUILD_SHA, &releases).cloned())
    }

    /// Check for updates (Settings › About): say what there is, download nothing.
    fn check_by_hand(&self) {
        let found = match self.newest() {
            Ok(Some(r)) if r.download.is_empty() => {
                Manual::Failed(format!("{} has no {} to update with", r.tag, self.kind.suffix().trim_start_matches('-')))
            }
            Ok(Some(r)) => {
                let tag = r.tag.clone();
                *self.shared.found.lock().unwrap_or_else(PoisonError::into_inner) = Some(r);
                Manual::Available(tag)
            }
            Ok(None) => Manual::UpToDate,
            Err(e) => Manual::Failed(e),
        };
        log::info!("updates: checked by hand: {found:?}");
        *self.shared.manual.lock().unwrap_or_else(PoisonError::into_inner) = found;
        self.wake_window();
    }

    /// Download and install (Settings › About): fetch what the check found (or look again), and
    /// once it's checked the window restarts into it.
    fn install_by_hand(&self) {
        if matches!(self.status(), Status::Ready { .. }) {
            self.wake_window();
            return;
        }
        let found = self.shared.found.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let release = match found {
            Some(r) => Ok(Some(r)),
            None => self.newest(),
        };
        let result = match release {
            Ok(Some(r)) => self.download(&r),
            Ok(None) => {
                *self.shared.manual.lock().unwrap_or_else(PoisonError::into_inner) = Manual::UpToDate;
                Ok(())
            }
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            log::info!("updates: {e}");
            self.shared.install_when_ready.store(false, Ordering::SeqCst);
            *self.shared.manual.lock().unwrap_or_else(PoisonError::into_inner) = Manual::Failed(e);
        }
        self.wake_window();
    }

    /// One look at the releases, and the download of a newer one.
    fn check(&self, etag: &mut Option<String>) -> Result<(), String> {
        let releases = match updates::releases_since(&self.kind.suffix(), etag.as_deref()).map_err(|e| e.to_string())? {
            Listing::Unchanged => return Ok(()),
            Listing::Changed(list, tag) => {
                *etag = tag;
                for_channel(list, test_channel())
            }
        };
        let Some(release) = newer_release(&current_version(), BUILD_SHA, &releases) else { return Ok(()) };
        if release.download.is_empty() {
            return Err(format!("{} has no {} to update with", release.tag, self.kind.suffix().trim_start_matches('-')));
        }
        if just_installed(&self.data, &release.tag, now()) {
            return Ok(());
        }
        if let Status::Ready { tag, .. } = self.status()
            && tag == release.tag
        {
            return Ok(());
        }
        self.download(release)
    }

    /// Fetch and check `release`'s installer into the downloads folder.
    fn download(&self, release: &Release) -> Result<(), String> {
        let dir = self.data.join(DOWNLOADS);
        let expected = updates::expected_sum(release).map_err(|e| e.to_string())?;
        // An older version ready to install stays so while a newer one downloads.
        let ready = self.status();
        if !matches!(ready, Status::Ready { .. }) {
            self.set_status(Status::Downloading { tag: release.tag.clone() });
        }
        match updates::download_installer(release, &expected, &dir, &self.shared.progress, &self.shared.stop) {
            Ok(Some(file)) => {
                // Under the status's lock, so closing finds either the older version with its file
                // or this one, never an older one whose file is gone.
                let mut status = self.shared.status.lock().unwrap_or_else(PoisonError::into_inner);
                if self.shared.stop.load(Ordering::SeqCst) {
                    return Ok(());
                }
                if let Ok(list) = std::fs::read_dir(&dir) {
                    for e in list.flatten().filter(|e| e.path() != file) {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
                log::info!("updates: {} is downloaded and checked; it installs when DownloadCraft closes", release.tag);
                *status = Status::Ready { tag: release.tag.clone(), file };
                drop(status);
                self.wake_window();
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(e) => {
                self.set_status(if matches!(ready, Status::Ready { .. }) { ready } else { Status::Waiting });
                Err(e.to_string())
            }
        }
    }

    /// Install what's waiting once this process has ended; `restart` opens the new version then.
    pub fn install(&self, restart: bool) -> Result<bool, String> {
        let Status::Ready { tag, file } = self.status() else { return Ok(false) };
        if self.shared.started.swap(true, Ordering::SeqCst) {
            return Ok(false);
        }
        if let Err(e) = std::fs::write(self.data.join(MARKER), format!("{tag}\n{}\n", now())) {
            log::warn!("updates: couldn't note the update: {e}");
        }
        let log = self.data.join(DOWNLOADS).join("install.log");
        let result = install::helper(&self.kind, &file, std::process::id(), restart, &log);
        if result.is_err() {
            self.shared.started.store(false, Ordering::SeqCst);
        }
        result.map(|()| {
            log::info!("updates: installing {tag}{}", if restart { " and restarting" } else { " as DownloadCraft closes" });
            true
        })
    }
}

/// The version this build counts as: its own, or `DOWNLOADCRAFT_UPDATE_PRETEND_VERSION` (to try
/// the updater against the newest release).
fn current_version() -> String {
    std::env::var("DOWNLOADCRAFT_UPDATE_PRETEND_VERSION")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| v.split('.').count() == 3 && v.split('.').all(|p| !p.is_empty() && p.len() < 7 && p.bytes().all(|b| b.is_ascii_digit())))
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

/// Whether a download was left part way in `dir`.
fn has_part(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|l| l.flatten().any(|e| e.file_name().to_string_lossy().ends_with(".part")))
}

/// Whether `tag` was installed by the updater within [`JUST_INSTALLED_FOR`] of `now`.
fn just_installed(data: &Path, tag: &str, now: u64) -> bool {
    let Ok(text) = std::fs::read_to_string(data.join(MARKER)) else { return false };
    let mut lines = text.lines();
    let (Some(done), Some(at)) = (lines.next(), lines.next().and_then(|a| a.trim().parse::<u64>().ok())) else { return false };
    done.trim() == tag && now.checked_sub(at).is_some_and(|age| age < JUST_INSTALLED_FOR)
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Something different each time, to spread checks out.
fn seed() -> u64 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    u64::from(t) ^ u64::from(std::process::id()).rotate_left(17)
}

/// The wait before the next check: [`EVERY`], give or take [`JITTER_SECS`], from `seed`.
fn next_wait(seed: u64) -> Duration {
    let spread = seed % (2 * JITTER_SECS + 1);
    Duration::from_secs((EVERY.as_secs() + spread).saturating_sub(JITTER_SECS))
}

/// The wait after `failed` checks in a row failed: [`RETRY`], doubling, at most [`EVERY`].
fn retry_wait(failed: u32) -> Duration {
    RETRY.saturating_mul(1 << failed.saturating_sub(1).min(6)).min(EVERY)
}
