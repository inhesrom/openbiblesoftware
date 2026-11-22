//! Sync on the desktop (plan S29, ADR 0022): Settings → Sync, the status indicator in the
//! menu bar, the prompt after a Backup is restored on a syncing device, and the frame's sync
//! tick. Every state and word comes from the engine ([`obs_engine::Session::sync_panel`],
//! [`obs_engine::sync::texts`]); this file lays them out and runs `sync.*` commands. The
//! folder is asked for with the native folder dialog, on its own thread like Back up &
//! restore's (`DataPick::SyncFolder`).

use egui::{RichText, Ui, vec2};
use obs_engine::sync::texts;
use serde_json::{Value, json};

use super::State;
use super::backups_tab::DataPick;
use crate::icons::{self, Icon};
use crate::theme;
use crate::tokens::{size, space};

/// A command a control asked for.
type Run = Option<(&'static str, Value)>;

/// Settings → Sync's state across frames.
#[derive(Debug, Default)]
pub(crate) struct SyncUi {
    /// Stop syncing asks first.
    confirming_stop: bool,
    /// The device's name as typed, while its field has the keyboard.
    name: Option<String>,
}

impl State {
    /// The frame's sync: a due sync runs (after a frame showing "Syncing…"), and the next
    /// frame is asked for when the next sync is due. Nothing without a Sync folder.
    pub(super) fn sync_frame(&mut self, ctx: &egui::Context) {
        let Some(due_ms) = self.session.sync_tick(obs_store::clock::now_ms()) else {
            return;
        };
        match u64::try_from(due_ms) {
            Ok(0) | Err(_) => ctx.request_repaint(),
            Ok(ms) => ctx.request_repaint_after(std::time::Duration::from_millis(ms)),
        }
    }

    /// The status indicator, left of Home, once a Sync folder is chosen: its tooltip says
    /// where sync stands; a click opens Settings → Sync.
    pub(super) fn sync_indicator(&mut self, ui: &mut Ui) {
        let Some(indicator) = self.session.sync_indicator(obs_store::clock::now_ms()) else {
            return;
        };
        let colour = if indicator.warn {
            theme::c().warn
        } else {
            theme::c().muted
        };
        let r = icons::button(
            ui,
            Icon::Sync,
            icons::BODY,
            colour,
            &format!("{}: {}", texts::SECTION, indicator.text),
        )
        .on_hover_text(&indicator.text);
        if r.clicked() {
            self.show_sync_settings();
        }
    }

    /// Open Settings showing Sync.
    fn show_sync_settings(&mut self) {
        let opened = self.run("app.settings", &json!({}));
        if let Some(tab) = opened.and_then(|v| v["tab"].as_u64()) {
            self.tabs.entry(obs_engine::TabId(tab)).or_default().filter =
                texts::SECTION.to_string();
        }
    }

    /// Settings → Sync: what Sync is and Choose Sync folder…, or, once a folder is chosen,
    /// the restore's question, the folder, this device's name, Sync now with the status, the
    /// Synced devices and Stop syncing.
    pub(super) fn sync_setting(&mut self, ui: &mut Ui) -> Run {
        let panel = self.session.sync_panel(obs_store::clock::now_ms());
        let Some(folder) = panel.folder.clone() else {
            ui.add(egui::Label::new(RichText::new(texts::ONBOARDING).color(theme::c().ink)).wrap());
            ui.add_space(space::XS);
            ui.add(egui::Label::new(crate::components::hint_text(panel.guidance)).wrap());
            ui.add_space(space::M);
            if ui
                .add_enabled(
                    !self.data_dialog_waiting(),
                    crate::components::button(texts::CHOOSE_FOLDER),
                )
                .clicked()
            {
                self.ask_data_file(ui.ctx(), DataPick::SyncFolder, String::new());
            }
            return None;
        };
        let mut run: Run = None;
        if panel.restore_pending {
            run = run.or(restore_question(ui));
            ui.add_space(space::M);
        }
        if let Some(newer) = panel.folder_newer {
            crate::components::notice_frame(theme::c().warn).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(newer).color(theme::c().ink)).wrap());
                if ui
                    .add(crate::components::button(
                        self.session
                            .commands()
                            .get("help.check_for_updates")
                            .map_or("Check for updates", |c| c.label),
                    ))
                    .clicked()
                {
                    run = Some(("help.check_for_updates", json!({})));
                }
            });
            ui.add_space(space::M);
        }
        ui.label(RichText::new(texts::FOLDER_LABEL).strong().color(theme::c().ink));
        ui.add(
            egui::Label::new(RichText::new(folder).monospace().color(theme::c().ink))
                .selectable(true)
                .wrap(),
        );
        ui.add_space(space::M);
        ui.label(
            RichText::new(texts::DEVICE_NAME_LABEL)
                .strong()
                .color(theme::c().ink),
        );
        ui.label(crate::components::hint_text(texts::DEVICE_NAME_HINT));
        run = run.or(self.device_name_field(ui, &panel.device_name));
        ui.add_space(space::M);
        ui.horizontal_wrapped(|ui| {
            let r = ui
                .add_enabled(
                    !panel.syncing && !panel.restore_pending,
                    crate::components::button(icons::with_icon(ui, Icon::Sync, texts::SYNC_NOW)),
                )
                .on_hover_text(&panel.status);
            if r.clicked() {
                run = Some(("sync.now", json!({})));
            }
            let colour = if panel.warn {
                theme::c().warn
            } else {
                theme::c().muted
            };
            ui.add(egui::Label::new(RichText::new(&panel.status).color(colour)).wrap());
        });
        if let Some(details) = &panel.details {
            crate::components::error_notice(ui, texts::UNREACHABLE, details);
        }
        if !panel.devices.is_empty() {
            ui.add_space(space::M);
            ui.label(RichText::new(texts::DEVICES_LABEL).strong().color(theme::c().ink));
            for device in &panel.devices {
                let r = ui.add(
                    egui::Label::new(RichText::new(&device.line).color(theme::c().ink)).wrap(),
                );
                if let Some(problem) = &device.problem {
                    r.on_hover_text(problem);
                }
                if let Some(newer) = device.needs_newer_app {
                    ui.horizontal_top(|ui| {
                        let warn = theme::c().warn;
                        ui.add(icons::image(ui.ctx(), Icon::Warning, icons::SMALL, warn));
                        ui.add(egui::Label::new(RichText::new(newer).small().color(warn)).wrap());
                    });
                }
            }
        }
        ui.add_space(space::M);
        if self.sync_ui.confirming_stop {
            ui.add(egui::Label::new(RichText::new(texts::STOP_CONFIRM).color(theme::c().warn)).wrap());
            ui.horizontal(|ui| {
                if ui
                    .button(texts::STOP_SYNCING)
                    .accessible_name(format!("Confirm {}", texts::STOP_SYNCING))
                    .clicked()
                {
                    self.sync_ui.confirming_stop = false;
                    run = Some(("sync.stop", json!({ "confirm": true })));
                }
                if ui.button("Cancel").clicked() {
                    self.sync_ui.confirming_stop = false;
                }
            });
        } else if ui
            .add(crate::components::button(format!("{}…", texts::STOP_SYNCING)))
            .clicked()
        {
            self.sync_ui.confirming_stop = true;
        }
        run
    }

    /// This device's name: typed in place, saved on Enter or when the field lets go.
    fn device_name_field(&mut self, ui: &mut Ui, saved: &str) -> Run {
        let mut text = self.sync_ui.name.clone().unwrap_or_else(|| saved.to_string());
        let r = ui.add(
            crate::components::text_field(&mut text)
                .hint_text(texts::DEVICE_NAME_PLACEHOLDER)
                .desired_width(size::FIELD_WIDE),
        );
        let r = r.accessible_name(texts::DEVICE_NAME_LABEL);
        if r.has_focus() {
            self.sync_ui.name = Some(text);
            return None;
        }
        let typed = self.sync_ui.name.take();
        match typed {
            Some(name) if r.lost_focus() && name.trim() != saved => {
                Some(("sync.device_name", json!({ "name": name })))
            }
            _ => None,
        }
    }

    /// The prompt after a Backup was restored on this syncing device (ADR 0022 rule 7):
    /// Catch up (the default, and Enter) or Replace. It stays until one is chosen, also
    /// across a restart.
    pub(super) fn sync_restore_prompt(&mut self, ctx: &egui::Context) {
        if !self.session.sync_restore_pending() {
            return;
        }
        let mut run: Run = None;
        egui::Window::new(texts::RESTORE_TITLE)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_max_width(size::PROSE);
                run = restore_buttons(ui, true);
            });
        if let Some((id, params)) = run {
            self.run(id, &params);
        }
    }
}

/// The restore's question inside Settings → Sync, in a notice.
fn restore_question(ui: &mut Ui) -> Run {
    let mut run = None;
    crate::components::notice_frame(theme::c().warn).show(ui, |ui| {
        ui.label(
            RichText::new(texts::RESTORE_TITLE)
                .strong()
                .color(theme::c().ink),
        );
        run = restore_buttons(ui, false);
    });
    run
}

/// The restore's body and its two buttons; Catch up is the default (Enter, in the prompt).
fn restore_buttons(ui: &mut Ui, enter_catches_up: bool) -> Run {
    let mut run = None;
    ui.add(egui::Label::new(RichText::new(texts::RESTORE_BODY).color(theme::c().ink)).wrap());
    ui.add_space(space::M);
    ui.horizontal(|ui| {
        let catch_up = ui.add(crate::components::button(texts::CATCH_UP).selected(true));
        if catch_up.clicked()
            || (enter_catches_up && ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            run = Some(("sync.restore_choice", json!({ "choice": "catch_up" })));
        }
        if ui.add(crate::components::button(texts::REPLACE)).clicked() {
            run = Some(("sync.restore_choice", json!({ "choice": "replace" })));
        }
    });
    run
}
