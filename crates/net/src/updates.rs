//! DownloadCraft's own updates (for the test builds' updater, `apps/downloadcraft/src/self_update.rs`):
//! the releases as GitHub lists them, picking the newer one, fetching its installer resumably and
//! proving it is the one the release lists.
//!
//! Test builds are pre-releases (`v0.2.0-test.15`), which GitHub's "latest release" leaves out, so
//! the recent ones are listed and [`newer_release`] picks. The answer is untrusted: its size is
//! capped, and only addresses on the project's own releases pages are kept. A download is accepted
//! only if its SHA-256 matches the release's record of it: GitHub's per-file `digest`, else the
//! release's `SHA256SUMS.txt`. The way MailCraft updates itself.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde_json::Value;

use crate::NetError;

/// Where DownloadCraft is published.
pub const REPO: &str = "ZIDAN44/downloadcraft";
/// The largest answer read (ten releases with their files lists).
const MAX_ANSWER: u64 = 2 << 20;
/// The largest installer accepted.
const MAX_INSTALLER: u64 = 400 << 20;
/// The largest `SHA256SUMS.txt` read.
const MAX_SUMS: u64 = 256 << 10;

type Result<T> = std::result::Result<T, NetError>;

/// One release, as far as updating goes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Release {
    /// As tagged (`v0.2.0-test.15`).
    pub tag: String,
    /// `0.2.0`, and its test build (15; 0 for none).
    pub version: String,
    pub build: u32,
    /// The commit it was built from (empty when not said).
    pub commit: String,
    /// The installer for this system (empty when the release has none).
    pub download: String,
    /// That installer's SHA-256 (lowercase hex) as GitHub records it, when it does.
    pub sha256: String,
    /// That installer's size in bytes (0 when not said).
    pub size: u64,
    /// The release's `SHA256SUMS.txt` (empty when there is none).
    pub sums: String,
}

/// What a conditional listing found.
#[derive(Debug)]
pub enum Listing {
    /// The list, and its `ETag` to ask with next time.
    Changed(Vec<Release>, Option<String>),
    /// Nothing new since the `ETag` given (GitHub doesn't count this against the quota).
    Unchanged,
}

fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .http_status_as_error(false)
        .user_agent(concat!("DownloadCraft/", env!("CARGO_PKG_VERSION")))
        .build();
    ureq::Agent::new_with_config(config)
}

fn header(resp: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
}

/// A small answer's body, refused past `max` bytes.
fn small_body(resp: &mut ureq::http::Response<ureq::Body>, max: u64) -> Result<String> {
    resp.body_mut().with_config().limit(max).read_to_string().map_err(|e| NetError::Http(e.to_string()))
}

