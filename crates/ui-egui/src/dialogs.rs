//! The Add link and Settings dialogs. They only collect answers; the caller runs the commands.

use downloadcraft_engine::model::{MAX_ACTIVE_LIMIT, MAX_CONNECTIONS, Prefs, Theme, names, source};
use egui::{Rect, RichText, Sense, Ui, vec2};
use serde_json::{Value, json};

use crate::Services;
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::{self, Kind};

pub struct AddForm {
    pub links: String,
    pub folder: String,
    pub start: bool,
}

impl AddForm {
    pub fn new(text: &str, folder: &str) -> Self {
        AddForm { links: text.trim().to_string(), folder: folder.to_string(), start: true }
    }
    fn links(&self) -> Vec<String> {
        self.links.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
    }
}

/// The Settings dialog's answers, applied on Save.
pub struct SettingsForm {
    pub folder: String,
    pub max_active: usize,
    pub connections: usize,
    pub theme: Theme,
}

impl SettingsForm {
    pub fn new(p: &Prefs) -> Self {
        SettingsForm { folder: p.folder.clone(), max_active: p.max_active, connections: usize::from(p.connections), theme: p.theme }
    }
}

fn frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new().fill(t.surface).corner_radius(14).inner_margin(24).stroke(egui::Stroke::new(1.0, t.border))
}

/// The title row with a close button; true when it was clicked.
fn header(ui: &mut Ui, t: &Tokens, title: &str, subtitle: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).size(19.0).strong());
            ui.label(RichText::new(subtitle).size(13.0).color(t.muted));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            close = widgets::button(ui, t, Kind::Ghost, Some(Icon::Close), "", true).on_hover_text("Close (Esc)").clicked();
        });
    });
    ui.add_space(10.0);
    close
}

/// The folder field with Browse beside it.
fn folder_row(ui: &mut Ui, t: &Tokens, folder: &mut String, services: &dyn Services, width: f32) {
    ui.horizontal(|ui| {
        widgets::text_field(ui, folder, "Choose a folder", width - 110.0);
        if widgets::button(ui, t, Kind::Secondary, Some(Icon::Folder), "Browse", true).clicked()
            && let Some(p) = services.pick_folder(folder)
        {
            *folder = p;
        }
    });
}

/// One link as the dialog sees it: its kind or why it can't be added, and the file name.
fn link_row(ui: &mut Ui, t: &Tokens, link: &str, check: &Result<source::SourceKind, &str>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 8.0, t.card);
    let (chip, color, ok) = match check {
        Ok(k) => match k.not_yet() {
            None => (k.label().to_string(), t.success, true),
            Some(why) => (format!("{} · {why}", k.label()), t.warning, false),
        },
        Err(e) => ((*e).to_string(), t.danger, false),
    };
    let name = if ok { names::from_url(link) } else { link.chars().take(60).collect() };
    icons::file_badge(p, Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(30.0, 30.0)), &name, t.dark);
    let w = rect.width() - 60.0;
    let g =
        egui::WidgetText::from(RichText::new(name).size(13.5).strong()).into_galley(ui, Some(egui::TextWrapMode::Truncate), w, egui::TextStyle::Body);
    ui.painter().galley(rect.min + vec2(48.0, 6.0), g, t.text);
    let c = egui::WidgetText::from(RichText::new(chip).size(12.0)).into_galley(ui, Some(egui::TextWrapMode::Truncate), w, egui::TextStyle::Body);
    ui.painter().galley(rect.min + vec2(48.0, 25.0), c, color);
}

