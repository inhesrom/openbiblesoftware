//! The Update check's notice and Update now (#276, ADR 0017). The engine checks, downloads,
//! verifies and installs ([`obs_engine::updates`]); this draws where it is:
//!
//! - The launch Update check starts [`LAUNCH_CHECK_DELAY_SECS`] after the first frame, once
//!   the app armed it ([`super::Shell::arm_launch_update_check`]), so it never delays the
//!   first window; the check itself runs on the job queue.
//! - The update notice is a window at the bottom right that does not block the app: the new
//!   version, its release notes, Update now / Later / Skip this version; then the download's
//!   progress with Cancel; then Restart now. A failure says what went wrong in plain words
//!   and offers Open the download page.
//! - The first launch that checks automatically says so once, with Turn off.
//!
//! Restart now saves any Note being written and the Last session, then starts the new
//! executable ([`UpdateUi::relaunch`], replaced in tests) and closes this window, so the new
//! app never reads them before they are written.

use std::path::Path;
use std::sync::Arc;

use egui::{RichText, vec2};
use obs_engine::updates::{
    LAUNCH_CHECK_DELAY_SECS, NotesBlock, Release, UpdateError, UpdateStatus, Verification,
};
use serde_json::json;

use super::State;
use crate::theme;

/// Starts the updated app (its executable) as a new process.
pub(crate) type Relauncher = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// The first-run note about automatic Update checks.
pub(crate) const FIRST_RUN_NOTE: &str = "Open Bible Software asks GitHub for its newest \
     version a few seconds after it starts. Only the release list is requested; nothing about \
     you or your data is sent.";

/// What the update notice says about checking a download before it is installed, before any
/// download (no release key is compiled in yet, ADR 0017).
pub(crate) const CHECKSUM_NOTE: &str = "The download is checked against the release's SHA-256 \
     checksums (SHA256SUMS) before anything is replaced. Release signing is not set up yet.";

/// When the launch Update check is due.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LaunchCheck {
    /// Not armed (tests, the bench): no automatic check.
    Off,
    /// Armed; the first frame starts the clock.
    Armed,
    /// The first frame was drawn at this time.
    Waiting(f64),
    Done,
}

/// The update notice's state across frames.
pub(crate) struct UpdateUi {
    pub(crate) relaunch: Relauncher,
    pub(crate) launch_check: LaunchCheck,
    /// Restart now started the new app: close this window.
    close: bool,
    /// Why Update now cannot replace this copy, probed once per release on offer (the probe
    /// writes a file in the install folder), keyed by its tag.
    cannot: Option<(String, Option<UpdateError>)>,
}

impl Default for UpdateUi {
    fn default() -> Self {
        Self {
            relaunch: Arc::new(system_relaunch),
            launch_check: LaunchCheck::Off,
            close: false,
            cannot: None,
        }
    }
}

/// The smoke-check automation settings (`App::with_automation_from_env`): they drive this
/// process only, so the restarted app never replays them.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const AUTOMATION_VARS: [&str; 5] = [
    "OBS_SCRIPT",
    "OBS_INPUT",
    "OBS_SHOT",
    "OBS_SHOT_FRAME",
    "OBS_STEPS",
];
/// `OBS_RELAUNCH_<NAME>` becomes `OBS_<NAME>` in the restarted app: the update e2e's
/// automation for the new version.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const RELAUNCH_PREFIX: &str = "OBS_RELAUNCH_";

/// The restarted app's environment, from this one's `vars`: the variables to remove and the
/// ones to set.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn relaunch_env(
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> (Vec<std::ffi::OsString>, Vec<(String, std::ffi::OsString)>) {
    let mut remove: Vec<std::ffi::OsString> = AUTOMATION_VARS.iter().map(Into::into).collect();
    let mut set = Vec::new();
    for (name, value) in vars {
        if let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(RELAUNCH_PREFIX)) {
            set.push((format!("OBS_{rest}"), value));
            remove.push(name);
        }
    }
    (remove, set)
}

