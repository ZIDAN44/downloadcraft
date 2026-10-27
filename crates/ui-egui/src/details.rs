//! The details pane under the list, for the selected torrent: General (how it's going), Trackers
//! (where peers come from, and what the trackers say about the swarm), Peers (who it's connected
//! to) and Files (each file's progress, picked or left out).

use downloadcraft_engine::model::torrent::{EngineStatus, TorrentLive, TrackerStatus};
use downloadcraft_engine::model::{Download, Prefs, State, format};
use egui::{Align, Layout, RichText, Ui, vec2};

use crate::theme::Tokens;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    General,
    Trackers,
    Peers,
    Files,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::General, Tab::Trackers, Tab::Peers, Tab::Files];
    fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Trackers => "Trackers",
            Tab::Peers => "Peers",
            Tab::Files => "Files",
        }
    }
}

/// What the pane keeps between frames.
#[derive(Default)]
pub struct Pane {
    pub tab: Tab,
    /// The Files tab's filter.
    filter: String,
}

/// What the pane asks the app to do.
pub enum Action {
    /// Download only these files of the torrent (indexes in its list).
    PickFiles(Vec<usize>),
    /// Download this file before the others (`true`), or with them.
    First(usize, bool),
    /// This torrent's own speed limits, down and up, in KiB/s.
    Limits(u32, u32),
    Copy(String),
}

pub fn show(ui: &mut Ui, t: &Tokens, pane: &mut Pane, d: &Download, prefs: &Prefs, engine: Option<&EngineStatus>, act: &mut Option<Action>) {
    // The pane keeps its height from tab to tab (each tab scrolls inside it).
    ui.set_min_height(ui.available_height());
    ui.horizontal(|ui| {
        for tab in Tab::ALL {
            let on = pane.tab == tab;
            let text = RichText::new(tl!(tab.label())).size(13.0).color(if on { t.accent } else { t.muted });
            let b = egui::Button::new(if on { text.strong() } else { text }).frame(on).fill(t.accent_soft).corner_radius(6);
            if ui.add(b).clicked() {
                pane.tab = tab;
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let name = egui::WidgetText::from(RichText::new(&d.file_name).size(12.5).color(t.faint));
            ui.add(egui::Label::new(name).truncate());
        });
    });
    ui.separator();
    let live = d.live.as_deref();
    match pane.tab {
        Tab::General => general(ui, t, d, live, prefs, act),
        Tab::Trackers => trackers(ui, t, live, engine),
        Tab::Peers => peers(ui, t, live),
        Tab::Files => files(ui, t, pane, d, live, act),
    }
}

fn not_running(ui: &mut Ui, t: &Tokens) {
    ui.add_space(12.0);
    ui.label(RichText::new(tl!("These show while it's downloading or sharing.")).size(12.5).color(t.muted));
}

/// A label and its value, in a grid row; a long value wraps in the column.
fn pair(ui: &mut Ui, t: &Tokens, label: &str, value: String) {
    ui.label(RichText::new(tl!(label)).size(12.5).color(t.muted));
    ui.add(egui::Label::new(RichText::new(value).size(12.5)).wrap());
    ui.end_row();
}

