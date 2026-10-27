//! The download list: one row per download, newest first.

use std::path::{Path, PathBuf};

use downloadcraft_engine::model::format;
use downloadcraft_engine::{Download, DownloadId, Session, State};
use egui::{Align2, FontId, Rect, Sense, Ui, pos2, vec2};

use crate::icons;
use crate::theme::Tokens;

pub enum Action {
    Select(DownloadId),
    Run(&'static str, DownloadId),
    Open(PathBuf),
    CopyLink(String),
    Add,
}

const ROW: f32 = 54.0;

pub fn show(s: &Session, ui: &mut Ui, t: &Tokens, act: &mut Option<Action>) {
    let rows: Vec<&Download> = s.visible().collect();
    if rows.is_empty() {
        empty(s, ui, t, act);
        return;
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW, rows.len(), |ui, range| {
        for d in rows.get(range).unwrap_or_default() {
            row(d, s.selection == Some(d.id), ui, t, act);
        }
    });
}

fn empty(s: &Session, ui: &mut Ui, t: &Tokens, act: &mut Option<Action>) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.3);
        let (title, hint) = if s.downloads.is_empty() {
            ("No downloads yet", "Paste a link anywhere in the window (Ctrl+V), or add one.")
        } else {
            ("Nothing here", "Downloads in this category show up here.")
        };
        ui.label(egui::RichText::new(title).size(18.0).strong());
        ui.add_space(4.0);
        ui.label(egui::RichText::new(hint).color(t.muted));
        if s.downloads.is_empty() {
            ui.add_space(12.0);
            if ui.button("Add link").clicked() {
                *act = Some(Action::Add);
            }
        }
    });
}

/// What the second line says: how far, how fast, how long.
fn detail(d: &Download) -> String {
    let size = match d.total {
        Some(t) if d.state != State::Completed => format!("{} of {}", format::bytes(d.done), format::bytes(t)),
        Some(t) => format::bytes(t),
        None if d.done > 0 => format::bytes(d.done),
        None => String::new(),
    };
    let mut bits = vec![];
    if !size.is_empty() {
        bits.push(size);
    }
    match d.state {
        State::Downloading if d.speed > 0 => {
            bits.push(format::speed(d.speed));
            if let Some(eta) = d.eta() {
                bits.push(format!("{} left", format::duration(eta)));
            }
        }
        State::Downloading => bits.push("Connecting…".into()),
        State::Failed => bits.push(d.error.clone().unwrap_or_else(|| "Failed".into())),
        State::Paused if d.done > 0 && !d.resumable => bits.push("starts over when resumed".into()),
        _ => {}
    }
    bits.join(" · ")
}

fn row(d: &Download, selected: bool, ui: &mut Ui, t: &Tokens, act: &mut Option<Action>) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    let p = ui.painter_at(rect);
    if selected {
        p.rect_filled(rect, 0.0, t.selected);
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, t.hover);
    }
    p.hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, t.border));
    icons::file_badge(&p, Rect::from_min_size(rect.min + vec2(14.0, 11.0), vec2(32.0, 32.0)), &d.file_name, t.dark);

    let bar_w = (rect.width() * 0.28).clamp(120.0, 260.0);
    let text_right = rect.right() - bar_w - 120.0;
    let name_w = (text_right - rect.left() - 60.0).max(40.0);
    let name = egui::WidgetText::from(egui::RichText::new(&d.file_name).size(14.0).strong()).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        name_w,
        egui::TextStyle::Body,
    );
    p.galley(pos2(rect.left() + 58.0, rect.top() + 9.0), name, t.text);
    let info = egui::WidgetText::from(egui::RichText::new(detail(d)).size(12.0)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        name_w,
        egui::TextStyle::Body,
    );
    let info_color = if d.state == State::Failed { t.danger } else { t.muted };
    p.galley(pos2(rect.left() + 58.0, rect.top() + 30.0), info, info_color);

    // The bar and the state.
    let bar = Rect::from_min_size(pos2(rect.right() - bar_w - 110.0, rect.center().y - 4.0), vec2(bar_w, 8.0));
    p.rect_filled(bar, 4.0, t.chrome);
    let color = match d.state {
        State::Completed => t.success,
        State::Failed => t.danger,
        State::Paused => t.warning,
        _ => t.accent,
    };
    match d.fraction() {
        Some(f) => {
            let w = (bar.width() * f).max(if f > 0.0 { 8.0 } else { 0.0 });
            p.rect_filled(Rect::from_min_size(bar.min, vec2(w, bar.height())), 4.0, color);
        }
        // An unknown size: a sliding stripe while it runs.
        None if d.state == State::Downloading => {
            let time = ui.input(|i| i.time) as f32;
            let x = bar.left() + (time * 0.6).fract() * (bar.width() - 40.0);
            p.rect_filled(Rect::from_min_size(pos2(x, bar.top()), vec2(40.0, bar.height())), 4.0, color);
            ui.ctx().request_repaint();
        }
        None => {}
    }
    let pct = d.fraction().map(|f| format!("{:.0}%", f * 100.0)).unwrap_or_default();
    let label = if d.state == State::Downloading { pct } else { d.state.label().to_string() };
    p.text(pos2(rect.right() - 14.0, rect.center().y), Align2::RIGHT_CENTER, label, FontId::proportional(12.5), color);

    if resp.clicked() {
        *act = Some(Action::Select(d.id));
    }
    let file = Path::new(&d.folder).join(&d.file_name);
    if resp.double_clicked() {
        *act = Some(if d.state == State::Completed {
            Action::Open(file.clone())
        } else if d.state.active() {
            Action::Run("download.pause", d.id)
        } else {
            Action::Run("download.resume", d.id)
        });
    }
    resp.context_menu(|ui| {
        if d.state.startable() && ui.button("Resume").clicked() {
            *act = Some(Action::Run("download.resume", d.id));
        }
        if d.state.active() && ui.button("Pause").clicked() {
            *act = Some(Action::Run("download.pause", d.id));
        }
        if d.state == State::Completed && ui.button("Open").clicked() {
            *act = Some(Action::Open(file.clone()));
        }
        if ui.button("Open folder").clicked() {
            *act = Some(Action::Open(PathBuf::from(&d.folder)));
        }
        if ui.button("Copy link").clicked() {
            *act = Some(Action::CopyLink(d.url.clone()));
        }
        ui.separator();
        if ui.button("Remove").clicked() {
            *act = Some(Action::Run("download.remove", d.id));
        }
    });
}
