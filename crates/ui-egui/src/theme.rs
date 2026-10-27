//! Colours, spacing and type: DownloadCraft's own blue, light and dark.

use egui::{Color32, CornerRadius, Stroke, Visuals};

/// DownloadCraft blue (the app icon's colour).
pub const APP_COLOR: Color32 = Color32::from_rgb(0x1F, 0x6F, 0xD6);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    /// Behind everything (the sidebar).
    pub bg: Color32,
    /// The main area and dialogs.
    pub surface: Color32,
    /// A raised card or field on the surface.
    pub card: Color32,
    pub chrome: Color32,
    pub border: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    /// The accent, softly, for selected things and chips.
    pub accent_soft: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
}

impl Tokens {
    pub fn light() -> Self {
        Tokens {
            dark: false,
            bg: Color32::from_rgb(0xF3, 0xF5, 0xF8),
            surface: Color32::WHITE,
            card: Color32::from_rgb(0xF7, 0xF9, 0xFC),
            chrome: Color32::from_rgb(0xEE, 0xF1, 0xF5),
            border: Color32::from_rgb(0xDF, 0xE3, 0xE9),
            hover: Color32::from_rgb(0xEC, 0xF2, 0xFB),
            selected: Color32::from_rgb(0xE0, 0xEB, 0xFB),
            text: Color32::from_rgb(0x1B, 0x20, 0x28),
            muted: Color32::from_rgb(0x5C, 0x66, 0x73),
            faint: Color32::from_rgb(0x95, 0x9E, 0xAA),
            accent: APP_COLOR,
            accent_soft: Color32::from_rgb(0xE3, 0xEE, 0xFD),
            success: Color32::from_rgb(0x1E, 0x8E, 0x3E),
            warning: Color32::from_rgb(0xC2, 0x74, 0x00),
            danger: Color32::from_rgb(0xD0, 0x31, 0x31),
        }
    }
    pub fn dark() -> Self {
        Tokens {
            dark: true,
            bg: Color32::from_rgb(0x14, 0x16, 0x1A),
            surface: Color32::from_rgb(0x1B, 0x1E, 0x23),
            card: Color32::from_rgb(0x22, 0x26, 0x2C),
            chrome: Color32::from_rgb(0x20, 0x23, 0x29),
            border: Color32::from_rgb(0x30, 0x35, 0x3D),
            hover: Color32::from_rgb(0x27, 0x2D, 0x36),
            selected: Color32::from_rgb(0x1F, 0x33, 0x52),
            text: Color32::from_rgb(0xE7, 0xEA, 0xEE),
            muted: Color32::from_rgb(0x9C, 0xA5, 0xB0),
            faint: Color32::from_rgb(0x67, 0x6F, 0x7A),
            accent: Color32::from_rgb(0x4C, 0x8F, 0xEA),
            accent_soft: Color32::from_rgb(0x1E, 0x30, 0x4C),
            success: Color32::from_rgb(0x4C, 0xC3, 0x6B),
            warning: Color32::from_rgb(0xE8, 0xA3, 0x3D),
            danger: Color32::from_rgb(0xEF, 0x6B, 0x6B),
        }
    }
    pub fn for_mode(dark: bool) -> Self {
        if dark { Tokens::dark() } else { Tokens::light() }
    }
}

pub fn apply(ctx: &egui::Context, t: &Tokens) {
    let mut v = if t.dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = t.surface;
    v.window_fill = t.surface;
    v.extreme_bg_color = t.card;
    v.faint_bg_color = t.chrome;
    v.selection.bg_fill = t.accent.gamma_multiply(if t.dark { 0.55 } else { 0.35 });
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent;
    v.override_text_color = Some(t.text);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0, t.border);
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if t.dark { 120 } else { 40 }) };
    v.window_shadow = egui::epaint::Shadow { offset: [0, 10], blur: 32, spread: 0, color: Color32::from_black_alpha(if t.dark { 140 } else { 50 }) };
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(8);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.weak_bg_fill = t.card;
    v.widgets.inactive.bg_fill = t.card;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_fill = t.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.accent.gamma_multiply(0.6));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.active.weak_bg_fill = t.selected;
    v.widgets.active.bg_fill = t.selected;
    v.widgets.active.bg_stroke = Stroke::new(1.0, t.accent);
    v.slider_trailing_fill = true;
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.button_padding = egui::vec2(12.0, 6.0);
        s.spacing.interact_size.y = 32.0;
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
    });
}
