//! The dialogs: Add link (`dialogs/add.rs`), Remove (`dialogs/remove.rs`) and Settings. They only
//! collect answers; the caller runs the commands.

mod add;
mod remove;

pub use add::{AddForm, add};
pub use remove::{RemoveForm, remove};

use downloadcraft_engine::model::{MAX_ACTIVE_LIMIT, MAX_CONNECTIONS, Prefs, Theme};
use egui::{RichText, Ui};

use crate::Services;
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets::{self, Kind};

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

pub(super) fn frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new().fill(t.surface).corner_radius(14).inner_margin(24).stroke(egui::Stroke::new(1.0, t.border))
}

/// The title row with a close button; true when it was clicked.
pub(super) fn header(ui: &mut Ui, t: &Tokens, title: &str, subtitle: &str) -> bool {
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
pub(super) fn folder_row(ui: &mut Ui, t: &Tokens, folder: &mut String, services: &dyn Services, width: f32) {
    ui.horizontal(|ui| {
        widgets::text_field(ui, folder, "Choose a folder", width - 110.0);
        if widgets::button(ui, t, Kind::Secondary, Some(Icon::Folder), "Browse", true).clicked()
            && let Some(p) = services.pick_folder(folder)
        {
            *folder = p;
        }
    });
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
        ui.label(
            RichText::new("DownloadCraft keeps itself up to date. When a new version is ready, a bar at the top lets you restart to install it.")
                .size(12.0)
                .color(t.muted),
        );
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
