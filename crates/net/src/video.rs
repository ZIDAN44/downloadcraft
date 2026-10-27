//! Video pages (YouTube, Vimeo…), downloaded through yt-dlp.
//!
//! yt-dlp is the program that knows how to find the video in a page; DownloadCraft runs it and
//! reads what it says in the forms its authors recommend for programs: `-J` for the page's
//! details (title, the formats it offers) and `--progress-template` / `--print` lines that carry
//! numbers rather than text meant for people. The quality is a yt-dlp format selector
//! (`downloadcraft_model::probe`). Pausing ends the yt-dlp run; resuming starts another with
//! `--continue`, which carries on from its partial files. Several fragments at once
//! (`--concurrent-fragments`) follow the connections setting. Videos protected by DRM aren't
//! downloaded: yt-dlp refuses them.

pub mod tools;

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use downloadcraft_model::probe::{self, Probe};
use downloadcraft_model::{DownloadId, Event};
use serde_json::Value;

use crate::{NetError, Sink};

/// How long reading a page may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(90);
/// The largest `-J` answer read.
const MAX_INFO: usize = 32 << 20;
/// Marks DownloadCraft's own lines in yt-dlp's output.
const PROGRESS: &str = "DCPROGRESS";
const FILE: &str = "DCFILE";

pub(crate) struct VideoFetch {
    pub id: DownloadId,
    pub run: u64,
    pub url: String,
    pub folder: PathBuf,
    pub file_name: String,
    pub selector: String,
    pub connections: u8,
}

fn command(ytdlp: &Path) -> Command {
    let mut cmd = Command::new(ytdlp);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashing up behind the window.
        cmd.creation_flags(0x0800_0000);
    }
    cmd
}

/// The last line yt-dlp wrote as an error (`ERROR: …`), made readable.
fn error_line(stderr: &str) -> String {
    stderr
        .lines()
        .rev()
        .find(|l| l.starts_with("ERROR:"))
        .map(|l| l.trim_start_matches("ERROR:").trim().chars().take(300).collect())
        .unwrap_or_else(|| "yt-dlp couldn't read the page".into())
}

/// Read everything from `r` (at most `max` bytes) on a thread of its own.
fn drain(mut r: impl Read + Send + 'static, max: usize) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = r.by_ref().take(max as u64).read_to_end(&mut buf);
        let _ = std::io::copy(&mut r, &mut std::io::sink());
        let _ = tx.send(buf);
    });
    rx
}

/// Look at a video page: its title, the name the file will get, and its qualities.
pub fn probe_video(url: &str) -> Probe {
    let Some(ytdlp) = tools::ytdlp() else { return Probe::Failed("video downloads need yt-dlp".into()) };
    let mut cmd = command(&ytdlp);
    cmd.args(["-J", "--no-playlist", "--no-warnings", "--skip-download", "--", url]);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Probe::Failed(format!("yt-dlp didn't start ({e})")),
    };
    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else { return Probe::Failed("yt-dlp didn't start".into()) };
    let (out, err) = (drain(out, MAX_INFO), drain(err, 1 << 20));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() > PROBE_TIMEOUT => {
                let _ = child.kill();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => break None,
        }
    };
    let out = out.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
    let err = String::from_utf8_lossy(&err.recv_timeout(Duration::from_secs(5)).unwrap_or_default()).into_owned();
    match status {
        Some(s) if s.success() => {}
        Some(_) => return Probe::Failed(error_line(&err)),
        None => return Probe::Failed("the page took too long to read".into()),
    }
    let info: Value = match serde_json::from_slice(&out) {
        Ok(v) => v,
        Err(_) => return Probe::Failed("yt-dlp's answer couldn't be read".into()),
    };
    let (title, file_name) = probe::title_and_name(&info);
    Probe::Video { title, file_name, qualities: probe::qualities(&info) }
}

/// One `DCPROGRESS downloaded total estimate speed` line: (downloaded, total, speed), each when
/// yt-dlp knew it (`NA` otherwise).
pub fn parse_progress(line: &str) -> Option<(u64, Option<u64>, Option<u64>)> {
    let mut it = line.strip_prefix(PROGRESS)?.split_whitespace();
    let n = |s: Option<&str>| s.and_then(|v| v.parse::<f64>().ok()).filter(|f| f.is_finite() && *f >= 0.0).map(|f| f as u64);
    let done = n(it.next())?;
    let (total, estimate, speed) = (n(it.next()), n(it.next()), n(it.next()));
    Some((done, total.or(estimate), speed))
}

