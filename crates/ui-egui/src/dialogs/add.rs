//! The Add link dialog: links pasted one per line, each looked at as it's typed (the name its file
//! will get and its size, or a video page's title and qualities), the folder, and Add.

use std::collections::HashMap;

use downloadcraft_engine::model::probe::{Probe, Quality, ToolState};
use downloadcraft_engine::model::{SourceKind, format, names, source};
use egui::{Rect, RichText, Sense, Ui, vec2};
use serde_json::{Value, json};

use super::{folder_row, frame, header};
use crate::Services;
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::{self, Kind};

pub struct AddForm {
    pub links: String,
    pub folder: String,
    pub start: bool,
    /// The quality picked for each video link (an index into its qualities).
    quality: HashMap<String, usize>,
}

impl AddForm {
    pub fn new(text: &str, folder: &str) -> Self {
        AddForm { links: text.trim().to_string(), folder: folder.to_string(), start: true, quality: HashMap::new() }
    }
    fn links(&self) -> Vec<String> {
        self.links.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
    }
}

/// A link that can be added now, with what looking at it found.
struct Row {
    link: String,
    check: Result<SourceKind, &'static str>,
    probe: Probe,
}

impl Row {
    fn addable(&self, ytdlp_ready: bool) -> bool {
        match self.check {
            Ok(SourceKind::VideoPage) => ytdlp_ready,
            Ok(k) => k.not_yet().is_none(),
            Err(_) => false,
        }
    }
}

/// The first quality that can be had: the best, or without FFmpeg the best that needs none.
fn default_quality(qualities: &[Quality], ffmpeg: bool) -> usize {
    qualities.iter().position(|q| ffmpeg || !q.needs_ffmpeg).unwrap_or(0)
}

fn quality_text(q: &Quality) -> String {
    match q.size {
        Some(s) => format!("{}  ·  {}", q.label, format::bytes(s)),
        None => q.label.clone(),
    }
}

/// One link: its badge, the name its file will get, and what it is (or why it can't be added); a
/// video's quality picker on the right.
fn link_row(ui: &mut Ui, t: &Tokens, row: &Row, choice: &mut usize, ffmpeg: bool, ytdlp_ready: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    ui.painter().rect_filled(rect, 8.0, t.card);
    let fallback = || names::from_url(&row.link);
    let (name, chip, color) = match (&row.check, &row.probe) {
        (Err(e), _) => (row.link.chars().take(60).collect(), (*e).to_string(), t.danger),
        (Ok(k), _) if k.not_yet().is_some() => {
            (row.link.chars().take(60).collect(), format!("{} · {}", k.label(), k.not_yet().unwrap_or("")), t.warning)
        }
        (Ok(SourceKind::VideoPage), _) if !ytdlp_ready => {
            (row.link.chars().take(60).collect(), "Video page · needs yt-dlp (below)".into(), t.warning)
        }
        (Ok(_), Probe::Pending) => (fallback(), "Checking the link…".into(), t.muted),
        (Ok(_), Probe::File { name, size, note: Some(n) }) => {
            (name.clone(), format!("{}{n}", size.map(|s| format!("{} · ", format::bytes(s))).unwrap_or_default()), t.warning)
        }
        (Ok(_), Probe::File { name, size, note: None }) => {
            (name.clone(), size.map_or_else(|| "Direct link · size not given".into(), |s| format!("Direct link · {}", format::bytes(s))), t.success)
        }
        (Ok(_), Probe::Video { title, .. }) => (title.clone(), "Video".into(), t.success),
        (Ok(_), Probe::Failed(e)) => (fallback(), e.clone(), t.warning),
    };
    let video = match &row.probe {
        Probe::Video { qualities, file_name, .. } if row.addable(ytdlp_ready) => Some((qualities, file_name)),
        _ => None,
    };
    let badge = video.map_or(name.clone(), |(_, f)| f.clone());
    icons::file_badge(ui.painter(), Rect::from_min_size(rect.min + vec2(9.0, 9.0), vec2(30.0, 30.0)), &badge, t.dark);
    let picker_w = if video.is_some() { 190.0 } else { 0.0 };
    let w = rect.width() - 62.0 - picker_w;
    let g =
        egui::WidgetText::from(RichText::new(name).size(13.5).strong()).into_galley(ui, Some(egui::TextWrapMode::Truncate), w, egui::TextStyle::Body);
    ui.painter().galley(rect.min + vec2(48.0, 7.0), g, t.text);
    let c = egui::WidgetText::from(RichText::new(chip).size(12.0)).into_galley(ui, Some(egui::TextWrapMode::Truncate), w, egui::TextStyle::Body);
    ui.painter().galley(rect.min + vec2(48.0, 26.0), c, color);
    if let Some((qualities, _)) = video {
        let area = Rect::from_min_max(egui::pos2(rect.right() - picker_w, rect.top() + 8.0), egui::pos2(rect.right() - 8.0, rect.bottom() - 8.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
            let current = qualities.get(*choice).map(quality_text).unwrap_or_default();
            egui::ComboBox::from_id_salt(("quality", &row.link)).width(picker_w - 10.0).selected_text(current).show_ui(ui, |ui| {
                for (i, q) in qualities.iter().enumerate() {
                    let usable = ffmpeg || !q.needs_ffmpeg;
                    let text = if usable { quality_text(q) } else { format!("{}  ·  needs FFmpeg", q.label) };
                    if ui.add_enabled(usable, egui::Button::selectable(*choice == i, text)).clicked() {
                        *choice = i;
                    }
                }
            });
        });
    }
}