fn general(ui: &mut Ui, t: &Tokens, d: &Download, live: Option<&TorrentLive>, prefs: &Prefs, act: &mut Option<Action>) {
    let Some(l) = live else { return not_running(ui, t) };
    let ratio = if l.size > 0 { d.sent as f64 / l.size as f64 } else { 0.0 };
    let active = if l.seeding_secs > 0 {
        tf!("{time} (sharing for {sharing})", time = format::duration(l.active_secs), sharing = format::duration(l.seeding_secs))
    } else {
        format::duration(l.active_secs)
    };
    // The limit that holds: this torrent's own or the overall one, whichever is lower.
    let limit = |own: u32, overall: u32| match (own, overall) {
        (0, 0) => tl!("none").to_string(),
        (0, k) | (k, 0) => format::speed(u64::from(k) * 1024),
        (a, b) => format::speed(u64::from(a.min(b)) * 1024),
    };
    let (own_down, own_up) = d.torrent.as_ref().map_or((0, 0), |c| (c.down_kib, c.up_kib));
    let connections = if l.peer_limit == 0 { l.connected.to_string() } else { tf!("{n} (up to {max})", n = l.connected, max = l.peer_limit) };
    let swarm = l.trackers.iter().filter(|x| x.status == TrackerStatus::Working).fold(None, |best: Option<(u32, u32)>, x| {
        let here = (x.seeds.unwrap_or(0), x.leechers.unwrap_or(0));
        Some(best.map_or(here, |b| if here.0 + here.1 > b.0 + b.1 { here } else { b }))
    });
    let swarm =
        swarm.map_or_else(|| tl!("not known yet").to_string(), |(s, p)| tf!("{seeds} have it all · {peers} downloading", seeds = s, peers = p));
    let eta = match (d.state, d.eta()) {
        (State::Downloading, Some(e)) => format::duration(e),
        _ => "—".into(),
    };
    let grid = |ui: &mut Ui, id: &str, rows: &mut dyn FnMut(&mut Ui)| {
        egui::Grid::new(id).num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| rows(ui));
    };
    egui::ScrollArea::vertical().id_salt("details-general").auto_shrink([false, false]).show(ui, |ui| {
        // Three columns of equal width, each a label and its value.
        ui.columns(3, |cols| {
            if let [a, b, c] = cols {
                grid(a, "details-general-1", &mut |ui| {
                    pair(ui, t, "Time active", active.clone());
                    pair(ui, t, "Downloaded", tf!("{have} of {size}", have = format::bytes(l.have), size = format::bytes(l.size)));
                    pair(ui, t, "This run", tf!("{down} received · {up} sent", down = format::bytes(l.downloaded), up = format::bytes(l.uploaded)));
                    pair(ui, t, "Download speed", format::speed(l.down_speed));
                    pair(ui, t, "Download limit", limit(own_down, prefs.torrent.down_kib));
                });
                grid(b, "details-general-2", &mut |ui| {
                    pair(ui, t, "Time left", eta.clone());
                    pair(ui, t, "Sent", format::bytes(d.sent));
                    pair(ui, t, "Share ratio", format!("{ratio:.2}"));
                    pair(ui, t, "Upload speed", format::speed(l.up_speed));
                    pair(ui, t, "Upload limit", limit(own_up, prefs.torrent.up_kib));
                });
                grid(c, "details-general-3", &mut |ui| {
                    pair(ui, t, "Connections", connections.clone());
                    pair(ui, t, "Peers", tf!("{connecting} connecting · {known} known", connecting = l.connecting, known = l.known));
                    pair(ui, t, "Swarm", swarm.clone());
                    pair(ui, t, "Wasted", l.wasted.map_or_else(|| "—".into(), format::bytes));
                    pair(ui, t, "Pieces", tf!("{n} × {size}", n = l.pieces, size = format::bytes(u64::from(l.piece_size))));
                });
            }
        });
        ui.add_space(6.0);
        if let Some(choice) = &d.torrent {
            let (mut down, mut up) = (choice.down_kib, choice.up_kib);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tl!("This torrent's limits (KiB/s, 0: none)")).size(12.5).color(t.muted));
                ui.label(RichText::new(tl!("Download")).size(12.5));
                let a = ui.add(egui::DragValue::new(&mut down).range(0..=4_000_000).speed(16));
                ui.label(RichText::new(tl!("Upload")).size(12.5));
                let b = ui.add(egui::DragValue::new(&mut up).range(0..=4_000_000).speed(16));
                if a.changed() || b.changed() {
                    *act = Some(Action::Limits(down, up));
                }
                ui.label(RichText::new(tl!("from its next start")).size(12.0).color(t.faint));
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tl!("Info hash")).size(12.0).color(t.muted));
            if ui.add(egui::Button::new(RichText::new(&l.info_hash).monospace().size(11.5)).frame(false)).on_hover_text(tl!("Copy")).clicked() {
                *act = Some(Action::Copy(l.info_hash.clone()));
            }
        });
        ui.add(egui::Label::new(RichText::new(tf!("Saved in {folder}", folder = d.folder)).size(12.0).color(t.faint)).wrap());
    });
}

