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

/// The font links are written in: monospace, so long ones read character by character.
const LINK_FONT: f32 = 13.0;
/// Room inside the box around the text.
const LINK_PAD: f32 = 8.0;
/// The numbered strip on the left.
const GUTTER: f32 = 46.0;

/// Where a line of the links box stands: nothing, a link that downloads now, one that's coming
/// in a later version, or not a link.
fn line_color(line: &str, t: &Tokens) -> Option<egui::Color32> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    Some(match source::classify(line) {
        Ok(k) if k.not_yet().is_none() => t.success,
        Ok(_) => t.warning,
        Err(_) => t.danger,
    })
}

/// The links box: one link per line, never wrapped (a long one scrolls sideways), each line
/// numbered with a dot saying whether it can be added; a count and Clear above it.
fn links_box(ui: &mut Ui, t: &Tokens, links: &mut String) {
    let count = links.lines().filter(|l| !l.trim().is_empty()).count();
    ui.horizontal(|ui| {
        widgets::field_label(ui, t, "Links");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if count > 0 && ui.add(egui::Button::new(RichText::new("Clear").size(12.5).color(t.accent)).frame(false)).clicked() {
                links.clear();
            }
            let text = match count {
                0 => "one per line".to_string(),
                1 => "1 link".to_string(),
                n => format!("{n} links"),
            };
            ui.label(RichText::new(text).size(12.5).color(t.faint));
        });
    });
    let font = egui::FontId::monospace(LINK_FONT);
    let row = ui.fonts_mut(|fonts| fonts.row_height(&font));
    let rows = links.lines().count().max(1);
    // Four lines tall at first, growing to seven; past that it scrolls.
    let visible = rows.clamp(4, 7) as f32;
    let height = visible * row + 2.0 * LINK_PAD;
    let id = egui::Id::unique("add-links");
    let focused = ui.memory(|m| m.has_focus(id));
    let frame = egui::Frame::new()
        .fill(t.card)
        .corner_radius(8)
        .stroke(egui::Stroke::new(if focused { 1.5 } else { 1.0 }, if focused { t.accent } else { t.border }));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().id_salt("links-v").max_height(height).min_scrolled_height(height).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                // The gutter: a number and a dot per line.
                let lines: Vec<&str> = if links.is_empty() { vec![""] } else { links.split('\n').collect() };
                let (gutter, _) =
                    ui.allocate_exact_size(vec2(GUTTER, (lines.len().max(visible as usize)) as f32 * row + 2.0 * LINK_PAD), Sense::hover());
                let p = ui.painter();
                p.vline(gutter.right() - 0.5, gutter.y_range(), egui::Stroke::new(1.0, t.border));
                for (i, line) in lines.iter().enumerate() {
                    let y = gutter.top() + LINK_PAD + row * (i as f32 + 0.5);
                    if let Some(c) = line_color(line, t) {
                        p.circle_filled(egui::pos2(gutter.left() + 11.0, y), 3.5, c);
                    }
                    let number = (i + 1).to_string();
                    p.text(egui::pos2(gutter.right() - 9.0, y), egui::Align2::RIGHT_CENTER, number, egui::FontId::monospace(11.5), t.faint);
                }
                // The text: laid out without wrapping, so each link stays on its own line.
                let text_color = t.text;
                let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                    let job = egui::text::LayoutJob::simple(buf.as_str().to_string(), egui::FontId::monospace(LINK_FONT), text_color, f32::INFINITY);
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                egui::ScrollArea::horizontal().id_salt("links-h").show(ui, |ui| {
                    let edit = ui.add(
                        egui::TextEdit::multiline(links)
                            .id(id)
                            .frame(egui::Frame::new())
                            .desired_rows(visible as usize)
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin { left: 12, right: LINK_PAD as i8, top: LINK_PAD as i8, bottom: LINK_PAD as i8 })
                            .layouter(&mut layouter)
                            .hint_text(RichText::new("https://example.com/file.zip").font(egui::FontId::monospace(LINK_FONT)).color(t.faint)),
                    );
                    if ui.memory(|m| m.focused().is_none()) {
                        edit.request_focus();
                    }
                });
            });
        });
    });
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
        links_box(ui, t, &mut f.links);
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
