//! DownloadCraft's egui front end.
//!
//! [`DownloadApp`] draws the window around an engine [`Session`]: the toolbar, the categories on
//! the left, the download list and the status bar, and the Add link and Settings dialogs. Every
//! button runs an engine command by id; the jobs those queue are the app's to hand to the
//! network layer (`session.take_jobs()` after each frame). What only the platform can do (a
//! folder picker, opening files) comes through [`Services`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod dialogs;
pub mod icons;
mod list;
pub mod theme;
mod widgets;

use std::path::{Path, PathBuf};

use downloadcraft_engine::{Filter, Session, State};
use egui::{Margin, Ui};
use serde_json::{Value, json};

use icons::Icon;
use theme::Tokens;

/// What the platform does for the UI.
pub trait Services {
    /// A folder chosen by the user, starting at `start`.
    fn pick_folder(&self, _start: &str) -> Option<String> {
        None
    }
    /// Open a file with its default app, or a folder in the file manager.
    fn open(&self, _path: &Path) {}
}

/// No platform: the tests and anything headless.
pub struct NoServices;
impl Services for NoServices {}

/// A dialog over the window.
pub(crate) enum Dialog {
    Add(dialogs::AddForm),
    Settings { folder: String, max_active: usize },
}

pub struct DownloadApp {
    pub session: Session,
    services: Box<dyn Services>,
    /// `Some(dark)` forces a mode; `None` follows the system.
    pub force_dark: Option<bool>,
    tokens: Option<Tokens>,
    dialog: Option<Dialog>,
}

impl DownloadApp {
    pub fn new(session: Session, services: Box<dyn Services>) -> Self {
        DownloadApp { session, services, force_dark: None, tokens: None, dialog: None }
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

    pub fn open_settings(&mut self) {
        self.dialog = Some(Dialog::Settings { folder: self.session.prefs.folder.clone(), max_active: self.session.prefs.max_active });
    }

    fn tokens(&mut self, ctx: &egui::Context) -> Tokens {
        let dark = self.force_dark.unwrap_or_else(|| ctx.system_theme() == Some(egui::Theme::Dark));
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

        egui::Panel::top("toolbar")
            .frame(egui::Frame::new().fill(t.chrome).inner_margin(Margin::symmetric(10, 6)))
            .show(ui, |ui| self.toolbar(ui, &t));
        egui::Panel::bottom("status")
            .frame(egui::Frame::new().fill(t.chrome).inner_margin(Margin::symmetric(12, 4)))
            .show(ui, |ui| self.status_bar(ui, &t));
        egui::Panel::left("categories")
            .exact_size(190.0)
            .frame(egui::Frame::new().fill(t.bg).inner_margin(Margin::symmetric(8, 10)))
            .show(ui, |ui| self.categories(ui, &t));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.surface)).show(ui, |ui| {
            let mut act = None;
            list::show(&self.session, ui, &t, &mut act);
            if let Some(a) = act {
                self.list_action(&ctx, a);
            }
        });

        if let Some(mut d) = self.dialog.take() {
            let keep = match &mut d {
                Dialog::Add(form) => dialogs::add(&ctx, form, &t, &*self.services).map(|done| {
                    if let Some(params) = done {
                        self.add(params);
                    }
                }),
                Dialog::Settings { folder, max_active } => dialogs::settings(&ctx, folder, max_active, &t, &*self.services).map(|done| {
                    if done {
                        self.run("prefs.folder", json!({"folder": folder}));
                        self.run("prefs.maxActive", json!({"count": max_active}));
                    }
                }),
            };
            // `None` = still open.
            if keep.is_none() {
                self.dialog = Some(d);
            }
        }
    }

    /// Add what the dialog asked for: one link or many.
    fn add(&mut self, params: Value) {
        let many = params.get("urls").and_then(Value::as_array).map_or(0, Vec::len);
        let id = if many > 0 { "download.addMany" } else { "download.add" };
        if let Some(folder) = params.get("folder").and_then(Value::as_str).filter(|f| !f.is_empty() && self.session.prefs.folder.is_empty()) {
            self.run("prefs.folder", json!({"folder": folder}));
        }
        self.run(id, params);
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
        let (add, delete, down, up, space, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::N),
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
        if delete {
            self.run("download.remove", Value::Null);
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

    fn toolbar(&mut self, ui: &mut Ui, t: &Tokens) {
        let sel = self.session.selection.and_then(|id| self.session.get(id)).map(|d| d.state);
        let any = !self.session.downloads.is_empty();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if widgets::tool_button(ui, t, Icon::Add, "Add link", true, true).on_hover_text("Add a link (Ctrl+N, or paste one)").clicked() {
                self.open_add("");
            }
            widgets::separator(ui, t);
            if widgets::tool_button(ui, t, Icon::Resume, "Resume", sel.is_some_and(State::startable), false).clicked() {
                self.run("download.resume", Value::Null);
            }
            if widgets::tool_button(ui, t, Icon::Pause, "Pause", sel.is_some_and(State::active), false).clicked() {
                self.run("download.pause", Value::Null);
            }
            if widgets::tool_button(ui, t, Icon::Remove, "Remove", sel.is_some(), false).on_hover_text("Remove from the list (Delete)").clicked() {
                self.run("download.remove", Value::Null);
            }
            widgets::separator(ui, t);
            if widgets::tool_button(ui, t, Icon::ResumeAll, "Resume all", any, false).clicked() {
                self.run("download.resumeAll", Value::Null);
            }
            if widgets::tool_button(ui, t, Icon::PauseAll, "Pause all", any, false).clicked() {
                self.run("download.pauseAll", Value::Null);
            }
            if widgets::tool_button(ui, t, Icon::Clear, "Clear completed", self.session.count(Filter::Completed) > 0, false).clicked() {
                self.run("download.clearCompleted", Value::Null);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::tool_button(ui, t, Icon::Settings, "Settings", true, false).clicked() {
                    self.open_settings();
                }
                let folder = self.session.prefs.folder.clone();
                if widgets::tool_button(ui, t, Icon::Folder, "Open folder", !folder.is_empty(), false).on_hover_text(&folder).clicked() {
                    self.services.open(Path::new(&folder));
                }
            });
        });
    }

    fn categories(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.label(egui::RichText::new("DownloadCraft").strong().size(15.0).color(t.accent));
        ui.add_space(8.0);
        for f in Filter::ALL {
            let n = self.session.count(f);
            if widgets::category(ui, t, f.label(), n, self.session.filter == f).clicked() {
                let name = serde_json::to_value(f).unwrap_or(Value::Null);
                self.run("list.filter", json!({"filter": name}));
            }
        }
    }

    fn status_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&self.session.status).color(t.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let running = self.session.downloads.iter().filter(|d| d.state == State::Downloading).count();
                if running > 0 {
                    let speed = downloadcraft_engine::model::format::speed(self.session.total_speed());
                    ui.label(egui::RichText::new(format!("{speed} · {running} downloading")).color(t.text));
                } else {
                    ui.label(egui::RichText::new(format!("{} downloads", self.session.downloads.len())).color(t.muted));
                }
            });
        });
    }

    fn list_action(&mut self, ctx: &egui::Context, a: list::Action) {
        match a {
            list::Action::Select(id) => self.run("list.select", json!({"id": id.0})),
            list::Action::Run(cmd, id) => self.run(cmd, json!({"id": id.0})),
            list::Action::Open(path) => self.services.open(&path),
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