/// What video downloads need, when a video link is in the box and something's missing.
fn tools_banner(ui: &mut Ui, t: &Tokens, services: &dyn Services, wants_ffmpeg: bool) {
    let tools = services.video_tools();
    let ytdlp_missing = tools.ytdlp != ToolState::Ready;
    let ffmpeg_missing = wants_ffmpeg && tools.ffmpeg != ToolState::Ready;
    if !ytdlp_missing && !ffmpeg_missing {
        return;
    }
    egui::Frame::new().fill(t.accent_soft).corner_radius(8).inner_margin(12).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_max_width(ui.available_width());
        if ytdlp_missing {
            tool_line(
                ui,
                t,
                "Video links use yt-dlp, a free, open-source program that finds the video in a page. DownloadCraft fetches its official build and checks it before use.",
                &tools.ytdlp,
                "Get yt-dlp",
                || services.fetch_tool(false),
            );
        }
        if ffmpeg_missing {
            if services.can_fetch_ffmpeg() {
                tool_line(ui, t, "Higher qualities come as separate video and sound, which FFmpeg joins into one file.", &tools.ffmpeg, "Get FFmpeg", || {
                    services.fetch_tool(true)
                });
            } else {
                ui.label(
                    RichText::new("Higher qualities need FFmpeg to join video and sound. Install it with brew install ffmpeg (macOS) or sudo apt install ffmpeg (Linux).")
                        .size(12.5),
                );
            }
        }
    });
}

fn tool_line(ui: &mut Ui, t: &Tokens, text: &str, state: &ToolState, action: &str, fetch: impl FnOnce()) {
    // The text wraps to the dialog's width; the button sits on its own line, on the right.
    ui.add(egui::Label::new(RichText::new(text).size(12.5)).wrap());
    if let ToolState::Failed(e) = state {
        ui.add(egui::Label::new(RichText::new(e).size(12.0).color(t.danger)).wrap());
    }
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 34.0), egui::Layout::right_to_left(egui::Align::Center), |ui| match state {
        ToolState::Fetching => {
            ui.spinner();
            ui.label(RichText::new("Getting it and checking it…").size(12.5).color(t.muted));
        }
        _ => {
            let label = if matches!(state, ToolState::Failed(_)) { "Try again" } else { action };
            if widgets::button(ui, t, Kind::Primary, Some(Icon::Download), label, true).clicked() {
                fetch();
            }
        }
    });
    ui.add_space(4.0);
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
    // A row as the text box lays it out (taller than the font's own row height).
    let probe = egui::text::LayoutJob::simple("X".into(), egui::FontId::monospace(LINK_FONT), t.text, f32::INFINITY);
    let row = ui.fonts_mut(|f| f.layout_job(probe)).rows.first().map_or(18.0, |r| r.rect().height());
    let rows = links.split('\n').count().max(1);
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
                // The gutter's place; its numbers and dots are drawn once the text is laid out, at
                // the text's own rows, so they stay beside their lines however far it scrolls.
                let (gutter, _) = ui.allocate_exact_size(vec2(GUTTER, height), Sense::hover());
                let painter = ui.painter().clone();
                // The text: laid out without wrapping, so each link stays on its own line.
                let text_color = t.text;
                let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                    let job = egui::text::LayoutJob::simple(buf.as_str().to_string(), egui::FontId::monospace(LINK_FONT), text_color, f32::INFINITY);
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let shown = egui::ScrollArea::horizontal().id_salt("links-h").show(ui, |ui| {
                    egui::TextEdit::multiline(links)
                        .id(id)
                        .frame(egui::Frame::new())
                        .desired_rows(visible as usize)
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin { left: 12, right: LINK_PAD as i8, top: LINK_PAD as i8, bottom: LINK_PAD as i8 })
                        .layouter(&mut layouter)
                        .hint_text(RichText::new("https://example.com/file.zip").font(egui::FontId::monospace(LINK_FONT)).color(t.faint))
                        .show(ui)
                });
                let out = shown.inner;
                if ui.memory(|m| m.focused().is_none()) {
                    out.response.response.request_focus();
                }
                let bottom = gutter.bottom().max(out.galley_pos.y + out.galley.size().y + LINK_PAD);
                painter.vline(gutter.right() - 0.5, gutter.top()..=bottom, egui::Stroke::new(1.0, t.border));
                // With no wrapping, the galley's rows are the lines, in order.
                let lines: Vec<&str> = links.split('\n').collect();
                for (i, placed) in out.galley.rows.iter().enumerate() {
                    let y = out.galley_pos.y + placed.rect().center().y;
                    if let Some(c) = lines.get(i).and_then(|l| line_color(l, t)) {
                        painter.circle_filled(egui::pos2(gutter.left() + 11.0, y), 3.5, c);
                    }
                    let number = (i + 1).to_string();
                    painter.text(egui::pos2(gutter.right() - 9.0, y), egui::Align2::RIGHT_CENTER, number, egui::FontId::monospace(11.5), t.faint);
                }
            });
        });
    });
}

