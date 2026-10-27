//! Remove's question, as torrent clients ask it: take the download off the list, and, with the box
//! ticked, the file too (to the Recycle Bin or Trash, so it can be got back). The box remembers how
//! it was last left.

use egui::RichText;

use super::{frame, header};
use crate::theme::Tokens;
use crate::widgets::{self, Kind};

pub struct RemoveForm {
    pub id: downloadcraft_engine::DownloadId,
    pub name: String,
    /// The file is whole (and there, as far as the app knows): the box offers to delete it.
    pub finished: bool,
    pub delete_file: bool,
}

/// `None` while open; `Some(None)` cancelled; `Some(Some(delete_file))` to remove.
pub fn remove(ctx: &egui::Context, f: &mut RemoveForm, t: &Tokens) -> Option<Option<bool>> {
    let mut out = None;
    let modal = egui::Modal::new(egui::Id::unique("remove-dialog")).frame(frame(t)).show(ctx, |ui| {
        ui.set_width(440.0);
        if header(ui, t, "Remove download?", "It comes off the list.") {
            out = Some(None);
        }
        egui::Frame::new().fill(t.card).corner_radius(8).inner_margin(12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(&f.name).strong());
        });
        ui.add_space(8.0);
        if f.finished {
            let text = if cfg!(windows) {
                tl!("Also delete the file (it goes to the Recycle Bin)")
            } else {
                tl!("Also delete the file (it goes to the Trash)")
            };
            ui.checkbox(&mut f.delete_file, text);
        } else {
            ui.label(RichText::new(tl!("The part downloaded so far is deleted.")).size(13.0).color(t.muted));
        }
        ui.add_space(14.0);
        let (enter, esc) = ui.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if f.finished && f.delete_file { "Remove and delete" } else { "Remove" };
            if widgets::button(ui, t, Kind::Danger, None, label, true).clicked() || enter {
                out = Some(Some(f.finished && f.delete_file));
            }
            if widgets::button(ui, t, Kind::Secondary, None, "Cancel", true).clicked() || esc {
                out = Some(None);
            }
        });
    });
    if modal.should_close() && out.is_none() {
        out = Some(None);
    }
    out
}
