//! Headless frames: the window draws with every state of download and every dialog open.

use downloadcraft_engine::{Event, Job, Session};
use serde_json::json;

use crate::{DownloadApp, NoServices};

fn frame(app: &mut DownloadApp, ctx: &egui::Context, events: Vec<egui::Event>) {
    let input =
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 700.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| app.ui(ui));
    // No renderer here: drop the texture uploads deliberately.
    out.textures_delta.clear();
}

fn sample() -> Session {
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
            0 => s.apply(Event::Progress { id, run, done: 420_000, speed: 1_300_000 }),
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
