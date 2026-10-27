//! The dialogs: Add link (`dialogs/add.rs`), Remove (`dialogs/remove.rs`) and Settings. They only
//! collect answers; the caller runs the commands.

mod add;
mod remove;
mod torrents;

pub use add::{AddForm, add};
pub use remove::{RemoveForm, remove};

use downloadcraft_engine::model::torrent::TorrentPrefs;
use downloadcraft_engine::model::{MAX_ACTIVE_LIMIT, MAX_CONNECTIONS, Prefs, Theme};
use egui::{RichText, Ui, vec2};

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
    pub subtitles: String,
    /// How many times its size a finished torrent sends to others (0: none).
    pub seed_ratio: f32,
    /// Ports, limits, proxy and IP filter for torrents.
    pub torrent: TorrentPrefs,
    /// `auto` (the system's) or a language code (`i18n::LANGUAGES`).
    pub language: String,
}

impl SettingsForm {
    pub fn new(p: &Prefs) -> Self {
        SettingsForm {
            folder: p.folder.clone(),
            max_active: p.max_active,
            connections: usize::from(p.connections),
            theme: p.theme,
            subtitles: p.subtitles.clone(),
            seed_ratio: p.seed_ratio,
            torrent: p.torrent.clone(),
            language: p.language.clone(),
        }
    }
}

/// Where DownloadCraft's source lives (the workspace's `repository`).
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");

pub(super) fn frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new().fill(t.surface).corner_radius(14).inner_margin(24).stroke(egui::Stroke::new(1.0, t.border))
}

/// The title row with a close button; true when it was clicked.
pub(super) fn header(ui: &mut Ui, t: &Tokens, title: &str, subtitle: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(tl!(title)).size(19.0).strong());
            ui.label(RichText::new(tl!(subtitle)).size(13.0).color(t.muted));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            close = widgets::button(ui, t, Kind::Ghost, Some(Icon::Close), "", true).on_hover_text(tl!("Close (Esc)")).clicked();
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

