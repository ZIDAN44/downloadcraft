//! Transfers against a small HTTP server on 127.0.0.1 (never a real site).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use downloadcraft_model::{DownloadId, Event};

use crate::http::{Fetch, agent, agent_with, fetch};
use crate::{Sink, Worker};

#[derive(Clone, Default)]
pub(crate) struct Serve {
    pub(crate) body: Vec<u8>,
    pub(crate) ranges: bool,
    pub(crate) disposition: Option<String>,
    /// Close the first answer after this many body bytes.
    pub(crate) cut_after: Option<usize>,
    pub(crate) status: Option<u16>,
    /// Turn away this many requests for a closed range (`bytes=a-b`) with 503, as servers that
    /// allow few connections per client do.
    pub(crate) refuse_parts: usize,
    /// After this many body bytes, the first answer goes silent, keeping the connection open.
    pub(crate) stall_after: Option<usize>,
    /// The first answer is this status (later ones are the file).
    pub(crate) first_status: Option<u16>,
    /// Claim this size in `Content-Length`, sending nothing.
    pub(crate) claim_len: Option<u64>,
}

/// Serve `s` until the test ends, each connection on a thread of its own; the `Range` header of
/// every request is recorded (`"123"` for `bytes=123-`, `"5-9"` for `bytes=5-9`).
pub(crate) fn serve(s: Serve) -> (String, Arc<Mutex<Vec<Option<String>>>>) {
    let refused = Arc::new(AtomicUsize::new(0));
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/files/data.bin", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let answered = Arc::new(AtomicUsize::new(0));
    let s = Arc::new(s);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let (s, log, answered, refused) = (s.clone(), log.clone(), answered.clone(), refused.clone());
            std::thread::spawn(move || answer(&s, stream, &log, &answered, &refused));
        }
    });
    (url, seen)
}