/// The recent releases, with `suffix` picking each one's installer, unless unchanged since `etag`.
pub fn releases_since(suffix: &str, etag: Option<&str>) -> Result<Listing> {
    let mut req = agent()
        .get(&format!("https://api.github.com/repos/{REPO}/releases?per_page=10"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(tag) = etag.filter(|t| t.len() < 200 && t.is_ascii() && !t.contains(['\r', '\n'])) {
        req = req.header("If-None-Match", tag);
    }
    let mut resp = req.call().map_err(|e| NetError::Http(e.to_string()))?;
    match resp.status().as_u16() {
        304 => return Ok(Listing::Unchanged),
        // GitHub's anonymous quota (60 an hour for everyone behind one address) ran out.
        403 | 429 => return Err(NetError::Http("GitHub is limiting update checks for now; trying again later".into())),
        200 => {}
        s => return Err(NetError::Http(format!("GitHub answered HTTP {s}"))),
    }
    let etag = header(&resp, "etag").filter(|t| t.len() < 200);
    let body = small_body(&mut resp, MAX_ANSWER)?;
    Ok(Listing::Changed(parse(&body, suffix)?, etag))
}

/// An address on the project's releases pages, else `None`.
fn ours(url: &str) -> Option<String> {
    let base = format!("https://github.com/{REPO}/releases/");
    (url.starts_with(&base) && url.len() < 1024 && !url.contains(['\r', '\n', ' '])).then(|| url.to_string())
}

fn is_sha256(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A tag as a version and test build: `v0.2.0-test.15` → ("0.2.0", 15); `v1.2.3` → ("1.2.3", 0).
pub fn tag_version(tag: &str) -> Option<(String, u32)> {
    let t = tag.trim().strip_prefix('v').unwrap_or(tag.trim());
    let (version, rest) = t.split_once('-').unwrap_or((t, ""));
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || p.len() > 6 || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let build = rest.rsplit('.').next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
    Some((version.to_string(), build))
}

/// The releases in GitHub's answer; `suffix` picks each one's installer for this system.
pub fn parse(body: &str, suffix: &str) -> Result<Vec<Release>> {
    let v: Value = serde_json::from_str(body).map_err(|e| NetError::Http(format!("GitHub's answer can't be read: {e}")))?;
    let list = v.as_array().ok_or_else(|| NetError::Http("GitHub's answer isn't a list of releases".into()))?;
    let text = |r: &Value, k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let mut out = Vec::new();
    for r in list.iter().take(20) {
        if r.get("draft").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let tag = text(r, "tag_name");
        let Some((version, build)) = tag_version(&tag) else { continue };
        let assets: &[Value] = r.get("assets").and_then(Value::as_array).map_or(&[], |a| a.as_slice());
        let assets = assets.get(..assets.len().min(100)).unwrap_or_default();
        let installer = assets.iter().find(|a| !suffix.is_empty() && text(a, "name").ends_with(suffix));
        let download = installer.and_then(|a| ours(&text(a, "browser_download_url"))).unwrap_or_default();
        // GitHub records each file's SHA-256 as `"digest": "sha256:<hex>"`.
        let sha256 = installer
            .filter(|_| !download.is_empty())
            .and_then(|a| text(a, "digest").strip_prefix("sha256:").filter(|h| is_sha256(h)).map(str::to_ascii_lowercase))
            .unwrap_or_default();
        let size = installer.filter(|_| !download.is_empty()).and_then(|a| a.get("size")?.as_u64()).unwrap_or(0);
        let sums =
            assets.iter().find(|a| text(a, "name") == "SHA256SUMS.txt").and_then(|a| ours(&text(a, "browser_download_url"))).unwrap_or_default();
        let commit = text(r, "target_commitish");
        out.push(Release {
            tag: tag.chars().take(64).collect(),
            version,
            build,
            commit: if commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()) { commit } else { String::new() },
            download,
            sha256,
            size,
            sums,
        });
    }
    Ok(out)
}

/// The newest release newer than `version` (this build; `build_sha` the commit it was built from).
/// Never an older or the same one; of two test builds of one version, the later one, but only when
/// this build is known to be the earlier.
pub fn newer_release<'a>(version: &str, build_sha: Option<&str>, releases: &'a [Release]) -> Option<&'a Release> {
    let parse = |v: &str| -> Option<(u32, u32, u32)> {
        let mut p = v.split('.').map(|n| n.parse::<u32>().ok());
        Some((p.next()??, p.next()??, p.next()??))
    };
    let current = parse(version)?;
    let newest = releases.iter().filter_map(|r| parse(&r.version).map(|v| (v, r))).max_by_key(|(v, r)| (*v, r.build))?;
    match newest.0.cmp(&current) {
        std::cmp::Ordering::Greater => Some(newest.1),
        std::cmp::Ordering::Less => None,
        std::cmp::Ordering::Equal => {
            let sha = build_sha.map(str::trim).filter(|s| s.len() >= 7)?;
            let mine = releases.iter().find(|r| !r.commit.is_empty() && (r.commit.starts_with(sha) || sha.starts_with(&r.commit)))?;
            (mine.version == newest.1.version && mine.build < newest.1.build).then_some(newest.1)
        }
    }
}

/// The SHA-256 a `SHA256SUMS.txt` (as `sha256sum` writes it) gives `file`, in lowercase hex.
pub fn sum_for(sums: &str, file: &str) -> Option<String> {
    sums.lines().take(10_000).find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == file && is_sha256(hash)).then(|| hash.to_ascii_lowercase())
    })
}

/// The checksum to hold `release`'s installer to: GitHub's own record, else its line in the
/// release's `SHA256SUMS.txt`.
pub fn expected_sum(release: &Release) -> Result<String> {
    if is_sha256(&release.sha256) {
        return Ok(release.sha256.to_ascii_lowercase());
    }
    let file = release.download.rsplit('/').next().unwrap_or_default();
    if release.sums.is_empty() || file.is_empty() {
        return Err(NetError::Http(format!("{} lists no checksum for its installer", release.tag)));
    }
    let mut resp = agent().get(&release.sums).call().map_err(|e| NetError::Http(e.to_string()))?;
    if resp.status().as_u16() != 200 {
        return Err(NetError::Http(format!("the checksums file answered HTTP {}", resp.status().as_u16())));
    }
    let body = small_body(&mut resp, MAX_SUMS)?;
    sum_for(&body, file).ok_or_else(|| NetError::Http(format!("{}'s checksums don't list {file}", release.tag)))
}

