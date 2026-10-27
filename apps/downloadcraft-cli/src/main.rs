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
      [--files <list>] [--first <list>] [--limits <down>,<up>] [--seed <ratio>]
      [--torrent <json>] [--details]
                                download links into a folder (default: the current one),
                                each on up to n connections (default 8); a playlist or
                                channel link becomes its videos; videos with subtitles in
                                langs (such as en,es). Magnet links and .torrent files or
                                links download as torrents: --files picks files by number
                                (1,3-5, as `files` lists them), --first downloads those
                                before the rest, --limits caps this torrent (KiB/s),
                                --seed shares a finished
                                torrent until it has sent ratio times its size (default 0);
                                --torrent sets prefs.torrent ({\"port\": 51413,
                                \"downKib\": 500, ...}); --details prints peers and trackers as it goes
  files <torrent>               the files inside a torrent or magnet link, numbered
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
    let mut files = Vec::new();
    let mut seed = 0.0;
    let mut torrent = None;
    let mut details = false;
    let mut first = Vec::new();
    let mut limits = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--dir" {
            dir = it.next().ok_or("--dir needs a folder")?.clone();
        } else if a == "--subtitles" {
            subtitles = Some(it.next().ok_or("--subtitles needs languages, such as en,es")?.clone());
        } else if a == "--files" {
            files = numbers(it.next().ok_or("--files needs numbers, such as 1,3-5")?)?;
        } else if a == "--torrent" {
            let json = it.next().ok_or("--torrent needs settings as JSON")?;
            torrent = Some(serde_json::from_str::<Value>(json).map_err(|e| format!("--torrent: {e}"))?);
        } else if a == "--first" {
            first = numbers(it.next().ok_or("--first needs numbers, such as 1,3-5")?)?;
        } else if a == "--limits" {
            let l = it.next().ok_or("--limits needs down,up in KiB/s, such as 500,100")?;
            let (d, u) = l.split_once(',').unwrap_or((l.as_str(), "0"));
            let n = |x: &str| x.trim().parse::<u32>().map_err(|_| format!("--limits takes down,up in KiB/s, not {l}"));
            limits = Some((n(d)?, n(u)?));
        } else if a == "--details" {
            details = true;
        } else if a == "--seed" {
            seed = it.next().and_then(|n| n.parse::<f64>().ok()).filter(|r| r.is_finite() && *r >= 0.0).ok_or("--seed needs a ratio, such as 1")?;
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
    s.run("prefs.seedRatio", &json!({"ratio": seed})).map_err(|e| e.to_string())?;
    if let Some(t) = &torrent {
        s.run("prefs.torrent", t).map_err(|e| e.to_string())?;
    }
    for u in &urls {
        // Added paused, so a torrent's first files and limits are set before it starts.
        let mut params = json!({"url": u, "paused": true});
        if !files.is_empty() {
            params["files"] = json!(files);
        }
        match s.run("download.add", &params) {
            Err(e) => say(&format!("skipped {u}: {e}")),
            Ok(r) => {
                let id = r["id"].clone();
                let torrent = s.get(downloadcraft_engine::DownloadId(id.as_u64().unwrap_or(0))).is_some_and(|d| d.torrent.is_some());
                if torrent && !first.is_empty() {
                    s.run("download.first", &json!({"id": id, "files": first, "first": true})).map_err(|e| e.to_string())?;
                }
                if let (true, Some((d, up))) = (torrent, limits) {
                    s.run("download.torrentLimits", &json!({"id": id, "downKib": d, "upKib": up})).map_err(|e| e.to_string())?;
                }
                s.run("download.resume", &json!({"id": id})).map_err(|e| e.to_string())?;
            }
        }
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
        if s.downloads.iter().all(|d| !d.state.active() && !d.seeding) && w.running() == 0 {
            break;
        }
        let Ok(ev) = rx.recv() else { break };
        let id = ev.id();
        if let Event::Details { live, .. } = &ev
            && details
        {
            say(&details_line(live, downloadcraft_net::torrent::status()));
        }
        let progress = matches!(ev, Event::Progress { .. } | Event::Uploaded { seeding: true, .. });
        s.apply(ev);
        if let Some(d) = s.get(id) {
            let line = match (d.state, d.total) {
                (State::Downloading, Some(t)) => {
                    format!("{}: {} of {} at {}", d.file_name, format::bytes(d.done), format::bytes(t), format::speed(d.speed))
                }
                (State::Downloading, None) => format!("{}: {} at {}", d.file_name, format::bytes(d.done), format::speed(d.speed)),
                (State::Completed, _) if d.seeding => format!("{}: sharing, {} sent", d.file_name, format::bytes(d.uploaded)),
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

/// One line about a running torrent: peers, the swarm as each tracker says, the DHT.
fn details_line(l: &downloadcraft_engine::model::torrent::TorrentLive, engine: Option<downloadcraft_engine::model::torrent::EngineStatus>) -> String {
    use downloadcraft_engine::model::torrent::TrackerStatus;
    let trackers: Vec<String> = l
        .trackers
        .iter()
        .map(|t| match &t.status {
            TrackerStatus::Working => format!("{} {}/{}/{}", t.url, t.seeds.unwrap_or(0), t.leechers.unwrap_or(0), t.completed.unwrap_or(0)),
            TrackerStatus::Failing(why) => format!("{} failing ({why})", t.url),
            TrackerStatus::Waiting => format!("{} waiting", t.url),
            TrackerStatus::Unsupported => format!("{} not asked", t.url),
        })
        .collect();
    let top = l.peers.first().map(|p| format!("{} {} {} {}", p.address, p.connection, p.client, format::speed(p.down_speed))).unwrap_or_default();
    let dht = engine.as_ref().and_then(|e| e.dht_nodes).map_or("off".to_string(), |n| n.to_string());
    let port = engine.as_ref().and_then(|e| e.port).map_or("-".to_string(), |p| p.to_string());
    format!(
        "[details] {} of {} · {} connected, {} connecting, {} known · wasted {} · DHT {dht} · port {port} · busiest {top} · trackers: {}",
        format::bytes(l.have),
        format::bytes(l.size),
        l.connected,
        l.connecting,
        l.known,
        l.wasted.map_or_else(|| "-".into(), format::bytes),
        trackers.join("; ")
    )
}

/// `1,3-5` as file numbers counted from 1, into indexes counted from 0.
fn numbers(list: &str) -> Result<Vec<usize>, String> {
    let bad = || format!("--files takes numbers such as 1,3-5, not {list}");
    let mut out = Vec::new();
    for part in list.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (a, b) = part.split_once('-').unwrap_or((part, part));
        let (a, b) = (a.trim().parse::<usize>().map_err(|_| bad())?, b.trim().parse::<usize>().map_err(|_| bad())?);
        if a == 0 || b < a || b > downloadcraft_engine::model::probe::MAX_TORRENT_FILES {
            return Err(bad());
        }
        out.extend((a..=b).map(|n| n - 1));
    }
    Ok(out)
}

/// List a torrent's files, numbered as `get --files` takes them.
fn files(url: Option<&str>) -> Result<(), String> {
    let url = url.ok_or("files needs a magnet link or a torrent")?;
    say("Getting the torrent's details…");
    match downloadcraft_net::probe(url) {
        Probe::Torrent { name, files } => {
            let total = files.iter().fold(0u64, |a, f| a.saturating_add(f.size));
            let _ = writeln!(std::io::stdout(), "{name}: {} files, {}", files.len(), format::bytes(total));
            for f in &files {
                let _ = writeln!(std::io::stdout(), "{:>5}  {:>10}  {}", f.index + 1, format::bytes(f.size), f.path);
            }
            Ok(())
        }
        Probe::Failed(e) => Err(e),
        _ => Err("that isn't a torrent or magnet link".into()),
    }
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
        if tools::deno().is_none() && tools::can_fetch(downloadcraft_engine::model::probe::Tool::Deno) {
            let p = tools::fetch_deno().map_err(|e| format!("Deno: {e}"))?;
            say(&format!("Deno: fetched and checked: {}", p.display()));
        }
    }
    let s = tools::status();
    let show = |name: &str, state: &ToolState, path: Option<std::path::PathBuf>| match state {
        ToolState::Ready => say(&format!("{name}: {}", path.map(|p| p.display().to_string()).unwrap_or_default())),
        _ => say(&format!("{name}: not found")),
    };
    show("yt-dlp", &s.ytdlp, tools::ytdlp());
    show("FFmpeg", &s.ffmpeg, tools::ffmpeg());
    show("Deno", &s.deno, tools::deno());
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
        downloadcraft_net::torrent::set_dir(dir.join("torrents"));
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("get") => get(args.get(1..).unwrap_or_default()),
        Some("tools") => tools(args.get(1).map(String::as_str)),
        Some("files") => files(args.get(1).map(String::as_str)),
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
