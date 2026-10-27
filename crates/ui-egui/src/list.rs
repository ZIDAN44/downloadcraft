//! The download list: one row per download, newest first, with a bar that shows each part a
//! connection is fetching (as IDM does), and the row's actions when the pointer is over it.

use std::path::{Path, PathBuf};

use downloadcraft_engine::model::format;
use downloadcraft_engine::{Download, DownloadId, Session, State};
use egui::{Align2, Color32, FontId, Rect, Sense, Ui, pos2, vec2};

use crate::icons::{self, Icon};
use crate::theme::Tokens;

pub enum Action {
    Select(DownloadId),
    Run(&'static str, DownloadId),
    Open(PathBuf),
    /// Show the file in the file manager, selected (its folder when the file has gone).
    Reveal(PathBuf),
    /// Ask before removing.
    Remove(DownloadId),
    CopyLink(String),
    Add,
}

const ROW: f32 = 72.0;

/// The downloads the filter and the search show.
pub fn visible<'a>(s: &'a Session, search: &str) -> Vec<&'a Download> {
    let needle = search.trim().to_lowercase();
    s.visible().filter(|d| needle.is_empty() || d.file_name.to_lowercase().contains(&needle) || d.url.to_lowercase().contains(&needle)).collect()
}

pub fn show(s: &Session, search: &str, ui: &mut Ui, t: &Tokens, act: &mut Option<Action>) {
    let rows = visible(s, search);
    if rows.is_empty() {
        empty(s, !search.trim().is_empty(), ui, t, act);
        return;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW, rows.len(), |ui, range| {
        for d in rows.get(range).unwrap_or_default() {
            row(d, s.selection == Some(d.id), ui, t, act);
        }
    });
}

fn empty(s: &Session, searching: bool, ui: &mut Ui, t: &Tokens, act: &mut Option<Action>) {
    let (title, hint) = if searching {
        ("No matches", "Nothing in this list has that name or link.")
    } else if s.downloads.is_empty() {
        ("No downloads yet", "Paste a link anywhere in this window with Ctrl+V, or add one.")
    } else {
        ("Nothing here", "Downloads in this category show up here.")
    };
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.5 - 130.0).max(24.0));
        let (r, _) = ui.allocate_exact_size(vec2(88.0, 88.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 44.0, t.accent_soft);
        icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(40.0, 40.0)), Icon::Download, t.accent);
        ui.add_space(14.0);
        ui.label(egui::RichText::new(tl!(title)).size(20.0).strong());
        ui.add_space(2.0);
        ui.label(egui::RichText::new(tl!(hint)).color(t.muted));
        if s.downloads.is_empty() && !searching {
            ui.add_space(16.0);
            if crate::widgets::button(ui, t, crate::widgets::Kind::Primary, Some(Icon::Add), "Add link", true).clicked() {
                *act = Some(Action::Add);
            }
            ui.add_space(18.0);
            ui.label(
                egui::RichText::new(tl!("Direct links and video pages download now · torrents, magnet links and MEGA are on the way"))
                    .size(12.0)
                    .color(t.faint),
            );
        }
    });
}

/// What the second line says: how far, how fast, how long, on how many connections.
fn detail(d: &Download) -> String {
    if d.missing {
        return tl!("Deleted or moved  ·  Download again fetches it once more").into();
    }
    let size = match d.total {
        Some(t) if d.state != State::Completed => tf!("{done} of {total}", done = format::bytes(d.done), total = format::bytes(t)),
        Some(t) => format::bytes(t),
        None if d.done > 0 => format::bytes(d.done),
        None => String::new(),
    };
    let mut bits = vec![];
    if let Some(v) = &d.video {
        bits.push(tf!("Video · {quality}", quality = tl!(&v.label)));
    }
    if !size.is_empty() {
        bits.push(size);
    }
    match d.state {
        State::Downloading if d.speed > 0 => {
            bits.push(format::speed(d.speed));
            if let Some(eta) = d.eta() {
                bits.push(tf!("{time} left", time = format::duration(eta)));
            }
            let going = d.segments.iter().filter(|s| !s.finished()).count();
            if going > 1 {
                bits.push(tf!("{n} connections", n = going));
            }
        }
        State::Downloading => bits.push(tl!("Connecting…").into()),
        State::Queued => bits.push(tl!("Waiting for a free slot").into()),
        State::Failed => bits.push(d.error.clone().unwrap_or_else(|| tl!("Failed").into())),
        State::Paused if d.done > 0 && !d.resumable => bits.push(tl!("starts over when resumed").into()),
        _ => {}
    }
    bits.join("  ·  ")
}