/// Download the video, reporting through `sink`; a cancelled run leaves without a word.
pub(crate) fn fetch(f: &VideoFetch, cancel: &AtomicBool, sink: &Sink) {
    let (id, run) = (f.id, f.run);
    match run_ytdlp(f, cancel, sink) {
        Ok(Some(done)) => sink.send(Event::Finished { id, run, done }),
        Ok(None) => {}
        Err(e) => sink.send(Event::Failed { id, run, error: e.to_string() }),
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn run_ytdlp(f: &VideoFetch, cancel: &AtomicBool, sink: &Sink) -> Result<Option<u64>, NetError> {
    let ytdlp = tools::ytdlp().ok_or_else(|| NetError::Http("video downloads need yt-dlp: open Add link to get it".into()))?;
    std::fs::create_dir_all(&f.folder).map_err(|e| NetError::Disk(format!("{}: {e}", f.folder.display())))?;
    // A name found when the link was added is kept; without one (the name is just the address's
    // last part), the file is named after the video's title.
    let template = if f.file_name == downloadcraft_model::names::from_url(&f.url) {
        f.folder.join("%(title).180B.%(ext)s")
    } else {
        let stem = f.file_name.rsplit_once('.').map_or(f.file_name.as_str(), |(s, _)| s);
        f.folder.join(format!("{stem}.%(ext)s"))
    };
    let mut cmd = command(&ytdlp);
    cmd.args(["-f", &f.selector, "-S", probe::SORT, "--no-playlist", "--newline", "--no-colors", "--windows-filenames", "--continue", "--no-mtime"]);
    cmd.args(["--merge-output-format", "mp4", "--concurrent-fragments", &f.connections.max(1).to_string()]);
    cmd.args([
        "--progress-template",
        &format!("download:{PROGRESS} %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s"),
    ]);
    cmd.args(["--print", &format!("after_move:{FILE} %(filepath)s")]);
    if let Some(dir) = tools::own_ffmpeg_dir() {
        cmd.arg("--ffmpeg-location").arg(dir);
    }
    cmd.arg("-o").arg(&template).arg("--").arg(&f.url);
    let mut child = cmd.spawn().map_err(|e| NetError::Http(format!("yt-dlp didn't start ({e})")))?;
    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        stop(&mut child);
        return Err(NetError::Http("yt-dlp didn't start".into()));
    };
    let err = drain(err, 1 << 20);
    sink.send(Event::Started { id: f.id, run: f.run, total: None, resumable: true, file_name: f.file_name.clone(), done: 0 });

    // Lines come on a thread of their own, so a pause is noticed while yt-dlp is quiet.
    let (tx, lines) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    // Video and audio are fetched one after the other, each counted from 0: what earlier streams
    // brought is kept in `before`.
    let (mut before, mut last, mut last_total) = (0u64, 0u64, 0u64);
    let mut final_name = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            stop(&mut child);
            return Ok(None);
        }
        match lines.recv_timeout(Duration::from_millis(200)) {
            Ok(line) => {
                if let Some((done, total, speed)) = parse_progress(&line) {
                    if done + (1 << 20) < last {
                        before += last_total.max(last);
                    }
                    last = done;
                    last_total = total.unwrap_or(0);
                    let all = before + done;
                    sink.send(Event::Progress { id: f.id, run: f.run, done: all, speed: speed.unwrap_or(0), segments: Vec::new() });
                } else if let Some(path) = line.strip_prefix(FILE) {
                    final_name = Path::new(path.trim()).file_name().map(|n| n.to_string_lossy().into_owned());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait().map_err(|e| NetError::Http(e.to_string()))?;
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let err = String::from_utf8_lossy(&err.recv_timeout(Duration::from_secs(5)).unwrap_or_default()).into_owned();
    if !status.success() {
        return Err(NetError::Http(error_line(&err)));
    }
    let name = final_name.unwrap_or_else(|| f.file_name.clone());
    let size = std::fs::metadata(f.folder.join(&name)).map(|m| m.len()).unwrap_or(before + last);
    // The name yt-dlp settled on (its extension follows the formats it got).
    sink.send(Event::Started { id: f.id, run: f.run, total: Some(size), resumable: true, file_name: name, done: size });
    Ok(Some(size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines_are_read_and_others_ignored() {
        assert_eq!(parse_progress("DCPROGRESS 1024 4096 NA 512.5"), Some((1024, Some(4096), Some(512))));
        assert_eq!(parse_progress("DCPROGRESS 1024 NA 9000.0 NA"), Some((1024, Some(9000), None)), "an estimate when the size isn't known");
        assert_eq!(parse_progress("[download] Destination: x.mp4"), None);
        assert_eq!(parse_progress("DCPROGRESS NA NA NA NA"), None);
        assert_eq!(error_line("WARNING: x\nERROR: [youtube] abc: Video unavailable\n"), "[youtube] abc: Video unavailable");
    }
}