/// `None` while open; `Some(None)` cancelled; `Some(Some(params))` for `download.add(Many)`.
pub fn add(ctx: &egui::Context, f: &mut AddForm, t: &Tokens, services: &dyn Services) -> Option<Option<Value>> {
    let mut out = None;
    let modal = egui::Modal::new(egui::Id::unique("add-dialog")).frame(frame(t)).show(ctx, |ui| {
        let width = 560.0;
        ui.set_width(width);
        if header(ui, t, "Add link", "Paste one link per line. Direct links download now.") {
            out = Some(None);
        }
        widgets::field_label(ui, t, "Links");
        let edit = ui.add(
            egui::TextEdit::multiline(&mut f.links)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(10, 8))
                .hint_text("https://example.com/file.zip"),
        );
        if ui.memory(|m| m.focused().is_none()) {
            edit.request_focus();
        }
        let links = f.links();
        let checks: Vec<Result<source::SourceKind, &str>> = links.iter().map(|l| source::classify(l)).collect();
        let ok = checks.iter().filter(|c| matches!(c, Ok(k) if k.not_yet().is_none())).count();
        if !links.is_empty() {
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for (link, check) in links.iter().zip(&checks).take(50) {
                    link_row(ui, t, link, check);
                }
            });
        }
        ui.add_space(10.0);
        widgets::field_label(ui, t, "Save to");
        folder_row(ui, t, &mut f.folder, services, width);
        ui.add_space(4.0);
        ui.checkbox(&mut f.start, "Start downloading now");
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Ctrl+Enter adds").size(12.0).color(t.faint));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can = ok > 0 && !f.folder.trim().is_empty();
                let label = if ok > 1 { format!("Add {ok} downloads") } else { "Add download".to_string() };
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
                if widgets::button(ui, t, Kind::Primary, Some(Icon::Download), &label, can).clicked() || (can && enter) {
                    let urls: Vec<&str> = links.iter().zip(&checks).filter(|(_, c)| c.is_ok()).map(|(l, _)| l.as_str()).collect();
                    let paused = !f.start;
                    out = Some(Some(if urls.len() == 1 {
                        json!({"url": urls.first().copied().unwrap_or_default(), "folder": f.folder.trim(), "paused": paused})
                    } else {
                        json!({"urls": urls, "folder": f.folder.trim(), "paused": paused})
                    }));
                }
                if widgets::button(ui, t, Kind::Secondary, None, "Cancel", true).clicked() {
                    out = Some(None);
                }
            });
        });
    });
    if modal.should_close() && out.is_none() {
        out = Some(None);
    }
    out
}

/// A labelled setting: the name and a line under it on the left, the control on the right.
fn setting(ui: &mut Ui, t: &Tokens, name: &str, help: &str, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(name).size(14.0));
            ui.label(RichText::new(help).size(12.0).color(t.muted));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

/// `None` while open; `Some(true)` to save, `Some(false)` cancelled. `theme` is applied as it's
/// picked, so the dialog shows it.
pub fn settings(ctx: &egui::Context, f: &mut SettingsForm, t: &Tokens, services: &dyn Services, version: &str) -> Option<bool> {
    let mut out = None;
    let modal = egui::Modal::new(egui::Id::unique("settings-dialog")).frame(frame(t)).show(ctx, |ui| {
        let width = 560.0;
        ui.set_width(width);
        if header(ui, t, "Settings", "Changes apply when you save.") {
            out = Some(false);
        }
        widgets::section(ui, t, "Appearance");
        setting(ui, t, "Theme", "System follows your computer.", |ui| {
            widgets::segmented(
                ui,
                t,
                &mut f.theme,
                &[(Theme::Dark, Icon::Moon, "Dark"), (Theme::Light, Icon::Sun, "Light"), (Theme::System, Icon::Monitor, "System")],
            );
        });
        ui.add_space(8.0);
        ui.separator();
        widgets::section(ui, t, "Downloads");
        widgets::field_label(ui, t, "Download folder");
        folder_row(ui, t, &mut f.folder, services, width);
        ui.add_space(6.0);
        setting(ui, t, "Downloads at once", "The rest wait in the queue.", |ui| {
            widgets::stepper(ui, t, &mut f.max_active, 1, MAX_ACTIVE_LIMIT);
        });
        ui.add_space(4.0);
        setting(ui, t, "Connections per download", "Parts of one file fetched at the same time, when the server allows.", |ui| {
            widgets::stepper(ui, t, &mut f.connections, 1, usize::from(MAX_CONNECTIONS));
        });
        ui.add_space(8.0);
        ui.separator();
        widgets::section(ui, t, "About");
        ui.label(RichText::new(format!("DownloadCraft {version}")).size(13.5));
        ui.label(RichText::new("Updates install by themselves: a bar says when one is ready.").size(12.0).color(t.muted));
        ui.add_space(14.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::button(ui, t, Kind::Primary, None, "Save", !f.folder.trim().is_empty()).clicked() {
                out = Some(true);
            }
            if widgets::button(ui, t, Kind::Secondary, None, "Cancel", true).clicked() {
                out = Some(false);
            }
        });
    });
    if modal.should_close() && out.is_none() {
        out = Some(false);
    }
    out
}