/// A labelled setting: the name and a line under it on the left, wrapping in the room `control`
/// leaves (`room`, its width), and the control on the right, centred on them. A longer name or
/// line in another language wraps beside the control instead of running under it.
pub(super) fn setting(ui: &mut Ui, t: &Tokens, name: &str, help: &str, room: f32, control: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let w = (ui.available_width() - room - 12.0).max(120.0);
        ui.allocate_ui_with_layout(egui::vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(w);
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(egui::Label::new(RichText::new(tl!(name)).size(14.0)).wrap());
            ui.add(egui::Label::new(RichText::new(tl!(help)).size(12.0).color(t.muted)).wrap());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

/// The parts of Settings, each a page of its own beside the list of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Section {
    #[default]
    General,
    Downloads,
    Videos,
    Torrents,
    Connection,
    About,
}

impl Section {
    /// Each with its icon and name, in the order they're listed.
    pub(crate) const ALL: [(Section, Icon, &'static str); 6] = [
        (Section::General, Icon::Settings, "General"),
        (Section::Downloads, Icon::Download, "Downloads"),
        (Section::Videos, Icon::Video, "Videos"),
        (Section::Torrents, Icon::Swarm, "Torrents"),
        (Section::Connection, Icon::Globe, "Connection"),
        (Section::About, Icon::Info, "About"),
    ];
}

/// Where the section last shown is kept, for this run.
pub(crate) fn section_key() -> egui::Id {
    egui::Id::unique("settings-section")
}

/// `None` while open; `Some(true)` to save, `Some(false)` cancelled. `theme` is applied as it's
/// picked, so the dialog shows it.
/// What Settings › About shows of updates, and where its buttons' answer goes.
pub type Updates<'a> = (&'a crate::UpdateState, &'a crate::UpdateCheck, &'a mut Option<crate::UpdateChoice>);

pub fn settings(ctx: &egui::Context, f: &mut SettingsForm, t: &Tokens, services: &dyn Services, version: &str, updates: Updates) -> Option<bool> {
    let (update, check, choice) = updates;
    let mut out = None;
    // The section last shown, for this run.
    let key = section_key();
    let mut section: Section = ctx.data(|d| d.get_temp(key)).unwrap_or_default();
    let modal = egui::Modal::new(egui::Id::unique("settings-dialog")).frame(frame(t)).show(ctx, |ui| {
        let width = (ctx.content_rect().width() - 80.0).clamp(560.0, 780.0);
        ui.set_width(width);
        if header(ui, t, "Settings", "Changes apply when you save.") {
            out = Some(false);
        }
        // One height for every section, so the dialog doesn't jump from one to the next; a
        // section taller than the window scrolls inside it.
        let height = (ctx.content_rect().height() - 230.0).clamp(220.0, 440.0);
        ui.horizontal_top(|ui| {
            // The sections, as the window's sidebar lists categories.
            ui.allocate_ui_with_layout(vec2(176.0, height), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(176.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                for (s, icon, label) in Section::ALL {
                    if widgets::nav_row(ui, t, icon, label, 0, section == s).clicked() {
                        section = s;
                    }
                }
            });
            ui.add_space(8.0);
            let (line, _) = ui.allocate_exact_size(vec2(1.0, height), egui::Sense::hover());
            ui.painter().rect_filled(line, 0.0, t.border);
            ui.add_space(16.0);
            ui.allocate_ui_with_layout(vec2(ui.available_width(), height), egui::Layout::top_down(egui::Align::Min), |ui| {
                egui::ScrollArea::vertical().id_salt(("settings-page", section as u8)).max_height(height).auto_shrink([false, false]).show(
                    ui,
                    |ui| {
                        // The width the scroll bar's lane leaves, when there is one.
                        let width = ui.available_width();
                        ui.set_width(width);
                        page(ui, t, f, section, services, version, width);
                        if section == Section::About {
                            ui.add_space(14.0);
                            widgets::section(ui, t, "Updates");
                            crate::DownloadApp::update_panel(ui, t, update, check, choice);
                        }
                    },
                );
            });
        });
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
    ctx.data_mut(|d| d.insert_temp(key, section));
    if modal.should_close() && out.is_none() {
        out = Some(false);
    }
    out
}

/// One section's settings.
fn page(ui: &mut Ui, t: &Tokens, f: &mut SettingsForm, section: Section, services: &dyn Services, version: &str, width: f32) {
    match section {
        Section::General => {
            widgets::section(ui, t, "Appearance");
            setting(ui, t, "Theme", "System follows your computer.", 340.0, |ui| {
                widgets::segmented(
                    ui,
                    t,
                    &mut f.theme,
                    &[(Theme::Dark, Icon::Moon, "Dark"), (Theme::Light, Icon::Sun, "Light"), (Theme::System, Icon::Monitor, "System")],
                );
            });
            ui.add_space(8.0);
            setting(ui, t, "Language", "Follows your computer's language unless you pick one.", 172.0, |ui| {
                let name = |code: &str| crate::i18n::Lang::from_code(code).map_or(tl!("Same as the system"), crate::i18n::Lang::name);
                egui::ComboBox::from_id_salt("language").width(170.0).selected_text(name(&f.language)).show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.language, crate::i18n::AUTO.to_string(), tl!("Same as the system"));
                    for l in crate::i18n::Lang::all() {
                        ui.selectable_value(&mut f.language, l.code().to_string(), l.name());
                    }
                });
            });
        }
        Section::Downloads => {
            widgets::section(ui, t, "Downloads");
            widgets::field_label(ui, t, "Download folder");
            folder_row(ui, t, &mut f.folder, services, width);
            ui.add_space(10.0);
            setting(ui, t, "Downloads at once", "The rest wait in the queue.", 120.0, |ui| {
                widgets::stepper(ui, t, &mut f.max_active, 1, MAX_ACTIVE_LIMIT);
            });
            ui.add_space(8.0);
            setting(ui, t, "Connections per download", "Parts of one file fetched at the same time, when the server allows.", 120.0, |ui| {
                widgets::stepper(ui, t, &mut f.connections, 1, usize::from(MAX_CONNECTIONS));
            });
        }
        Section::Videos => {
            widgets::section(ui, t, "Videos");
            setting(ui, t, "Subtitles", "Languages to save with videos, such as en, es. Empty: none.", 152.0, |ui| {
                ui.add(egui::TextEdit::singleline(&mut f.subtitles).desired_width(150.0).hint_text(tl!("none")).char_limit(100));
            });
        }
        Section::Torrents => {
            widgets::section(ui, t, "Torrents");
            torrents::sharing(ui, t, &mut f.seed_ratio, &mut f.torrent);
        }
        Section::Connection => {
            widgets::section(ui, t, "Connection");
            torrents::connection(ui, t, &mut f.torrent, services, width);
        }
        Section::About => {
            widgets::section(ui, t, "About");
            // The name and version, the licence with a link to the source, and how updates come:
            // lines of one block, set close together.
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.spacing_mut().interact_size.y = 16.0;
                ui.label(RichText::new(format!("DownloadCraft {version}")).size(15.0).strong());
                ui.add(
                    egui::Label::new(RichText::new(tl!("Free and open source, under the MIT or Apache-2.0 licence.")).size(12.5).color(t.muted))
                        .wrap(),
                );
                let shown = REPOSITORY.trim_start_matches("https://");
                ui.hyperlink_to(RichText::new(shown).size(12.5).color(t.accent), REPOSITORY).on_hover_text(REPOSITORY);
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(tl!(
                            "DownloadCraft keeps itself up to date. When a new version is ready, a bar at the top lets you restart to install it."
                        ))
                        .size(12.5)
                        .color(t.muted),
                    )
                    .wrap(),
                );
            });
        }
    }
}
