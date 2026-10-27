//! Toolbar, sidebar and list icons, drawn in code (no icon files, nobody else's artwork).

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
    Search,
    Close,
    /// The app's mark: an arrow onto a tray.
    Download,
    List,
    Check,
    Alert,
    Sun,
    Moon,
    /// A screen: follow the system.
    Monitor,
}

/// Draw `icon` filling `r` (a square) in `color`.
pub fn paint(p: &Painter, r: Rect, icon: Icon, color: Color32) {
    let s = r.width().min(r.height());
    let c = r.center();
    let u = s / 16.0;
    let stroke = Stroke::new((1.6 * u).max(1.2), color);
    let at = |x: f32, y: f32| pos2(c.x + (x - 8.0) * u, c.y + (y - 8.0) * u);
    let line = |pts: &[(f32, f32)]| Shape::line(pts.iter().map(|(x, y)| at(*x, *y)).collect(), stroke);
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
            p.line_segment([at(3.0, 4.5), at(13.0, 4.5)], stroke);
            p.line_segment([at(6.5, 2.6), at(9.5, 2.6)], stroke);
            p.add(line(&[(4.4, 4.8), (5.4, 13.5), (10.6, 13.5), (11.6, 4.8)]));
        }
        Icon::Clear | Icon::Check => {
            p.add(line(&[(3.0, 8.5), (6.5, 12.0), (13.0, 4.5)]));
        }
        Icon::Folder => {
            let pts = vec![at(2.0, 4.0), at(6.5, 4.0), at(8.0, 5.5), at(14.0, 5.5), at(14.0, 12.5), at(2.0, 12.5)];
            p.add(Shape::closed_line(pts, stroke));
        }
        Icon::Settings => {
            p.circle_stroke(c, 2.4 * u, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * 4.6 * u, c + d * 6.6 * u], stroke);
            }
        }
        Icon::Search => {
            p.circle_stroke(at(7.0, 7.0), 4.2 * u, stroke);
            p.line_segment([at(10.2, 10.2), at(13.5, 13.5)], stroke);
        }
        Icon::Close => {
            p.line_segment([at(4.0, 4.0), at(12.0, 12.0)], stroke);
            p.line_segment([at(12.0, 4.0), at(4.0, 12.0)], stroke);
        }
        Icon::Download => {
            p.line_segment([at(8.0, 2.5), at(8.0, 10.0)], stroke);
            p.add(line(&[(4.8, 7.2), (8.0, 10.4), (11.2, 7.2)]));
            p.line_segment([at(3.5, 13.3), at(12.5, 13.3)], stroke);
        }
        Icon::List => {
            for y in [4.0, 8.0, 12.0] {
                p.circle_filled(at(3.2, y), 1.0 * u, color);
                p.line_segment([at(5.8, y), at(13.5, y)], stroke);
            }
        }
        Icon::Alert => {
            p.circle_stroke(c, 6.0 * u, stroke);
            p.line_segment([at(8.0, 4.6), at(8.0, 8.8)], stroke);
            p.circle_filled(at(8.0, 11.3), 1.0 * u, color);
        }
        Icon::Sun => {
            p.circle_stroke(c, 3.0 * u, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * 5.0 * u, c + d * 6.8 * u], stroke);
            }
        }
        Icon::Moon => {
            p.add(Shape::closed_line(crescent(c, 6.0 * u), stroke));
        }
        Icon::Monitor => {
            p.rect_stroke(Rect::from_min_max(at(2.0, 3.0), at(14.0, 11.0)), 1.5, stroke, egui::StrokeKind::Middle);
            p.line_segment([at(8.0, 11.0), at(8.0, 13.5)], stroke);
            p.line_segment([at(5.0, 13.5), at(11.0, 13.5)], stroke);
        }
    }
}

/// The outline of a crescent moon of radius `r` at `c`: the arc of the disc left showing when a
/// second disc, moved up and right, covers it, then that second disc's arc back.
fn crescent(c: Pos2, r: f32) -> Vec<Pos2> {
    let inner_c = c + vec2(0.42 * r, -0.42 * r);
    let inner_r = 0.78 * r;
    // Both arcs face down and left (135° with y down).
    let facing = 0.75 * std::f32::consts::PI;
    let point = |center: Pos2, radius: f32, a: f32| center + vec2(a.cos(), a.sin()) * radius;
    // How far either way from `facing` an arc runs before it meets the other disc.
    let reach = |center: Pos2, radius: f32, inside_other: &dyn Fn(Pos2) -> bool| {
        let mut half = 0.0f32;
        while half < std::f32::consts::PI && !inside_other(point(center, radius, facing + half + 0.02)) {
            half += 0.02;
        }
        half
    };
    let outer_half = reach(c, r, &|q| q.distance(inner_c) < inner_r);
    let inner_half = reach(inner_c, inner_r, &|q| q.distance(c) > r);
    let steps = 20;
    let mut pts: Vec<Pos2> = (0..=steps).map(|i| point(c, r, facing - outer_half + 2.0 * outer_half * i as f32 / steps as f32)).collect();
    pts.extend((0..=steps).rev().map(|i| point(inner_c, inner_r, facing - inner_half + 2.0 * inner_half * i as f32 / steps as f32)));
    pts
}

fn play(p: &Painter, a: Pos2, b: Pos2, c: Pos2, color: Color32) {
    p.add(Shape::convex_polygon(vec![a, b, c], color, Stroke::NONE));
}

/// The coloured badge before a file name: its extension's group, as a short label on a tile.
pub fn file_badge(p: &Painter, r: Rect, name: &str, dark: bool) {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let rgb = match ext.as_str() {
        "zip" | "rar" | "7z" | "tar" | "gz" | "xz" | "bz2" | "zst" => (0xC2, 0x74, 0x00),
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "m4v" => (0xD0, 0x31, 0x31),
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "opus" => (0x8E, 0x24, 0xAA),
        "exe" | "msi" | "dmg" | "pkg" | "deb" | "rpm" | "appimage" => (0x2E, 0x7D, 0x32),
        "iso" | "img" => (0x45, 0x5A, 0x64),
        "pdf" | "doc" | "docx" | "txt" | "epub" | "odt" => (0x15, 0x65, 0xC0),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" => (0x00, 0x83, 0x8F),
        _ => (0x60, 0x6B, 0x78),
    };
    let fill = Color32::from_rgb(rgb.0, rgb.1, rgb.2);
    let label: String = if ext.is_empty() || ext.len() > 4 { "FILE".into() } else { ext.to_ascii_uppercase() };
    p.rect_filled(r, 8.0, if dark { fill.gamma_multiply(0.85) } else { fill });
    let size = if label.len() >= 4 { r.height() * 0.26 } else { r.height() * 0.32 };
    p.text(r.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(size), Color32::WHITE);
}
