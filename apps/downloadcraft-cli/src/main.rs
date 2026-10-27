//! `downloadcraft-cli`: the engine and network layer without a window.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc::channel;

use downloadcraft_engine::model::format;
use downloadcraft_engine::model::probe::Probe;
use downloadcraft_engine::{Event, Session, State};
use downloadcraft_net::{Sink, Worker};
use serde_json::{Value, json};

const USAGE: &str = "\
usage: downloadcraft-cli <command>

  get <url>… [--dir <folder>] [--connections <n>] [--subtitles <langs>]
                                download links into a folder (default: the current one),
                                each on up to n connections (default 8); a playlist or
                                channel link becomes its videos; videos with subtitles in
                                langs (such as en,es)
  tools [get]                   where yt-dlp and FFmpeg stand; `get` fetches what's missing
  commands                      every engine command, as JSON
  run <id> [<json>]             run one command on an empty list and print its answer
  --version                     print the version
";

fn say(what: &str) {
    let _ = writeln!(std::io::stderr(), "{what}");
}

fn out(v: &Value) {
    let _ = writeln!(std::io::stdout(), "{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

/// Download every link, showing progress on standard error.
fn get(args: &[String]) -> Result<(), String> {
    let mut urls = Vec::new();
    let mut dir = std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|_| ".".into());
    let mut connections = None;
    let mut subtitles = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--dir" {
            dir = it.next().ok_or("--dir needs a folder")?.clone();
        } else if a == "--subtitles" {
            subtitles = Some(it.next().ok_or("--subtitles needs languages, such as en,es")?.clone());
        } else if a == "--connections" {
            connections = Some(it.next().and_then(|n| n.parse::<u64>().ok()).ok_or("--connections needs a number")?);
        } else {
            urls.push(a.clone());
        }
    }
    if urls.is_empty() {
        return Err("get needs a link".into());
    }
    // A playlist or channel link becomes its videos, each a download of its own.
    let mut links = Vec::new();
    for u in urls {
        let list = downloadcraft_engine::model::source::classify(&u) == Ok(downloadcraft_engine::SourceKind::VideoPage)
            && downloadcraft_engine::model::source::may_be_playlist(&u);
        match list.then(|| downloadcraft_net::probe(&u)) {
            Some(Probe::Playlist { title, videos, .. }) => {
                say(&format!("{title}: {} videos", videos.len()));
                links.extend(videos.into_iter().map(|v| v.url));
            }
            Some(Probe::Failed(e)) => say(&format!("skipped {u}: {e}")),
            _ => links.push(u),
        }
    }
    let urls = links;
    if urls.is_empty() {
        return Err("nothing to download".into());
    }
    let mut s = Session::new();
    s.prefs.folder = dir;
    if let Some(n) = connections {
        s.run("prefs.connections", &json!({"count": n})).map_err(|e| e.to_string())?;
    }
    if let Some(l) = subtitles {
        s.run("prefs.subtitles", &json!({"languages": l})).map_err(|e| e.to_string())?;
    }
    let added = s.run("download.addMany", &json!({"urls": urls})).map_err(|e| e.to_string())?;
    for r in added["refused"].as_array().into_iter().flatten() {
        say(&format!("skipped {}: {}", r["url"].as_str().unwrap_or(""), r["error"].as_str().unwrap_or("")));
    }
    if s.downloads.is_empty() {
        return Err("nothing to download".into());
    }
    let (tx, rx) = channel();
    let mut w = Worker::new(Sink::new(tx, Arc::new(|| {})));
    loop {
        for job in s.take_jobs() {
            w.submit(job);
        }
        if s.downloads.iter().all(|d| !d.state.active()) {
            break;
        }
        let Ok(ev) = rx.recv() else { break };
        let id = ev.id();
        let progress = matches!(ev, Event::Progress { .. });
        s.apply(ev);
        if let Some(d) = s.get(id) {
            let line = match (d.state, d.total) {
                (State::Downloading, Some(t)) => {
                    format!("{}: {} of {} at {}", d.file_name, format::bytes(d.done), format::bytes(t), format::speed(d.speed))
                }
                (State::Downloading, None) => format!("{}: {} at {}", d.file_name, format::bytes(d.done), format::speed(d.speed)),
                (State::Completed, _) => format!("{}: done ({})", d.file_name, format::bytes(d.done)),
                (State::Failed, _) => format!("{}: {}", d.file_name, d.error.as_deref().unwrap_or("failed")),
                _ => continue,
            };
            let _ = write!(std::io::stderr(), "\r{line:<100}{}", if progress { "" } else { "\n" });
        }
    }
    let failed = s.downloads.iter().filter(|d| d.state == State::Failed).count();
    if failed > 0 { Err(format!("{failed} download(s) failed")) } else { Ok(()) }
}

