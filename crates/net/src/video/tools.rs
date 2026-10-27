//! The helper programs video downloads use: yt-dlp (reads video pages; Unlicense, its release
//! builds GPLv3+) and FFmpeg (puts separate video and audio together; GPL). Neither is bundled:
//! each is found on the system's `PATH`, or fetched on request into DownloadCraft's own tools
//! folder from its official GitHub releases and kept only when its SHA-256 matches the checksums
//! file published with it. They run as separate programs.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use downloadcraft_model::probe::{ToolState, VideoTools};

use crate::NetError;

/// Where fetched tools are kept (the app sets it to `<data>/tools`).
static DIR: OnceLock<PathBuf> = OnceLock::new();
/// A fetch going on or failed, over what's on disk.
static FETCHING: Mutex<(Option<ToolState>, Option<ToolState>)> = Mutex::new((None, None));

/// The largest download accepted (FFmpeg's zip is about 200 MB).
const MAX_TOOL: u64 = 600 << 20;

pub fn set_dir(dir: PathBuf) {
    let _ = DIR.set(dir);
}

/// The tools folder: the one set, else `DOWNLOADCRAFT_TOOLS` (the CLI, scripts).
fn dir() -> Option<&'static Path> {
    if DIR.get().is_none()
        && let Some(d) = std::env::var_os("DOWNLOADCRAFT_TOOLS").filter(|d| !d.is_empty())
    {
        let _ = DIR.set(PathBuf::from(d));
    }
    DIR.get().map(PathBuf::as_path)
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// `name` in the tools folder, else on the `PATH`.
fn find(name: &str) -> Option<PathBuf> {
    let file = exe(name);
    if let Some(p) = dir().map(|d| d.join(&file)).filter(|p| p.is_file()) {
        return Some(p);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join(&file)).find(|p| p.is_file())
}

pub fn ytdlp() -> Option<PathBuf> {
    find("yt-dlp")
}

pub fn ffmpeg() -> Option<PathBuf> {
    find("ffmpeg")
}

/// The folder holding the FFmpeg DownloadCraft fetched, for yt-dlp's `--ffmpeg-location`.
pub fn own_ffmpeg_dir() -> Option<&'static Path> {
    dir().filter(|d| d.join(exe("ffmpeg")).is_file())
}

/// Where both tools stand.
pub fn status() -> VideoTools {
    let fetching = FETCHING.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let on_disk = |found: bool| if found { ToolState::Ready } else { ToolState::Missing };
    VideoTools { ytdlp: fetching.0.unwrap_or_else(|| on_disk(ytdlp().is_some())), ffmpeg: fetching.1.unwrap_or_else(|| on_disk(ffmpeg().is_some())) }
}

/// Whether FFmpeg can be fetched here (its builds come as zips for Windows; elsewhere the system's
/// package manager has it: `brew install ffmpeg`, `sudo apt install ffmpeg`).
pub fn can_fetch_ffmpeg() -> bool {
    cfg!(windows) && cfg!(any(target_arch = "x86_64", target_arch = "aarch64"))
}

fn set(ytdlp: bool, state: Option<ToolState>) {
    let mut f = FETCHING.lock().unwrap_or_else(PoisonError::into_inner);
    if ytdlp { f.0 = state } else { f.1 = state }
}

/// The yt-dlp build for this system.
fn ytdlp_asset() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "yt-dlp.exe",
        ("windows", "aarch64") => "yt-dlp_arm64.exe",
        ("windows", "x86") => "yt-dlp_x86.exe",
        ("macos", _) => "yt-dlp_macos",
        ("linux", "x86_64") => "yt-dlp_linux",
        ("linux", "aarch64") => "yt-dlp_linux_aarch64",
        _ => return None,
    })
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .max_redirects(10)
        .user_agent(concat!("DownloadCraft/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(config)
}

/// Fetch `url` into `dest`, at most [`MAX_TOOL`] bytes.
fn fetch_to(url: &str, dest: &Path) -> Result<(), NetError> {
    let mut resp = agent().get(url).call().map_err(|e| NetError::Http(e.to_string()))?;
    let mut body = resp.body_mut().with_config().limit(MAX_TOOL).reader();
    let mut file = File::create(dest).map_err(|e| NetError::Disk(e.to_string()))?;
    let mut buf = vec![0u8; 256 << 10];
    loop {
        let n = body.read(&mut buf).map_err(|e| NetError::Http(e.to_string()))?;
        if n == 0 {
            break;
        }
        file.write_all(buf.get(..n).unwrap_or_default()).map_err(|e| NetError::Disk(e.to_string()))?;
    }
    file.flush().map_err(|e| NetError::Disk(e.to_string()))
}

/// The checksums file at `url`, and the SHA-256 it gives `file`.
fn expected(url: &str, file: &str) -> Result<String, NetError> {
    let mut resp = agent().get(url).call().map_err(|e| NetError::Http(e.to_string()))?;
    let text = resp.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| NetError::Http(e.to_string()))?;
    crate::updates::sum_for(&text, file).ok_or_else(|| NetError::Http(format!("the checksums don't list {file}")))
}