fn answer(s: &Serve, mut stream: std::net::TcpStream, log: &Mutex<Vec<Option<String>>>, answered: &AtomicUsize, refused: &AtomicUsize) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut range: Option<(usize, Option<usize>)> = None;
    let mut raw = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
            let v = v.trim().to_string();
            let (a, b) = v.split_once('-').unwrap_or((&v, ""));
            range = a.parse::<usize>().ok().map(|a| (a, b.parse::<usize>().ok()));
            raw = Some(v.trim_end_matches('-').to_string());
        }
    }
    log.lock().unwrap().push(raw);
    let first = answered.fetch_add(1, Ordering::SeqCst) == 0;
    let len = s.body.len();
    let refuse = matches!(range, Some((_, Some(_)))) && refused.fetch_add(1, Ordering::SeqCst) < s.refuse_parts;
    let (status, start, end) = match (s.status, range) {
        (Some(code), _) => (code, 0, 0),
        _ if first && s.first_status.is_some() => (s.first_status.unwrap_or(500), 0, 0),
        _ if refuse => (503, 0, 0),
        (None, Some((a, _))) if s.ranges && a >= len => (416, 0, 0),
        (None, Some((a, b))) if s.ranges => (206, a, b.map_or(len, |b| (b + 1).min(len))),
        _ => (200, 0, len),
    };
    let part = if status == 200 || status == 206 { &s.body[start..end] } else { &[][..] };
    let (part, length) = match s.claim_len {
        Some(n) => (&[][..], n),
        None => (part, part.len() as u64),
    };
    let mut head = format!("HTTP/1.1 {status} X\r\nContent-Length: {length}\r\nConnection: close\r\n");
    if s.ranges {
        head.push_str("Accept-Ranges: bytes\r\n");
    }
    if status == 206 {
        head.push_str(&format!("Content-Range: bytes {start}-{}/{len}\r\n", end - 1));
    }
    if let Some(d) = &s.disposition {
        head.push_str(&format!("Content-Disposition: {d}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    if let (Some(n), true) = (s.stall_after, first) {
        let _ = stream.write_all(&part[..n.min(part.len())]);
        let _ = stream.flush();
        std::thread::sleep(Duration::from_secs(5));
        return;
    }
    let send = match s.cut_after {
        Some(n) if first => &part[..n.min(part.len())],
        _ => part,
    };
    let _ = stream.write_all(send);
    let _ = stream.flush();
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("downloadcraft-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn body(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i * 7 % 251) as u8).collect()
}

fn run(url: &str, folder: &Path, name: &str, may_rename: bool) -> Vec<Event> {
    let (tx, rx) = channel();
    let sink = Sink::new(tx, Arc::new(|| {}));
    let f = Fetch { id: DownloadId(1), run: 1, url: url.into(), folder: folder.to_path_buf(), file_name: name.into(), may_rename, connections: 1 };
    fetch(&agent(), &f, &AtomicBool::new(false), &sink);
    drop(sink);
    rx.iter().collect()
}

fn last(events: &[Event]) -> &Event {
    events.last().unwrap()
}

#[test]
fn downloads_a_whole_file() {
    let data = body(3_000_000);
    let (url, _) = serve(Serve { body: data.clone(), ..Default::default() });
    let dir = temp_dir("whole");
    let ev = run(&url, &dir, "data.bin", true);
    assert!(matches!(ev.first(), Some(Event::Started { total: Some(3_000_000), resumable: false, .. })), "{ev:?}");
    assert_eq!(last(&ev), &Event::Finished { id: DownloadId(1), run: 1, done: 3_000_000 });
    assert_eq!(std::fs::read(dir.join("data.bin")).unwrap(), data);
    assert!(!dir.join("data.bin.dcpart").exists());
    // Again: the name is taken, so the second copy is numbered.
    let ev = run(&url, &dir, "data.bin", true);
    assert!(matches!(ev.first(), Some(Event::Started { file_name, .. }) if file_name == "data (1).bin"), "{ev:?}");
    assert_eq!(std::fs::read(dir.join("data (1).bin")).unwrap(), data);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_broken_connection_is_tried_again_with_a_byte_range() {
    let data = body(500_000);
    let (url, seen) = serve(Serve { body: data.clone(), ranges: true, cut_after: Some(123_456), ..Default::default() });
    let dir = temp_dir("resume");
    let ev = run(&url, &dir, "data.bin", true);
    assert!(matches!(last(&ev), Event::Finished { done: 500_000, .. }), "carried on by itself: {ev:?}");
    let restarts: Vec<u64> = ev.iter().filter_map(|e| if let Event::Started { done, .. } = e { Some(*done) } else { None }).collect();
    assert_eq!(restarts, [0, 123_456], "the second try starts where the first stopped");
    assert_eq!(std::fs::read(dir.join("data.bin")).unwrap(), data);
    assert_eq!(seen.lock().unwrap().clone(), vec![None, Some("123456".to_string())]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_stalled_connection_is_dropped_and_tried_again() {
    let data = body(400_000);
    let (url, seen) = serve(Serve { body: data.clone(), ranges: true, stall_after: Some(100_000), ..Default::default() });
    let dir = temp_dir("stall");
    let (tx, rx) = channel();
    let sink = Sink::new(tx, Arc::new(|| {}));
    let f = Fetch { id: DownloadId(1), run: 1, url: url.clone(), folder: dir.clone(), file_name: "s.bin".into(), may_rename: true, connections: 1 };
    let started = std::time::Instant::now();
    fetch(&agent_with(Duration::from_millis(500)), &f, &AtomicBool::new(false), &sink);
    drop(sink);
    let ev: Vec<Event> = rx.iter().collect();
    assert!(matches!(last(&ev), Event::Finished { done: 400_000, .. }), "{ev:?}");
    assert!(started.elapsed() < Duration::from_secs(4), "the stall was noticed in about half a second, not after the server woke");
    assert_eq!(std::fs::read(dir.join("s.bin")).unwrap(), data);
    assert_eq!(seen.lock().unwrap().last().cloned().flatten().as_deref(), Some("100000"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_is_not_tried_again() {
    let (url, seen) = serve(Serve { body: body(10), status: Some(404), ..Default::default() });
    let dir = temp_dir("no-retry");
    let ev = run(&url, &dir, "x.bin", true);
    assert!(matches!(last(&ev), Event::Failed { error, .. } if error.contains("404")), "{ev:?}");
    assert_eq!(seen.lock().unwrap().len(), 1, "asked once");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_server_without_ranges_starts_over() {
    let data = body(10_000);
    let (url, _) = serve(Serve { body: data.clone(), ..Default::default() });
    let dir = temp_dir("norange");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("data.bin.dcpart"), b"stale bytes").unwrap();
    let ev = run(&url, &dir, "data.bin", false);
    assert!(matches!(ev.first(), Some(Event::Started { done: 0, .. })), "{ev:?}");
    assert_eq!(std::fs::read(dir.join("data.bin")).unwrap(), data);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_already_whole_partial_file_finishes() {
    let data = body(1000);
    let (url, _) = serve(Serve { body: data.clone(), ranges: true, ..Default::default() });
    let dir = temp_dir("whole-part");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("data.bin.dcpart"), &data).unwrap();
    let ev = run(&url, &dir, "data.bin", false);
    assert_eq!(ev, vec![Event::Finished { id: DownloadId(1), run: 1, done: 1000 }]);
    assert_eq!(std::fs::read(dir.join("data.bin")).unwrap(), data);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_servers_name_is_used_and_made_safe() {
    let (url, _) = serve(Serve { body: body(10), disposition: Some(r#"attachment; filename="..\..\Report: Q3.pdf""#.into()), ..Default::default() });
    let dir = temp_dir("disposition");
    let ev = run(&url, &dir, "data.bin", true);
    assert!(matches!(last(&ev), Event::Finished { .. }), "{ev:?}");
    assert!(dir.join("_.._Report_ Q3.pdf").exists(), "{:?}", std::fs::read_dir(&dir).unwrap().collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn http_errors_are_explained() {
    let (url, _) = serve(Serve { body: body(10), status: Some(404), ..Default::default() });
    let dir = temp_dir("404");
    let ev = run(&url, &dir, "data.bin", true);
    assert!(matches!(last(&ev), Event::Failed { error, .. } if error.contains("404")), "{ev:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_file_bigger_than_the_free_space_fails_before_writing() {
    let (url, seen) = serve(Serve { body: body(10), claim_len: Some(1 << 60), ..Default::default() });
    let dir = temp_dir("no-room");
    let ev = run(&url, &dir, "huge.bin", true);
    assert!(matches!(&ev[..], [Event::Failed { error, .. }] if error.contains("not enough room") && error.contains("1153 PB")), "{ev:?}");
    assert_eq!(seen.lock().unwrap().len(), 1, "not tried again");
    assert!(!dir.join("huge.bin.dcpart").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn room_is_checked_against_what_is_still_to_come() {
    assert!(crate::http::check_room(100, 100).is_ok());
    let e = crate::http::check_room(3 << 30, 1 << 30).unwrap_err().to_string();
    assert!(e.contains("3.22 GB needed") && e.contains("1.07 GB free"), "{e}");
}

#[test]
fn a_busy_server_is_asked_again() {
    let data = body(50_000);
    let (url, seen) = serve(Serve { body: data.clone(), first_status: Some(503), ..Default::default() });
    let dir = temp_dir("busy");
    let ev = run(&url, &dir, "b.bin", true);
    assert!(matches!(last(&ev), Event::Finished { done: 50_000, .. }), "{ev:?}");
    assert_eq!(seen.lock().unwrap().len(), 2, "503, then the file");
    assert_eq!(std::fs::read(dir.join("b.bin")).unwrap(), data);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_worker_runs_jobs_and_drops_partials() {
    let data = body(200_000);
    let (url, _) = serve(Serve { body: data.clone(), ..Default::default() });
    let dir = temp_dir("worker");
    let (tx, rx): (_, Receiver<Event>) = channel();
    let mut w = Worker::new(Sink::new(tx, Arc::new(|| {})));
    let folder = dir.to_string_lossy().into_owned();
    w.submit(downloadcraft_model::Job::Fetch {
        id: DownloadId(4),
        run: 9,
        url,
        folder: folder.clone(),
        file_name: "w.bin".into(),
        may_rename: true,
        connections: 1,
    });
    let done = rx.iter().find(|e| !matches!(e, Event::Started { .. } | Event::Progress { .. }));
    assert_eq!(done, Some(Event::Finished { id: DownloadId(4), run: 9, done: 200_000 }));
    std::fs::write(dir.join("gone.bin.dcpart"), b"x").unwrap();
    w.submit(downloadcraft_model::Job::DropPartial { id: DownloadId(5), folder, file_name: "gone.bin".into(), also_file: false });
    for _ in 0..100 {
        if !dir.join("gone.bin.dcpart").exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!dir.join("gone.bin.dcpart").exists());
    assert!(dir.join("w.bin").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

fn run_on(url: &str, folder: &Path, name: &str, may_rename: bool, connections: u8) -> Vec<Event> {
    let (tx, rx) = channel();
    let sink = Sink::new(tx, Arc::new(|| {}));
    let f = Fetch { id: DownloadId(1), run: 1, url: url.into(), folder: folder.to_path_buf(), file_name: name.into(), may_rename, connections };
    fetch(&agent(), &f, &AtomicBool::new(false), &sink);
    drop(sink);
    rx.iter().collect()
}

#[test]
fn a_big_file_comes_in_parts_on_several_connections() {
    let data = body(9_000_000);
    let (url, seen) = serve(Serve { body: data.clone(), ranges: true, ..Default::default() });
    let dir = temp_dir("parts");
    let ev = run_on(&url, &dir, "big.bin", true, 4);
    assert!(matches!(last(&ev), Event::Finished { done: 9_000_000, .. }), "{ev:?}");
    assert_eq!(std::fs::read(dir.join("big.bin")).unwrap(), data);
    assert!(!dir.join("big.bin.dcpart.parts").exists(), "the parts' record goes once the file is whole");
    let ranges = seen.lock().unwrap().clone();
    assert_eq!(ranges.first().cloned().flatten().as_deref(), Some("0"), "the first ask finds out ranges work");
    assert!(ranges.iter().flatten().filter(|r| r.contains('-')).count() >= 3, "the other parts are asked for by range: {ranges:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_part_cut_off_is_fetched_again() {
    let data = body(4_500_000);
    let (url, _) = serve(Serve { body: data.clone(), ranges: true, cut_after: Some(300_000), ..Default::default() });
    let dir = temp_dir("parts-cut");
    let ev = run_on(&url, &dir, "cut.bin", true, 3);
    assert!(matches!(last(&ev), Event::Finished { done: 4_500_000, .. }), "{ev:?}");
    assert_eq!(std::fs::read(dir.join("cut.bin")).unwrap(), data);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn parts_carry_on_from_their_record() {
    let data = body(4 * 1024 * 1024);
    let (url, seen) = serve(Serve { body: data.clone(), ranges: true, ..Default::default() });
    let dir = temp_dir("parts-resume");
    std::fs::create_dir_all(&dir).unwrap();
    // Two parts, each half done: what a pause leaves.
    let half = 2 * 1024 * 1024;
    let mut partial = vec![0u8; data.len()];
    partial[..half / 2].copy_from_slice(&data[..half / 2]);
    partial[half..half + half / 2].copy_from_slice(&data[half..half + half / 2]);
    std::fs::write(dir.join("r.bin.dcpart"), &partial).unwrap();
    let record = format!(r#"{{"total":{},"parts":[[0,{half},{}],[{half},{},{}]]}}"#, data.len(), half / 2, data.len(), half + half / 2);
    std::fs::write(dir.join("r.bin.dcpart.parts"), record).unwrap();
    let ev = run_on(&url, &dir, "r.bin", false, 2);
    assert!(matches!(ev.first(), Some(Event::Started { done, .. }) if *done == half as u64), "{ev:?}");
    assert!(matches!(last(&ev), Event::Finished { .. }), "{ev:?}");
    assert_eq!(std::fs::read(dir.join("r.bin")).unwrap(), data);
    let ranges = seen.lock().unwrap().clone();
    assert!(ranges.iter().flatten().all(|r| r.contains('-')), "only the missing bytes are asked for: {ranges:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_server_without_ranges_gets_one_connection() {
    let data = body(3_000_000);
    let (url, _) = serve(Serve { body: data.clone(), ..Default::default() });
    let dir = temp_dir("no-parts");
    let ev = run_on(&url, &dir, "one.bin", true, 8);
    assert!(matches!(last(&ev), Event::Finished { done: 3_000_000, .. }), "{ev:?}");
    assert_eq!(std::fs::read(dir.join("one.bin")).unwrap(), data);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn connections_the_server_turns_away_hand_their_parts_to_the_others() {
    let data = body(8_000_000);
    let (url, _) = serve(Serve { body: data.clone(), ranges: true, refuse_parts: 2, ..Default::default() });
    let dir = temp_dir("parts-refused");
    let ev = run_on(&url, &dir, "busy.bin", true, 4);
    assert!(matches!(last(&ev), Event::Finished { done: 8_000_000, .. }), "{ev:?}");
    assert_eq!(std::fs::read(dir.join("busy.bin")).unwrap(), data);
    // Two of four turned away: the next download from that server opens fewer. How many depends on
    // how many were still going at the refusal (on a slow runner a served one may have finished
    // already: macOS CI remembered 1), so the test holds it to fewer than asked, at least one.
    let cap = crate::http::segmented::connections_for(&url, 4);
    assert!((1..=2).contains(&cap), "{cap}");
    assert_eq!(crate::http::segmented::connections_for(&url, 1), 1);
    assert_eq!(crate::http::segmented::connections_for("http://127.0.0.1:9/other", 4), 4, "only that server");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn single_checksum_files_are_read_in_both_forms() {
    use crate::video::tools::single_sum;
    let hex = "CBA84B26E0DF5AE7F74FC66B86A95B866872A86292EBD84FA1E959927EDD93BF";
    // PowerShell's Get-FileHash list, as Deno publishes for Windows; sha256sum's line elsewhere.
    let powershell = format!("\r\nAlgorithm : SHA256\r\nHash      : {hex}\r\nPath      : D:/a/deno.zip\r\n");
    assert_eq!(single_sum(&powershell).as_deref(), Some(hex.to_ascii_lowercase().as_str()));
    assert_eq!(
        single_sum(&format!("{}  deno-x86_64-unknown-linux-gnu.zip\n", hex.to_ascii_lowercase())).as_deref(),
        Some(hex.to_ascii_lowercase().as_str())
    );
    assert_eq!(single_sum("Hash : nothere"), None);
    assert_eq!(single_sum(&"a".repeat(65)), None);
}
