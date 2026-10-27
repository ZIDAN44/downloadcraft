//! Headless frames: the window draws with every state of download and every dialog open.

use downloadcraft_engine::{Event, Job, Session};
use serde_json::json;

use crate::{DownloadApp, NoServices};

pub(crate) fn frame(app: &mut DownloadApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    let input =
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 700.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| app.ui(ui));
    // No renderer here: drop the texture uploads deliberately.
    out.textures_delta.clear();
}

pub(crate) fn sample() -> Session {
    let mut s = Session::new();
    s.prefs.folder = "/downloads".into();
    s.prefs.max_active = 4;
    for u in ["https://e.com/movie.mkv", "https://e.com/disc.iso", "https://e.com/a.zip", "https://e.com/notes.pdf"] {
        let _ = s.run("download.add", &json!({"url": u}));
    }
    let _ = s.run("download.add", &json!({"url": "https://e.com/later.exe", "paused": true}));
    for (i, job) in s.take_jobs().into_iter().enumerate() {
        let Job::Fetch { id, run, .. } = job else { continue };
        s.apply(Event::Started { id, run, total: (i != 1).then_some(1_000_000), resumable: true, file_name: String::new(), done: 0 });
        match i {
            0 => s.apply(Event::Progress { id, run, done: 420_000, speed: 1_300_000, segments: Vec::new() }),
            2 => s.apply(Event::Finished { id, run, done: 1_000_000 }),
            3 => s.apply(Event::Failed { id, run, error: "the file isn't there any more (404)".into() }),
            _ => {}
        }
    }
    s
}

#[test]
fn draws_every_state_in_light_and_dark() {
    for dark in [false, true] {
        let ctx = egui::Context::default();
        let mut app = DownloadApp::new(sample(), Box::new(NoServices));
        app.force_dark = Some(dark);
        for _ in 0..3 {
            frame(&mut app, &ctx, vec![]);
        }
        for f in ["active", "paused", "completed", "failed", "all"] {
            app.run("list.filter", json!({"filter": f}));
            frame(&mut app, &ctx, vec![]);
        }
    }
}

#[test]
fn an_empty_list_and_the_dialogs_draw() {
    let ctx = egui::Context::default();
    let mut app = DownloadApp::new(Session::new(), Box::new(NoServices));
    frame(&mut app, &ctx, vec![]);
    app.open_add("https://e.com/a.zip\nmagnet:?xt=urn:btih:abc\nnot a link\nhttps://mega.nz/file/x");
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![]);
    app.open_settings();
    frame(&mut app, &ctx, vec![]);
}

#[test]
fn pasting_a_link_opens_the_add_dialog() {
    let ctx = egui::Context::default();
    let mut app = DownloadApp::new(sample(), Box::new(NoServices));
    frame(&mut app, &ctx, vec![]);
    frame(&mut app, &ctx, vec![egui::Event::Paste("https://e.com/pasted.bin".into())]);
    assert!(matches!(&app.dialog, Some(crate::Dialog::Add(f)) if f.links == "https://e.com/pasted.bin"));
}

#[test]
fn links_from_another_launch_join_the_add_box() {
    let ctx = egui::Context::default();
    let mut app = DownloadApp::new(sample(), Box::new(NoServices));
    let add = |app: &DownloadApp| match &app.dialog {
        Some(crate::Dialog::Add(f)) => Some(f.links.clone()),
        _ => None,
    };
    app.hand_over(&["https://e.com/a.zip".into()]);
    assert_eq!(add(&app).as_deref(), Some("https://e.com/a.zip"));
    app.hand_over(&["https://e.com/b.zip".into(), "https://e.com/a.zip".into()]);
    assert_eq!(
        add(&app).as_deref(),
        Some(
            "https://e.com/a.zip
https://e.com/b.zip"
        ),
        "added once each"
    );
    // Settings open: the links wait for it to close, then open the Add box.
    app.open_settings();
    app.hand_over(&["https://e.com/c.zip".into()]);
    assert!(add(&app).is_none());
    app.dialog = None;
    frame(&mut app, &ctx, vec![]);
    assert_eq!(add(&app).as_deref(), Some("https://e.com/c.zip"));
}

