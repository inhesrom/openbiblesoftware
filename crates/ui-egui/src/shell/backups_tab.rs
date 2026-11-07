//! Back up & restore (#217): a keyless, single-instance Tab (Tools → Back up & restore…) to
//! back up everything to one file, restore from a Backup after a preview of what will change
//! (a Safety backup is taken first, and Undo restore takes it back), restore one of the
//! Backups the app keeps by itself, and export everything as a zip of Markdown and JSON.
//! The work is the engine's ([`obs_engine::backups`]); this Tab asks where with the native
//! file dialogs, on their own thread like Export Notes', and runs `data.*`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use egui::{Align, Layout, RichText, Ui, vec2};
use obs_engine::{RestorePreview, TabId};
use serde_json::json;

use super::State;
use super::library_tab::section;
use crate::theme;

/// What a file dialog of the Tab is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DataPick {
    /// Where to write a Backup (Back up now).
    BackUp,
    /// Which Backup to restore.
    Restore,
    /// Where to write the zip (Export everything).
    Export,
}

/// Asks for a file: (what for, suggested file name) → the chosen path.
pub(crate) type DataDialog = Arc<dyn Fn(DataPick, &str) -> Option<PathBuf> + Send + Sync>;

/// Whether command `id` replaces the whole User database (a restore or its undo), so the
/// Notes being written are saved before it and the Notes Tab's drafts forgotten after.
pub(super) fn replaces_user_data(id: &str) -> bool {
    matches!(id, "data.restore" | "data.undo_restore")
}

/// The Back up & restore Tab's state across frames.
pub(crate) struct BackupsUi {
    pub(super) dialog: DataDialog,
    /// A file dialog's answer, waited for without blocking a frame.
    waiting: Option<Receiver<(DataPick, Option<PathBuf>)>>,
    /// The restore being previewed: what will change, or why the file cannot be restored.
    preview: Option<Result<RestorePreview, String>>,
}

