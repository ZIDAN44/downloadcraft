//! Transfers against a small HTTP server on 127.0.0.1 (never a real site).

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use downloadcraft_model::{DownloadId, Event};

use crate::http::{Fetch, agent, fetch};
use crate::{Sink, Worker};

#[derive(Clone, Default)]
pub(crate) struct Serve {
    pub(crate) body: Vec<u8>,
    pub(crate) ranges: bool,
    pub(crate) disposition: Option<String>,
    /// Close the first answer after this many body bytes.
    pub(crate) cut_after: Option<usize>,
    pub(crate) status: Option<u16>,
}

/// Serve `s` until the test ends; the `Range` header of every request is recorded.
pub(crate) fn serve(s: Serve) -> (String, Arc<Mutex<Vec<Option<String>>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/files/data.bin", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let answered = AtomicUsize::new(0);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut range = None;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("range: bytes=") {
                    range = v.trim().trim_end_matches('-').parse::<usize>().ok();
                }
            }
            log.lock().unwrap().push(range.map(|r| r.to_string()));
            let first = answered.fetch_add(1, Ordering::SeqCst) == 0;
            let mut head = String::new();
            let (status, start) = match (s.status, range) {
                (Some(code), _) => (code, 0),
                (None, Some(r)) if s.ranges && r >= s.body.len() => (416, 0),
                (None, Some(r)) if s.ranges => (206, r),
                _ => (200, 0),
            };
            let part = if status == 200 || status == 206 { &s.body[start..] } else { &[][..] };
            head.push_str(&format!("HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n", part.len()));
            if s.ranges {
                head.push_str("Accept-Ranges: bytes\r\n");
            }
            if status == 206 {
                head.push_str(&format!("Content-Range: bytes {start}-{}/{}\r\n", s.body.len() - 1, s.body.len()));
            }
            if let Some(d) = &s.disposition {
                head.push_str(&format!("Content-Disposition: {d}\r\n"));
            }
            head.push_str("\r\n");
            let _ = stream.write_all(head.as_bytes());
            let send = match s.cut_after {
                Some(n) if first => &part[..n.min(part.len())],
                _ => part,
            };
            let _ = stream.write_all(send);
            let _ = stream.flush();
        }
    });
    (url, seen)
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
    let f = Fetch { id: DownloadId(1), run: 1, url: url.into(), folder: folder.to_path_buf(), file_name: name.into(), may_rename };
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
fn resumes_with_a_byte_range_after_a_broken_connection() {
    let data = body(500_000);
    let (url, seen) = serve(Serve { body: data.clone(), ranges: true, cut_after: Some(123_456), ..Default::default() });
    let dir = temp_dir("resume");
    let ev = run(&url, &dir, "data.bin", true);
    assert!(matches!(last(&ev), Event::Failed { error, .. } if error.contains("Resume")), "{ev:?}");
    assert_eq!(std::fs::metadata(dir.join("data.bin.dcpart")).unwrap().len(), 123_456);
    let ev = run(&url, &dir, "data.bin", false);
    assert!(matches!(ev.first(), Some(Event::Started { done: 123_456, total: Some(500_000), resumable: true, .. })), "{ev:?}");
    assert!(matches!(last(&ev), Event::Finished { done: 500_000, .. }), "{ev:?}");
    assert_eq!(std::fs::read(dir.join("data.bin")).unwrap(), data);
    assert_eq!(seen.lock().unwrap().clone(), vec![None, Some("123456".to_string())]);
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
    let ev = run("http://127.0.0.1:1/nothing", &dir, "x", true);
    assert!(matches!(last(&ev), Event::Failed { .. }), "{ev:?}");
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
    w.submit(downloadcraft_model::Job::Fetch { id: DownloadId(4), run: 9, url, folder: folder.clone(), file_name: "w.bin".into(), may_rename: true });
    let done = rx.iter().find(|e| !matches!(e, Event::Started { .. } | Event::Progress { .. }));
    assert_eq!(done, Some(Event::Finished { id: DownloadId(4), run: 9, done: 200_000 }));
    std::fs::write(dir.join("gone.bin.dcpart"), b"x").unwrap();
    w.submit(downloadcraft_model::Job::DropPartial { id: DownloadId(5), folder, file_name: "gone.bin".into() });
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