/// `DOWNLOADCRAFT_LOG=1`: what the network layer does, on standard error.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            let _ = writeln!(std::io::stderr(), "[{}] {}", r.level(), r.args());
        }
    }
    fn flush(&self) {}
}

static LOGGER: StderrLog = StderrLog;

/// Report where the video tools stand, fetching what's missing with `get`.
fn tools(action: Option<&str>) -> Result<(), String> {
    use downloadcraft_engine::model::probe::ToolState;
    use downloadcraft_net::video::tools;
    if action == Some("get") {
        if tools::ytdlp().is_none() {
            let p = tools::fetch_ytdlp().map_err(|e| format!("yt-dlp: {e}"))?;
            say(&format!("yt-dlp: fetched and checked: {}", p.display()));
        }
        if tools::ffmpeg().is_none() && tools::can_fetch_ffmpeg() {
            let p = tools::fetch_ffmpeg().map_err(|e| format!("FFmpeg: {e}"))?;
            say(&format!("FFmpeg: fetched and checked: {}", p.display()));
        }
    }
    let s = tools::status();
    let show = |name: &str, state: &ToolState, path: Option<std::path::PathBuf>| match state {
        ToolState::Ready => say(&format!("{name}: {}", path.map(|p| p.display().to_string()).unwrap_or_default())),
        _ => say(&format!("{name}: not found")),
    };
    show("yt-dlp", &s.ytdlp, tools::ytdlp());
    show("FFmpeg", &s.ffmpeg, tools::ffmpeg());
    Ok(())
}

fn main() -> ExitCode {
    if std::env::var_os("DOWNLOADCRAFT_LOG").is_some_and(|v| v == "1") && log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    // Video downloads use the yt-dlp and FFmpeg the app fetched (DOWNLOADCRAFT_TOOLS overrides).
    if std::env::var_os("DOWNLOADCRAFT_TOOLS").is_none()
        && let Some(data) = std::env::var_os(if cfg!(windows) { "APPDATA" } else { "HOME" })
    {
        let base = std::path::PathBuf::from(data);
        let dir = if cfg!(windows) {
            base.join("DownloadCraft")
        } else if cfg!(target_os = "macos") {
            base.join("Library/Application Support/DownloadCraft")
        } else {
            base.join(".local/share/DownloadCraft")
        };
        downloadcraft_net::video::tools::set_dir(dir.join("tools"));
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("get") => get(args.get(1..).unwrap_or_default()),
        Some("tools") => tools(args.get(1).map(String::as_str)),
        Some("commands") => {
            out(&Session::new().registry().describe());
            Ok(())
        }
        Some("run") => match args.get(1) {
            None => Err("run needs a command id".into()),
            Some(id) => {
                let params = args.get(2).map_or(Ok(Value::Null), |j| serde_json::from_str(j)).map_err(|e| format!("bad JSON: {e}"));
                params.and_then(|p| {
                    let mut s = Session::new();
                    s.prefs.folder = ".".into();
                    s.run(id, &p).map(|v| out(&v)).map_err(|e| e.to_string())
                })
            }
        },
        Some("--version" | "-V") => {
            say(&format!("DownloadCraft {}", env!("CARGO_PKG_VERSION")));
            Ok(())
        }
        _ => {
            say(USAGE);
            Ok(())
        }
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            say(&format!("downloadcraft-cli: {e}"));
            ExitCode::FAILURE
        }
    }
}
