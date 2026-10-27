//! Colours and the egui style: DownloadCraft's own blue, light and dark.

use egui::{Color32, CornerRadius, Stroke, Visuals};

/// DownloadCraft blue (the app icon's colour).
pub const APP_COLOR: Color32 = Color32::from_rgb(0x1F, 0x6F, 0xD6);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub chrome: Color32,
    pub border: Color32,
    pub hover: Color32,
    pub selected: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
}

impl Tokens {
    pub fn light() -> Self {
        Tokens {
            dark: false,
            bg: Color32::from_rgb(0xF4, 0xF6, 0xF9),
            surface: Color32::WHITE,
            chrome: Color32::from_rgb(0xEC, 0xF0, 0xF5),
            border: Color32::from_rgb(0xD9, 0xDE, 0xE5),
            hover: Color32::from_rgb(0xE6, 0xEE, 0xFA),
            selected: Color32::from_rgb(0xD3, 0xE3, 0xFA),
            text: Color32::from_rgb(0x1E, 0x23, 0x2B),
            muted: Color32::from_rgb(0x5E, 0x67, 0x73),
            accent: APP_COLOR,
            success: Color32::from_rgb(0x1E, 0x8E, 0x3E),
            warning: Color32::from_rgb(0xB2, 0x6B, 0x00),
            danger: Color32::from_rgb(0xC6, 0x28, 0x28),
        }
    }
    pub fn dark() -> Self {
        Tokens {
            dark: true,
            bg: Color32::from_rgb(0x17, 0x19, 0x1D),
            surface: Color32::from_rgb(0x1F, 0x22, 0x27),
            chrome: Color32::from_rgb(0x26, 0x2A, 0x30),
            border: Color32::from_rgb(0x38, 0x3D, 0x45),
            hover: Color32::from_rgb(0x2B, 0x33, 0x40),
            selected: Color32::from_rgb(0x24, 0x3B, 0x5E),
            text: Color32::from_rgb(0xE6, 0xE9, 0xED),
            muted: Color32::from_rgb(0x9A, 0xA3, 0xAE),
            accent: Color32::from_rgb(0x4C, 0x8F, 0xEA),
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
    v.extreme_bg_color = if t.dark { Color32::from_rgb(0x15, 0x17, 0x1A) } else { Color32::WHITE };
    v.faint_bg_color = t.chrome;
    v.selection.bg_fill = t.accent.gamma_multiply(if t.dark { 0.55 } else { 0.35 });
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent;
    v.override_text_color = Some(t.text);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(6);
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.hovered.weak_bg_fill = t.hover;
    v.widgets.hovered.bg_fill = t.hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.active.weak_bg_fill = t.selected;
    ctx.set_visuals(v);
}
