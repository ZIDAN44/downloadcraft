//! The helper programs video downloads use: yt-dlp (reads video pages; Unlicense, its release
//! builds GPLv3+), FFmpeg (puts separate video and audio together; GPL) and Deno (the JavaScript
//! runtime yt-dlp solves YouTube's challenges with; MIT). None is bundled:
//! each is found on the system's `PATH`, or fetched on request into DownloadCraft's own tools
//! folder from its official GitHub releases and kept only when its SHA-256 matches the checksums
//! file published with it. They run as separate programs.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use downloadcraft_model::probe::{Tool, ToolState, VideoTools};

use crate::NetError;

/// Where fetched tools are kept (the app sets it to `<data>/tools`).
static DIR: OnceLock<PathBuf> = OnceLock::new();
/// A fetch going on or failed, over what's on disk: yt-dlp, FFmpeg, Deno.
static FETCHING: Mutex<[Option<ToolState>; 3]> = Mutex::new([None, None, None]);

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

pub fn deno() -> Option<PathBuf> {
    find("deno")
}

/// The Deno DownloadCraft fetched, for yt-dlp's `--js-runtimes` (one on the `PATH` it finds itself).
pub fn own_deno() -> Option<PathBuf> {
    dir().map(|d| d.join(exe("deno"))).filter(|p| p.is_file())
}

/// The folder holding the FFmpeg DownloadCraft fetched, for yt-dlp's `--ffmpeg-location`.
pub fn own_ffmpeg_dir() -> Option<&'static Path> {
    dir().filter(|d| d.join(exe("ffmpeg")).is_file())
}

/// Where the tools stand.
pub fn status() -> VideoTools {
    let [y, f, d] = FETCHING.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let on_disk = |found: bool| if found { ToolState::Ready } else { ToolState::Missing };
    VideoTools {
        ytdlp: y.unwrap_or_else(|| on_disk(ytdlp().is_some())),
        ffmpeg: f.unwrap_or_else(|| on_disk(ffmpeg().is_some())),
        deno: d.unwrap_or_else(|| on_disk(deno().is_some())),
    }
}

/// yt-dlp's FFmpeg build for this system (from `yt-dlp/FFmpeg-Builds`): a zip on Windows, an
/// `.tar.xz` on Linux. There is none for macOS, where Homebrew has it (`brew install ffmpeg`).
fn ffmpeg_asset() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "ffmpeg-master-latest-win64-gpl.zip",
        ("windows", "aarch64") => "ffmpeg-master-latest-winarm64-gpl.zip",
        ("windows", "x86") => "ffmpeg-master-latest-win32-gpl.zip",
        ("linux", "x86_64") => "ffmpeg-master-latest-linux64-gpl.tar.xz",
        ("linux", "aarch64") => "ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
        _ => return None,
    })
}

/// Deno's build for this system (from `denoland/deno`).
fn deno_asset() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "deno-x86_64-pc-windows-msvc.zip",
        ("windows", "aarch64") => "deno-aarch64-pc-windows-msvc.zip",
        ("linux", "x86_64") => "deno-x86_64-unknown-linux-gnu.zip",
        ("linux", "aarch64") => "deno-aarch64-unknown-linux-gnu.zip",
        ("macos", "x86_64") => "deno-x86_64-apple-darwin.zip",
        ("macos", "aarch64") => "deno-aarch64-apple-darwin.zip",
        _ => return None,
    })
}

/// Whether `tool` can be fetched here (else the system's package manager has it).
pub fn can_fetch(tool: Tool) -> bool {
    match tool {
        Tool::YtDlp => ytdlp_asset().is_some(),
        Tool::Ffmpeg => ffmpeg_asset().is_some(),
        Tool::Deno => deno_asset().is_some(),
    }
}

/// Whether FFmpeg can be fetched here (see [`ffmpeg_asset`]).
pub fn can_fetch_ffmpeg() -> bool {
    can_fetch(Tool::Ffmpeg)
}

