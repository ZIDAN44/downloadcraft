//! Small widgets: buttons, sidebar rows, text fields, steppers and a segmented choice.

use egui::{Color32, FontId, Rect, Response, RichText, Sense, Stroke, Ui, vec2};

use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// How a [`button`] looks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Filled with the app colour: the one main action.
    Primary,
    /// Outlined.
    Secondary,
    /// No frame until hovered (toolbars).
    Ghost,
}

/// A button with an optional icon; `label` may be empty for an icon-only one.
pub fn button(ui: &mut Ui, t: &Tokens, kind: Kind, icon: Option<Icon>, label: &str, enabled: bool) -> Response {
    let font = FontId::proportional(14.0);
    let fg = match (enabled, kind) {
        (false, _) => t.faint,
        (true, Kind::Primary) => Color32::WHITE,
        (true, _) => t.text,
    };
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, fg);
    let icon_w = if icon.is_some() { 18.0 } else { 0.0 };
    let gap = if icon.is_some() && !label.is_empty() { 8.0 } else { 0.0 };
    let pad = if label.is_empty() { 8.0 } else { 14.0 };
    let size = vec2((pad * 2.0 + icon_w + gap + galley.size().x).max(34.0), 34.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let hovered = enabled && resp.hovered();
    match kind {
        Kind::Primary => {
            let fill = if !enabled {
                t.chrome
            } else if hovered {
                t.accent.gamma_multiply(0.88)
            } else {
                t.accent
            };
            p.rect_filled(rect, 8.0, fill);
        }
        Kind::Secondary => {
            p.rect_filled(rect, 8.0, if hovered { t.hover } else { t.card });
            p.rect_stroke(rect, 8.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        }
        Kind::Ghost => {
            if hovered {
                p.rect_filled(rect, 8.0, t.hover);
            }
        }
    }
    let mut x = rect.left() + pad;
    if label.is_empty() {
        x = rect.center().x - icon_w / 2.0;
    }
    if let Some(i) = icon {
        let color = match (enabled, kind) {
            (false, _) => t.faint,
            (true, Kind::Primary) => Color32::WHITE,
            (true, Kind::Ghost) => t.accent,
            (true, Kind::Secondary) => t.text,
        };
        icons::paint(p, Rect::from_center_size(egui::pos2(x + icon_w / 2.0, rect.center().y), vec2(16.0, 16.0)), i, color);
        x += icon_w + gap;
    }
    p.galley_with_override_text_color(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    resp.on_hover_cursor(if enabled { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Default })
}

/// A row in the sidebar: an icon, a name, and a count in a pill.
pub fn nav_row(ui: &mut Ui, t: &Tokens, icon: Icon, label: &str, count: usize, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 8.0, t.accent_soft);
    } else if resp.hovered() {
        p.rect_filled(rect, 8.0, t.hover);
    }
    let color = if selected { t.accent } else { t.muted };
    icons::paint(p, Rect::from_center_size(egui::pos2(rect.left() + 20.0, rect.center().y), vec2(16.0, 16.0)), icon, color);
    let text = RichText::new(label).size(14.0).color(if selected { t.text } else { t.muted });
    let text = if selected { text.strong() } else { text };
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Truncate), rect.width() - 80.0, egui::TextStyle::Body);
    ui.painter().galley(egui::pos2(rect.left() + 38.0, rect.center().y - galley.size().y / 2.0), galley, t.text);
    if count > 0 {
        let g = ui.painter().layout_no_wrap(count.to_string(), FontId::proportional(11.5), if selected { Color32::WHITE } else { t.muted });
        let w = g.size().x + 14.0;
        let pill = Rect::from_center_size(egui::pos2(rect.right() - 12.0 - w / 2.0, rect.center().y), vec2(w, 20.0));
        ui.painter().rect_filled(pill, 10.0, if selected { t.accent } else { t.chrome });
        ui.painter().galley(pill.center() - g.size() / 2.0, g, t.text);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small label over a field.
pub fn field_label(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).size(12.5).color(t.muted).strong());
}

/// A section heading in a dialog.
pub fn section(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text.to_uppercase()).size(11.5).color(t.faint).strong());
}

/// A one-line text field with comfortable padding.
pub fn text_field(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> Response {
    ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(width).margin(egui::Margin::symmetric(10, 7)))
}

/// `−  n  +`: a number from `min` to `max` (laid out the right way round in a right-to-left row).
pub fn stepper(ui: &mut Ui, t: &Tokens, value: &mut usize, min: usize, max: usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let rtl = ui.layout().prefer_right_to_left();
        let minus = |ui: &mut Ui, value: &mut usize| {
            if button(ui, t, Kind::Secondary, None, "−", *value > min).clicked() {
                *value = value.saturating_sub(1).max(min);
            }
        };
        let plus = |ui: &mut Ui, value: &mut usize| {
            if button(ui, t, Kind::Secondary, None, "+", *value < max).clicked() {
                *value = (*value + 1).min(max);
            }
        };
        let number = |ui: &mut Ui, value: &usize| {
            let (rect, _) = ui.allocate_exact_size(vec2(44.0, 34.0), Sense::hover());
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, value.to_string(), FontId::proportional(15.0), t.text);
        };
        if rtl {
            plus(ui, value);
            number(ui, value);
            minus(ui, value);
        } else {
            minus(ui, value);
            number(ui, value);
            plus(ui, value);
        }
    });
}

/// A row of options of which one is chosen, as one control.
pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, t: &Tokens, value: &mut T, options: &[(T, Icon, &str)]) {
    let width = 112.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width * options.len() as f32 + 8.0, 40.0), Sense::hover());
    ui.painter().rect_filled(rect, 10.0, t.chrome);
    for (i, (v, icon, label)) in options.iter().enumerate() {
        let r = Rect::from_min_size(rect.min + vec2(4.0 + width * i as f32, 4.0), vec2(width, 32.0));
        let resp = ui.interact(r, ui.scope_id().with(("segmented", i)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let on = *value == *v;
        if on {
            ui.painter().rect_filled(r, 8.0, t.surface);
            ui.painter().rect_stroke(r, 8.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, 8.0, t.hover);
        }
        let g = ui.painter().layout_no_wrap(label.to_string(), FontId::proportional(13.5), if on { t.text } else { t.muted });
        let x0 = r.center().x - (22.0 + g.size().x) / 2.0;
        icons::paint(
            ui.painter(),
            Rect::from_center_size(egui::pos2(x0 + 8.0, r.center().y), vec2(15.0, 15.0)),
            *icon,
            if on { t.accent } else { t.muted },
        );
        ui.painter().galley(egui::pos2(x0 + 22.0, r.center().y - g.size().y / 2.0), g, t.text);
        if resp.clicked() {
            *value = *v;
        }
    }
}