/// A grid with a header row, scrolling when it's taller than the pane.
fn table(ui: &mut Ui, t: &Tokens, id: &str, heads: &[&str], body: impl FnOnce(&mut Ui)) {
    egui::ScrollArea::both().id_salt(id).auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new(id).num_columns(heads.len()).striped(true).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
            for h in heads {
                ui.label(RichText::new(tl!(h)).size(12.0).strong().color(t.muted));
            }
            ui.end_row();
            body(ui);
        });
    });
}

fn cell(ui: &mut Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).size(12.5));
}

fn count(n: Option<u32>) -> String {
    n.map_or_else(|| "—".into(), |n| n.to_string())
}

/// The widths of the Trackers tab's first two columns: an address and a reason both fit.
const SOURCE_W: f32 = 250.0;
const STATUS_W: f32 = 270.0;

/// A label in a cell `width` wide, left-aligned, cut short with … when longer (all of it on hover).
fn fixed(ui: &mut Ui, width: f32, text: RichText) -> egui::Response {
    let hover = text.text().to_string();
    ui.allocate_ui_with_layout(vec2(width, 18.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_width(width);
        ui.add(egui::Label::new(text).truncate())
    })
    .inner
    .on_hover_text(hover)
}

fn trackers(ui: &mut Ui, t: &Tokens, live: Option<&TorrentLive>, engine: Option<&EngineStatus>) {
    let Some(l) = live else { return not_running(ui, t) };
    let on = |yes: bool| if yes { tl!("On") } else { tl!("Off") };
    table(ui, t, "details-trackers", &["Source", "Status", "With all of it", "Downloading", "Finished"], |ui| {
        let dht = engine.and_then(|e| e.dht_nodes);
        let rows = [
            (tl!("DHT (peers found without a tracker)"), dht.map_or_else(|| on(false).to_string(), |n| tf!("On · {n} nodes", n = n))),
            (tl!("Peer exchange (peers tell each other)"), on(true).to_string()),
            (tl!("Local peers (on this network)"), on(true).to_string()),
        ];
        for (name, status) in rows {
            ui.label(RichText::new(name).size(12.5).color(t.muted));
            cell(ui, status);
            cell(ui, "");
            cell(ui, "");
            cell(ui, "");
            ui.end_row();
        }
        for x in &l.web_seeds {
            fixed(ui, SOURCE_W, RichText::new(tf!("Web seed: {url}", url = x.url)).size(12.5));
            let (text, color) = match &x.status {
                TrackerStatus::Waiting => (tl!("Not asked yet").to_string(), t.muted),
                TrackerStatus::Working => (tl!("Answering").to_string(), t.success),
                TrackerStatus::Failing(why) => (tf!("Not answering: {why}", why = why), t.danger),
                TrackerStatus::Unsupported => (tl!("Can't be asked about the swarm").to_string(), t.faint),
                TrackerStatus::Skipped(why) => (tl!(why).to_string(), t.faint),
            };
            fixed(ui, STATUS_W, RichText::new(&text).size(12.5).color(color));
            cell(ui, "");
            cell(ui, "");
            cell(ui, x.completed.map_or_else(String::new, |n| tf!("{n} files", n = n)));
            ui.end_row();
        }
        for x in &l.trackers {
            fixed(ui, SOURCE_W, RichText::new(&x.url).size(12.5));
            let (text, color) = match &x.status {
                TrackerStatus::Waiting => (tl!("Not asked yet").to_string(), t.muted),
                TrackerStatus::Working => (tl!("Answering").to_string(), t.success),
                TrackerStatus::Failing(why) => (tf!("Not answering: {why}", why = why), t.danger),
                TrackerStatus::Unsupported => (tl!("Can't be asked about the swarm").to_string(), t.faint),
                TrackerStatus::Skipped(why) => (tl!(why).to_string(), t.faint),
            };
            fixed(ui, STATUS_W, RichText::new(&text).size(12.5).color(color));
            cell(ui, count(x.seeds));
            cell(ui, count(x.leechers));
            cell(ui, count(x.completed));
            ui.end_row();
        }
    });
}

