//! The Add link and Settings dialogs. They only collect answers; the caller runs the commands.

use downloadcraft_engine::model::{MAX_ACTIVE_LIMIT, source};
use egui::RichText;
use serde_json::{Value, json};

use crate::Services;
use crate::theme::Tokens;

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

fn modal_frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new().fill(t.surface).corner_radius(10).inner_margin(18).stroke(egui::Stroke::new(1.0, t.border))
}

/// `None` while open; `Some(None)` cancelled; `Some(Some(params))` for `download.add(Many)`.
pub fn add(ctx: &egui::Context, f: &mut AddForm, t: &Tokens, services: &dyn Services) -> Option<Option<Value>> {
    let mut out = None;
    let modal = egui::Modal::new(egui::Id::unique("add-dialog")).frame(modal_frame(t)).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(RichText::new("Add link").size(17.0).strong());
        ui.add_space(8.0);
        ui.label("Links, one per line:");
        let edit = ui.add(egui::TextEdit::multiline(&mut f.links).desired_rows(3).desired_width(f32::INFINITY).hint_text("https://…"));
        if ui.memory(|m| m.focused().is_none()) {
            edit.request_focus();
        }
        let links = f.links();
        let checks: Vec<Result<source::SourceKind, &str>> = links.iter().map(|l| source::classify(l)).collect();
        let ok = checks.iter().filter(|c| matches!(c, Ok(k) if k.not_yet().is_none())).count();
        // What the links are, so a torrent or MEGA link says when it's coming instead of failing.
        for (link, check) in links.iter().zip(&checks).take(4) {
            let (text, color) = match check {
                Ok(k) => match k.not_yet() {
                    None => (k.label().to_string(), t.success),
                    Some(why) => (format!("{}: {why}", k.label()), t.warning),
                },
                Err(e) => ((*e).to_string(), t.danger),
            };
            let short: String = link.chars().take(48).collect();
            ui.label(RichText::new(format!("{short} — {text}")).size(12.0).color(color));
        }
        if links.len() > 4 {
            ui.label(RichText::new(format!("…and {} more", links.len() - 4)).size(12.0).color(t.muted));
        }
        ui.add_space(8.0);
        ui.label("Save to:");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut f.folder).desired_width(400.0));
            if ui.button("Browse…").clicked()
                && let Some(p) = services.pick_folder(&f.folder)
            {
                f.folder = p;
            }
        });
        ui.checkbox(&mut f.start, "Start now");
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let can = ok > 0 && !f.folder.trim().is_empty();
            let label = if ok > 1 { format!("Add {ok} downloads") } else { "Add".to_string() };
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
            if ui.add_enabled(can, egui::Button::new(label)).clicked() || (can && enter) {
                let urls: Vec<&str> = links.iter().zip(&checks).filter(|(_, c)| c.is_ok()).map(|(l, _)| l.as_str()).collect();
                let paused = !f.start;
                out = Some(Some(if urls.len() == 1 {
                    json!({"url": urls.first().copied().unwrap_or_default(), "folder": f.folder.trim(), "paused": paused})
                } else {
                    json!({"urls": urls, "folder": f.folder.trim(), "paused": paused})
                }));
            }
            if ui.button("Cancel").clicked() {
                out = Some(None);
            }
        });
    });
    if modal.should_close() && out.is_none() {
        out = Some(None);
    }
    out
}

/// `None` while open; `Some(true)` to save, `Some(false)` cancelled.
pub fn settings(ctx: &egui::Context, folder: &mut String, max_active: &mut usize, t: &Tokens, services: &dyn Services) -> Option<bool> {
    let mut out = None;
    let modal = egui::Modal::new(egui::Id::unique("settings-dialog")).frame(modal_frame(t)).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.label(RichText::new("Settings").size(17.0).strong());
        ui.add_space(8.0);
        ui.label("Download folder:");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(folder).desired_width(340.0));
            if ui.button("Browse…").clicked()
                && let Some(p) = services.pick_folder(folder)
            {
                *folder = p;
            }
        });
        ui.add_space(6.0);
        ui.add(egui::Slider::new(max_active, 1..=MAX_ACTIVE_LIMIT).text("downloads at once"));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!folder.trim().is_empty(), egui::Button::new("Save")).clicked() {
                out = Some(true);
            }
            if ui.button("Cancel").clicked() {
                out = Some(false);
            }
        });
    });
    if modal.should_close() && out.is_none() {
        out = Some(false);
    }
    out
}