#[test]
fn the_update_bar_and_progress_draw() {
    let ctx = egui::Context::default();
    let mut app = DownloadApp::new(sample(), Box::new(NoServices));
    app.update = crate::UpdateState::Downloading { version: "0.2.0-test.3".into(), done: 5, total: 10 };
    frame(&mut app, &ctx, vec![]);
    app.update = crate::UpdateState::Ready { version: "0.2.0-test.3".into() };
    frame(&mut app, &ctx, vec![]);
    assert!(app.update_bar_wanted());
    assert_eq!(app.take_update_choice(), None);
}

/// A torrent that's downloading, with peers, trackers in every state and files, selected.
pub(crate) fn add_torrent(s: &mut Session) {
    use downloadcraft_engine::model::probe::TorrentFile;
    use downloadcraft_engine::model::torrent::{Peer, TorrentLive, Tracker, TrackerStatus};
    let _ = s.run("download.add", &json!({"url": "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel", "files": [1, 2]}));
    let Some(Job::FetchTorrent { id, run, .. }) = s.take_jobs().into_iter().find(|j| matches!(j, Job::FetchTorrent { .. })) else { return };
    s.apply(Event::Started { id, run, total: Some(129_000_000), resumable: true, file_name: "Sintel".into(), done: 0 });
    let tracker = |url: &str, status| Tracker { url: url.into(), status, seeds: Some(368), leechers: Some(2), completed: Some(5347) };
    let live = TorrentLive {
        info_hash: "08ada5a7a6183aae1e09d831df6748d566095a10".into(),
        active_secs: 3700,
        seeding_secs: 0,
        downloaded: 52_000_000,
        uploaded: 3_000_000,
        wasted: Some(16_384),
        down_speed: 2_400_000,
        up_speed: 30_000,
        size: 129_000_000,
        have: 51_000_000,
        pieces: 493,
        piece_size: 262_144,
        connected: 12,
        connecting: 4,
        queued: 30,
        known: 120,
        peer_limit: 200,
        peers: vec![Peer {
            address: "203.0.113.7:51413".into(),
            connection: "µTP".into(),
            client: "Transmission 4.1".into(),
            down_speed: 800_000,
            up_speed: 1_000,
            downloaded: 20_000_000,
            uploaded: 400_000,
        }],
        trackers: vec![
            tracker("udp://tracker.example:1337/announce", TrackerStatus::Working),
            tracker("https://t.example/announce", TrackerStatus::Failing("timed out".into())),
            tracker("udp://quiet.example:80", TrackerStatus::Waiting),
            tracker("wss://tracker.example", TrackerStatus::Unsupported),
        ],
        file_done: vec![0, 1_514, 46_115, 0],
        files: vec![
            TorrentFile { index: 0, path: "Sintel/Sintel.de.srt".into(), size: 1_652 },
            TorrentFile { index: 1, path: "Sintel/Sintel.en.srt".into(), size: 1_514 },
            TorrentFile { index: 2, path: "Sintel/poster.jpg".into(), size: 46_115 },
            TorrentFile { index: 3, path: "Sintel/Sintel.mp4".into(), size: 129_241_752 },
        ],
    };
    s.apply(Event::Details { id, run, live: Box::new(live) });
    let _ = s.run("list.select", &json!({"id": id.0}));
}

#[test]
fn a_torrents_details_draw_on_every_tab() {
    let ctx = egui::Context::default();
    let mut s = sample();
    add_torrent(&mut s);
    let mut app = DownloadApp::new(s, Box::new(NoServices));
    assert!(app.session.selection.and_then(|id| app.session.get(id)).is_some_and(|d| d.live.is_some()), "the details arrived");
    for tab in [crate::details::Tab::General, crate::details::Tab::Trackers, crate::details::Tab::Peers, crate::details::Tab::Files] {
        app.details.tab = tab;
        frame(&mut app, &ctx, vec![]);
        frame(&mut app, &ctx, vec![]);
    }
    // Paused, it has no details but still lists its files to pick from.
    app.run("download.pause", json!({}));
    frame(&mut app, &ctx, vec![]);
}
