//! What the window says about DownloadCraft's own updates (test builds only): a bar once a new
//! version is downloaded and checked, and the download's progress in the status bar. The
//! updater itself is the app's (`apps/downloadcraft/src/self_update.rs`).

use egui::{RichText, Ui};

use crate::DownloadApp;
use crate::theme::Tokens;

/// Where the updater is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdateState {
    #[default]
    None,
    /// A newer version is downloading.
    Downloading { version: String, done: u64, total: u64 },
    /// A newer version is downloaded and checked; it installs when DownloadCraft closes.
    Ready { version: String },
}

/// What the update bar asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateChoice {
    /// Close now, install, and open the new version.
    RestartNow,
}

impl DownloadApp {
    /// The bar's answer, once (the app restarts on it).
    pub fn take_update_choice(&mut self) -> Option<UpdateChoice> {
        self.update_choice.take()
    }

    pub(crate) fn update_bar_wanted(&self) -> bool {
        matches!(&self.update, UpdateState::Ready { version } if self.update_dismissed.as_ref() != Some(version))
    }

    pub(crate) fn update_bar(&mut self, ui: &mut Ui, t: &Tokens) {
        let UpdateState::Ready { version } = self.update.clone() else { return };
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("DownloadCraft {version} is ready.")).strong());
            ui.label(
                RichText::new("Restart now to install it, or it installs when you close DownloadCraft. Downloads pause and carry on after.")
                    .color(t.muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Later").clicked() {
                    self.update_dismissed = Some(version.clone());
                }
                if ui.button(RichText::new("Restart now").strong()).clicked() {
                    self.update_choice = Some(UpdateChoice::RestartNow);
                }
            });
        });
    }

    /// The download's progress, or that a restart installs it, at the end of the status bar.
    pub(crate) fn update_status(&self, ui: &mut Ui, t: &Tokens) {
        let text = match &self.update {
            UpdateState::None => return,
            UpdateState::Downloading { version, done, total } if *total > 0 => {
                format!("Downloading DownloadCraft {version}: {}%", done.saturating_mul(100) / total)
            }
            UpdateState::Downloading { version, .. } => format!("Downloading DownloadCraft {version}"),
            UpdateState::Ready { .. } => "Restart to update".into(),
        };
        ui.label(RichText::new(text).color(t.accent));
        ui.label(RichText::new("·").color(t.muted));
    }
}
