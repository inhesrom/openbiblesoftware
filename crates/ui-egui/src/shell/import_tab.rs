//! Native source selection and live, keyless Import report Tabs (#47).

use egui::{RichText, Ui};
use obs_engine::{DEFERRED_INDEX, ImportIndexStatus, ImportPhase, ImportRecord, ImportReport};
use serde_json::json;

use super::State;
use super::library_tab::section;
use crate::theme;

impl State {
    /// The Import report Tab: live phases while the job runs, then the summary and actions.
    pub(super) fn import_body(&mut self, ui: &mut Ui, id: &str) {
        let Some(record) = self.session.imports.report(id).cloned() else {
            ui.centered_and_justified(|ui| {
                ui.colored_label(theme::c().warn, "No Import report for this Resource.")
            });
            return;
        };
        let mut action: Option<&'static str> = None;
        egui::Frame::new()
            .fill(theme::c().field)
            .inner_margin(egui::Margin::symmetric(18, 12))
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let title = match record.phase {
                            ImportPhase::Complete => "Import complete",
                            ImportPhase::Failed => "Import failed",
                            ImportPhase::Parse | ImportPhase::Index => "Import in progress",
                        };
                        ui.label(
                            RichText::new(title)
                                .font(theme::display(20.0))
                                .color(theme::c().ink),
                        );
                        ui.label(RichText::new(&record.source).color(theme::c().muted));
                        ui.add_space(10.0);
                        phases(ui, &record);
                        if let Some(report) = &record.report {
                            ui.add_space(10.0);
                            summary(ui, id, report);
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                if ui.button("Open").clicked() {
                                    action = Some("resource.open");
                                }
                                if ui.button("About").clicked() {
                                    action = Some("resource.about");
                                }
                                if ui.button("Remove import…").clicked() {
                                    action = Some("library.remove");
                                }
                            });
                        }
                    });
            });
        if let Some(command) = action {
            self.run(command, &json!({ "id": id }));
        }
    }

    /// The Import… chooser: a USFM file or a folder of Bible books. The native picker can
    /// wait for the user indefinitely, so it runs on its own thread and reports back through
    /// [`State::poll_picker`].
    pub(super) fn import_chooser(&mut self, ctx: &egui::Context) {
        if !self.pick_import {
            return;
        }
        let mut open = true;
        let mut folder = None;
        egui::Window::new("Import a Resource")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Choose a USFM file or a folder of Bible books.");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("USFM file…").clicked() {
                        folder = Some(false);
                    }
                    if ui.button("USFM folder…").clicked() {
                        folder = Some(true);
                    }
                });
            });
        self.pick_import = open;
        let Some(folder) = folder else {
            return;
        };
        self.pick_import = false;
        self.spawn_picker(ctx, folder);
    }

    #[cfg(target_arch = "wasm32")]
    fn spawn_picker(&mut self, _: &egui::Context, _: bool) {
        self.toast("Local file Import requires desktop");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_picker(&mut self, ctx: &egui::Context, folder: bool) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("obs-import-picker".into())
            .spawn(move || {
                let dialog = rfd::FileDialog::new().set_title("Import USFM Resource");
                let selected = if folder {
                    dialog.pick_folder()
                } else {
                    dialog.add_filter("USFM", &["usfm", "sfm"]).pick_file()
                };
                let _ = tx.send(selected);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.picker = Some(rx),
            Err(e) => self.toast(format!("Could not open file picker: {e}")),
        }
    }

    /// Start the Import once the picker thread has answered; never wait for it.
    pub(super) fn poll_picker(&mut self) {
        let Some(rx) = &self.picker else {
            return;
        };
        match rx.try_recv() {
            Ok(path) => {
                self.picker = None;
                if let Some(path) = path {
                    self.run("import.start", &json!({"path": path}));
                }
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.picker = None;
                self.toast("File picker closed unexpectedly");
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }
}

/// Parse, then the Search index: live progress from the worker, never simulated.
fn phases(ui: &mut Ui, record: &ImportRecord) {
    section(ui, "Parse");
    match record.phase {
        ImportPhase::Parse if record.total == 0 => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading the USFM source…");
            });
        }
        ImportPhase::Parse => {
            ui.add(
                progress(record).text(format!("{} / {} Bible books", record.done, record.total)),
            );
        }
        ImportPhase::Index | ImportPhase::Complete => {
            ui.label("Parse complete");
        }
        ImportPhase::Failed => {
            ui.colored_label(
                theme::c().warn,
                record.error.as_deref().unwrap_or("Import failed"),
            );
        }
    }
    ui.add_space(8.0);
    section(ui, "Search index");
    match (&record.phase, &record.index) {
        (ImportPhase::Parse, _) => {
            ui.label("Search index: waiting for Parse");
        }
        (ImportPhase::Index, _) if record.total > 0 => {
            ui.add(
                progress(record).text(format!("Search index: {} / {}", record.done, record.total)),
            );
        }
        (ImportPhase::Index, _) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Search index: starting…");
            });
        }
        (ImportPhase::Failed, _) => {
            ui.label("Search index: not built");
        }
        (ImportPhase::Complete, ImportIndexStatus::Deferred) => {
            ui.label(DEFERRED_INDEX);
        }
        (ImportPhase::Complete, ImportIndexStatus::Built) => {
            ui.label("Search index: built");
        }
        (ImportPhase::Complete, ImportIndexStatus::Failed(e)) => {
            ui.colored_label(theme::c().warn, format!("Search index failed: {e}"));
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn progress(record: &ImportRecord) -> egui::ProgressBar {
    egui::ProgressBar::new(record.done as f32 / record.total.max(1) as f32)
}

/// What was brought in, dropped markup, Unmapped/Absent counts and licence warnings.
fn summary(ui: &mut Ui, id: &str, report: &ImportReport) {
    section(ui, "Summary");
    ui.label(
        RichText::new(&report.title)
            .font(theme::display(16.0))
            .color(theme::c().ink),
    );
    ui.label(format!(
        "{} source file(s) · {}",
        report.source_files.len(),
        report.source_format
    ));
    ui.label(format!(
        "{} Bible book(s) · {} Verses · {} notes · {} Tagged words",
        report.books.len(),
        report.verses,
        report.notes,
        report.tagged_words
    ));
    if report.entries > 0 {
        ui.label(format!("{} entries", report.entries));
    }
    if let Some(counts) = report.verse_map {
        ui.label(format!("Unmapped verses: {}", counts.unmapped));
        ui.label(format!("Absent verses: {}", counts.absent));
    }
    ui.add_space(8.0);
    section(ui, "Licence");
    if report.license_missing {
        ui.colored_label(
            theme::c().warn,
            "Licence: unknown (user-imported). No licence found in the source; for personal \
             use only.",
        );
    } else {
        ui.label("Licence recorded from the source; see About.");
    }
    ui.add_space(8.0);
    let dropped: u64 = report.dropped_markup.values().sum();
    egui::CollapsingHeader::new(format!("Dropped markup ({dropped})"))
        .id_salt(("import-dropped", id))
        .show(ui, |ui| {
            if report.dropped_markup.is_empty() {
                ui.label("No markup dropped.");
            }
            for (marker, count) in &report.dropped_markup {
                ui.label(format!("{marker} ×{count}"));
            }
        });
    egui::CollapsingHeader::new(format!("Warnings ({})", report.warnings.len()))
        .id_salt(("import-warnings", id))
        .show(ui, |ui| {
            if report.warnings.is_empty() {
                ui.label("No other warnings.");
            }
            for warning in &report.warnings {
                ui.label(warning);
            }
        });
    egui::CollapsingHeader::new("Source files and Bible books")
        .id_salt(("import-files", id))
        .show(ui, |ui| {
            for file in &report.source_files {
                ui.label(file);
            }
            for book in &report.books {
                ui.label(format!(
                    "{} · {} chapters · {} Verses",
                    book.code, book.chapters, book.verses
                ));
            }
        });
}
