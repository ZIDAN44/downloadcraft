//! Toolbar and list icons, drawn in code (no icon files, nobody else's artwork).

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Add,
    Resume,
    Pause,
    Remove,
    ResumeAll,
    PauseAll,
    Clear,
    Folder,
    Settings,
}

/// Draw `icon` filling `r` (a square) in `color`.
pub fn paint(p: &Painter, r: Rect, icon: Icon, color: Color32) {
    let s = r.width().min(r.height());
    let c = r.center();
    let u = s / 16.0;
    let stroke = Stroke::new((1.6 * u).max(1.2), color);
    let at = |x: f32, y: f32| pos2(c.x + (x - 8.0) * u, c.y + (y - 8.0) * u);
    match icon {
        Icon::Add => {
            p.line_segment([at(8.0, 2.5), at(8.0, 13.5)], stroke);
            p.line_segment([at(2.5, 8.0), at(13.5, 8.0)], stroke);
        }
        Icon::Resume => play(p, at(5.0, 3.0), at(13.0, 8.0), at(5.0, 13.0), color),
        Icon::Pause => {
            p.rect_filled(Rect::from_min_max(at(4.0, 3.0), at(6.8, 13.0)), 1.0, color);
            p.rect_filled(Rect::from_min_max(at(9.2, 3.0), at(12.0, 13.0)), 1.0, color);
        }
        Icon::ResumeAll => {
            play(p, at(2.0, 3.5), at(8.5, 8.0), at(2.0, 12.5), color);
            play(p, at(8.0, 3.5), at(14.5, 8.0), at(8.0, 12.5), color);
        }
        Icon::PauseAll => {
            for x in [2.0, 5.5, 9.0, 12.5] {
                p.rect_filled(Rect::from_min_max(at(x, 3.5), at(x + 2.0, 12.5)), 0.8, color);
            }
        }
        Icon::Remove => {
            p.line_segment([at(3.5, 4.5), at(12.5, 4.5)], stroke);
            p.line_segment([at(6.5, 2.8), at(9.5, 2.8)], stroke);
            let body = [at(4.6, 4.8), at(5.6, 13.5), at(10.4, 13.5), at(11.4, 4.8)];
            p.add(Shape::line(body.to_vec(), stroke));
        }
        Icon::Clear => {
            p.line_segment([at(2.5, 8.5), at(6.0, 12.0)], stroke);
            p.line_segment([at(6.0, 12.0), at(13.5, 4.0)], stroke);
        }
        Icon::Folder => {
            let pts = vec![at(2.0, 4.0), at(6.5, 4.0), at(8.0, 5.5), at(14.0, 5.5), at(14.0, 12.5), at(2.0, 12.5)];
            p.add(Shape::closed_line(pts, stroke));
        }
        Icon::Settings => {
            p.circle_stroke(c, 2.6 * u, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * 4.6 * u, c + d * 6.6 * u], stroke);
            }
        }
    }
}

fn play(p: &Painter, a: Pos2, b: Pos2, c: Pos2, color: Color32) {
    p.add(Shape::convex_polygon(vec![a, b, c], color, Stroke::NONE));
}

/// The coloured badge before a file name: its extension's group, as a letter on a tile.
pub fn file_badge(p: &Painter, r: Rect, name: &str, dark: bool) {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let (letter, rgb) = match ext.as_str() {
        "zip" | "rar" | "7z" | "tar" | "gz" | "xz" | "bz2" | "zst" => ("Z", (0xB2, 0x6B, 0x00)),
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "m4v" => ("V", (0xC6, 0x28, 0x28)),
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "opus" => ("A", (0x8E, 0x24, 0xAA)),
        "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" => ("P", (0x2E, 0x7D, 0x32)),
        "iso" | "img" => ("D", (0x45, 0x5A, 0x64)),
        "pdf" | "doc" | "docx" | "txt" | "epub" | "odt" => ("T", (0x15, 0x65, 0xC0)),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" => ("I", (0x00, 0x83, 0x8F)),
        _ => ("F", (0x60, 0x6B, 0x78)),
    };
    let fill = Color32::from_rgb(rgb.0, rgb.1, rgb.2);
    p.rect_filled(r, 6.0, if dark { fill.gamma_multiply(0.8) } else { fill });
    p.text(r.center(), egui::Align2::CENTER_CENTER, letter, egui::FontId::proportional(r.height() * 0.5), Color32::WHITE);
}
