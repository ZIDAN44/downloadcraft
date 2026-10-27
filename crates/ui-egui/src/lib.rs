//! DownloadCraft's egui front end.
//!
//! [`DownloadApp`] draws the window around an engine [`Session`]: the sidebar (Add link, the
//! categories, Settings), the header with the search and the theme switch, the actions, the
//! download list and the status bar, and the Add link and Settings dialogs. Every button runs an
//! engine command by id; the jobs those queue are the app's to hand to the network layer
//! (`session.take_jobs()` after each frame). What only the platform can do (a folder picker,
//! opening files) comes through [`Services`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod dialogs;
pub mod icons;
mod list;
pub mod theme;
mod updates;
mod widgets;

use std::path::{Path, PathBuf};

use downloadcraft_engine::model::Theme;
use downloadcraft_engine::model::probe::{Probe, VideoTools};
use downloadcraft_engine::{Filter, Session, State};
use egui::{Margin, Rect, RichText, Ui, vec2};
use serde_json::{Value, json};

use icons::Icon;
use theme::Tokens;
pub use updates::{UpdateChoice, UpdateState};
use widgets::Kind;

/// What the platform does for the UI.
pub trait Services {
    /// A folder chosen by the user, starting at `start`.
    fn pick_folder(&self, _start: &str) -> Option<String> {
        None
    }
    /// Open a file with its default app, or a folder in the file manager.
    fn open(&self, _path: &Path) {}
    /// Show a file in the file manager, selected (as a browser's Show in folder does).
    fn reveal(&self, path: &Path) {
        if let Some(dir) = path.parent() {
            self.open(dir);
        }
    }
    /// Whether a file is (still) there.
    fn exists(&self, _path: &Path) -> bool {
        true
    }
    /// What a link turns out to be: asked for in the background, `Pending` until it's known.
    fn probe(&self, _url: &str) -> Probe {
        Probe::Pending
    }
    /// Where yt-dlp and FFmpeg stand, for video links.
    fn video_tools(&self) -> VideoTools {
        VideoTools::default()
    }
    /// Fetch yt-dlp, or FFmpeg with `ffmpeg`, in the background.
    fn fetch_tool(&self, _ffmpeg: bool) {}
    /// Whether FFmpeg can be fetched here (else the system's package manager has it).
    fn can_fetch_ffmpeg(&self) -> bool {
        false
    }
}

/// No platform: the tests and anything headless.
pub struct NoServices;
impl Services for NoServices {}

/// A dialog over the window.
pub(crate) enum Dialog {
    Add(dialogs::AddForm),
    Settings(dialogs::SettingsForm),
    Remove(dialogs::RemoveForm),
}

/// How often finished downloads' files are looked for (a deleted or moved one shows as Deleted).
const LOOK_EVERY: f64 = 2.0;

pub struct DownloadApp {
    pub session: Session,
    services: Box<dyn Services>,
    /// `Some(dark)` forces a mode for this run (`--light` / `--dark`), over the saved theme.
    pub force_dark: Option<bool>,
    tokens: Option<Tokens>,
    dialog: Option<Dialog>,
    /// What the search box holds.
    search: String,
    /// When finished files were last looked for.
    looked: f64,
    /// Where the updater is (the app keeps it current).
    pub update: UpdateState,
    /// What the update bar's buttons asked for, for the app to act on (`take_update_choice`).
    update_choice: Option<UpdateChoice>,
    /// The version whose bar Later hid.
    update_dismissed: Option<String>,
    /// Links another launch handed over while another dialog was open (`hand_over`).
    handed: Vec<String>,
}

fn filter_icon(f: Filter) -> Icon {
    match f {
        Filter::All => Icon::List,
        Filter::Active => Icon::Download,
        Filter::Paused => Icon::Pause,
        Filter::Completed => Icon::Check,
        Filter::Failed => Icon::Alert,
    }
}

impl DownloadApp {
    pub fn new(session: Session, services: Box<dyn Services>) -> Self {
        DownloadApp {
            session,
            services,
            force_dark: None,
            tokens: None,
            dialog: None,
            search: String::new(),
            looked: f64::NEG_INFINITY,
            update: UpdateState::None,
            update_choice: None,
            update_dismissed: None,
            handed: Vec::new(),
        }
    }

    /// Run a command, as a button does; failures go to the status bar (the engine sets it).
    pub fn run(&mut self, id: &str, params: Value) {
        if let Err(e) = self.session.run(id, &params) {
            log::info!("{id}: {e}");
        }
    }

    /// Open the Add link dialog with `text` in it (a paste, a link handed to the app).
    pub fn open_add(&mut self, text: &str) {
        self.dialog = Some(Dialog::Add(dialogs::AddForm::new(text, &self.session.prefs.folder)));
    }

