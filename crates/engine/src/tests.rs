use serde_json::{Value, json};

use crate::{CmdError, DownloadId, Event, Job, Session, State};

fn session() -> Session {
    let mut s = Session::new();
    s.prefs.folder = "/downloads".into();
    s
}

fn fetches(jobs: &[Job]) -> Vec<(DownloadId, u64)> {
    jobs.iter().filter_map(|j| if let Job::Fetch { id, run, .. } = j { Some((*id, *run)) } else { None }).collect()
}

#[test]
fn every_command_survives_junk_params() {
    let junk = [
        json!(null),
        json!({}),
        json!([]),
        json!("x"),
        json!(-1),
        json!({"id": -5, "url": 7, "urls": "nope", "filter": [], "folder": {}, "count": -3.5, "name": null}),
        json!({"id": 1e300, "url": "", "urls": [1, null, "", "https://e.com/a"], "filter": "nope", "folder": "", "count": 1e300}),
        json!({"id": "1", "url": "magnet:?xt=urn:btih:x", "count": 0}),
    ];
    let ids: Vec<&str> = Session::new().registry().all().iter().map(|c| c.id).collect();
    for id in ids {
        for params in &junk {
            let mut s = session();
            let _ = s.run("download.add", &json!({"url": "https://example.com/a.zip"}));
            let _ = s.run(id, params);
            let _ = s.take_jobs();
        }
    }
}

#[test]
fn adding_queues_and_starts_up_to_the_limit() {
    let mut s = session();
    s.prefs.max_active = 2;
    for n in 0..3 {
        s.run("download.add", &json!({"url": format!("https://example.com/f{n}.bin")})).unwrap();
    }
    let jobs = s.take_jobs();
    assert_eq!(fetches(&jobs).len(), 2);
    assert_eq!(s.count(crate::Filter::Active), 3);
    let Some(Job::Fetch { file_name, folder, may_rename, .. }) = jobs.first() else { panic!("{jobs:?}") };
    assert_eq!((file_name.as_str(), folder.as_str(), *may_rename), ("f0.bin", "/downloads", true));
    // When the first finishes, the third starts.
    let (id, run) = fetches(&jobs)[0];
    s.apply(Event::Finished { id, run, done: 10 });
    assert_eq!(s.get(id).map(|d| (d.state, d.total)), Some((State::Completed, Some(10))));
    assert_eq!(fetches(&s.take_jobs()).len(), 1);
}

