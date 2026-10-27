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

/// What Settings › About knows of checking for a newer version by hand.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdateCheck {
    /// This copy doesn't update itself, and why.
    Unavailable(String),
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available {
        version: String,
    },
    Failed(String),
}

/// What the update bar or Settings › About asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateChoice {
    /// Close now, install, and open the new version.
    RestartNow,
    /// Look for a newer version now.
    CheckNow,
    /// Download the newer version, then restart into it.
    DownloadAndInstall,
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
        // The buttons are laid out first, at the right; the words take the room left, the longer
        // line cut short with … (all of it on hover), so neither ever runs under the buttons.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tl!("Later")).clicked() {
                    self.update_dismissed = Some(version.clone());
                }
                if ui.button(RichText::new(tl!("Restart now")).strong()).clicked() {
                    self.update_choice = Some(UpdateChoice::RestartNow);
                }
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(RichText::new(tf!("DownloadCraft {version} is ready.", version = version)).strong());
                    let more = tl!("Restart now to install it, or it installs when you close DownloadCraft. Downloads pause and carry on after.");
                    ui.add(egui::Label::new(RichText::new(more).color(t.muted)).truncate()).on_hover_text(more);
                });
            });
        });
    }

    /// Settings › About's updates: check now, and download and install what's found.
    pub(crate) fn update_panel(ui: &mut Ui, t: &Tokens, state: &UpdateState, check: &UpdateCheck, choice: &mut Option<UpdateChoice>) {
        use crate::icons::Icon;
        use crate::widgets::{self, Kind};
        // Kept moving while it checks or downloads.
        if matches!(state, UpdateState::Downloading { .. }) || matches!(check, UpdateCheck::Checking) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
        let note = |ui: &mut Ui, text: String, color| {
            ui.add(egui::Label::new(RichText::new(text).size(12.5).color(color)).wrap());
        };
        match (state, check) {
            (UpdateState::Downloading { version, done, total }, _) => {
                note(ui, tf!("Downloading DownloadCraft {version}…", version = version), t.text);
                let fraction = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                ui.add(egui::ProgressBar::new(fraction).desired_width(280.0).desired_height(8.0).fill(t.accent));
                note(ui, tl!("It's checked once it's here, then DownloadCraft restarts into it.").into(), t.muted);
            }
            (UpdateState::Ready { version }, _) => {
                note(ui, tf!("DownloadCraft {version} is ready to install.", version = version), t.success);
                if widgets::button(ui, t, Kind::Primary, Some(Icon::Download), "Restart and install", true).clicked() {
                    *choice = Some(UpdateChoice::RestartNow);
                }
            }
            (_, UpdateCheck::Unavailable(why)) => note(ui, tl!(why).into(), t.muted),
            (_, UpdateCheck::Checking) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(tl!("Checking for updates…")).size(12.5).color(t.muted));
                });
            }
            (_, UpdateCheck::Available { version }) => {
                note(ui, tf!("DownloadCraft {version} is available.", version = version), t.accent);
                if widgets::button(ui, t, Kind::Primary, Some(Icon::Download), "Download and install", true).clicked() {
                    *choice = Some(UpdateChoice::DownloadAndInstall);
                }
            }
            (_, check) => {
                match check {
                    UpdateCheck::UpToDate => note(ui, tl!("DownloadCraft is up to date.").into(), t.success),
                    UpdateCheck::Failed(why) => note(ui, tf!("Couldn't check: {why}", why = why), t.danger),
                    _ => {}
                }
                let label = if matches!(check, UpdateCheck::Idle) { "Check for updates" } else { "Check again" };
                if widgets::button(ui, t, Kind::Secondary, None, label, true).clicked() {
                    *choice = Some(UpdateChoice::CheckNow);
                }
            }
        }
    }

    /// The download's progress, or that a restart installs it, at the end of the status bar.
    pub(crate) fn update_status(&self, ui: &mut Ui, t: &Tokens) {
        let text = match &self.update {
            UpdateState::None => return,
            UpdateState::Downloading { version, done, total } if *total > 0 => {
                tf!("Downloading DownloadCraft {version}: {percent}%", version = version, percent = done.saturating_mul(100) / total)
            }
            UpdateState::Downloading { version, .. } => tf!("Downloading DownloadCraft {version}", version = version),
            UpdateState::Ready { .. } => tl!("Restart to update").to_string(),
        };
        ui.label(RichText::new(text).color(t.accent));
        ui.label(RichText::new("·").color(t.muted));
    }
}