/// Start `exe` with this process's arguments, without its automation settings.
#[cfg(not(target_arch = "wasm32"))]
fn system_relaunch(exe: &Path) -> Result<(), String> {
    let (remove, set) = relaunch_env(std::env::vars_os());
    let mut command = std::process::Command::new(exe);
    command.args(std::env::args_os().skip(1));
    for name in remove {
        command.env_remove(name);
    }
    command
        .envs(set)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
fn system_relaunch(_: &Path) -> Result<(), String> {
    Err("the app cannot restart itself here".into())
}

/// A click in the notice, done after it is drawn.
enum Act {
    Run(&'static str),
    OpenPage,
    TurnOff,
}

/// `bytes` in MB, one decimal.
fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// Release notes, block by block.
fn notes_ui(ui: &mut egui::Ui, blocks: &[NotesBlock]) {
    for block in blocks {
        match block {
            NotesBlock::Heading(t) => {
                ui.label(RichText::new(t).strong().color(theme::c().ink));
            }
            NotesBlock::Paragraph(t) => {
                ui.label(t.as_str());
            }
            NotesBlock::Item(t) => {
                ui.label(format!("· {t}"));
            }
            NotesBlock::Code(t) => {
                ui.label(RichText::new(t).monospace());
            }
        }
    }
}

impl State {
    /// Each frame: start the launch Update check once its delay after the first frame has
    /// passed, and keep frames coming until then.
    pub(super) fn launch_update_check(&mut self, ctx: &egui::Context) {
        match self.updates_ui.launch_check {
            LaunchCheck::Off | LaunchCheck::Done => {}
            LaunchCheck::Armed => {
                self.updates_ui.launch_check = LaunchCheck::Waiting(self.now);
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                    LAUNCH_CHECK_DELAY_SECS,
                ));
            }
            LaunchCheck::Waiting(first) => {
                let left = first + LAUNCH_CHECK_DELAY_SECS - self.now;
                if left <= 0.0 {
                    self.updates_ui.launch_check = LaunchCheck::Done;
                    self.session.launch_update_check();
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
                }
            }
        }
    }

    /// Restart now: start the new app and close this one.
    pub(super) fn restart_into(&mut self, exe: &Path) {
        match (self.updates_ui.relaunch)(exe) {
            Ok(()) => {
                log::info!("restarting into the updated app");
                self.updates_ui.close = true;
            }
            Err(e) => self.toast(format!(
                "Could not start the new version ({e}). Close Open Bible Software and start it \
                 again to use it."
            )),
        }
    }

    /// After Restart now started the new app: close this window (as its close button would).
    pub(super) fn close_after_restart(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.updates_ui.close) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub(super) fn update_notice(&mut self, ctx: &egui::Context) {
        let updates = self.session.updates();
        let mut acts = Vec::new();
        if updates.first_run_note() && !updates.notice_shown() {
            egui::Area::new(egui::Id::unique("update-first-run"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::RIGHT_BOTTOM, vec2(-16.0, -16.0))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(380.0);
                        ui.label(
                            RichText::new("Update checks")
                                .strong()
                                .color(theme::c().ink),
                        );
                        ui.label(FIRST_RUN_NOTE);
                        ui.label(
                            RichText::new("Settings → Updates turns this on or off.")
                                .color(theme::c().muted),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("OK").clicked() {
                                acts.push(Act::Run("updates.explained"));
                            }
                            if ui.button("Turn off").clicked() {
                                acts.push(Act::TurnOff);
                            }
                        });
                    });
                });
        }
        // A launch check that failed quietly has nothing to show.
        if updates.notice_shown() && updates.status() != &UpdateStatus::Idle {
            let status = updates.status().clone();
            let current = updates.current().to_string();
            let cannot = match &status {
                UpdateStatus::Available(release) => {
                    let cached = (self.updates_ui.cannot.as_ref())
                        .filter(|(tag, _)| tag == &release.tag)
                        .map(|(_, why)| why.clone());
                    let why = cached.unwrap_or_else(|| updates.cannot_update());
                    self.updates_ui.cannot = Some((release.tag.clone(), why.clone()));
                    why
                }
                _ => None,
            };
            let signed = updates.source.public_key.is_some();
            let title = match &status {
                UpdateStatus::Available(_) => "Update available",
                UpdateStatus::Downloading { .. } => "Updating Open Bible Software",
                UpdateStatus::Installed { .. } => "Update installed",
                UpdateStatus::Failed {
                    release: Some(_), ..
                } => "Update not installed",
                UpdateStatus::Failed { release: None, .. } => "Could not check for updates",
                UpdateStatus::UpToDate => "No update available",
                UpdateStatus::Checking { .. } | UpdateStatus::Idle => "Check for updates",
            };
            let max_notes = ctx.content_rect().height() * 0.35;
            egui::Window::new(title)
                .id(egui::Id::unique("update-notice"))
                .collapsible(false)
                .resizable(false)
                .default_width(440.0)
                .anchor(egui::Align2::RIGHT_BOTTOM, vec2(-16.0, -16.0))
                .show(ctx, |ui| {
                    ui.set_max_width(440.0);
                    match &status {
                        UpdateStatus::Checking { .. } => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Checking for updates…");
                            });
                        }
                        UpdateStatus::Idle => {}
                        UpdateStatus::UpToDate => {
                            ui.label(format!(
                                "You have the newest version of Open Bible Software ({current})."
                            ));
                            if ui.button("OK").clicked() {
                                acts.push(Act::Run("update.later"));
                            }
                        }
                        UpdateStatus::Available(release) => {
                            available_ui(ui, release, &current, max_notes);
                            match &cannot {
                                Some(why) => {
                                    ui.label(RichText::new(why.message()).color(theme::c().ink));
                                    ui.horizontal(|ui| {
                                        if ui.button("Open the download page").clicked() {
                                            acts.push(Act::OpenPage);
                                        }
                                        later_and_skip(ui, &mut acts);
                                    });
                                }
                                None => {
                                    if !signed {
                                        ui.label(
                                            RichText::new(CHECKSUM_NOTE).color(theme::c().muted),
                                        );
                                    }
                                    ui.horizontal(|ui| {
                                        if ui.button("Update now").clicked() {
                                            acts.push(Act::Run("update.install"));
                                        }
                                        later_and_skip(ui, &mut acts);
                                    });
                                }
                            }
                        }
                        UpdateStatus::Downloading {
                            release,
                            done,
                            total,
                        } => {
                            ui.label(format!(
                                "Downloading Open Bible Software {}…",
                                release.version
                            ));
                            let fraction = if *total > 0 {
                                *done as f32 / *total as f32
                            } else {
                                0.0
                            };
                            ui.add(egui::ProgressBar::new(fraction).text(format!(
                                "{} of {}",
                                megabytes(*done),
                                megabytes(*total)
                            )));
                            ui.label(
                                RichText::new(
                                    "Nothing is replaced until the download is complete and \
                                     checked.",
                                )
                                .color(theme::c().muted),
                            );
                            if ui.button("Cancel").clicked() {
                                acts.push(Act::Run("update.cancel"));
                            }
                        }
                        UpdateStatus::Installed {
                            release,
                            backup,
                            verification,
                            ..
                        } => installed_ui(ui, release, backup.is_some(), *verification, &mut acts),
                        UpdateStatus::Failed { error, release } => {
                            failed_ui(ui, error, release.as_ref(), &mut acts);
                        }
                    }
                });
        }
        for act in acts {
            match act {
                Act::Run(command) => {
                    self.run(command, &json!({}));
                }
                Act::TurnOff => {
                    self.run("updates.auto_check", &json!({ "on": false }));
                }
                Act::OpenPage => {
                    let page = self.session.updates().download_page();
                    ctx.open_url(egui::OpenUrl::new_tab(page));
                }
            }
        }
    }
}