/// Fetch `tool` into the tools folder (blocking).
pub fn fetch(tool: Tool) -> Result<PathBuf, NetError> {
    match tool {
        Tool::YtDlp => fetch_ytdlp(),
        Tool::Ffmpeg => fetch_ffmpeg(),
        Tool::Deno => fetch_deno(),
    }
}

fn set(tool: Tool, state: Option<ToolState>) {
    let mut f = FETCHING.lock().unwrap_or_else(PoisonError::into_inner);
    let at = match tool {
        Tool::YtDlp => 0,
        Tool::Ffmpeg => 1,
        Tool::Deno => 2,
    };
    if let Some(slot) = f.get_mut(at) {
        *slot = state;
    }
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

/// The checksums file at `url`, and the SHA-256 `pick` finds in it for `file`.
fn expected(url: &str, file: &str, pick: impl Fn(&str) -> Option<String>) -> Result<String, NetError> {
    let mut resp = agent().get(url).call().map_err(|e| NetError::Http(e.to_string()))?;
    let text = resp.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|e| NetError::Http(e.to_string()))?;
    pick(&text).ok_or_else(|| NetError::Http(format!("the checksums don't list {file}")))
}

/// The one SHA-256 in a checksum file for a single asset, in `sha256sum`'s form (`hash  name`) or
/// PowerShell's `Get-FileHash` list (`Hash      : HASH`), as Deno publishes them.
pub fn single_sum(text: &str) -> Option<String> {
    text.split(|c: char| c.is_whitespace() || c == ':')
        .find(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
}

/// Fetch, check and put `asset` from `base` (a release's download address) in place as `target`;
/// `sums` lists the checksums of all the release's files.
fn fetch_checked(base: &str, sums: &str, asset: &str, staged: &Path) -> Result<(), NetError> {
    fetch_checked_with(&format!("{base}/{sums}"), base, asset, staged, |t| crate::updates::sum_for(t, asset))
}

fn fetch_checked_with(sums_url: &str, base: &str, asset: &str, staged: &Path, pick: impl Fn(&str) -> Option<String>) -> Result<(), NetError> {
    let want = expected(sums_url, asset, pick)?;
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
    set(Tool::YtDlp, Some(ToolState::Fetching));
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
    set(Tool::YtDlp, result.as_ref().err().map(|e| ToolState::Failed(e.to_string())));
    result
}

/// Fetch FFmpeg (yt-dlp's FFmpeg build, its `ffmpeg` and `ffprobe`) into the tools folder
/// (blocking).
pub fn fetch_ffmpeg() -> Result<PathBuf, NetError> {
    set(Tool::Ffmpeg, Some(ToolState::Fetching));
    let result: Result<PathBuf, NetError> = (|| {
        let Some(asset) = ffmpeg_asset() else {
            return Err(NetError::Http("install FFmpeg with the system's package manager (on a Mac, `brew install ffmpeg`)".into()));
        };
        let dir = dir().ok_or_else(|| NetError::Disk("no tools folder".into()))?;
        std::fs::create_dir_all(dir).map_err(|e| NetError::Disk(e.to_string()))?;
        let staged = dir.join(format!("{asset}.part"));
        fetch_checked("https://github.com/yt-dlp/FFmpeg-Builds/releases/latest/download", "checksums.sha256", asset, &staged)?;
        let programs = [exe("ffmpeg"), exe("ffprobe")];
        let wanted: Vec<&str> = programs.iter().map(String::as_str).collect();
        let unpacked = if asset.ends_with(".zip") { unzip_programs(&staged, dir, &wanted) } else { untar_programs(&staged, dir, &wanted) };
        let _ = std::fs::remove_file(&staged);
        unpacked?;
        Ok(dir.join(exe("ffmpeg")))
    })();
    set(Tool::Ffmpeg, result.as_ref().err().map(|e| ToolState::Failed(e.to_string())));
    result
}

/// Fetch Deno's newest release build into the tools folder (blocking), checked against the
/// checksum file published beside it.
pub fn fetch_deno() -> Result<PathBuf, NetError> {
    set(Tool::Deno, Some(ToolState::Fetching));
    let result: Result<PathBuf, NetError> = (|| {
        let asset = deno_asset().ok_or_else(|| NetError::Http("Deno has no build for this system".into()))?;
        let dir = dir().ok_or_else(|| NetError::Disk("no tools folder".into()))?;
        std::fs::create_dir_all(dir).map_err(|e| NetError::Disk(e.to_string()))?;
        let base = "https://github.com/denoland/deno/releases/latest/download";
        let staged = dir.join(format!("{asset}.part"));
        fetch_checked_with(&format!("{base}/{asset}.sha256sum"), base, asset, &staged, single_sum)?;
        let deno = exe("deno");
        let unpacked = unzip_programs(&staged, dir, &[deno.as_str()]);
        let _ = std::fs::remove_file(&staged);
        unpacked?;
        Ok(dir.join(deno))
    })();
    set(Tool::Deno, result.as_ref().err().map(|e| ToolState::Failed(e.to_string())));
    result
}

/// Take the programs named `wanted` (in the archive's `bin/`) out of an `.tar.xz` into `dir`, with
/// the system's `tar` (GNU tar, with xz, is on every Linux desktop).
fn untar_programs(archive: &Path, dir: &Path, wanted: &[&str]) -> Result<(), NetError> {
    let out = dir.join("ffmpeg-unpack");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|e| NetError::Disk(e.to_string()))?;
    let patterns: Vec<String> = wanted.iter().map(|w| format!("*/bin/{w}")).collect();
    let status = std::process::Command::new("tar")
        .arg("-xJf")
        .arg(archive)
        .arg("-C")
        .arg(&out)
        .arg("--wildcards")
        .args(&patterns)
        .stdin(std::process::Stdio::null())
        .status()
        .map_err(|e| NetError::Disk(format!("tar didn't start ({e}); install FFmpeg with the system's package manager")))?;
    let mut found = 0;
    if status.success() {
        for top in std::fs::read_dir(&out).map_err(|e| NetError::Disk(e.to_string()))?.flatten() {
            for name in wanted {
                let from = top.path().join("bin").join(name);
                if from.is_file() && from.metadata().is_ok_and(|m| m.len() <= MAX_TOOL) {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = std::fs::set_permissions(&from, std::fs::Permissions::from_mode(0o755));
                    }
                    std::fs::rename(&from, dir.join(name)).map_err(|e| NetError::Disk(e.to_string()))?;
                    found += 1;
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&out);
    if found == wanted.len() { Ok(()) } else { Err(NetError::Disk("the FFmpeg archive didn't have its programs".into())) }
}

/// Take the files named `wanted` (wherever they are inside) out of the zip into `dir`.
fn unzip_programs(zip: &Path, dir: &Path, wanted: &[&str]) -> Result<(), NetError> {
    let file = File::open(zip).map_err(|e| NetError::Disk(e.to_string()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| NetError::Disk(format!("the zip can't be read ({e})")))?;
    let mut found = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| NetError::Disk(e.to_string()))?;
        let name = entry.name().map(|n| n.rsplit('/').next().unwrap_or("").to_string()).unwrap_or_default();
        if !wanted.contains(&name.as_str()) || entry.size() > MAX_TOOL {
            continue;
        }
        let staged = dir.join(format!("{name}.part"));
        let mut out = File::create(&staged).map_err(|e| NetError::Disk(e.to_string()))?;
        std::io::copy(&mut (&mut entry).take(MAX_TOOL), &mut out).map_err(|e| NetError::Disk(e.to_string()))?;
        drop(out);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(|e| NetError::Disk(e.to_string()))?;
        }
        std::fs::rename(&staged, dir.join(&name)).map_err(|e| NetError::Disk(e.to_string()))?;
        found += 1;
    }
    if found == wanted.len() { Ok(()) } else { Err(NetError::Disk("the zip didn't have its programs".into())) }
}
