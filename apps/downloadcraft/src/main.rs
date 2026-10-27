//! DownloadCraft, the desktop app: the egui window, the network worker and the saved list.
//!
//! `downloadcraft [links…] [--cmd id=json]… [--light|--dark]`. Links on the command line open
//! the Add link dialog with them; `--cmd` runs engine commands at start-up.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
// No console window behind the app on Windows (release builds).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod graphics;
mod logfile;
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
  --log                also write the log (downloadcraft.log in the data folder) to the terminal
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
    /// Frames drawn, and the window's GPU device until its errors are only logged
    /// (`graphics::log_errors`).
    frames: u32,
    gpu: Option<eframe::wgpu::Device>,
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
        self.frames = self.frames.saturating_add(1);
        if self.frames >= graphics::STARTUP_FRAMES
            && let Some(device) = self.gpu.take()
        {
            graphics::log_errors(&device);
        }
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
    /// `--log`: log lines to the terminal too.
    log: bool,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut a = Args { links: Vec::new(), cmds: Vec::new(), dark: None, data: None, log: false };
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
            "--log" => a.log = true,
            "--data" => a.data = Some(PathBuf::from(it.next().ok_or("--data needs a folder")?)),
            "--cmd" => {
                let c = it.next().ok_or("--cmd needs a command")?;
                let (id, json) = c.split_once('=').unwrap_or((c.as_str(), "{}"));
                let v = serde_json::from_str(json).map_err(|e| format!("--cmd {id}: {e}"))?;
                a.cmds.push((id.to_string(), v));
            }
            s if s.starts_with("--") => return Err(format!("unknown option {s}\n\n{USAGE}")),
            // A `.torrent` file opened with the app, or named from a shell's folder: its full path.
            _ if arg.to_ascii_lowercase().ends_with(".torrent") && std::path::Path::new(&arg).is_file() => {
                let full = std::fs::canonicalize(&arg).map(|p| p.to_string_lossy().into_owned()).unwrap_or(arg);
                a.links.push(full.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(full));
            }
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
            say(if args.links.is_empty() {
                "DownloadCraft is already running: its window is brought forward"
            } else {
                "DownloadCraft is already running: the links open in its Add box"
            });
            return Ok(());
        }
        Some(single_instance::Claim::Primary(server)) => Some(server),
        _ => None,
    };
    // Diagnostics go to `downloadcraft.log` in the data folder (after the claim above, so a
    // second launch leaves the running one's log alone).
    let log_path = logfile::start(store.as_ref().map(store::Store::dir), cfg!(debug_assertions) || args.log);
    if let Some(path) = &log_path {
        log::info!("DownloadCraft {} started; log at {}", env!("CARGO_PKG_VERSION"), path.display());
    }
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

    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("DownloadCraft")
            .with_app_id("ai.storyteller.downloadcraft")
            .with_inner_size([1100.0, 680.0])
            .with_min_inner_size([720.0, 420.0])
            .with_icon(Arc::new(app_icon())),
        ..Default::default()
    };
    graphics::configure(&mut options);
    // A window that can't open says why, rather than the app leaving without a word (or staying in
    // the background); a panic on the way is caught for that too.
    let socket = instance.as_ref().map(single_instance::Server::socket);
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(options, args, store, session, instance)));
    let failed = match ran {
        Ok(Ok(())) => return Ok(()),
        Ok(Err(e)) => e.to_string(),
        Err(p) => p
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "it stopped unexpectedly".into()),
    };
    log::error!("the window couldn't open: {failed}");
    // No window: a launch while the box is up must open its own, not be told this one runs.
    if let Some(p) = &socket {
        single_instance::close_at(p);
    }
    say(&format!("the window couldn't open: {failed}"));
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("DownloadCraft")
        .set_description(cant_open(&failed, log_path.as_deref()))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    std::process::exit(1)
}

/// Tell the terminal, as `eprintln!` would without its panic on a closed one.
fn say(what: &str) {
    let _ = std::io::Write::write_fmt(&mut std::io::stderr(), format_args!("downloadcraft: {what}\n"));
}

/// What the message box says when the window can't open; `log` is the log file, when there is one.
fn cant_open(why: &str, log: Option<&std::path::Path>) -> String {
    let help = if cfg!(windows) {
        "Updating it may help, or starting DownloadCraft with the environment variable WGPU_FORCE_FALLBACK_ADAPTER=1, which draws it with \
         Windows' software renderer."
    } else {
        "Updating it may help."
    };
    let details = log.map(|p| format!("\n\nThe details are in {}.", p.display())).unwrap_or_default();
    format!("DownloadCraft's window couldn't open:\n{why}\n\nThis is usually the graphics driver. {help}{details}")
}

/// Open the window and run the app until it closes.
fn run(
    options: eframe::NativeOptions,
    args: Args,
    store: Option<store::Store>,
    session: Session,
    instance: Option<single_instance::Server>,
) -> eframe::Result {
    eframe::run_native(
        "DownloadCraft",
        options,
        Box::new(move |cc| {
            let (tx, rx) = channel();
            let ctx = cc.egui_ctx.clone();
            let worker = Worker::new(Sink::new(tx, Arc::new(move || ctx.request_repaint())));
            if let Some(s) = &store {
                downloadcraft_net::video::tools::set_dir(s.dir().join("tools"));
                downloadcraft_net::torrent::set_dir(s.dir().join("torrents"));
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
            let gpu = cc.wgpu_render_state.as_ref().map(|rs| rs.device.clone());
            Ok(Box::new(App { app, worker, events: rx, store, last_save: Instant::now(), updater, instance, frames: 0, gpu }))
        }),
    )
}
