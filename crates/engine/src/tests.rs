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
    assert!(matches!(s.run("download.add", &json!({"url": "magnet:?xt=urn:btih:abc"})), Err(CmdError::Disabled(_))));
    assert!(matches!(s.run("download.add", &json!({"url": "https://mega.nz/file/x#k"})), Err(CmdError::Disabled(_))));
    assert!(matches!(s.run("download.add", &json!({"url": "not a link"})), Err(CmdError::Params(_))));
    assert!(s.downloads.is_empty());
    assert!(!s.status.is_empty());
    let r = s.run("download.addMany", &json!({"urls": ["https://e.com/a", "magnet:?x", "https://e.com/b"]})).unwrap();
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
    assert!(t.take_jobs().is_empty());
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
    assert_eq!(s.take_jobs(), vec![Job::DropPartial { id, folder: "/downloads".into(), file_name: "a.zip".into(), also_file: true }]);
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