/// `None` while open; `Some(None)` cancelled; `Some(Some(list))` with one `download.add`'s
/// params per link to add (each with the name it was found to have, and a video's quality).
pub fn add(ctx: &egui::Context, f: &mut AddForm, t: &Tokens, services: &dyn Services) -> Option<Option<Vec<Value>>> {
    let mut out = None;
    let modal = egui::Modal::new(egui::Id::unique("add-dialog")).frame(frame(t)).show(ctx, |ui| {
        let width = 600.0;
        ui.set_width(width);
        if header(ui, t, "Add link", "Paste one link per line: files, or video pages such as YouTube.") {
            out = Some(None);
        }
        links_box(ui, t, &mut f.links);
        let tools = services.video_tools();
        let (ytdlp, ffmpeg) = (tools.ytdlp == ToolState::Ready, tools.ffmpeg == ToolState::Ready);
        let rows: Vec<Row> = f
            .links()
            .into_iter()
            .take(50)
            .map(|link| {
                let check = source::classify(&link);
                let looked = match check {
                    Ok(SourceKind::Http) => true,
                    Ok(SourceKind::VideoPage) => ytdlp,
                    _ => false,
                };
                let probe = if looked { services.probe(&link) } else { Probe::Pending };
                Row { link, check, probe }
            })
            .collect();
        // A video's quality starts at the best that can be had, once its qualities are known.
        for r in &rows {
            if let Probe::Video { qualities, .. } = &r.probe {
                f.quality.entry(r.link.clone()).or_insert_with(|| default_quality(qualities, ffmpeg));
            }
        }
        let has_video = rows.iter().any(|r| r.check == Ok(SourceKind::VideoPage));
        let wants_ffmpeg = rows.iter().any(|r| matches!(&r.probe, Probe::Video { qualities, .. } if qualities.iter().any(|q| q.needs_ffmpeg)));
        if !rows.is_empty() {
            ui.add_space(4.0);
            egui::ScrollArea::vertical().max_height(170.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for r in &rows {
                    let choice = f.quality.entry(r.link.clone()).or_insert(0);
                    link_row(ui, t, r, choice, ffmpeg, ytdlp);
                }
            });
        }
        if has_video {
            ui.add_space(6.0);
            tools_banner(ui, t, services, wants_ffmpeg);
        }
        ui.add_space(10.0);
        widgets::field_label(ui, t, "Save to");
        folder_row(ui, t, &mut f.folder, services, width);
        ui.add_space(4.0);
        ui.checkbox(&mut f.start, "Start downloading now");
        ui.add_space(14.0);
        let ok = rows.iter().filter(|r| r.addable(ytdlp)).count();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Ctrl+Enter adds").size(12.0).color(t.faint));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can = ok > 0 && !f.folder.trim().is_empty();
                let label = if ok > 1 { format!("Add {ok} downloads") } else { "Add download".to_string() };
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
                if widgets::button(ui, t, Kind::Primary, Some(Icon::Download), &label, can).clicked() || (can && enter) {
                    let list = rows
                        .iter()
                        .filter(|r| r.addable(ytdlp))
                        .map(|r| {
                            let mut v = json!({"url": r.link, "folder": f.folder.trim(), "paused": !f.start});
                            match &r.probe {
                                Probe::File { name, .. } => v["name"] = json!(name),
                                Probe::Video { file_name, qualities, .. } => {
                                    v["name"] = json!(file_name);
                                    if let Some(q) = f.quality.get(&r.link).and_then(|i| qualities.get(*i)) {
                                        v["quality"] = json!({"selector": q.selector, "label": q.label});
                                    }
                                }
                                _ => {}
                            }
                            v
                        })
                        .collect();
                    out = Some(Some(list));
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
