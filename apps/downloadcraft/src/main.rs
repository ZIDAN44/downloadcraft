//! DownloadCraft, the desktop app: the egui window, the network worker and the saved list.
//!
//! `downloadcraft [links…] [--cmd id=json]… [--light|--dark]`. Links on the command line open
//! the Add link dialog with them; `--cmd` runs engine commands at start-up.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
// No console window behind the app on Windows (release builds).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod platform;
mod self_update;
mod single_instance;
mod store;
#[cfg(test)]
mod windows_manifest;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use downloadcraft_engine::{Event, Session};
use downloadcraft_net::{Sink, Worker};
use downloadcraft_ui_egui::DownloadApp;

const USAGE: &str = "\
usage: downloadcraft [links…] [options]

  --cmd <id>[=<json>]  run an engine command at start-up (repeatable); ui.settings and
                       ui.add={\"links\": \"…\"} open those dialogs
  --light | --dark     force light or dark mode
  --data <folder>      keep the download list here instead of the user's data folder
  --version            print the version
  --help               this text
";

struct App {
    app: DownloadApp,
    worker: Worker,
    events: Receiver<Event>,
    store: Option<store::Store>,
    last_save: Instant,
    updater: Option<self_update::Updater>,
    /// Answering later launches on the same data folder (`single_instance.rs`).
    instance: Option<single_instance::Server>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl App {
    fn pump(&mut self) {
        self.app.session.now = now();
        while let Ok(ev) = self.events.try_recv() {
            self.app.session.apply(ev);
        }
        for job in self.app.session.take_jobs() {
            self.worker.submit(job);
        }
    }
    fn save(&mut self, force: bool) {
        if !(self.app.session.dirty && (force || self.last_save.elapsed() > Duration::from_secs(2))) {
            return;
        }
        if let Some(s) = &self.store
            && let Err(e) = s.save(&self.app.session.save())
        {
            log::warn!("couldn't save the list: {e}");
            self.app.session.status = format!("couldn't save the list: {e}");
        }
        self.app.session.dirty = false;
        self.last_save = Instant::now();
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();
        // Progress arrives a few times a second; a slow repaint keeps times and speeds fresh too.
        if self.worker.running() > 0 {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(links) = self.instance.as_ref().and_then(single_instance::Server::take) {
            single_instance::show(ui.ctx(), native_wayland());
            if !links.is_empty() {
                self.app.hand_over(&links);
            }
        }
        if let Some(u) = &self.updater {
            self.app.update = u.state();
        }
        self.app.ui(ui);
        if self.app.take_update_choice() == Some(downloadcraft_ui_egui::UpdateChoice::RestartNow)
            && self.updater.as_ref().is_some_and(self_update::Updater::restart_after_close)
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.pump();
        self.save(false);
    }
    fn on_exit(&mut self) {
        if let Some(server) = &self.instance {
            server.close();
        }
        // Transfers stop; their partial files stay, and they come back paused. When an update
        // installs now, they come back queued instead, so they carry on in the new version.
        if self.updater.as_ref().is_some_and(self_update::Updater::installs_on_close) {
            self.app.session.requeue_running();
        }
        self.worker.stop_all();
        self.app.session.dirty = true;
        self.save(true);
        if let Some(u) = &self.updater {
            u.closing();
        }
    }
}

/// A Wayland window can't bring itself forward: it asks for attention instead.
fn native_wayland() -> bool {
    cfg!(all(unix, not(target_os = "macos"))) && std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
}

fn app_icon() -> egui::IconData {
    const PNG: &[u8] = include_bytes!("../../../assets/app-icon/hicolor/64x64/apps/ai.storyteller.downloadcraft.png");
    match image::load_from_memory_with_format(PNG, image::ImageFormat::Png) {
        Ok(img) => {
            let img = img.into_rgba8();
            egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
        }
        Err(_) => egui::IconData::default(),
    }
}

struct Args {
    links: Vec<String>,
    cmds: Vec<(String, serde_json::Value)>,
    dark: Option<bool>,
    data: Option<PathBuf>,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut a = Args { links: Vec::new(), cmds: Vec::new(), dark: None, data: None };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            "--version" | "-V" => {
                println!("DownloadCraft {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            "--light" => a.dark = Some(false),
            "--dark" => a.dark = Some(true),
            "--data" => a.data = Some(PathBuf::from(it.next().ok_or("--data needs a folder")?)),
            "--cmd" => {
                let c = it.next().ok_or("--cmd needs a command")?;
                let (id, json) = c.split_once('=').unwrap_or((c.as_str(), "{}"));
                let v = serde_json::from_str(json).map_err(|e| format!("--cmd {id}: {e}"))?;
                a.cmds.push((id.to_string(), v));
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}\n\n{USAGE}")),
            _ => a.links.push(arg),
        }
    }
    Ok(Some(a))
}

fn main() -> eframe::Result {
    let args = match parse_args() {
        Ok(Some(a)) => a,
        Ok(None) => return Ok(()),
        Err(e) => {
            eprintln!("downloadcraft: {e}");
            std::process::exit(2);
        }
    };
    let store = args.data.clone().or_else(store::default_dir).map(store::Store::new);
    // One DownloadCraft per data folder: started again, it hands its links to the running one and
    // leaves before reading or writing anything there (`single_instance.rs`).
    let instance = match store.as_ref().map(|s| single_instance::claim(s.dir(), &args.links)) {
        Some(single_instance::Claim::Shown) => {
            println!(
                "{}",
                if args.links.is_empty() {
                    "DownloadCraft is already running: its window is brought forward"
                } else {
                    "DownloadCraft is already running: the links open in its Add box"
                }
            );
            return Ok(());
        }
        Some(single_instance::Claim::Primary(server)) => Some(server),
        _ => None,
    };
    let mut session = Session::new();
    if let Some(s) = &store {
        match s.load() {
            Ok(Some(v)) => {
                if let Err(e) = session.load(v) {
                    s.set_aside();
                    session.status = format!("The saved list couldn't be read ({e}); it was kept as downloads.json.bad");
                }
            }
            Ok(None) => {}
            Err(e) => session.status = format!("couldn't read the saved list: {e}"),
        }
    }
    if session.prefs.folder.is_empty()
        && let Some(d) = dirs::download_dir().or_else(dirs::home_dir)
    {
        session.prefs.folder = d.to_string_lossy().into_owned();
    }
    session.now = now();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DownloadCraft")
            .with_app_id("ai.storyteller.downloadcraft")
            .with_inner_size([1100.0, 680.0])
            .with_min_inner_size([720.0, 420.0])
            .with_icon(Arc::new(app_icon())),
        ..Default::default()
    };
    eframe::run_native(
        "DownloadCraft",
        options,
        Box::new(move |cc| {
            let (tx, rx) = channel();
            let ctx = cc.egui_ctx.clone();
            let worker = Worker::new(Sink::new(tx, Arc::new(move || ctx.request_repaint())));
            if let Some(s) = &store {
                downloadcraft_net::video::tools::set_dir(s.dir().join("tools"));
            }
            let repaint = cc.egui_ctx.clone();
            let platform = platform::Platform::new(Arc::new(move || repaint.request_repaint()));
            let mut app = DownloadApp::new(session, Box::new(platform));
            app.force_dark = args.dark;
            for (id, v) in &args.cmds {
                // `ui.*` opens a dialog (for scripting and screenshots); the rest are engine commands.
                match id.as_str() {
                    "ui.settings" => app.open_settings(),
                    "ui.add" => app.open_add(v.get("links").and_then(serde_json::Value::as_str).unwrap_or("")),
                    _ => app.run(id, v.clone()),
                }
            }
            if !args.links.is_empty() {
                app.open_add(&args.links.join("\n"));
            }
            if let Some(server) = &instance {
                server.connect(&cc.egui_ctx);
            }
            let updater = store.as_ref().and_then(|s| self_update::Updater::start(s.dir().to_path_buf()));
            if let Some(u) = &updater {
                let ctx = cc.egui_ctx.clone();
                u.set_waker(Arc::new(move || ctx.request_repaint()));
            }
            Ok(Box::new(App { app, worker, events: rx, store, last_save: Instant::now(), updater, instance }))
        }),
    )
}