fn state_color(d: &Download, t: &Tokens) -> Color32 {
    if d.missing {
        return t.faint;
    }
    match d.state {
        State::Completed => t.success,
        State::Failed => t.danger,
        State::Paused => t.warning,
        State::Queued => t.muted,
        State::Downloading => t.accent,
    }
}

/// The progress bar: one thin bar, filled part by part when connections fetch parts.
fn bar(d: &Download, rect: Rect, ui: &Ui, t: &Tokens) {
    let p = ui.painter();
    p.rect_filled(rect, rect.height() / 2.0, t.chrome);
    let color = state_color(d, t);
    let total = d.total.unwrap_or(0);
    if !d.segments.is_empty() && total > 0 && d.state == State::Downloading {
        let x = |b: u64| rect.left() + rect.width() * (b as f64 / total as f64) as f32;
        for s in &d.segments {
            let (a, b) = (x(s.start), x(s.start.saturating_add(s.done)));
            if b > a {
                p.rect_filled(Rect::from_x_y_ranges(a..=b, rect.y_range()), 1.5, color);
            }
            // Where each part starts, as a hairline.
            if s.start > 0 {
                p.vline(x(s.start), rect.y_range(), egui::Stroke::new(1.0, t.surface));
            }
        }
        return;
    }
    match d.fraction() {
        Some(f) if f > 0.0 => {
            let w = (rect.width() * f).max(rect.height());
            p.rect_filled(Rect::from_min_size(rect.min, vec2(w, rect.height())), rect.height() / 2.0, color);
        }
        None if d.state == State::Downloading => {
            // An unknown size: a sliding stripe while it runs.
            let time = ui.input(|i| i.time) as f32;
            let x = rect.left() + (time * 0.6).fract() * (rect.width() - 60.0);
            p.rect_filled(Rect::from_min_size(pos2(x, rect.top()), vec2(60.0, rect.height())), rect.height() / 2.0, color);
            ui.ctx().request_repaint();
        }
        _ => {}
    }
}

