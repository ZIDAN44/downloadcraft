//! The command registry: every user-visible action, by id.
//!
//! Shortcuts follow the ones download managers already have: Delete removes, Mod+A in the list
//! selects; the Add dialog (Mod+N) and pasting a link (Mod+V) belong to the UI, which then calls
//! `download.add`.

use serde_json::{Value, json};

use crate::session::{Filter, MAX_DOWNLOADS};
use crate::{CmdError, CmdResult, CommandSpec, Registry, Session, p};
use downloadcraft_model::{
    Download, DownloadId, Job, MAX_ACTIVE_LIMIT, MAX_CONNECTIONS, SourceKind, State, Theme, TorrentChoice, VideoChoice, names, probe, source,
};

/// The most links one `download.addMany` takes.
const MAX_BATCH: usize = 1000;

fn has_downloads(s: &Session) -> Option<&'static str> {
    if s.downloads.is_empty() { Some("the list is empty") } else { None }
}

/// Every command, in the order the CLI lists them.
pub fn registry() -> Registry {
    Registry::new(vec![
        CommandSpec::new("download.add", "Add link", "Toolbar", add)
            .params(r#"{"url": string, "folder"?: string, "name"?: string, "paused"?: bool, "quality"?: {"selector": string, "label": string}, "files"?: [number]}"#),
        CommandSpec::new("download.addMany", "Add links", "Toolbar › Add link", add_many)
            .params(r#"{"urls": [string], "folder"?: string, "paused"?: bool}"#),
        CommandSpec::new("download.resume", "Resume", "Toolbar", resume).params(r#"{"id"?: number}"#),
        CommandSpec::new("download.pause", "Pause", "Toolbar", pause).params(r#"{"id"?: number}"#),
        CommandSpec::new("download.remove", "Remove", "Toolbar", remove).key("Delete").params(r#"{"id"?: number, "deleteFile"?: bool}"#),
        CommandSpec::new("download.again", "Download again", "List", again).params(r#"{"id"?: number}"#),
        CommandSpec::new("download.resumeAll", "Resume all", "Toolbar › Queue", resume_all).when(has_downloads),
        CommandSpec::new("download.pauseAll", "Pause all", "Toolbar › Queue", pause_all).when(has_downloads),
        CommandSpec::new("download.clearCompleted", "Clear completed", "Toolbar › Queue", clear_completed).when(has_downloads),
        CommandSpec::new("download.list", "List downloads", "CLI", list).params(r#"{"filter"?: "all"|"active"|"paused"|"completed"|"failed"}"#),
        CommandSpec::new("list.select", "Select", "List", select).params(r#"{"id": number | null}"#),
        CommandSpec::new("list.selectNext", "Select next", "List", |s, _| step(s, 1)).key("ArrowDown"),
        CommandSpec::new("list.selectPrevious", "Select previous", "List", |s, _| step(s, -1)).key("ArrowUp"),
        CommandSpec::new("list.filter", "Show", "Categories", filter).params(r#"{"filter": "all"|"active"|"paused"|"completed"|"failed"}"#),
        CommandSpec::new("prefs.folder", "Download folder", "Settings", set_folder).params(r#"{"folder": string}"#),
        CommandSpec::new("prefs.maxActive", "Downloads at once", "Settings", set_max_active).params(r#"{"count": number}"#),
        CommandSpec::new("prefs.connections", "Connections per download", "Settings", set_connections).params(r#"{"count": number}"#),
        CommandSpec::new("prefs.theme", "Theme", "Settings › Appearance", set_theme).params(r#"{"theme": "system"|"light"|"dark"}"#),
        CommandSpec::new("prefs.subtitles", "Subtitles", "Settings › Videos", set_subtitles).params(r#"{"languages": "en, es"}"#),
        CommandSpec::new("prefs.seedRatio", "Share finished torrents", "Settings › Torrents", set_seed_ratio).params(r#"{"ratio": number}"#),
        CommandSpec::new("prefs.torrent", "Torrent connections", "Settings › Torrents", set_torrent).params(
            r#"{"port"?: number, "protocol"?: "tcp"|"both", "upnp"?: bool, "peerLimit"?: number, "downKib"?: number, "upKib"?: number, "proxy"?: string, "ipFilter"?: string}"#,
        ),
        CommandSpec::new("download.pickFiles", "Pick files", "Details › Files", pick_files).params(r#"{"id"?: number, "files": [number]}"#),
        CommandSpec::new("prefs.language", "Language", "Settings › Appearance", set_language).params(r#"{"language": "auto"|"de"|"pt-br"}"#),
    ])
}

/// `id` from the params, else the selected download.
fn target(s: &Session, v: &Value) -> Result<DownloadId, CmdError> {
    let id = match v.get("id") {
        Some(x) if !x.is_null() => DownloadId(p::u64(v, "id").ok_or_else(|| CmdError::Params("`id` is a number".into()))?),
        _ => s.selection.ok_or_else(|| CmdError::Disabled("select a download first".into()))?,
    };
    if s.get(id).is_none() {
        return Err(CmdError::Params(format!("no download {}", id.0)));
    }
    Ok(id)
}

fn describe(d: &Download) -> Value {
    json!({
        "id": d.id.0, "url": d.url, "kind": d.kind, "file": d.file_name, "folder": d.folder, "state": d.state,
        "done": d.done, "total": d.total, "speed": d.speed, "resumable": d.resumable, "error": d.error,
        "uploaded": d.uploaded, "seeding": d.seeding,
    })
}

/// Make a new entry for `url`, queued unless `paused`.
fn new_download(
    s: &mut Session,
    url: &str,
    folder: Option<&str>,
    name: Option<&str>,
    paused: bool,
    quality: Option<&Value>,
    files: Option<&Value>,
) -> Result<DownloadId, CmdError> {
    if s.downloads.len() >= MAX_DOWNLOADS {
        return Err(CmdError::Failed("the list is full; clear completed downloads first".into()));
    }
    let url = source::normalize(url);
    let url = url.as_str();
    let kind = source::classify(url).map_err(|e| CmdError::Params(e.to_string()))?;
    if let Some(why) = kind.not_yet() {
        return Err(CmdError::Disabled(format!("{} links aren't supported yet: {why}", kind.label())));
    }
    let folder = folder.map(str::trim).filter(|f| !f.is_empty()).map_or_else(|| s.prefs.folder.clone(), str::to_string);
    if folder.is_empty() {
        return Err(CmdError::Params("choose a download folder first (`prefs.folder`)".into()));
    }
    let torrent = matches!(kind, SourceKind::Torrent | SourceKind::Magnet);
    let guess = || if torrent { source::torrent_name(url) } else { names::from_url(url) };
    let file_name = names::sanitize(name.map(str::trim).filter(|n| !n.is_empty()).map_or_else(guess, str::to_string).as_str());
    // A torrent downloads the files picked from it (all of them when none are).
    let torrent = torrent.then(|| {
        let mut files: Vec<usize> = files
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_u64).filter_map(|i| usize::try_from(i).ok()).filter(|i| *i < probe::MAX_TORRENT_FILES).collect())
            .unwrap_or_default();
        files.sort_unstable();
        files.dedup();
        TorrentChoice { files, list: Vec::new() }
    });
    // A video page downloads through yt-dlp, in the quality picked (the best there is otherwise).
    let video = (kind == SourceKind::VideoPage).then(|| {
        let selector = quality.and_then(|q| p::str(q, "selector")).filter(|s| !s.is_empty() && s.len() < 400).unwrap_or(probe::BEST);
        let label = quality.and_then(|q| p::str(q, "label")).filter(|l| l.len() < 40).unwrap_or("Best");
        VideoChoice { selector: selector.to_string(), label: label.to_string() }
    });
    let id = s.new_id();
    s.downloads.push(Download {
        id,
        url: url.to_string(),
        kind,
        file_name,
        folder,
        total: None,
        done: 0,
        state: if paused { State::Paused } else { State::Queued },
        error: None,
        resumable: false,
        added: s.now,
        speed: 0,
        run: 0,
        segments: Vec::new(),
        video,
        missing: false,
        torrent,
        uploaded: 0,
        sent: 0,
        sent_before: 0,
        live: None,
        seeding: false,
    });
    s.selection = Some(id);
    s.dirty = true;
    Ok(id)
}

fn add(s: &mut Session, v: &Value) -> CmdResult {
    let url = p::req_str(v, "url")?;
    let id = new_download(s, url, p::str(v, "folder"), p::str(v, "name"), p::bool(v, "paused").unwrap_or(false), v.get("quality"), v.get("files"))?;
    let name = s.get(id).map(|d| d.file_name.clone()).unwrap_or_default();
    s.status = format!("Added {name}");
    Ok(json!({"id": id.0, "file": name}))
}

fn add_many(s: &mut Session, v: &Value) -> CmdResult {
    let urls = v.get("urls").and_then(Value::as_array).ok_or_else(|| CmdError::Params("`urls` (array of strings) is required".into()))?;
    let (folder, paused) = (p::str(v, "folder"), p::bool(v, "paused").unwrap_or(false));
    let mut added = Vec::new();
    let mut refused = Vec::new();
    for u in urls.iter().take(MAX_BATCH) {
        let Some(u) = u.as_str() else { continue };
        if u.trim().is_empty() {
            continue;
        }
        match new_download(s, u, folder, None, paused, None, None) {
            Ok(id) => added.push(id.0),
            Err(e) => refused.push(json!({"url": u, "error": e.to_string()})),
        }
    }
    s.status = format!("Added {} of {} links", added.len(), added.len() + refused.len());
    Ok(json!({"added": added, "refused": refused}))
}

fn resume(s: &mut Session, v: &Value) -> CmdResult {
    let id = target(s, v)?;
    let Some(d) = s.get_mut(id) else { return Err(CmdError::Params(format!("no download {}", id.0))) };
    if !d.state.startable() {
        return Err(CmdError::Disabled(format!("it is {}", d.state.label().to_lowercase())));
    }
    d.state = State::Queued;
    d.error = None;
    s.dirty = true;
    Ok(json!({"id": id.0}))
}

fn pause(s: &mut Session, v: &Value) -> CmdResult {
    let id = target(s, v)?;
    let state = s.get(id).map(|d| d.state).unwrap_or_default();
    if !state.active() {
        return Err(CmdError::Disabled(format!("it is {}", state.label().to_lowercase())));
    }
    s.stop(id, State::Paused);
    Ok(json!({"id": id.0}))
}

/// Remove from the list. An unfinished download's partial file always goes; with `deleteFile`
/// the finished file goes to the Recycle Bin or Trash too (the choice is remembered for next time).
fn remove(s: &mut Session, v: &Value) -> CmdResult {
    let id = target(s, v)?;
    let finished = s.get(id).is_some_and(|d| d.state == State::Completed);
    let delete = p::bool(v, "deleteFile");
    if let Some(d) = delete {
        s.prefs.delete_files_on_remove = d;
    }
    let delete = delete.unwrap_or(false);
    if finished {
        s.stop_seeding(id);
    } else {
        s.stop(id, State::Paused);
    }
    let pos = s.downloads.iter().position(|d| d.id == id).ok_or_else(|| CmdError::Params(format!("no download {}", id.0)))?;
    let d = s.downloads.remove(pos);
    // A torrent's part is its files themselves, once it has started writing them.
    let torrent = d.torrent.is_some();
    let also_file = if torrent && !finished { d.total.is_some() } else { finished && delete };
    if (!finished && !torrent) || also_file {
        s.push_job(Job::DropPartial { id, folder: d.folder.clone(), file_name: d.file_name.clone(), also_file, torrent });
    }
    if s.selection == Some(id) {
        // The next one down the list on screen (which is newest first), else the one above.
        s.selection = s.downloads.get(pos.saturating_sub(1)).or_else(|| s.downloads.get(pos)).map(|d| d.id);
    }
    s.status = format!("Removed {}", d.file_name);
    s.dirty = true;
    Ok(json!({"id": id.0}))
}

/// Fetch a download once more from the start (its file was deleted or moved, or it failed).
fn again(s: &mut Session, v: &Value) -> CmdResult {
    let id = target(s, v)?;
    let Some(d) = s.get_mut(id) else { return Err(CmdError::Params(format!("no download {}", id.0))) };
    if d.state.active() {
        return Err(CmdError::Disabled("it is downloading".into()));
    }
    let torrent = d.torrent.is_some();
    s.stop_seeding(id);
    let Some(d) = s.get_mut(id) else { return Err(CmdError::Params(format!("no download {}", id.0))) };
    d.state = State::Queued;
    d.done = 0;
    d.error = None;
    d.missing = false;
    if torrent {
        // Its files are checked piece by piece and only what's missing or damaged is fetched, so
        // it keeps its name, and what's there is never taken for someone else's file.
        d.uploaded = 0;
    } else {
        d.total = None;
        d.file_name = names::from_url(&d.url);
    }
    s.dirty = true;
    Ok(json!({"id": id.0}))
}

fn resume_all(s: &mut Session, _: &Value) -> CmdResult {
    let mut n = 0;
    for d in s.downloads.iter_mut().filter(|d| d.state.startable()) {
        d.state = State::Queued;
        d.error = None;
        n += 1;
    }
    s.dirty |= n > 0;
    Ok(json!({"resumed": n}))
}

fn pause_all(s: &mut Session, _: &Value) -> CmdResult {
    let ids: Vec<DownloadId> = s.downloads.iter().filter(|d| d.state.active()).map(|d| d.id).collect();
    for id in &ids {
        s.stop(*id, State::Paused);
    }
    Ok(json!({"paused": ids.len()}))
}

fn clear_completed(s: &mut Session, _: &Value) -> CmdResult {
    let before = s.downloads.len();
    let finished: Vec<DownloadId> = s.downloads.iter().filter(|d| d.state == State::Completed).map(|d| d.id).collect();
    for id in finished {
        s.stop_seeding(id);
    }
    s.downloads.retain(|d| d.state != State::Completed);
    if s.selection.is_some_and(|id| s.get(id).is_none()) {
        s.selection = None;
    }
    let n = before - s.downloads.len();
    s.dirty |= n > 0;
    Ok(json!({"cleared": n}))
}

fn list(s: &mut Session, v: &Value) -> CmdResult {
    let f = match p::str(v, "filter") {
        Some(f) => Filter::parse(f).ok_or_else(|| CmdError::Params(format!("no filter `{f}`")))?,
        None => Filter::All,
    };
    Ok(Value::Array(s.downloads.iter().filter(|d| f.shows(d.state)).map(describe).collect()))
}

fn select(s: &mut Session, v: &Value) -> CmdResult {
    match v.get("id") {
        None | Some(Value::Null) => s.selection = None,
        Some(_) => s.selection = Some(target(s, v)?),
    }
    Ok(json!({"id": s.selection.map(|i| i.0)}))
}

/// Move the selection `by` rows down the list on screen.
fn step(s: &mut Session, by: isize) -> CmdResult {
    let ids: Vec<DownloadId> = s.visible().map(|d| d.id).collect();
    let at = s.selection.and_then(|sel| ids.iter().position(|i| *i == sel));
    let next = match at {
        Some(i) => i.saturating_add_signed(by).min(ids.len().saturating_sub(1)),
        None => 0,
    };
    s.selection = ids.get(next).copied().or(s.selection);
    Ok(json!({"id": s.selection.map(|i| i.0)}))
}

fn filter(s: &mut Session, v: &Value) -> CmdResult {
    let f = p::req_str(v, "filter")?;
    s.filter = Filter::parse(f).ok_or_else(|| CmdError::Params(format!("no filter `{f}`")))?;
    Ok(json!({"filter": s.filter}))
}

fn set_folder(s: &mut Session, v: &Value) -> CmdResult {
    let f = p::req_str(v, "folder")?.trim();
    if f.is_empty() {
        return Err(CmdError::Params("`folder` is empty".into()));
    }
    s.prefs.folder = f.to_string();
    s.dirty = true;
    Ok(json!({"folder": f}))
}

fn set_max_active(s: &mut Session, v: &Value) -> CmdResult {
    let n = p::u64(v, "count").ok_or_else(|| CmdError::Params("`count` (number) is required".into()))?;
    let n = usize::try_from(n).unwrap_or(MAX_ACTIVE_LIMIT).clamp(1, MAX_ACTIVE_LIMIT);
    s.prefs.max_active = n;
    s.dirty = true;
    Ok(json!({"count": n}))
}

fn set_connections(s: &mut Session, v: &Value) -> CmdResult {
    let n = p::u64(v, "count").ok_or_else(|| CmdError::Params("`count` (number) is required".into()))?;
    let n = u8::try_from(n).unwrap_or(MAX_CONNECTIONS).clamp(1, MAX_CONNECTIONS);
    s.prefs.connections = n;
    s.dirty = true;
    Ok(json!({"count": n}))
}

fn set_language(s: &mut Session, v: &Value) -> CmdResult {
    let code = v.get("language").and_then(Value::as_str).ok_or_else(|| CmdError::Params("`language` (text) is required".into()))?;
    let ok = code == "auto" || (2..=8).contains(&code.len()) && code.chars().all(|c| c.is_ascii_lowercase() || c == '-');
    if !ok {
        return Err(CmdError::Params("`language` is auto or a language code such as de or pt-br".into()));
    }
    s.prefs.language = code.to_string();
    s.dirty = true;
    Ok(json!({"language": code}))
}

fn set_subtitles(s: &mut Session, v: &Value) -> CmdResult {
    let typed = v.get("languages").and_then(Value::as_str).ok_or_else(|| CmdError::Params("`languages` (text) is required".into()))?;
    s.prefs.subtitles = typed.chars().take(100).collect::<String>().trim().to_string();
    s.dirty = true;
    Ok(json!({"languages": s.prefs.subtitles, "subLangs": downloadcraft_model::probe::subtitle_langs(&s.prefs.subtitles)}))
}

fn set_seed_ratio(s: &mut Session, v: &Value) -> CmdResult {
    let r =
        v.get("ratio").and_then(Value::as_f64).filter(|r| r.is_finite()).ok_or_else(|| CmdError::Params("`ratio` (number) is required".into()))?;
    let r = (r as f32).clamp(0.0, downloadcraft_model::MAX_SEED_RATIO);
    s.prefs.seed_ratio = (r * 10.0).round() / 10.0;
    s.dirty = true;
    Ok(json!({"ratio": s.prefs.seed_ratio}))
}

/// Change any of the torrent settings; the rest stay. The network layer is told at once.
fn set_torrent(s: &mut Session, v: &Value) -> CmdResult {
    use downloadcraft_model::torrent::{MAX_PEER_LIMIT, Protocol, proxy_url};
    let mut t = s.prefs.torrent.clone();
    let num = |k: &str| v.get(k).map(|x| x.as_u64().ok_or_else(|| CmdError::Params(format!("`{k}` is a number"))));
    if let Some(p) = num("port") {
        t.port = u16::try_from(p?).map_err(|_| CmdError::Params("`port` is 0 to 65535".into()))?;
    }
    if let Some(p) = v.get("protocol") {
        t.protocol = serde_json::from_value::<Protocol>(p.clone()).map_err(|_| CmdError::Params("`protocol` is tcp or both".into()))?;
    }
    if let Some(u) = v.get("upnp") {
        t.upnp = u.as_bool().ok_or_else(|| CmdError::Params("`upnp` is true or false".into()))?;
    }
    if let Some(n) = num("peerLimit") {
        t.peer_limit = u32::try_from(n?).unwrap_or(MAX_PEER_LIMIT).min(MAX_PEER_LIMIT);
    }
    let kib = |n: u64| u32::try_from(n).unwrap_or(u32::MAX).min(4_000_000);
    if let Some(n) = num("downKib") {
        t.down_kib = kib(n?);
    }
    if let Some(n) = num("upKib") {
        t.up_kib = kib(n?);
    }
    if let Some(p) = v.get("proxy") {
        let p = p.as_str().ok_or_else(|| CmdError::Params("`proxy` is text".into()))?;
        proxy_url(p).map_err(|e| CmdError::Params(e.into()))?;
        t.proxy = p.trim().chars().take(300).collect();
    }
    if let Some(f) = v.get("ipFilter") {
        t.ip_filter = f.as_str().ok_or_else(|| CmdError::Params("`ipFilter` is a file's path".into()))?.trim().chars().take(1000).collect();
    }
    if t != s.prefs.torrent {
        s.prefs.torrent = t.clone();
        s.dirty = true;
        s.push_job(Job::Configure { torrent: t });
    }
    Ok(serde_json::to_value(&s.prefs.torrent).unwrap_or(Value::Null))
}

/// Change the files a torrent downloads. Running, it carries on with the new choice; finished, a
/// file added to the choice sends it back to the queue for what's missing.
fn pick_files(s: &mut Session, v: &Value) -> CmdResult {
    let id = target(s, v)?;
    let mut files: Vec<usize> = v
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| CmdError::Params("`files` (array of numbers) is required".into()))?
        .iter()
        .filter_map(Value::as_u64)
        .filter_map(|i| usize::try_from(i).ok())
        .filter(|i| *i < probe::MAX_TORRENT_FILES)
        .collect();
    files.sort_unstable();
    files.dedup();
    if files.is_empty() {
        return Err(CmdError::Params("pick at least one file".into()));
    }
    let Some(d) = s.get_mut(id) else { return Err(CmdError::Params(format!("no download {}", id.0))) };
    let Some(choice) = d.torrent.as_mut() else { return Err(CmdError::Disabled("it isn't a torrent".into())) };
    // All the files listed, when the torrent's whole list is known: the same as none picked.
    let all = !choice.list.is_empty() && choice.list.iter().all(|f| files.contains(&f.index));
    let before = std::mem::replace(&mut choice.files, if all { Vec::new() } else { files.clone() });
    let grew = before.is_empty() && !all || !before.is_empty() && (all || files.iter().any(|f| !before.contains(f)));
    let (state, running) = (d.state, d.run != 0);
    if state == State::Completed && grew {
        // Finished (perhaps still sharing): back to the queue to fetch what's now picked too.
        s.stop_seeding(id);
        if let Some(d) = s.get_mut(id) {
            d.state = State::Queued;
            d.missing = false;
        }
    } else if running {
        s.push_job(Job::PickFiles { id, files: if all { Vec::new() } else { files } });
    }
    s.dirty = true;
    Ok(json!({"id": id.0}))
}

fn set_theme(s: &mut Session, v: &Value) -> CmdResult {
    let t = v.get("theme").cloned().ok_or_else(|| CmdError::Params("`theme` is required".into()))?;
    let theme: Theme = serde_json::from_value(t).map_err(|_| CmdError::Params("`theme` is system, light or dark".into()))?;
    s.prefs.theme = theme;
    s.dirty = true;
    Ok(json!({"theme": theme}))
}
