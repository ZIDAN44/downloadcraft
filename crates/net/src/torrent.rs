//! Torrents and magnet links, through librqbit (the engine of the rqbit client, Apache-2.0):
//! BitTorrent (BEP 3) with the extension protocol (BEP 10), a magnet link's details fetched from
//! peers (BEP 9), and peers found through trackers and the DHT (BEP 5).
//!
//! One librqbit session serves every torrent, on a Tokio runtime of its own that starts with the
//! first torrent. A transfer runs on its own thread, like an HTTP one: it gets the torrent's
//! details, checks every path inside it, adds it to the session, reports progress, and when it's
//! all there shares it until it has sent `seed_ratio` times its size. A file already in the folder
//! is never written over on a first start; on later starts librqbit checks what's on disk piece by
//! piece and fetches only what's missing.
//!
//! A torrent's files are written in place under their own names (there's no `.dcpart`): a
//! torrent of one file becomes `folder/name`, one of several `folder/name/…`. Only the files
//! picked appear (`torrent/storage.rs`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use downloadcraft_model::probe::{MAX_TORRENT_FILES, Probe, TorrentFile};
use downloadcraft_model::{DownloadId, Event, names, source};
use librqbit::storage::StorageFactoryExt;
use librqbit::{AddTorrent, AddTorrentOptions, AddTorrentResponse, ListOnlyResponse, Session, SessionOptions};

use crate::{NetError, Sink};

mod storage;

/// A torrent to download (`Job::FetchTorrent`).
pub struct TorrentFetch {
    pub id: DownloadId,
    pub run: u64,
    pub url: String,
    pub folder: PathBuf,
    pub file_name: String,
    pub files: Vec<usize>,
    pub first: bool,
    pub seed_ratio: f32,
}

/// The largest `.torrent` file read (they're rarely over a few megabytes).
const MAX_TORRENT_BYTES: u64 = 16 * 1024 * 1024;
/// How long a magnet link may take to find someone who sends its details.
const DETAILS_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// How long Add link waits for them before saying so.
const PROBE_TIMEOUT: Duration = Duration::from_secs(90);
/// How often progress is reported.
const TICK: Duration = Duration::from_millis(500);

struct Engine {
    rt: tokio::runtime::Runtime,
    session: Arc<Session>,
}

static ENGINE: OnceLock<Result<Engine, String>> = OnceLock::new();
static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Where the DHT remembers the nodes it knew (`<data>/torrents`), so it finds peers sooner next
/// time. Set before the first torrent; without it nothing is kept.
pub fn set_dir(dir: PathBuf) {
    let _ = DIR.set(dir);
}

fn engine() -> Result<&'static Engine, NetError> {
    ENGINE.get_or_init(start).as_ref().map_err(|e| NetError::Http(e.clone()))
}

fn start() -> Result<Engine, String> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("torrents")
        .enable_all()
        .build()
        .map_err(|e| format!("couldn't start torrents: {e}"))?;
    // The DHT's nodes are kept in DownloadCraft's data folder, or not at all (never in a folder
    // of librqbit's choosing).
    let kept = DIR.get().and_then(|d| std::fs::create_dir_all(d).ok().map(|()| d.join("dht.json")));
    let persistence = kept.map(|f| librqbit::dht::DhtPersistenceConfig { config_filename: Some(f), ..Default::default() });
    let dht = librqbit::DhtSessionConfig { persistence, ..Default::default() };
    let opts = SessionOptions {
        dht: Some(dht),
        // Incoming connections on a port picked by the system; no UPnP (nothing is opened on the
        // router behind the user's back).
        listen: Some(librqbit::ListenerOptions::default()),
        client_name_and_version: Some(format!("DownloadCraft {}", env!("CARGO_PKG_VERSION"))),
        ..SessionOptions::default()
    };
    let session = rt.block_on(Session::new_with_opts(std::env::temp_dir(), opts)).map_err(|e| format!("couldn't start torrents: {e:#}"))?;
    Ok(Engine { rt, session })
}

/// Look at a torrent before it's added: its name and files (blocking; a magnet link waits for a
/// peer to send its details, up to [`PROBE_TIMEOUT`]).
pub fn probe_torrent(url: &str) -> Probe {
    let never = AtomicBool::new(false);
    match details(url, &never, PROBE_TIMEOUT) {
        Ok(found) => match describe(&found) {
            Ok((name, files)) => Probe::Torrent { name, files },
            Err(e) => Probe::Failed(e.to_string()),
        },
        Err(e) => Probe::Failed(e.to_string()),
    }
}

/// What to give librqbit for `url`: a magnet link as it is, a `.torrent` file's bytes (fetched
/// here over HTTPS, or read from this computer).
fn source_of(url: &str) -> Result<AddTorrent<'static>, NetError> {
    if url.get(..7).is_some_and(|s| s.eq_ignore_ascii_case("magnet:")) {
        return Ok(AddTorrent::from_url(url.to_string()));
    }
    let bytes = if source::is_torrent_file(url) { read_file(Path::new(url))? } else { fetch_file(url)? };
    Ok(AddTorrent::from_bytes(bytes))
}