/// A small round button drawn at `center`; true when clicked.
fn row_button(ui: &mut Ui, id: egui::Id, center: egui::Pos2, icon: Icon, tip: &str, t: &Tokens) -> bool {
    let r = Rect::from_center_size(center, vec2(32.0, 32.0));
    let resp = ui.interact(r, id, Sense::click()).on_hover_text(tl!(tip)).on_hover_cursor(egui::CursorIcon::PointingHand);
    let p = ui.painter();
    p.rect_filled(r, 8.0, if resp.hovered() { t.surface } else { t.card });
    p.rect_stroke(r, 8.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    icons::paint(p, Rect::from_center_size(center, vec2(15.0, 15.0)), icon, if icon == Icon::Remove { t.danger } else { t.text });
    resp.clicked()
}

fn row(d: &Download, selected: bool, ui: &mut Ui, t: &Tokens, act: &mut Option<Action>) {
    let (outer, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    let rect = outer.shrink2(vec2(12.0, 4.0));
    let hovered = ui.rect_contains_pointer(outer);
    {
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, 10.0, t.selected);
        } else if hovered {
            p.rect_filled(rect, 10.0, t.hover);
        }
        icons::file_badge(p, Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(40.0, 40.0)), &d.file_name, t.dark);
        // A file that's gone is shown faded, as a browser's downloads page does.
        if d.missing {
            let veil = if selected {
                t.selected
            } else if hovered {
                t.hover
            } else {
                t.surface
            };
            p.rect_filled(Rect::from_min_size(rect.min + vec2(12.0, 12.0), vec2(40.0, 40.0)), 8.0, veil.gamma_multiply(0.65));
        }
    }

    let file = Path::new(&d.folder).join(&d.file_name);
    let actions_w = if hovered { 124.0 } else { 0.0 };
    let right = rect.right() - 16.0;
    let label_w = 92.0;
    let bar_w = (rect.width() * 0.26).clamp(110.0, 280.0);
    let text_x = rect.left() + 66.0;
    let text_w = (right - label_w - bar_w - 24.0 - text_x - actions_w).max(60.0);
    let name = egui::WidgetText::from(egui::RichText::new(&d.file_name).size(14.5).strong()).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        text_w,
        egui::TextStyle::Body,
    );
    let info = egui::WidgetText::from(egui::RichText::new(detail(d)).size(12.5)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        text_w,
        egui::TextStyle::Body,
    );
    ui.painter().galley(pos2(text_x, rect.top() + 14.0), name, if d.missing { t.faint } else { t.text });
    if d.missing {
        // The name struck through.
        let w = ui.painter().layout_no_wrap(d.file_name.clone(), FontId::proportional(14.5), t.faint).size().x.min(text_w);
        let y = rect.top() + 14.0 + 10.0;
        ui.painter().hline(text_x..=text_x + w, y, egui::Stroke::new(1.0, t.faint));
    }
    ui.painter().galley(pos2(text_x, rect.top() + 37.0), info, if d.state == State::Failed { t.danger } else { t.muted });

    let bar_rect = Rect::from_min_size(pos2(right - label_w - bar_w - actions_w, rect.center().y - 3.0), vec2(bar_w, 6.0));
    bar(d, bar_rect, ui, t);
    let label = match (d.state, d.fraction()) {
        (State::Downloading, Some(f)) => format!("{:.0}%", f * 100.0),
        (State::Downloading, None) => format::bytes(d.done),
        _ if d.missing => tl!("Deleted").to_string(),
        // One download's state, which some languages say differently from the category
        // (`Completed` the category, `Completed` this one).
        (s, _) => crate::i18n::tc("state", s.label()).to_string(),
    };
    ui.painter().text(pos2(right - actions_w, rect.center().y), Align2::RIGHT_CENTER, label, FontId::proportional(13.0), state_color(d, t));

    if hovered {
        let y = rect.center().y;
        let mut x = right - 16.0;
        let id = ui.scope_id().with(("row", d.id.0));
        if row_button(ui, id.with("remove"), pos2(x, y), Icon::Remove, "Remove (Delete)", t) {
            *act = Some(Action::Remove(d.id));
        }
        x -= 40.0;
        let folder_tip = if d.state == State::Completed && !d.missing { "Show in folder" } else { "Open folder" };
        if row_button(ui, id.with("folder"), pos2(x, y), Icon::Folder, folder_tip, t) {
            *act = Some(Action::Reveal(file.clone()));
        }
        x -= 40.0;
        let (icon, tip, cmd) = if d.missing {
            (Icon::Download, "Download again", Some("download.again"))
        } else if d.state.active() {
            (Icon::Pause, "Pause", Some("download.pause"))
        } else if d.state.startable() {
            (Icon::Resume, "Resume", Some("download.resume"))
        } else {
            (Icon::Check, "Open", None)
        };
        if row_button(ui, id.with("main"), pos2(x, y), icon, tip, t) {
            *act = Some(match cmd {
                Some(c) => Action::Run(c, d.id),
                None => Action::Open(file.clone()),
            });
        }
    }

    if resp.clicked() && act.is_none() {
        *act = Some(Action::Select(d.id));
    }
    if resp.double_clicked() {
        *act = Some(if d.missing {
            Action::Run("download.again", d.id)
        } else if d.state == State::Completed {
            Action::Open(file.clone())
        } else if d.state.active() {
            Action::Run("download.pause", d.id)
        } else {
            Action::Run("download.resume", d.id)
        });
    }
    resp.context_menu(|ui| {
        if d.state.startable() && ui.button(tl!("Resume")).clicked() {
            *act = Some(Action::Run("download.resume", d.id));
        }
        if d.state.active() && ui.button(tl!("Pause")).clicked() {
            *act = Some(Action::Run("download.pause", d.id));
        }
        if d.state == State::Completed && !d.missing && ui.button(tl!("Open")).clicked() {
            *act = Some(Action::Open(file.clone()));
        }
        if d.missing && ui.button(tl!("Download again")).clicked() {
            *act = Some(Action::Run("download.again", d.id));
        }
        let folder = if d.state == State::Completed && !d.missing { "Show in folder" } else { "Open folder" };
        if ui.button(tl!(folder)).clicked() {
            *act = Some(Action::Reveal(file.clone()));
        }
        if ui.button(tl!("Copy link")).clicked() {
            *act = Some(Action::CopyLink(d.url.clone()));
        }
        ui.separator();
        if ui.button(tl!("Remove…")).clicked() {
            *act = Some(Action::Remove(d.id));
        }
    });
}