fn peers(ui: &mut Ui, t: &Tokens, live: Option<&TorrentLive>) {
    let Some(l) = live else { return not_running(ui, t) };
    if l.peers.is_empty() {
        ui.add_space(8.0);
        ui.label(
            RichText::new(tf!("No one connected yet · {connecting} connecting · {known} known", connecting = l.connecting, known = l.known))
                .size(12.5)
                .color(t.muted),
        );
        return;
    }
    table(ui, t, "details-peers", &["Address", "Connection", "Program", "Download speed", "Upload speed", "Received", "Sent"], |ui| {
        for p in &l.peers {
            cell(ui, p.address.clone());
            cell(ui, p.connection.clone());
            cell(ui, if p.client.is_empty() { "—".to_string() } else { p.client.clone() });
            cell(ui, format::speed(p.down_speed));
            cell(ui, format::speed(p.up_speed));
            cell(ui, format::bytes(p.downloaded));
            cell(ui, format::bytes(p.uploaded));
            ui.end_row();
        }
    });
}

fn files(ui: &mut Ui, t: &Tokens, pane: &mut Pane, d: &Download, live: Option<&TorrentLive>, act: &mut Option<Action>) {
    let Some(choice) = &d.torrent else { return };
    if choice.list.is_empty() {
        return not_running(ui, t);
    }
    let all: Vec<usize> = choice.list.iter().map(|f| f.index).collect();
    let picked = |i: usize| choice.files.is_empty() || choice.files.contains(&i);
    ui.horizontal(|ui| {
        crate::widgets::text_field(ui, &mut pane.filter, "Filter files", 220.0);
        if ui.small_button(tl!("Pick all")).clicked() {
            *act = Some(Action::PickFiles(all.clone()));
        }
        let n = all.iter().filter(|i| picked(**i)).count();
        ui.label(RichText::new(tf!("{picked} of {n} files picked", picked = n, n = all.len())).size(12.5).color(t.muted));
    });
    let filter = pane.filter.trim().to_lowercase();
    let shown: Vec<_> = choice.list.iter().filter(|f| filter.is_empty() || f.path.to_lowercase().contains(&filter)).collect();
    let row_h = 22.0;
    // Compact rows: the window's controls are taller than a list of files wants.
    ui.spacing_mut().item_spacing.y = 2.0;
    ui.spacing_mut().interact_size.y = 18.0;
    egui::ScrollArea::vertical().id_salt("details-files").auto_shrink([false, false]).show_rows(ui, row_h, shown.len(), |ui, range| {
        for f in shown.get(range).unwrap_or_default() {
            ui.horizontal(|ui| {
                ui.set_height(row_h - 4.0);
                let mut on = picked(f.index);
                if ui.checkbox(&mut on, "").changed() {
                    let mut next: Vec<usize> = all.iter().copied().filter(|i| if *i == f.index { on } else { picked(*i) }).collect();
                    next.sort_unstable();
                    if !next.is_empty() {
                        *act = Some(Action::PickFiles(next));
                    }
                }
                let first = choice.first.contains(&f.index);
                let star =
                    egui::Button::new(RichText::new(tl!("First")).size(11.5).color(if first { t.accent } else { t.faint })).frame(first).small();
                if ui.add_enabled(on, star).on_hover_text(tl!("Download this file before the others")).clicked() {
                    *act = Some(Action::First(f.index, !first));
                }
                let done = live.and_then(|l| l.file_done.get(f.index)).copied();
                let fraction = done.map(|x| if f.size == 0 { 1.0 } else { (x as f64 / f.size as f64) as f32 });
                let bar = egui::ProgressBar::new(fraction.unwrap_or(0.0)).desired_width(110.0).desired_height(8.0).fill(if fraction == Some(1.0) {
                    t.success
                } else {
                    t.accent
                });
                ui.add(bar);
                let pct = fraction.map_or_else(|| "—".to_string(), |x| format!("{:.0}%", x * 100.0));
                ui.add_sized(vec2(40.0, row_h - 4.0), egui::Label::new(RichText::new(pct).size(12.0).color(t.muted)));
                ui.add_sized(vec2(72.0, row_h - 4.0), egui::Label::new(RichText::new(format::bytes(f.size)).size(12.0).color(t.muted)));
                let color = if on { t.text } else { t.faint };
                ui.add(egui::Label::new(RichText::new(&f.path).size(12.5).color(color)).truncate()).on_hover_text(&f.path);
            });
        }
    });
}