/// How a download is going, for whoever shows it.
#[derive(Debug, Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}

impl Progress {
    /// The bytes fetched and the size (0 until known).
    pub fn get(&self) -> (u64, u64) {
        (self.done.load(Ordering::Relaxed), self.total.load(Ordering::Relaxed))
    }
}

/// The last part of a download address, if it's a plain file name.
pub fn file_name(url: &str) -> Option<&str> {
    let name = url.rsplit('/').next()?;
    (!name.is_empty()
        && name.len() < 200
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')))
    .then_some(name)
}

/// The SHA-256 of a file, in lowercase hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = vec![0u8; 256 << 10];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.update(buf.get(..n).unwrap_or_default());
    }
    Ok(ctx.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect())
}

/// Fetch `release`'s installer into `dir`, checked against `expected`: the finished file, or
/// `None` when `stop` was set (what came stays in `<file>.<size>.part`, to carry on from with a
/// range request). A resumed file that doesn't match is fetched once more from the start.
pub fn download_installer(release: &Release, expected: &str, dir: &Path, progress: &Progress, stop: &AtomicBool) -> Result<Option<PathBuf>> {
    let url = ours(&release.download).ok_or_else(|| NetError::Http("the release has no installer for this system".into()))?;
    let name = file_name(&url).ok_or_else(|| NetError::Http("the installer has no usable file name".into()))?;
    std::fs::create_dir_all(dir).map_err(|e| NetError::Disk(format!("{}: {e}", dir.display())))?;
    let dest = dir.join(name);
    if dest.is_file() && sha256_file(&dest).is_ok_and(|s| s == expected) {
        return Ok(Some(dest));
    }
    let size = release.size.min(MAX_INSTALLER);
    let part = dir.join(format!("{name}.{size}.part"));
    for attempt in 0..2 {
        let resumed = attempt == 0 && part.is_file();
        let Some(done) = fetch_part(&url, &part, size, progress, stop)? else { return Ok(None) };
        let sum = sha256_file(&part).map_err(|e| NetError::Disk(e.to_string()))?;
        if (size == 0 || done == size) && sum == expected {
            std::fs::rename(&part, &dest).map_err(|e| NetError::Disk(format!("{}: {e}", dest.display())))?;
            return Ok(Some(dest));
        }
        let _ = std::fs::remove_file(&part);
        if !resumed {
            break;
        }
    }
    Err(NetError::Http(format!("{name} didn't match the release's checksum; it was deleted")))
}

/// Grow `part` to the whole file (carrying on from what's there). `Some(bytes)` when the server
/// sent all it had, `None` when stopped.
fn fetch_part(url: &str, part: &Path, size: u64, progress: &Progress, stop: &AtomicBool) -> Result<Option<u64>> {
    let mut have = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    if size > 0 && have > size {
        let _ = std::fs::remove_file(part);
        have = 0;
    }
    progress.total.store(size, Ordering::Relaxed);
    progress.done.store(have, Ordering::Relaxed);
    let mut req = agent().get(url).header("Accept-Encoding", "identity");
    if have > 0 {
        req = req.header("Range", format!("bytes={have}-"));
    }
    let mut resp = req.call().map_err(|e| NetError::Http(e.to_string()))?;
    let status = resp.status().as_u16();
    let mut file = match status {
        416 if have > 0 => return Ok(Some(have)),
        206 => OpenOptions::new().append(true).open(part),
        200 => {
            have = 0;
            File::create(part)
        }
        s => return Err(NetError::Http(format!("the installer answered HTTP {s}"))),
    }
    .map_err(|e| NetError::Disk(format!("{}: {e}", part.display())))?;
    let mut body = resp.body_mut().with_config().limit(MAX_INSTALLER).reader();
    let mut buf = vec![0u8; 256 << 10];
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = file.flush();
            return Ok(None);
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(NetError::Http(format!("the update's download broke off ({e}); it carries on later"))),
        };
        file.write_all(buf.get(..n).unwrap_or_default()).map_err(|e| NetError::Disk(e.to_string()))?;
        have = have.saturating_add(n as u64);
        progress.done.store(have, Ordering::Relaxed);
        if have > MAX_INSTALLER {
            return Err(NetError::Http("the installer is too big".into()));
        }
    }
    file.flush().map_err(|e| NetError::Disk(e.to_string()))?;
    Ok(Some(have))
}

#[cfg(test)]
mod tests;
