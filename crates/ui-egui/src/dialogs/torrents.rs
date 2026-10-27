//! Settings › Torrents (sharing and speed limits) and Settings › Connection (peers per torrent, the
//! port and how peers connect, port forwarding, a proxy and an IP filter).

use downloadcraft_engine::model::MAX_SEED_RATIO;
use downloadcraft_engine::model::torrent::{MAX_PEER_LIMIT, Protocol, TorrentPrefs, proxy_url};
use egui::{RichText, Ui};

use super::setting;
use crate::Services;
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets::{self, Kind};

/// A port for incoming connections, picked at random from those no common program claims.
fn random_port(ui: &Ui) -> u16 {
    let seed = ui.input(|i| (i.time * 1_000_003.0) as u64) ^ u64::from(std::process::id());
    let mixed = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32;
    20_000 + (mixed % 40_000) as u16
}

/// Settings › Torrents: sharing, and speed limits over all torrents.
pub(super) fn sharing(ui: &mut Ui, t: &Tokens, seed_ratio: &mut f32, p: &mut TorrentPrefs) {
    let ratio = egui::DragValue::new(seed_ratio).range(0.0..=MAX_SEED_RATIO).speed(0.05).fixed_decimals(1).suffix(" ×");
    setting(ui, t, "Share finished torrents", "Until each has sent this many times its size. 0: not at all.", 120.0, |ui| {
        ui.add(ratio);
    });
    ui.add_space(4.0);
    let (down, up) = (&mut p.down_kib, &mut p.up_kib);
    setting(ui, t, "Speed limits", "Download and upload over all torrents, in KiB/s. 0: no limit.", 200.0, |ui| {
        // Laid out right to left: Download, its value, Upload, its value.
        ui.add(egui::DragValue::new(up).range(0..=4_000_000).speed(16));
        ui.label(RichText::new(tl!("Upload")).size(12.5));
        ui.add(egui::DragValue::new(down).range(0..=4_000_000).speed(16));
        ui.label(RichText::new(tl!("Download")).size(12.5));
    });
}

/// Settings › Connection: how peers reach this computer and how it reaches them.
pub(super) fn connection(ui: &mut Ui, t: &Tokens, p: &mut TorrentPrefs, services: &dyn Services, width: f32) {
    setting(ui, t, "Peers per torrent", "The most one torrent connects to at once. 0: no limit.", 120.0, |ui| {
        ui.add(egui::DragValue::new(&mut p.peer_limit).range(0..=MAX_PEER_LIMIT).speed(4));
    });
    ui.add_space(4.0);
    let port = &mut p.port;
    setting(ui, t, "Port for incoming connections", "Others reach this computer on it. 0: a new one each start.", 200.0, |ui| {
        if widgets::button(ui, t, Kind::Secondary, None, "Random", true).clicked() {
            *port = random_port(ui);
        }
        ui.add(egui::DragValue::new(port).range(0..=65535).speed(8));
    });
    ui.add_space(4.0);
    let protocol = &mut p.protocol;
    setting(ui, t, "Connect to peers over", "µTP is easier on a busy network; through a proxy only TCP is used.", 172.0, |ui| {
        let name = |pr: Protocol| if pr == Protocol::Both { tl!("TCP and µTP") } else { tl!("TCP") };
        egui::ComboBox::from_id_salt("torrent-protocol").width(170.0).selected_text(name(*protocol)).show_ui(ui, |ui| {
            for pr in [Protocol::Tcp, Protocol::Both] {
                ui.selectable_value(protocol, pr, name(pr));
            }
        });
    });
    ui.add_space(4.0);
    ui.checkbox(&mut p.upnp, tl!("Forward the port on the router (NAT-PMP or UPnP)"));
    ui.add_space(6.0);
    widgets::field_label(ui, t, "SOCKS5 proxy for peers and web trackers");
    ui.horizontal(|ui| {
        widgets::text_field(ui, &mut p.proxy, "host:port, or empty for none", width - 4.0);
    });
    if let Err(e) = proxy_url(&p.proxy) {
        ui.label(RichText::new(tl!(e)).size(12.0).color(t.danger));
    } else if !p.proxy.trim().is_empty() {
        ui.add(
            egui::Label::new(
                RichText::new(tl!("The DHT, UDP trackers and .torrent links still connect directly, and web trackers' names are looked up directly; web seeds aren't used."))
                    .size(12.0)
                    .color(t.muted),
            )
            .wrap(),
        );
    }
    ui.add_space(6.0);
    widgets::field_label(ui, t, "IP filter");
    ui.horizontal(|ui| {
        widgets::text_field(ui, &mut p.ip_filter, "A .dat or .p2p file, or empty for none", width - 110.0);
        if widgets::button(ui, t, Kind::Secondary, Some(Icon::Folder), "Browse", true).clicked()
            && let Some(f) = services.pick_ip_filter()
        {
            p.ip_filter = f;
        }
    });
    ui.add_space(4.0);
    ui.add(
        egui::Label::new(RichText::new(tl!("The port, peers, proxy and IP filter apply once no torrent is running.")).size(12.0).color(t.muted))
            .wrap(),
    );
}