/// Fetch, check and put `asset` from `base` (a release's download address) in place as `target`.
fn fetch_checked(base: &str, sums: &str, asset: &str, staged: &Path) -> Result<(), NetError> {
    let want = expected(&format!("{base}/{sums}"), asset)?;
    fetch_to(&format!("{base}/{asset}"), staged)?;
    let got = crate::updates::sha256_file(staged).map_err(|e| NetError::Disk(e.to_string()))?;
    if got != want {
        let _ = std::fs::remove_file(staged);
        return Err(NetError::Http(format!("{asset} didn't match its published checksum; it was deleted")));
    }
    Ok(())
}

/// Fetch yt-dlp's newest release build into the tools folder (blocking).
pub fn fetch_ytdlp() -> Result<PathBuf, NetError> {
    set(true, Some(ToolState::Fetching));
    let result: Result<PathBuf, NetError> = (|| {
        let dir = dir().ok_or_else(|| NetError::Disk("no tools folder".into()))?;
        let asset = ytdlp_asset().ok_or_else(|| NetError::Http("yt-dlp has no build for this system".into()))?;
        std::fs::create_dir_all(dir).map_err(|e| NetError::Disk(e.to_string()))?;
        let staged = dir.join(format!("{asset}.part"));
        fetch_checked("https://github.com/yt-dlp/yt-dlp/releases/latest/download", "SHA2-256SUMS", asset, &staged)?;
        let target = dir.join(exe("yt-dlp"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| NetError::Disk(e.to_string()))?;
        }
        std::fs::rename(&staged, &target).map_err(|e| NetError::Disk(e.to_string()))?;
        Ok(target)
    })();
    set(true, result.as_ref().err().map(|e| ToolState::Failed(e.to_string())));
    result
}

/// Fetch FFmpeg (Windows: yt-dlp's FFmpeg build, its `ffmpeg.exe` and `ffprobe.exe`) into the
/// tools folder (blocking).
pub fn fetch_ffmpeg() -> Result<PathBuf, NetError> {
    set(false, Some(ToolState::Fetching));
    let result: Result<PathBuf, NetError> = (|| {
        if !can_fetch_ffmpeg() {
            return Err(NetError::Http("install FFmpeg with the system's package manager".into()));
        }
        let dir = dir().ok_or_else(|| NetError::Disk("no tools folder".into()))?;
        let asset = if cfg!(target_arch = "aarch64") { "ffmpeg-master-latest-winarm64-gpl.zip" } else { "ffmpeg-master-latest-win64-gpl.zip" };
        std::fs::create_dir_all(dir).map_err(|e| NetError::Disk(e.to_string()))?;
        let staged = dir.join(format!("{asset}.part"));
        fetch_checked("https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download", "checksums.sha256", asset, &staged)?;
        let unpacked = unzip_programs(&staged, dir, &["ffmpeg.exe", "ffprobe.exe"]);
        let _ = std::fs::remove_file(&staged);
        unpacked?;
        Ok(dir.join("ffmpeg.exe"))
    })();
    set(false, result.as_ref().err().map(|e| ToolState::Failed(e.to_string())));
    result
}

/// Take the files named `wanted` (wherever they are inside) out of the zip into `dir`.
fn unzip_programs(zip: &Path, dir: &Path, wanted: &[&str]) -> Result<(), NetError> {
    let file = File::open(zip).map_err(|e| NetError::Disk(e.to_string()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| NetError::Disk(format!("the FFmpeg zip can't be read ({e})")))?;
    let mut found = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| NetError::Disk(e.to_string()))?;
        let name = entry.name().rsplit('/').next().unwrap_or("").to_string();
        if !wanted.contains(&name.as_str()) || entry.size() > MAX_TOOL {
            continue;
        }
        let staged = dir.join(format!("{name}.part"));
        let mut out = File::create(&staged).map_err(|e| NetError::Disk(e.to_string()))?;
        std::io::copy(&mut (&mut entry).take(MAX_TOOL), &mut out).map_err(|e| NetError::Disk(e.to_string()))?;
        drop(out);
        std::fs::rename(&staged, dir.join(&name)).map_err(|e| NetError::Disk(e.to_string()))?;
        found += 1;
    }
    if found == wanted.len() { Ok(()) } else { Err(NetError::Disk("the FFmpeg zip didn't have its programs".into())) }
}