fn later_and_skip(ui: &mut egui::Ui, acts: &mut Vec<Act>) {
    if ui.button("Later").clicked() {
        acts.push(Act::Run("update.later"));
    }
    if ui.button("Skip this version").clicked() {
        acts.push(Act::Run("update.skip"));
    }
}

fn available_ui(ui: &mut egui::Ui, release: &Release, current: &str, max_notes: f32) {
    ui.label(
        RichText::new(format!(
            "Open Bible Software {} is available",
            release.version
        ))
        .strong()
        .color(theme::c().ink),
    );
    ui.label(RichText::new(format!("You have {current}.")).color(theme::c().muted));
    if !release.notes_blocks.is_empty() {
        ui.add_space(4.0);
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("update-notes")
                    .max_height(max_notes)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        notes_ui(ui, &release.notes_blocks);
                    });
            });
        ui.add_space(4.0);
    }
}

fn installed_ui(
    ui: &mut egui::Ui,
    release: &Release,
    backed_up: bool,
    verification: Verification,
    acts: &mut Vec<Act>,
) {
    ui.label(
        RichText::new(format!(
            "Open Bible Software {} is installed",
            release.version
        ))
        .strong()
        .color(theme::c().ink),
    );
    ui.label(if backed_up {
        "Restart to use it. Your Library, Notes and settings were not changed; a backup of your \
         data was taken first (Tools → Back up & restore…)."
    } else {
        "Restart to use it. Your Library, Notes and settings were not changed."
    });
    ui.label(RichText::new(verification.describe()).color(theme::c().muted));
    ui.horizontal(|ui| {
        if ui.button("Restart now").clicked() {
            acts.push(Act::Run("update.restart"));
        }
        if ui.button("Later").clicked() {
            acts.push(Act::Run("update.later"));
        }
    });
}

fn failed_ui(
    ui: &mut egui::Ui,
    error: &UpdateError,
    release: Option<&Release>,
    acts: &mut Vec<Act>,
) {
    ui.label(RichText::new(error.message()).color(theme::c().ink));
    if release.is_some() || error != &UpdateError::NoRelease {
        ui.label(
            RichText::new("You can download the new version yourself from the download page.")
                .color(theme::c().muted),
        );
    }
    ui.horizontal(|ui| {
        if ui.button("Open the download page").clicked() {
            acts.push(Act::OpenPage);
        }
        let retry = match (release, error) {
            (
                _,
                UpdateError::NotWritable(_)
                | UpdateError::NotUpdatable
                | UpdateError::RollbackIncomplete(_),
            ) => None,
            (Some(_), _) => Some("update.install"),
            (None, _) => Some("help.check_for_updates"),
        };
        if let Some(command) = retry
            && ui.button("Try again").clicked()
        {
            acts.push(Act::Run(command));
        }
        if ui.button("Close").clicked() {
            acts.push(Act::Run("update.later"));
        }
    });
}
