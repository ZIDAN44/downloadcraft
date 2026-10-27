//! Small widgets: toolbar buttons and category rows.

use egui::{Response, RichText, Sense, Ui, vec2};

use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// An icon-and-label toolbar button; `primary` fills it with the app colour.
pub fn tool_button(ui: &mut Ui, t: &Tokens, icon: Icon, label: &str, enabled: bool, primary: bool) -> Response {
    let font = egui::FontId::proportional(13.0);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, t.text);
    let size = vec2(galley.size().x + 40.0, 30.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let p = ui.painter();
    let fg = match (enabled, primary) {
        (false, _) => t.muted.gamma_multiply(0.6),
        (true, true) => egui::Color32::WHITE,
        (true, false) => t.text,
    };
    if primary && enabled {
        let fill = if resp.hovered() { t.accent.gamma_multiply(0.85) } else { t.accent };
        p.rect_filled(rect, 6.0, fill);
    } else if enabled && resp.hovered() {
        p.rect_filled(rect, 6.0, t.hover);
    }
    let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.left() + 18.0, rect.center().y), vec2(16.0, 16.0));
    icons::paint(p, icon_rect, icon, if enabled && !primary { t.accent } else { fg });
    p.galley_with_override_text_color(egui::pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0), galley, fg);
    resp.on_hover_cursor(if enabled { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Default })
}

pub fn separator(ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(9.0, 22.0), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), egui::Stroke::new(1.0, t.border));
}

/// A category in the left pane: its name and how many downloads it holds.
pub fn category(ui: &mut Ui, t: &Tokens, label: &str, count: usize, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 6.0, t.selected);
    } else if resp.hovered() {
        p.rect_filled(rect, 6.0, t.hover);
    }
    let text = RichText::new(label).size(13.5);
    let text = if selected { text.strong() } else { text };
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Truncate), rect.width() - 50.0, egui::TextStyle::Body);
    ui.painter().galley(egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0), galley, t.text);
    if count > 0 {
        ui.painter().text(
            rect.right_center() - vec2(10.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            count.to_string(),
            egui::FontId::proportional(12.0),
            t.muted,
        );
    }
    resp
}
