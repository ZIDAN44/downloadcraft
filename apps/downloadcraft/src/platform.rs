//! What the window asks of the system: a folder picker, opening files, showing a file selected in
//! the file manager, whether a file is still there, looking at links, and video tools.
//!
//! Show in folder, as browsers do it:
//! - Windows: `explorer.exe /select,"<path>"` (the quotes after the comma, so paths with spaces
//!   and commas work).
//! - macOS: `open -R <path>`, which reveals it in Finder.
//! - Linux and the BSDs: the freedesktop File Manager D-Bus interface's `ShowItems` with the
//!   file's `file://` address (Nautilus, Dolphin, Nemo, Caja, Thunar… implement it), through
//!   `dbus-send`; where nothing answers, the folder opens instead.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use downloadcraft_engine::model::probe::{Probe, Tool, VideoTools};
use downloadcraft_net::video::tools;
use downloadcraft_ui_egui::Services;

/// The most links looked at and remembered in one run.
const MAX_PROBES: usize = 500;

pub struct Platform {
    /// What each link turned out to be (`Pending` while it's being looked at).
    probes: Arc<Mutex<HashMap<String, Probe>>>,
    /// Repaints the window when something looked at in the background is known.
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Platform {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        Platform { probes: Arc::default(), wake }
    }
}

fn open_path(path: &Path) {
    if let Err(e) = open::that_detached(path) {
        log::warn!("couldn't open {}: {e}", path.display());
    }
}

/// `path` as a `file://` address, each byte outside the unreserved set and `/` percent-encoded.
#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
pub fn file_uri(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Show `path` selected in the file manager; false when that couldn't be done.
#[cfg(windows)]
fn reveal_in_file_manager(path: &Path) -> bool {
    use std::os::windows::process::CommandExt;
    // Explorer reads `/select,` then the path; its exit code is 1 even when it worked.
    let arg = format!("/select,\"{}\"", path.display());
    std::process::Command::new("explorer.exe").raw_arg(arg).spawn().is_ok()
}

#[cfg(target_os = "macos")]
fn reveal_in_file_manager(path: &Path) -> bool {
    std::process::Command::new("open").arg("-R").arg(path).spawn().is_ok()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn reveal_in_file_manager(path: &Path) -> bool {
    let status = std::process::Command::new("dbus-send")
        .args(["--session", "--print-reply", "--dest=org.freedesktop.FileManager1", "--type=method_call"])
        .args(["/org/freedesktop/FileManager1", "org.freedesktop.FileManager1.ShowItems"])
        .arg(format!("array:string:{}", file_uri(path)))
        .arg("string:")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    status.is_ok_and(|s| s.success())
}

impl Services for Platform {
    fn pick_folder(&self, start: &str) -> Option<String> {
        let mut d = rfd::FileDialog::new().set_title("Choose a download folder");
        if !start.is_empty() && Path::new(start).is_dir() {
            d = d.set_directory(start);
        }
        d.pick_folder().map(|p| p.to_string_lossy().into_owned())
    }

    fn open(&self, path: &Path) {
        open_path(path);
    }

    fn reveal(&self, path: &Path) {
        if !reveal_in_file_manager(path)
            && let Some(dir) = path.parent()
        {
            open_path(dir);
        }
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn probe(&self, url: &str) -> Probe {
        let mut probes = self.probes.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(p) = probes.get(url) {
            return p.clone();
        }
        if probes.len() >= MAX_PROBES {
            return Probe::Failed("too many links to look at in one go".into());
        }
        probes.insert(url.to_string(), Probe::Pending);
        drop(probes);
        let (cache, wake, link) = (self.probes.clone(), self.wake.clone(), url.to_string());
        let spawned = std::thread::Builder::new().name("probe".into()).spawn(move || {
            let found = downloadcraft_net::probe(&link);
            cache.lock().unwrap_or_else(PoisonError::into_inner).insert(link, found);
            wake();
        });
        if spawned.is_err() {
            return Probe::Failed("couldn't look at the link".into());
        }
        Probe::Pending
    }

    fn video_tools(&self) -> VideoTools {
        tools::status()
    }

    fn fetch_tool(&self, tool: Tool) {
        let (wake, probes) = (self.wake.clone(), self.probes.clone());
        let _ = std::thread::Builder::new().name("fetch-tool".into()).spawn(move || {
            let result = tools::fetch(tool);
            if let Err(e) = &result {
                log::warn!("fetching {}: {e}", tool.name());
            }
            // Video links looked at before the tool was there are looked at again; with Deno,
            // all of them, as YouTube then offers more qualities.
            probes.lock().unwrap_or_else(PoisonError::into_inner).retain(|_, p| tool != Tool::Deno && !matches!(p, Probe::Failed(_)));
            wake();
        });
        (self.wake)();
    }

    fn can_fetch(&self, tool: Tool) -> bool {
        tools::can_fetch(tool)
    }
}

#[cfg(test)]
mod tests {
    use super::file_uri;
    use std::path::Path;

    #[test]
    fn file_addresses_are_encoded() {
        assert_eq!(file_uri(Path::new("/home/me/My Files/ü.zip")), "file:///home/me/My%20Files/%C3%BC.zip");
        assert_eq!(file_uri(Path::new("/tmp/a#b?.txt")), "file:///tmp/a%23b%3F.txt");
    }
}