    /// Links another launch handed over: added to the Add box when it's open (one per line, none
    /// twice), else in a new one. Another dialog open stays; the links wait for it to close.
    pub fn hand_over(&mut self, links: &[String]) {
        match &mut self.dialog {
            None => self.open_add(&links.join("\n")),
            Some(Dialog::Add(form)) => {
                for l in links {
                    if !form.links.lines().any(|have| have.trim() == l.trim()) {
                        if !form.links.is_empty() && !form.links.ends_with('\n') {
                            form.links.push('\n');
                        }
                        form.links.push_str(l.trim());
                    }
                }
            }
            Some(_) => self.handed.extend(links.iter().cloned()),
        }
    }

    pub fn open_settings(&mut self) {
        self.dialog = Some(Dialog::Settings(dialogs::SettingsForm::new(&self.session.prefs)));
    }

    /// The theme in force: `--light`/`--dark`, else the one being picked in Settings, else the
    /// saved one (System following the system).
    fn tokens(&mut self, ctx: &egui::Context) -> Tokens {
        let theme = match &self.dialog {
            Some(Dialog::Settings(f)) => f.theme,
            _ => self.session.prefs.theme,
        };
        let dark = self.force_dark.unwrap_or(match theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => ctx.system_theme() == Some(egui::Theme::Dark),
        });
        let want = Tokens::for_mode(dark);
        if self.tokens != Some(want) {
            theme::apply(ctx, &want);
            self.tokens = Some(want);
        }
        want
    }

    /// The whole window.
    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let t = self.tokens(&ctx);
        self.shortcuts(&ctx);
        self.look_for_files(&ctx);

        egui::Panel::left("sidebar")
            .exact_size(236.0)
            .frame(egui::Frame::new().fill(t.bg).inner_margin(Margin::symmetric(14, 16)).stroke(egui::Stroke::new(1.0, t.border)))
            .show(ui, |ui| self.sidebar(ui, &t));
        egui::Panel::top("header").frame(egui::Frame::new().fill(t.surface).inner_margin(Margin { left: 24, right: 20, top: 18, bottom: 8 })).show(
            ui,
            |ui| {
                self.header(ui, &t);
                ui.add_space(8.0);
                self.actions(ui, &t);
            },
        );
        if self.update_bar_wanted() {
            egui::Panel::top("update-bar")
                .frame(egui::Frame::new().fill(t.accent_soft).inner_margin(Margin::symmetric(20, 8)).stroke(egui::Stroke::new(1.0, t.border)))
                .show(ui, |ui| self.update_bar(ui, &t));
        }
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(t.surface).inner_margin(Margin::symmetric(20, 6)).stroke(egui::Stroke::new(1.0, t.border)))
            .show(ui, |ui| self.status_bar(ui, &t));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.surface).inner_margin(Margin { left: 8, right: 8, top: 4, bottom: 4 })).show(
            ui,
            |ui| {
                let mut act = None;
                list::show(&self.session, &self.search, ui, &t, &mut act);
                if let Some(a) = act {
                    self.list_action(&ctx, a);
                }
            },
        );

        if let Some(mut d) = self.dialog.take() {
            let keep = match &mut d {
                Dialog::Add(form) => dialogs::add(&ctx, form, &t, &*self.services).map(|done| {
                    if let Some(list) = done {
                        self.add(list);
                    }
                }),
                Dialog::Remove(form) => dialogs::remove(&ctx, form, &t).map(|done| {
                    if let Some(delete) = done {
                        self.run("download.remove", json!({"id": form.id.0, "deleteFile": delete}));
                    }
                }),
                Dialog::Settings(form) => dialogs::settings(&ctx, form, &t, &*self.services, env!("CARGO_PKG_VERSION")).map(|save| {
                    if save {
                        self.run("prefs.folder", json!({"folder": form.folder}));
                        self.run("prefs.maxActive", json!({"count": form.max_active}));
                        self.run("prefs.connections", json!({"count": form.connections}));
                        self.run("prefs.theme", json!({"theme": form.theme}));
                    }
                }),
            };
            // `None` = still open.
            if keep.is_none() {
                self.dialog = Some(d);
            }
        }
        if self.dialog.is_none() && !self.handed.is_empty() {
            let links = std::mem::take(&mut self.handed);
            self.open_add(&links.join(
                "
",
            ));
        }
    }

    /// Add what the dialog asked for, one link at a time (each with its own name and quality).
    fn add(&mut self, list: Vec<Value>) {
        let folder = list.first().and_then(|p| p.get("folder")).and_then(Value::as_str).unwrap_or("").to_string();
        if !folder.is_empty() && self.session.prefs.folder.is_empty() {
            self.run("prefs.folder", json!({"folder": folder}));
        }
        let n = list.len();
        for params in list {
            self.run("download.add", params);
        }
        if n > 1 {
            self.session.status = format!("Added {n} downloads");
        }
    }

    /// Ask before removing download `id` (the selected one when `None`).
    pub fn ask_remove(&mut self, id: Option<downloadcraft_engine::DownloadId>) {
        let Some(d) = id.or(self.session.selection).and_then(|id| self.session.get(id)) else { return };
        let finished = d.state == State::Completed && !d.missing;
        self.dialog = Some(Dialog::Remove(dialogs::RemoveForm {
            id: d.id,
            name: d.file_name.clone(),
            finished,
            delete_file: self.session.prefs.delete_files_on_remove,
        }));
    }

    /// Every couple of seconds, whether each finished download's file is still where it was saved
    /// (as a browser's downloads page shows a deleted file).
    fn look_for_files(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now - self.looked < LOOK_EVERY {
            return;
        }
        self.looked = now;
        let done: Vec<(downloadcraft_engine::DownloadId, PathBuf)> =
            self.session.downloads.iter().filter(|d| d.state == State::Completed).map(|d| (d.id, Path::new(&d.folder).join(&d.file_name))).collect();
        for (id, path) in done {
            let missing = !self.services.exists(&path);
            self.session.set_missing(id, missing);
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(LOOK_EVERY));
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() || ctx.text_edit_focused() {
            return;
        }
        // A link pasted anywhere in the window opens the Add dialog with it.
        let pasted = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Paste(s) = e { Some(s.clone()) } else { None }));
        if let Some(text) = pasted {
            self.open_add(&text);
            return;
        }
        let (add, find, delete, down, up, space, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::N),
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::F),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Delete),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Space),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            )
        });
        if add {
            self.open_add("");
        }
        if find {
            ctx.memory_mut(|m| m.request_focus(egui::Id::unique("search")));
        }
        if delete {
            self.ask_remove(None);
        }
        if down {
            self.run("list.selectNext", Value::Null);
        }
        if up {
            self.run("list.selectPrevious", Value::Null);
        }
        if space {
            self.toggle_selected();
        }
        if enter && let Some(path) = self.selected_file() {
            self.services.open(&path);
        }
    }

    /// Pause the selected download if it's going, else resume it.
    fn toggle_selected(&mut self) {
        let Some(d) = self.session.selection.and_then(|id| self.session.get(id)) else { return };
        let id = if d.state.active() { "download.pause" } else { "download.resume" };
        self.run(id, Value::Null);
    }

    /// The selected download's file, when it is finished.
    fn selected_file(&self) -> Option<PathBuf> {
        let d = self.session.selection.and_then(|id| self.session.get(id))?;
        (d.state == State::Completed).then(|| Path::new(&d.folder).join(&d.file_name))
    }

    fn sidebar(&mut self, ui: &mut Ui, t: &Tokens) {
        // The app's mark and name.
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), egui::Sense::hover());
            ui.painter().rect_filled(r, 9.0, t.accent);
            icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(20.0, 20.0)), Icon::Download, egui::Color32::WHITE);
            ui.add_space(4.0);
            ui.label(RichText::new("DownloadCraft").size(17.0).strong());
        });
        ui.add_space(18.0);
        if widgets::button(ui, t, Kind::Primary, Some(Icon::Add), "Add link", true)
            .on_hover_text("Add a link (Ctrl+N), or paste one anywhere (Ctrl+V)")
            .clicked()
        {
            self.open_add("");
        }
        ui.add_space(18.0);
        widgets::section(ui, t, "Downloads");
        ui.spacing_mut().item_spacing.y = 2.0;
        for f in Filter::ALL {
            let n = self.session.count(f);
            if widgets::nav_row(ui, t, filter_icon(f), f.label(), n, self.session.filter == f).clicked() {
                let name = serde_json::to_value(f).unwrap_or(Value::Null);
                self.run("list.filter", json!({"filter": name}));
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if widgets::nav_row(ui, t, Icon::Settings, "Settings", 0, matches!(self.dialog, Some(Dialog::Settings(_)))).clicked() {
                self.open_settings();
            }
            let folder = self.session.prefs.folder.clone();
            if widgets::nav_row(ui, t, Icon::Folder, "Download folder", 0, false).on_hover_text(&folder).clicked() && !folder.is_empty() {
                self.services.open(Path::new(&folder));
            }
            ui.add_space(8.0);
            self.speed_card(ui, t);
        });
    }

    /// How fast everything is going, at the foot of the sidebar.
    fn speed_card(&self, ui: &mut Ui, t: &Tokens) {
        let running = self.session.downloads.iter().filter(|d| d.state == State::Downloading).count();
        egui::Frame::new().fill(t.card).corner_radius(10).inner_margin(12).stroke(egui::Stroke::new(1.0, t.border)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 2.0;
            // The sidebar's foot is laid out from the bottom up, so the lines go in last first.
            let text = match running {
                0 => "Nothing downloading".to_string(),
                1 => "1 download running".to_string(),
                n => format!("{n} downloads running"),
            };
            ui.label(RichText::new(text).size(12.0).color(t.muted));
            let speed = downloadcraft_engine::model::format::speed(self.session.total_speed());
            ui.label(RichText::new(speed).size(18.0).strong().color(if running > 0 { t.accent } else { t.muted }));
            ui.label(RichText::new("SPEED").size(11.0).color(t.faint).strong());
        });
    }

    fn header(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new(self.session.filter.label()).size(22.0).strong());
                let n = list::visible(&self.session, &self.search).len();
                ui.label(RichText::new(if n == 1 { "1 download".to_string() } else { format!("{n} downloads") }).size(13.0).color(t.muted));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (icon, next, tip) = match self.session.prefs.theme {
                    Theme::System => (Icon::Monitor, Theme::Light, "Theme: System. Click for Light"),
                    Theme::Light => (Icon::Sun, Theme::Dark, "Theme: Light. Click for Dark"),
                    Theme::Dark => (Icon::Moon, Theme::System, "Theme: Dark. Click for System"),
                };
                if widgets::button(ui, t, Kind::Secondary, Some(icon), "", true).on_hover_text(tip).clicked() {
                    self.force_dark = None;
                    self.run("prefs.theme", json!({"theme": next}));
                }
                // The search box, with its icon drawn inside.
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .id(egui::Id::unique("search"))
                        .hint_text("Search downloads  (Ctrl+F)")
                        .desired_width(260.0)
                        .margin(Margin { left: 32, right: 10, top: 8, bottom: 8 }),
                );
                icons::paint(
                    ui.painter(),
                    Rect::from_center_size(resp.rect.left_center() + vec2(17.0, 0.0), vec2(15.0, 15.0)),
                    Icon::Search,
                    t.faint,
                );
                if resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.search.clear();
                }
            });
        });
    }

    fn actions(&mut self, ui: &mut Ui, t: &Tokens) {
        let sel = self.session.selection.and_then(|id| self.session.get(id)).map(|d| d.state);
        let any = !self.session.downloads.is_empty();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if widgets::button(ui, t, Kind::Ghost, Some(Icon::Resume), "Resume", sel.is_some_and(State::startable))
                .on_hover_text("Resume (Space)")
                .clicked()
            {
                self.run("download.resume", Value::Null);
            }
            if widgets::button(ui, t, Kind::Ghost, Some(Icon::Pause), "Pause", sel.is_some_and(State::active))
                .on_hover_text("Pause (Space)")
                .clicked()
            {
                self.run("download.pause", Value::Null);
            }
            if widgets::button(ui, t, Kind::Ghost, Some(Icon::Remove), "Remove", sel.is_some())
                .on_hover_text("Remove from the list (Delete)")
                .clicked()
            {
                self.ask_remove(None);
            }
            let (r, _) = ui.allocate_exact_size(vec2(17.0, 24.0), egui::Sense::hover());
            ui.painter().vline(r.center().x, r.y_range(), egui::Stroke::new(1.0, t.border));
            if widgets::button(ui, t, Kind::Ghost, Some(Icon::ResumeAll), "Resume all", any).clicked() {
                self.run("download.resumeAll", Value::Null);
            }
            if widgets::button(ui, t, Kind::Ghost, Some(Icon::PauseAll), "Pause all", any).clicked() {
                self.run("download.pauseAll", Value::Null);
            }
            if widgets::button(ui, t, Kind::Ghost, Some(Icon::Clear), "Clear completed", self.session.count(Filter::Completed) > 0).clicked() {
                self.run("download.clearCompleted", Value::Null);
            }
        });
    }

    fn status_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.session.status).size(12.5).color(t.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.update_status(ui, t);
                let n = self.session.downloads.len();
                ui.label(RichText::new(format!("{n} in the list · {} connections each", self.session.prefs.connections)).size(12.5).color(t.faint));
            });
        });
    }

    fn list_action(&mut self, ctx: &egui::Context, a: list::Action) {
        match a {
            list::Action::Select(id) => self.run("list.select", json!({"id": id.0})),
            list::Action::Run(cmd, id) => self.run(cmd, json!({"id": id.0})),
            list::Action::Open(path) => self.services.open(&path),
            list::Action::Reveal(path) => {
                // A file that's gone opens its folder instead.
                if self.services.exists(&path) {
                    self.services.reveal(&path);
                } else if let Some(dir) = path.parent() {
                    self.services.open(dir);
                }
            }
            list::Action::Remove(id) => self.ask_remove(Some(id)),
            list::Action::CopyLink(url) => {
                ctx.copy_text(url);
                self.session.status = "Link copied".into();
            }
            list::Action::Add => self.open_add(""),
        }
    }
}

#[cfg(test)]
mod tests;