fn read_file(path: &Path) -> Result<Vec<u8>, NetError> {
    use std::io::Read;
    let f = std::fs::File::open(path).map_err(|e| NetError::Disk(format!("couldn't open the torrent file: {e}")))?;
    let mut bytes = Vec::new();
    f.take(MAX_TORRENT_BYTES + 1).read_to_end(&mut bytes).map_err(|e| NetError::Disk(format!("couldn't read the torrent file: {e}")))?;
    if bytes.len() as u64 > MAX_TORRENT_BYTES {
        return Err(NetError::Disk("that isn't a torrent file (it's too big)".into()));
    }
    Ok(bytes)
}

fn fetch_file(url: &str) -> Result<Vec<u8>, NetError> {
    let agent = crate::http::agent();
    let mut resp = agent.get(url).call().map_err(|e| NetError::Http(format!("couldn't get the torrent file: {e}")))?;
    resp.body_mut().with_config().limit(MAX_TORRENT_BYTES).read_to_vec().map_err(|e| NetError::Http(format!("couldn't get the torrent file: {e}")))
}

/// The torrent's details (librqbit's "list only"): for a magnet link, asked of peers. Gives up
/// after `timeout`, or when `cancel` is set.
fn details(url: &str, cancel: &AtomicBool, timeout: Duration) -> Result<ListOnlyResponse, NetError> {
    let e = engine()?;
    let add = source_of(url)?;
    let session = e.session.clone();
    let job = e.rt.spawn(async move {
        let opts = AddTorrentOptions { list_only: true, ..AddTorrentOptions::default() };
        session.add_torrent(add, Some(opts)).await.map_err(|e| format!("{e:#}"))
    });
    let since = Instant::now();
    while !job.is_finished() {
        if cancel.load(Ordering::Relaxed) || since.elapsed() > timeout {
            job.abort();
            return Err(if cancel.load(Ordering::Relaxed) {
                NetError::Http("stopped".into())
            } else {
                NetError::Retry("no one sharing it sent its details; Resume tries again".into())
            });
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    match e.rt.block_on(job) {
        Ok(Ok(AddTorrentResponse::ListOnly(found))) => Ok(found),
        Ok(Ok(_)) => Err(NetError::Http("the torrent is already being downloaded".into())),
        Ok(Err(why)) => Err(NetError::Http(plain_error(&why))),
        Err(_) => Err(NetError::Http("stopped".into())),
    }
}

/// librqbit's error, said shortly.
fn plain_error(why: &str) -> String {
    let lower = why.to_ascii_lowercase();
    if lower.contains("magnet") && lower.contains("valid") {
        "that magnet link is damaged".into()
    } else if lower.contains("bencode") || lower.contains("deserializ") || lower.contains("torrent file") {
        "that isn't a torrent file, or it's damaged".into()
    } else {
        why.chars().take(200).collect()
    }
}

/// The torrent's name (safe as one file name) and its files, each path checked: a torrent whose
/// paths can't be written here as they are is refused.
fn describe(found: &ListOnlyResponse) -> Result<(String, Vec<TorrentFile>), NetError> {
    let info = &found.info;
    let mut files = Vec::new();
    for (index, fd) in info.iter_file_details().enumerate() {
        // BEP 47 padding, between files to line them up with pieces: never written.
        if fd.attrs().padding {
            continue;
        }
        if files.len() >= MAX_TORRENT_FILES {
            return Err(NetError::Http(format!("the torrent has more than {MAX_TORRENT_FILES} files")));
        }
        let parts = fd.filename.to_vec();
        if let Some(bad) = parts.iter().find(|p| names::unsafe_part(p, cfg!(windows))) {
            let bad: String = bad.chars().take(60).collect();
            return Err(NetError::Http(format!("a file in the torrent has a name this computer can't use (\u{201c}{bad}\u{201d})")));
        }
        files.push(TorrentFile { index, path: parts.join("/"), size: fd.len });
    }
    let name = info.name().map(|n| names::sanitize(&n)).unwrap_or_else(|| names::FALLBACK.to_string());
    Ok((name, files))
}

/// Whether the torrent's files go in a folder of their own (several files, or one inside folders).
fn in_own_folder(files: &[TorrentFile]) -> bool {
    files.len() != 1 || files.iter().any(|f| f.path.contains('/'))
}

/// Download `spec`, reporting to `sink`, until it's all there and shared, or `cancel` is set.
pub fn fetch(spec: &TorrentFetch, cancel: &AtomicBool, sink: &Sink) {
    let (id, run) = (spec.id, spec.run);
    if let Err(e) = transfer(spec, cancel, sink)
        && !cancel.load(Ordering::Relaxed)
    {
        sink.send(Event::Failed { id, run, error: e.to_string() });
    }
}

fn transfer(spec: &TorrentFetch, cancel: &AtomicBool, sink: &Sink) -> Result<(), NetError> {
    let (id, run) = (spec.id, spec.run);
    let found = details(&spec.url, cancel, DETAILS_TIMEOUT)?;
    let (name, files) = describe(&found)?;
    let own = in_own_folder(&files);
    // The name in the folder: the torrent's folder, or its one file.
    let top = if own { name.clone() } else { files.first().map(|f| f.path.clone()).unwrap_or(name.clone()) };
    let out = if own { spec.folder.join(&top) } else { spec.folder.clone() };
    // Indexes into the torrent's own list (padding included), as librqbit counts them.
    let only: Vec<usize> = spec.files.iter().copied().filter(|i| files.iter().any(|f| f.index == *i)).collect();
    let picked = |i: usize| only.is_empty() || only.contains(&i);
    if spec.first {
        // Never written over: an earlier file of the same name stays as it is.
        let taken = if own { out.exists() } else { out.join(&top).exists() };
        if taken {
            return Err(NetError::Disk(format!("\u{201c}{top}\u{201d} is already in the folder; move it, or save the torrent somewhere else")));
        }
    }
    let total = files.iter().filter(|f| picked(f.index)).fold(0u64, |a, f| a.saturating_add(f.size));
    if spec.first {
        crate::http::room_for(&spec.folder, total)?;
    }

    let e = engine()?;
    let opts = AddTorrentOptions {
        output_folder: Some(out.to_string_lossy().into_owned()),
        only_files: (!only.is_empty()).then(|| only.clone()),
        overwrite: !spec.first,
        storage_factory: Some(storage::PickedFactory { out: out.clone(), picked: (!only.is_empty()).then(|| only.clone()) }.boxed()),
        ..AddTorrentOptions::default()
    };
    let session = e.session.clone();
    let bytes = found.torrent_bytes.clone();
    let added = e.rt.block_on(async move { session.add_torrent(AddTorrent::from_bytes(bytes), Some(opts)).await });
    let handle = match added {
        Ok(AddTorrentResponse::Added(_, h)) => h,
        Ok(AddTorrentResponse::AlreadyManaged(..)) => return Err(NetError::Http("the torrent is already being downloaded".into())),
        Ok(AddTorrentResponse::ListOnly(_)) => return Err(NetError::Http("the torrent didn't start".into())),
        Err(why) => return Err(NetError::Http(plain_error(&format!("{why:#}")))),
    };
    let hash = handle.info_hash();
    let forget = || {
        let session = e.session.clone();
        if let Err(why) = e.rt.block_on(async move { session.delete(hash.into(), false).await }) {
            log::warn!("couldn't let go of torrent {}: {why:#}", hash.as_string());
        }
    };
    let first = handle.stats();
    sink.send(Event::Started { id, run, total: Some(total), resumable: true, file_name: top, done: first.progress_bytes.min(total) });

    // Downloading.
    loop {
        if cancel.load(Ordering::Relaxed) {
            forget();
            return Ok(());
        }
        let st = handle.stats();
        if let Some(why) = st.error {
            forget();
            return Err(disk_or_net(&why));
        }
        if st.finished {
            // librqbit counts whole pieces, which can run into files left out.
            sink.send(Event::Finished { id, run, done: total });
            break;
        }
        let speed = st.live.as_ref().map_or(0, |l| l.download_speed.as_bytes());
        sink.send(Event::Progress { id, run, done: st.progress_bytes.min(total), speed, segments: Vec::new() });
        std::thread::sleep(TICK);
    }

    // Sharing, while DownloadCraft is open, until it has sent `seed_ratio` times its size.
    let goal = (f64::from(spec.seed_ratio.max(0.0)) * total as f64) as u64;
    let mut last = u64::MAX;
    loop {
        let sent = handle.stats().uploaded_bytes;
        let done = spec.seed_ratio <= 0.0 || sent >= goal;
        if cancel.load(Ordering::Relaxed) || done {
            forget();
            sink.send(Event::Uploaded { id, run, uploaded: sent, seeding: false });
            return Ok(());
        }
        if sent != last {
            sink.send(Event::Uploaded { id, run, uploaded: sent, seeding: true });
            last = sent;
        }
        std::thread::sleep(TICK * 4);
    }
}

/// A torrent's error as the list shows it: a full disk said plainly, the rest shortened.
fn disk_or_net(why: &str) -> NetError {
    let lower = why.to_ascii_lowercase();
    if lower.contains("no space") || lower.contains("disk full") || lower.contains("not enough space") || lower.contains("os error 112") {
        NetError::Disk("the disk is full".into())
    } else {
        NetError::Http(why.chars().take(200).collect())
    }
}