impl Default for BackupsUi {
    fn default() -> Self {
        Self {
            dialog: Arc::new(native_dialog),
            waiting: None,
            preview: None,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_dialog(pick: DataPick, name: &str) -> Option<PathBuf> {
    let backups = (
        "Open Bible backup",
        &[obs_engine::backups::BACKUP_EXTENSION][..],
    );
    match pick {
        DataPick::BackUp => rfd::FileDialog::new()
            .set_title("Back up now")
            .set_file_name(name)
            .add_filter(backups.0, backups.1)
            .save_file(),
        DataPick::Restore => rfd::FileDialog::new()
            .set_title("Restore from backup")
            .add_filter(backups.0, backups.1)
            .add_filter("All files", &["*"])
            .pick_file(),
        DataPick::Export => rfd::FileDialog::new()
            .set_title("Export everything")
            .set_file_name(name)
            .add_filter("Zip", &["zip"])
            .save_file(),
    }
}

#[cfg(target_arch = "wasm32")]
fn native_dialog(_: DataPick, _: &str) -> Option<PathBuf> {
    None
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

impl State {
    pub(super) fn backups_body(&mut self, ui: &mut Ui, tab: TabId) {
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(18, 12))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical()
                    .id_salt(("backups", tab))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width().min(720.0));
                        self.backups_content(ui);
                    });
            });
    }

    fn backups_content(&mut self, ui: &mut Ui) {
        let muted = theme::c().muted;
        ui.label(
            RichText::new(obs_engine::BACKUPS_TITLE)
                .font(theme::display(20.0))
                .color(theme::c().ink),
        );
        ui.label(
            RichText::new(
                "Your Notes, Highlights, Notebooks, Layouts, Reading plans, Favorites, History \
                 and settings are kept on this computer in your User database.",
            )
            .color(muted),
        );
        let has_user = self.session.user.is_some();
        if !has_user {
            ui.add_space(8.0);
            ui.colored_label(
                theme::c().warn,
                "Your User database is not open, so there is nothing to back up or restore.",
            );
        }
        let busy = self.backups_ui.waiting.is_some();

        ui.add_space(16.0);
        section(ui, "Back up");
        ui.label(
            RichText::new(
                "One file holding everything, to keep somewhere safe or move to another computer.",
            )
            .color(muted),
        );
        if ui
            .add_enabled(has_user && !busy, egui::Button::new("Back up now…"))
            .clicked()
        {
            let name = self.session.backup_file_name();
            self.ask_data_file(ui.ctx(), DataPick::BackUp, name);
        }

        ui.add_space(16.0);
        section(ui, "Restore");
        ui.label(
            RichText::new(
                "Replaces all your data with a backup's, after showing what will change. A \
                 safety backup of your data as it is now is taken first, so a restore can be \
                 undone.",
            )
            .color(muted),
        );
        if ui
            .add_enabled(has_user && !busy, egui::Button::new("Restore from backup…"))
            .clicked()
        {
            self.ask_data_file(ui.ctx(), DataPick::Restore, String::new());
        }
        if let Some(last) = self.session.last_restore().cloned() {
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "Restored from {}. A safety backup was taken first.",
                    file_name(&last.from)
                ));
                if ui.button("Undo restore").clicked()
                    && self.run("data.undo_restore", &json!({})).is_some()
                {
                    self.toast("Restore undone: your data is back as it was before it");
                }
            });
        }
        self.restore_preview_panel(ui);

        ui.add_space(16.0);
        section(ui, "Automatic backups");
        ui.label(
            RichText::new(format!(
                "The app backs up your data each time it starts (keeping the last {}) and before \
                 every restore (keeping the last {}).",
                obs_engine::backups::ROLLING_BACKUPS,
                obs_engine::backups::SAFETY_BACKUPS
            ))
            .color(muted),
        );
        let kept = self.session.kept_backups();
        if kept.is_empty() {
            ui.label(RichText::new("None yet.").color(muted));
        }
        // Restore… first on each row, so a narrow Panel never cuts it off.
        let mut chosen = None;
        for b in &kept {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("Restore…"))
                    .on_hover_text(b.path.display().to_string())
                    .clicked()
                {
                    chosen = Some(b.path.clone());
                }
                ui.label(self.session.backup_time(b));
                ui.label(RichText::new(b.kind.label()).color(muted));
            });
        }
        if let Some(path) = chosen {
            self.preview_restore(&path);
        }

        ui.add_space(16.0);
        section(ui, "Export everything");
        ui.label(
            RichText::new(
                "A zip of your Notes as Markdown, one file per Notebook, and everything else as \
                 JSON, to read or keep elsewhere. The app cannot restore from it: use a backup \
                 for that.",
            )
            .color(muted),
        );
        if ui
            .add_enabled(has_user && !busy, egui::Button::new("Export everything…"))
            .clicked()
        {
            let name = self.session.data_export_file_name();
            self.ask_data_file(ui.ctx(), DataPick::Export, name);
        }
    }

    /// The preview of a restore, with Restore and Cancel.
    fn restore_preview_panel(&mut self, ui: &mut Ui) {
        let Some(preview) = &self.backups_ui.preview else {
            return;
        };
        let mut act = None;
        ui.add_space(8.0);
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, theme::c().rule))
            .corner_radius(4.0)
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| match preview {
                Err(why) => {
                    ui.colored_label(theme::c().warn, why);
                    if ui.button("Close").clicked() {
                        act = Some(false);
                    }
                }
                Ok(p) => {
                    ui.label(
                        RichText::new(format!("Restore {}?", file_name(&p.path)))
                            .strong()
                            .color(theme::c().ink),
                    );
                    ui.label(
                        RichText::new("Everything below is replaced with what the backup holds.")
                            .color(theme::c().muted),
                    );
                    if p.older_format {
                        ui.label(
                            RichText::new(
                                "This backup was made by an older version of the app; it is \
                                 upgraded as it is restored.",
                            )
                            .color(theme::c().muted),
                        );
                    }
                    ui.add_space(6.0);
                    egui::Grid::new("restore-preview")
                        .num_columns(3)
                        .spacing(vec2(24.0, 4.0))
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("");
                            ui.label(RichText::new("Now").strong());
                            ui.label(RichText::new("After restoring").strong());
                            ui.end_row();
                            for l in &p.lines {
                                ui.label(l.what);
                                ui.label(l.now.to_string());
                                let after = RichText::new(l.after.to_string());
                                ui.label(if l.after == l.now {
                                    after
                                } else {
                                    after.strong().color(theme::c().ink)
                                });
                                ui.end_row();
                            }
                        });
                    // What comes back and what goes, by name.
                    for l in p.lines.iter().filter(|l| !l.detail.is_empty()) {
                        ui.label(format!("{}: {}", l.what, l.detail));
                    }
                    if !p.changes_anything() {
                        ui.label(
                            RichText::new(
                                "The counts match your data now; the text of Notes and \
                                 settings may still differ.",
                            )
                            .color(theme::c().muted),
                        );
                    }
                    ui.add_space(6.0);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        if ui.button("Restore").clicked() {
                            act = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            act = Some(false);
                        }
                    });
                }
            });
        match act {
            Some(true) => {
                // Unsaved Note text keeps the preview open, for Restore once it is saved.
                if !self.settle_note_editors() {
                    return;
                }
                let Some(Ok(p)) = self.backups_ui.preview.take() else {
                    return;
                };
                let path = p.path.to_string_lossy().into_owned();
                if self.run("data.restore", &json!({ "path": path })).is_some() {
                    self.toast(format!(
                        "Restored from {}. Undo restore takes it back.",
                        file_name(&p.path)
                    ));
                }
            }
            Some(false) => self.backups_ui.preview = None,
            None => {}
        }
    }

    /// Before a restore (or its undo) replaces the User database: save the Note being written
    /// and the Notes card being edited, as closing the window does, so neither writes text
    /// typed before the restore over the restored Notes later (a card's failed save is
    /// retried by itself). False if one cannot be saved: it is shown with why, and nothing is
    /// restored until it is saved or its changes discarded. [`State::run`] calls it before
    /// every command [`replaces_user_data`], whatever ran it.
    pub(super) fn settle_note_editors(&mut self) -> bool {
        self.save_note();
        self.save_card_edit(true);
        if let Some(id) = self.notes_ui.editing.as_ref().map(|e| e.id.clone()) {
            self.run("notes.open", &json!({ "id": id }));
        }
        if self.note_editor.is_some() || self.notes_ui.editing.is_some() {
            self.toast(
                "Nothing was restored: a Note you are writing could not be saved. Save it or \
                 discard its changes, then restore.",
            );
            return false;
        }
        true
    }

    /// Show what restoring the Backup at `path` would change.
    pub(super) fn preview_restore(&mut self, path: &Path) {
        self.backups_ui.preview = Some(
            self.session
                .restore_preview(path)
                .map_err(|e| format!("{} cannot be restored: {e}", file_name(path))),
        );
    }

    /// Ask for a file with the native dialog, on its own thread.
    fn ask_data_file(&mut self, ctx: &egui::Context, pick: DataPick, name: String) {
        let dialog = Arc::clone(&self.backups_ui.dialog);
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("obs-data-dialog".into())
            .spawn(move || {
                let path = dialog(pick, &name);
                let _ = tx.send((pick, path));
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.backups_ui.waiting = Some(rx),
            Err(e) => self.toast(format!("Could not open the file dialog: {e}")),
        }
    }

    /// Act on a file dialog's answer once it comes; never wait for it.
    pub(super) fn poll_data_dialog(&mut self) {
        let Some(rx) = &self.backups_ui.waiting else {
            return;
        };
        let (pick, path) = match rx.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.backups_ui.waiting = None;
                self.toast("The file dialog closed unexpectedly");
                return;
            }
        };
        self.backups_ui.waiting = None;
        let Some(path) = path else {
            return;
        };
        let p = json!({ "path": path.to_string_lossy() });
        match pick {
            DataPick::BackUp => {
                if self.run("data.back_up", &p).is_some() {
                    self.toast(format!("Backed up everything to {}", path.display()));
                }
            }
            DataPick::Restore => self.preview_restore(&path),
            DataPick::Export => {
                if let Some(out) = self.run("data.export", &p) {
                    let n = out["files"].as_array().map_or(0, Vec::len);
                    self.toast(format!("Exported {n} files to {}", path.display()));
                }
            }
        }
    }

    /// Whether a file dialog of the Tab is open (frames keep polling).
    pub(super) fn data_dialog_waiting(&self) -> bool {
        self.backups_ui.waiting.is_some()
    }
}