#[test]
fn unsupported_and_bad_links_are_refused() {
    let mut s = session();
    assert!(matches!(s.run("download.add", &json!({"url": "ftp://e.com/a.iso"})), Err(CmdError::Disabled(_))));
    assert!(matches!(s.run("download.add", &json!({"url": "https://mega.nz/file/x#k"})), Err(CmdError::Disabled(_))));
    assert!(matches!(s.run("download.add", &json!({"url": "not a link"})), Err(CmdError::Params(_))));
    assert!(s.downloads.is_empty());
    assert!(!s.status.is_empty());
    let r = s.run("download.addMany", &json!({"urls": ["https://e.com/a", "ftp://e.com/x", "https://e.com/b"]})).unwrap();
    assert_eq!(r["added"].as_array().map(Vec::len), Some(2));
    assert_eq!(r["refused"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_folder_is_needed() {
    let mut s = Session::new();
    assert!(matches!(s.run("download.add", &json!({"url": "https://e.com/a"})), Err(CmdError::Params(_))));
    s.run("download.add", &json!({"url": "https://e.com/a", "folder": "/tmp"})).unwrap();
}

#[test]
fn pausing_ends_the_run_and_late_events_are_ignored() {
    let mut s = session();
    s.run("download.add", &json!({"url": "https://e.com/big.iso"})).unwrap();
    let (id, run) = fetches(&s.take_jobs())[0];
    s.apply(Event::Started { id, run, total: Some(1000), resumable: true, file_name: "Big Disc.iso".into(), done: 0 });
    s.apply(Event::Progress { id, run, done: 400, speed: 50, segments: Vec::new() });
    s.run("download.pause", &json!({"id": id.0})).unwrap();
    assert_eq!(s.take_jobs(), vec![Job::Stop { id }]);
    // The old transfer reports once more on its way out: dropped.
    s.apply(Event::Progress { id, run, done: 450, speed: 50, segments: Vec::new() });
    s.apply(Event::Failed { id, run, error: "stopped".into() });
    let d = s.get(id).unwrap();
    assert_eq!((d.state, d.done, d.file_name.as_str()), (State::Paused, 400, "Big Disc.iso"));
    // Resuming is a new run that keeps the name the server gave.
    s.run("download.resume", &json!({})).unwrap();
    let jobs = s.take_jobs();
    let Some(Job::Fetch { run: run2, file_name, may_rename, .. }) = jobs.first() else { panic!("{jobs:?}") };
    assert!(*run2 > run);
    assert_eq!((file_name.as_str(), *may_rename), ("Big Disc.iso", false));
}

#[test]
fn failures_can_be_retried_and_removed() {
    let mut s = session();
    s.run("download.add", &json!({"url": "https://e.com/a"})).unwrap();
    let (id, run) = fetches(&s.take_jobs())[0];
    s.apply(Event::Failed { id, run, error: "404 Not Found".into() });
    assert_eq!(s.get(id).and_then(|d| d.error.clone()).as_deref(), Some("404 Not Found"));
    assert!(s.status.contains("404"));
    s.run("download.resume", &json!({"id": id.0})).unwrap();
    assert_eq!(fetches(&s.take_jobs()).len(), 1);
    s.run("download.remove", &json!({})).unwrap();
    let jobs = s.take_jobs();
    assert!(jobs.contains(&Job::Stop { id }), "{jobs:?}");
    assert!(jobs.iter().any(|j| matches!(j, Job::DropPartial { .. })), "{jobs:?}");
    assert!(s.downloads.is_empty());
    assert_eq!(s.selection, None);
}

#[test]
fn removing_a_finished_download_keeps_its_file() {
    let mut s = session();
    s.run("download.add", &json!({"url": "https://e.com/a"})).unwrap();
    let (id, run) = fetches(&s.take_jobs())[0];
    s.apply(Event::Finished { id, run, done: 3 });
    s.run("download.remove", &json!({"id": id.0})).unwrap();
    assert_eq!(s.take_jobs(), vec![]);
}

#[test]
fn pause_all_resume_all_and_clear() {
    let mut s = session();
    s.prefs.max_active = 1;
    for n in 0..3 {
        s.run("download.add", &json!({"url": format!("https://e.com/{n}")})).unwrap();
    }
    let (id, run) = fetches(&s.take_jobs())[0];
    assert_eq!(s.run("download.pauseAll", &Value::Null).unwrap()["paused"], 3);
    assert_eq!(s.count(crate::Filter::Paused), 3);
    assert_eq!(s.take_jobs(), vec![Job::Stop { id }]);
    assert_eq!(s.run("download.resumeAll", &Value::Null).unwrap()["resumed"], 3);
    let (id2, run2) = fetches(&s.take_jobs())[0];
    assert_ne!(run2, run);
    s.apply(Event::Finished { id: id2, run: run2, done: 1 });
    assert_eq!(s.run("download.clearCompleted", &Value::Null).unwrap()["cleared"], 1);
    assert_eq!(s.downloads.len(), 2);
}

#[test]
fn filters_and_selection_steps() {
    let mut s = session();
    s.run("download.addMany", &json!({"urls": ["https://e.com/1", "https://e.com/2", "https://e.com/3"], "paused": true})).unwrap();
    s.run("list.filter", &json!({"filter": "paused"})).unwrap();
    assert_eq!(s.visible().count(), 3);
    s.run("list.filter", &json!({"filter": "completed"})).unwrap();
    assert_eq!(s.visible().count(), 0);
    s.run("list.filter", &json!({"filter": "all"})).unwrap();
    s.run("list.select", &json!({"id": null})).unwrap();
    s.run("list.selectNext", &Value::Null).unwrap();
    assert_eq!(s.selection, Some(DownloadId(3)), "newest first");
    s.run("list.selectNext", &Value::Null).unwrap();
    s.run("list.selectNext", &Value::Null).unwrap();
    s.run("list.selectNext", &Value::Null).unwrap();
    assert_eq!(s.selection, Some(DownloadId(1)));
    s.run("list.selectPrevious", &Value::Null).unwrap();
    assert_eq!(s.selection, Some(DownloadId(2)));
}

#[test]
fn saves_and_loads() {
    let mut s = session();
    s.prefs.max_active = 5;
    s.run("download.add", &json!({"url": "https://e.com/a.zip"})).unwrap();
    s.run("download.add", &json!({"url": "https://e.com/b.zip", "paused": true})).unwrap();
    let (id, run) = fetches(&s.take_jobs())[0];
    s.apply(Event::Progress { id, run, done: 77, speed: 9, segments: Vec::new() });
    let saved = s.save();

    let mut t = Session::new();
    t.load(saved).unwrap();
    assert_eq!(t.prefs.max_active, 5);
    assert_eq!(t.prefs.folder, "/downloads");
    let a = t.get(id).unwrap();
    assert_eq!((a.state, a.done, a.speed), (State::Paused, 77, 0), "a transfer that was running comes back paused");
    assert!(matches!(t.take_jobs().as_slice(), [Job::Configure { .. }]), "nothing starts by itself; the torrent settings are handed on");
    t.run("download.add", &json!({"url": "https://e.com/c.zip"})).unwrap();
    assert_eq!(t.downloads.last().map(|d| d.id), Some(DownloadId(3)));
}

#[test]
fn damaged_saves_are_refused_not_fatal() {
    let mut s = session();
    for v in [json!("x"), json!({"downloads": 5}), json!({"downloads": [{"id": "x"}]})] {
        assert!(s.load(v).is_err());
    }
    s.load(json!({"downloads": [{"id": 9, "url": "https://e.com/x", "kind": "http", "file_name": "../../x", "folder": "/d"}], "prefs": {"max_active": 0}}))
        .unwrap();
    assert_eq!(s.prefs.max_active, 1);
    assert_eq!(s.get(DownloadId(9)).map(|d| d.file_name.as_str()), Some("_.._x"));
}

#[test]
fn unknown_commands_are_errors() {
    let mut s = session();
    assert_eq!(s.run("no.such", &Value::Null), Err(CmdError::Unknown("no.such".into())));
}

#[test]
fn transfers_running_when_an_update_installs_come_back_queued() {
    let mut s = session();
    s.run("download.add", &json!({"url": "https://e.com/a.zip"})).unwrap();
    s.run("download.add", &json!({"url": "https://e.com/b.zip", "paused": true})).unwrap();
    let (id, run) = fetches(&s.take_jobs())[0];
    s.apply(Event::Progress { id, run, done: 10, speed: 5, segments: Vec::new() });
    s.requeue_running();
    let mut t = Session::new();
    t.load(s.save()).unwrap();
    assert_eq!(t.get(id).map(|d| d.state), Some(State::Queued));
    assert_eq!(t.get(DownloadId(2)).map(|d| d.state), Some(State::Paused), "a paused one stays paused");
    t.pump();
    assert_eq!(fetches(&t.take_jobs()).len(), 1, "it starts again by itself");
}

#[test]
fn theme_and_connections_are_settings_that_are_kept() {
    let mut s = session();
    s.run("prefs.theme", &json!({"theme": "dark"})).unwrap();
    assert!(s.run("prefs.theme", &json!({"theme": "purple"})).is_err());
    s.run("prefs.connections", &json!({"count": 99})).unwrap();
    assert_eq!(s.prefs.connections, downloadcraft_model::MAX_CONNECTIONS, "capped");
    s.run("prefs.connections", &json!({"count": 4})).unwrap();
    s.run("download.add", &json!({"url": "https://e.com/a.iso"})).unwrap();
    let jobs = s.take_jobs();
    assert!(matches!(jobs.first(), Some(Job::Fetch { connections: 4, .. })), "{jobs:?}");
    let mut t = Session::new();
    t.load(s.save()).unwrap();
    assert_eq!((t.prefs.theme, t.prefs.connections), (downloadcraft_model::Theme::Dark, 4));
}

#[test]
fn remove_can_also_delete_the_file_and_remembers_the_choice() {
    let mut s = session();
    s.run("download.add", &json!({"url": "https://e.com/a.zip"})).unwrap();
    let (id, run) = fetches(&s.take_jobs())[0];
    s.apply(Event::Finished { id, run, done: 3 });
    s.run("download.remove", &json!({"id": id.0, "deleteFile": true})).unwrap();
    assert_eq!(s.take_jobs(), vec![Job::DropPartial { id, folder: "/downloads".into(), file_name: "a.zip".into(), also_file: true, torrent: false }]);
    assert!(s.prefs.delete_files_on_remove, "the box is ticked next time");
    // Unticked: a finished download leaves its file alone.
    s.run("download.add", &json!({"url": "https://e.com/b.zip"})).unwrap();
    let (id, run) = fetches(&s.take_jobs())[0];
    s.apply(Event::Finished { id, run, done: 3 });
    s.run("download.remove", &json!({"id": id.0, "deleteFile": false})).unwrap();
    assert_eq!(s.take_jobs(), vec![]);
    assert!(!s.prefs.delete_files_on_remove);
}

#[test]
fn a_deleted_file_shows_and_can_be_downloaded_again() {
    let mut s = session();
    s.run("download.add", &json!({"url": "https://e.com/a.zip", "name": "Real Name.zip"})).unwrap();
    let (id, run) = fetches(&s.take_jobs())[0];
    assert_eq!(s.get(id).map(|d| d.file_name.as_str()), Some("Real Name.zip"), "the name found when adding");
    s.set_missing(id, true);
    assert!(!s.get(id).is_some_and(|d| d.missing), "only a finished download can be missing");
    s.apply(Event::Finished { id, run, done: 3 });
    s.set_missing(id, true);
    assert!(s.get(id).is_some_and(|d| d.missing));
    s.run("download.again", &json!({"id": id.0})).unwrap();
    let d = s.get(id).unwrap();
    assert_eq!((d.state, d.missing, d.done), (State::Downloading, false, 0));
    assert_eq!(fetches(&s.take_jobs()).len(), 1);
}

#[test]
fn video_pages_download_through_yt_dlp_in_the_chosen_quality() {
    let mut s = session();
    s.run(
        "download.add",
        &json!({"url": "https://youtu.be/abc", "name": "A talk.mp4", "quality": {"selector": "bv*[height<=720]+ba/b", "label": "720p"}}),
    )
    .unwrap();
    let jobs = s.take_jobs();
    let Some(Job::FetchVideo { selector, file_name, .. }) = jobs.first() else { panic!("{jobs:?}") };
    assert_eq!((selector.as_str(), file_name.as_str()), ("bv*[height<=720]+ba/b", "A talk.mp4"));
    // No quality given: the best there is.
    s.run("download.add", &json!({"url": "https://www.youtube.com/watch?v=x"})).unwrap();
    let jobs = s.take_jobs();
    assert!(matches!(jobs.first(), Some(Job::FetchVideo { selector, .. }) if selector == downloadcraft_model::probe::BEST), "{jobs:?}");
}

#[test]
fn torrents_download_the_files_picked_and_seed_once_finished() {
    let mut s = session();
    let magnet = "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel";
    s.run("download.add", &json!({"url": magnet, "files": [5, 1, 5, 99_999_999]})).unwrap();
    let jobs = s.take_jobs();
    let Some(Job::FetchTorrent { id, run, file_name, files, first, seed_ratio, .. }) = jobs.first().cloned() else { panic!("{jobs:?}") };
    assert_eq!((file_name.as_str(), files, first, seed_ratio), ("Sintel", vec![1, 5], true, 1.0), "picked once each, in order");
    s.apply(Event::Started { id, run, total: Some(100), resumable: true, file_name: "Sintel".into(), done: 0 });
    s.apply(Event::Finished { id, run, done: 100 });
    // Still sharing: its run stays, so its uploads are counted.
    s.apply(Event::Uploaded { id, run, uploaded: 40, seeding: true });
    assert_eq!(s.get(id).map(|d| (d.state, d.uploaded, d.seeding)), Some((State::Completed, 40, true)));
    s.apply(Event::Uploaded { id, run: run + 1, uploaded: 99, seeding: true });
    assert_eq!(s.get(id).map(|d| d.uploaded), Some(40), "another run's report is dropped");
    // Removing it stops the sharing and, unasked, leaves its files.
    s.run("download.remove", &json!({"id": id.0})).unwrap();
    assert_eq!(s.take_jobs(), vec![Job::Stop { id }]);

    // An unfinished torrent that has written nothing leaves nothing to take away: a file of the
    // same name that was already there (and refused it) is someone else's.
    s.run("download.add", &json!({"url": magnet})).unwrap();
    let (id, _) = s.downloads.last().map(|d| (d.id, d.run)).unwrap();
    s.run("download.remove", &json!({"id": id.0})).unwrap();
    assert!(!s.take_jobs().iter().any(|j| matches!(j, Job::DropPartial { .. })));
    // One that started writing takes its files with it.
    s.run("download.add", &json!({"url": magnet})).unwrap();
    let id = s.downloads.last().map(|d| d.id).unwrap();
    let run = s.get(id).map(|d| d.run).unwrap();
    s.apply(Event::Started { id, run, total: Some(100), resumable: true, file_name: "Sintel".into(), done: 0 });
    s.take_jobs();
    s.run("download.remove", &json!({"id": id.0})).unwrap();
    let jobs = s.take_jobs();
    assert!(jobs.iter().any(|j| matches!(j, Job::DropPartial { also_file: true, torrent: true, .. })), "{jobs:?}");

    // Fetched again, it keeps its name and checks what's there rather than refusing it.
    s.run("prefs.seedRatio", &json!({"ratio": 0})).unwrap();
    s.run("download.add", &json!({"url": magnet})).unwrap();
    let id = s.downloads.last().map(|d| d.id).unwrap();
    let run = s.get(id).map(|d| d.run).unwrap();
    s.apply(Event::Started { id, run, total: Some(100), resumable: true, file_name: "Sintel".into(), done: 0 });
    s.apply(Event::Failed { id, run, error: "the disk is full".into() });
    s.take_jobs();
    s.run("download.again", &json!({"id": id.0})).unwrap();
    let jobs = s.take_jobs();
    assert!(
        matches!(jobs.first(), Some(Job::FetchTorrent { first: false, seed_ratio, file_name, .. }) if *seed_ratio == 0.0 && file_name == "Sintel"),
        "{jobs:?}"
    );
}

#[test]
fn the_seeding_ratio_is_kept_in_range() {
    let mut s = session();
    s.run("prefs.seedRatio", &json!({"ratio": 2.345})).unwrap();
    assert_eq!(s.prefs.seed_ratio, 2.3, "to a tenth");
    s.run("prefs.seedRatio", &json!({"ratio": 1e9})).unwrap();
    assert_eq!(s.prefs.seed_ratio, downloadcraft_model::MAX_SEED_RATIO);
    s.run("prefs.seedRatio", &json!({"ratio": -3})).unwrap();
    assert_eq!(s.prefs.seed_ratio, 0.0);
    assert!(s.run("prefs.seedRatio", &json!({"ratio": "lots"})).is_err());
}

#[test]
fn a_torrents_files_can_be_picked_again_and_its_details_are_kept() {
    use downloadcraft_model::probe::TorrentFile;
    use downloadcraft_model::torrent::TorrentLive;
    let mut s = session();
    s.run("download.add", &json!({"url": "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel", "files": [0]})).unwrap();
    let Some(Job::FetchTorrent { id, run, sent, .. }) = s.take_jobs().first().cloned() else { panic!() };
    assert_eq!(sent, 0);
    s.apply(Event::Started { id, run, total: Some(10), resumable: true, file_name: "Sintel".into(), done: 0 });
    let files = vec![TorrentFile { index: 0, path: "a".into(), size: 10 }, TorrentFile { index: 1, path: "b".into(), size: 20 }];
    let live = TorrentLive { size: 10, uploaded: 4, files: files.clone(), ..TorrentLive::default() };
    s.apply(Event::Details { id, run, live: Box::new(live) });
    let d = s.get(id).unwrap();
    assert_eq!(d.torrent.as_ref().map(|t| t.list.clone()), Some(files), "the file list is kept for when it's paused");
    assert_eq!((d.sent, d.live.as_ref().map(|l| l.files.len())), (4, Some(0)), "moved, not copied");
    // Picked again while it runs: the transfer is told.
    s.run("download.pickFiles", &json!({"id": id.0, "files": [1]})).unwrap();
    assert_eq!(s.take_jobs(), vec![Job::PickFiles { id, files: vec![1], priority: vec![] }]);
    // Every file: the same as none picked.
    s.run("download.pickFiles", &json!({"id": id.0, "files": [0, 1]})).unwrap();
    assert_eq!(s.take_jobs(), vec![Job::PickFiles { id, files: vec![], priority: vec![] }]);
    assert!(s.get(id).is_some_and(|d| d.torrent.as_ref().is_some_and(|t| t.files.is_empty())));
    assert!(s.run("download.pickFiles", &json!({"id": id.0, "files": []})).is_err(), "at least one");
    // The size follows what's picked.
    s.apply(Event::Details { id, run, live: Box::new(TorrentLive { size: 30, uploaded: 4, ..TorrentLive::default() }) });
    assert_eq!(s.get(id).and_then(|d| d.total), Some(30));
    // Finished, with a file left out: picking it sends it back to the queue for that file.
    s.run("download.pickFiles", &json!({"id": id.0, "files": [0]})).unwrap();
    s.apply(Event::Finished { id, run, done: 10 });
    s.take_jobs();
    s.run("download.pickFiles", &json!({"id": id.0, "files": [0, 1]})).unwrap();
    let jobs = s.take_jobs();
    assert!(jobs.contains(&Job::Stop { id }), "its sharing stops: {jobs:?}");
    assert!(jobs.iter().any(|j| matches!(j, Job::FetchTorrent { first: false, sent: 4, .. })), "and it starts again with what it sent: {jobs:?}");
    // Not a torrent: refused.
    s.run("download.add", &json!({"url": "https://e.com/a.zip"})).unwrap();
    let plain = s.downloads.last().map(|d| d.id).unwrap();
    assert!(s.run("download.pickFiles", &json!({"id": plain.0, "files": [0]})).is_err());
}

#[test]
fn torrent_settings_are_checked_and_handed_to_the_network_layer() {
    let mut s = session();
    let r = s
        .run(
            "prefs.torrent",
            &json!({"port": 51413, "protocol": "both", "upnp": true, "peerLimit": 99_999, "downKib": 500, "proxy": "127.0.0.1:9050"}),
        )
        .unwrap();
    assert_eq!((r["port"].as_u64(), r["peerLimit"].as_u64()), (Some(51413), Some(2000)));
    let jobs = s.take_jobs();
    assert!(matches!(jobs.as_slice(), [Job::Configure { torrent }] if torrent.upnp && torrent.down_kib == 500), "{jobs:?}");
    s.run("prefs.torrent", &json!({"port": 51413})).unwrap();
    assert!(s.take_jobs().is_empty(), "nothing changed, nothing to tell");
    for bad in [json!({"port": 70_000}), json!({"protocol": "carrier pigeon"}), json!({"proxy": "u:p@h:1"}), json!({"upnp": "yes"})] {
        assert!(s.run("prefs.torrent", &bad).is_err(), "{bad}");
    }
    // Loading the list tells the network layer the settings it was saved with.
    let saved = s.save();
    let mut again = Session::new();
    again.load(saved).unwrap();
    assert!(matches!(again.take_jobs().as_slice(), [Job::Configure { torrent }] if torrent.port == 51413));
}

#[test]
fn a_torrents_first_files_and_own_limits_go_with_it() {
    let mut s = session();
    s.run("download.add", &json!({"url": "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel", "paused": true})).unwrap();
    let id = s.downloads.last().map(|d| d.id).unwrap();
    let r = s.run("download.first", &json!({"id": id.0, "files": [5, 2, 5], "first": true})).unwrap();
    assert_eq!(r["first"], json!([2, 5]));
    s.run("download.first", &json!({"id": id.0, "files": [2], "first": false})).unwrap();
    let r = s.run("download.torrentLimits", &json!({"id": id.0, "downKib": 1500})).unwrap();
    assert_eq!((r["downKib"].as_u64(), r["upKib"].as_u64(), r["fromNextStart"].as_bool()), (Some(1500), Some(0), Some(false)));
    assert!(s.run("download.torrentLimits", &json!({"id": id.0, "upKib": "fast"})).is_err());
    s.run("download.resume", &json!({"id": id.0})).unwrap();
    let jobs = s.take_jobs();
    assert!(
        matches!(jobs.as_slice(), [Job::FetchTorrent { priority, down_kib: 1500, up_kib: 0, connections: 8, .. }] if priority == &[5]),
        "{jobs:?}"
    );
    // While it runs, a file moved first reaches the transfer with the files picked.
    s.run("download.first", &json!({"id": id.0, "files": [1], "first": true})).unwrap();
    assert_eq!(s.take_jobs(), vec![Job::PickFiles { id, files: vec![], priority: vec![1, 5] }]);
    // Not a torrent: refused.
    s.run("download.add", &json!({"url": "https://e.com/a.zip"})).unwrap();
    let plain = s.downloads.last().map(|d| d.id).unwrap();
    assert!(s.run("download.first", &json!({"id": plain.0, "files": [0], "first": true})).is_err());
    assert!(s.run("download.torrentLimits", &json!({"id": plain.0, "downKib": 1})).is_err());
}

#[test]
fn the_torrent_port_is_picked_once_with_upnp_and_the_details_pane_is_remembered() {
    let mut s = session();
    assert!(s.prefs.torrent.upnp && !s.prefs.torrent.chosen, "UPnP on by default, as in other torrent clients");
    assert!(s.run("prefs.torrentFirstRun", &json!({"port": 80})).is_err(), "only from the random range");
    let r = s.run("prefs.torrentFirstRun", &json!({"port": 47895})).unwrap();
    assert_eq!(r["changed"], json!(true));
    assert!(matches!(s.take_jobs().as_slice(), [Job::Configure { torrent }] if torrent.port == 47895 && torrent.upnp && torrent.chosen));
    // Turned off and moved by the user: kept, however often the app starts again.
    s.run("prefs.torrent", &json!({"upnp": false, "port": 51000})).unwrap();
    s.take_jobs();
    let r = s.run("prefs.torrentFirstRun", &json!({"port": 30000})).unwrap();
    assert_eq!((r["changed"].clone(), s.prefs.torrent.port, s.prefs.torrent.upnp), (json!(false), 51000, false));
    assert!(s.take_jobs().is_empty());
    // A list saved before the setting existed gets it set up once too.
    let mut old = Session::new();
    old.load(json!({"prefs": {"torrent": {"port": 0, "upnp": false}}})).unwrap();
    assert!(!old.prefs.torrent.chosen);
    old.run("prefs.torrentFirstRun", &json!({"port": 40000})).unwrap();
    assert!(old.prefs.torrent.upnp && old.prefs.torrent.port == 40000);

    // The details pane: folded at first, then as left, across a save.
    assert!(!s.prefs.details_open);
    assert_eq!(s.run("view.details", &json!({})).unwrap()["open"], json!(true), "left out: the other way");
    let mut again = Session::new();
    again.load(s.save()).unwrap();
    assert!(again.prefs.details_open);
    again.run("view.details", &json!({"open": false})).unwrap();
    assert!(!again.prefs.details_open);
}
