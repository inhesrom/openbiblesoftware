//! Native source selection and live, keyless Import report Tabs (#47).

use egui::{RichText, Ui};
pub(super) use obs_engine::import::summary_counts;
use obs_engine::import::{LICENCE_MISSING, LICENCE_RECORDED, source_line};
use obs_engine::{
    DEFERRED_INDEX, ImportIndexStatus, ImportPhase, ImportRecord, ImportReport, IndexStatus,
    UNAVAILABLE_INDEX, failure_message,
};
use serde_json::json;

use super::State;
use super::library_tab::section;
use crate::theme;
use crate::tokens::{space, text};

/// What the Import… chooser asks the native picker for.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Pick {
    UsfmFile,
    UsfmFolder,
    SwordConf,
}

impl State {
    /// The Import report Tab: live phases while the job runs, then the summary and actions.
    pub(super) fn import_body(&mut self, ui: &mut Ui, id: &str) {
        let Some(record) = self.session.imports.report(id).cloned() else {
            ui.centered_and_justified(|ui| {
                ui.colored_label(theme::c().warn, "No Import report for this Resource.")
            });
            return;
        };
        // The Search index's live state, once the Resource is in the library (#58).
        let index = self.session.search.status(id).cloned();
        let mut action: Option<ReportAction> = None;
        crate::components::page_frame().show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let title = match record.phase {
                        ImportPhase::Complete => "Import complete",
                        ImportPhase::Failed => "Import failed",
                        ImportPhase::Download if record.origin.is_some() => "Downloading",
                        ImportPhase::Download | ImportPhase::Parse | ImportPhase::Index => {
                            "Import in progress"
                        }
                    };
                    ui.label(crate::components::title_text(title));
                    ui.label(RichText::new(&record.source).color(theme::c().muted));
                    ui.add_space(space::L);
                    if record.phase == ImportPhase::Failed {
                        // What went wrong in plain words, the error under Details, and a way
                        // on (LIBRARY-13); not three phases of "not built".
                        action = failed(ui, &record);
                        return;
                    }
                    phases(ui, &record, index.as_ref());
                    let running = matches!(
                        record.phase,
                        ImportPhase::Download | ImportPhase::Parse | ImportPhase::Index
                    );
                    if running && ui.button("Cancel").clicked() {
                        action = Some(ReportAction::Run("import.cancel"));
                    }
                    if let Some(report) = &record.report {
                        ui.add_space(space::L);
                        summary(ui, id, report);
                        ui.add_space(space::XL);
                        ui.horizontal(|ui| {
                            if ui.button("Open").clicked() {
                                action = Some(ReportAction::Run("resource.open"));
                            }
                            if ui.button("About").clicked() {
                                action = Some(ReportAction::Run("resource.about"));
                            }
                            if ui.button("Remove import…").clicked() {
                                action = Some(ReportAction::Run("library.remove"));
                            }
                        });
                    }
                });
        });
        match action {
            Some(ReportAction::ImportAnother) => self.pick_import = true,
            Some(ReportAction::Close) => {
                let kind = obs_engine::TabKind::ImportReport(id.to_string());
                let tab =
                    (self.session.workspace.tabs()).find_map(|t| (t.kind == kind).then_some(t.id));
                if let Some(tab) = tab {
                    self.run("tab.close", &json!({ "tab": tab.0 }));
                }
            }
            Some(ReportAction::Run(command)) => {
                self.run(command, &json!({ "id": id }));
            }
            None => {}
        }
    }

    /// The Import… chooser: a USFM file, a folder of Bible books, or a SWORD module's
    /// `.conf` file. The native picker can wait for the user indefinitely, so it runs on its
    /// own thread and reports back through [`State::poll_picker`].
    pub(super) fn import_chooser(&mut self, ctx: &egui::Context) {
        if !self.pick_import {
            return;
        }
        let mut open = true;
        let mut chosen = None;
        egui::Window::new("Import a Resource")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label("Choose a USFM file or a folder of Bible books.");
                ui.label("For a SWORD module, choose its .conf file in mods.d/.");
                ui.add_space(space::S);
                ui.horizontal(|ui| {
                    if ui.button("USFM file…").clicked() {
                        chosen = Some(Pick::UsfmFile);
                    }
                    if ui.button("USFM folder…").clicked() {
                        chosen = Some(Pick::UsfmFolder);
                    }
                    if ui.button("SWORD module…").clicked() {
                        chosen = Some(Pick::SwordConf);
                    }
                });
            });
        self.pick_import = open;
        let Some(chosen) = chosen else {
            return;
        };
        self.pick_import = false;
        self.spawn_picker(ctx, chosen);
    }

    #[cfg(target_arch = "wasm32")]
    fn spawn_picker(&mut self, _: &egui::Context, _: Pick) {
        self.toast("Local file Import requires desktop");
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_picker(&mut self, ctx: &egui::Context, chosen: Pick) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("obs-import-picker".into())
            .spawn(move || {
                let selected = match chosen {
                    Pick::UsfmFolder => rfd::FileDialog::new()
                        .set_title("Import USFM Resource")
                        .pick_folder(),
                    Pick::UsfmFile => rfd::FileDialog::new()
                        .set_title("Import USFM Resource")
                        .add_filter("USFM", &["usfm", "sfm"])
                        .pick_file(),
                    Pick::SwordConf => rfd::FileDialog::new()
                        .set_title("Import SWORD module (its .conf file in mods.d/)")
                        .add_filter("SWORD module conf", &["conf"])
                        .pick_file(),
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

/// A failed Import's report: what went wrong in plain words with the error itself under
/// Details, then Import another… (a file the user picks: not after a download, whose row on
/// Get Resources has Retry) and Close.
fn failed(ui: &mut Ui, record: &ImportRecord) -> Option<ReportAction> {
    let error = record.error.as_deref().unwrap_or_default();
    let mut action = None;
    crate::components::error_notice(ui, failure_message(error), error);
    ui.add_space(space::L);
    ui.horizontal(|ui| {
        let downloaded = record.origin.is_some() || record.open_origin.is_some();
        if !downloaded && ui.button("Import another…").clicked() {
            action = Some(ReportAction::ImportAnother);
        }
        if ui.button("Close").clicked() {
            action = Some(ReportAction::Close);
        }
    });
    action
}

/// What a Button on an Import report asks for: a command on the Resource, or one of a failed
/// report's own actions.
enum ReportAction {
    Run(&'static str),
    ImportAnother,
    Close,
}

/// Bytes as "4.5 MB".
#[allow(clippy::cast_precision_loss)]
pub(super) fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// Download, Parse, then the Search index: live progress from the worker, never simulated.
/// A failed Import's report shows [`failed`] instead.
fn phases(ui: &mut Ui, record: &ImportRecord, index: Option<&IndexStatus>) {
    if let Some(origin) = &record.origin {
        section(ui, "Download");
        match record.phase {
            ImportPhase::Download if record.total > 0 => {
                ui.add(progress(record).text(format!(
                    "{} of {}",
                    megabytes(record.done),
                    megabytes(record.total)
                )));
            }
            ImportPhase::Download => {
                ui.add(waiting(format!("Downloading from {}…", origin.repo)));
            }
            ImportPhase::Failed => {}
            _ => {
                ui.label(format!("Downloaded from {}", origin.repo));
            }
        }
        ui.add_space(space::M);
    }
    section(ui, "Parse");
    match record.phase {
        ImportPhase::Download => {
            ui.label("Parse: waiting for the download");
        }
        ImportPhase::Parse if record.total == 0 => {
            ui.add(waiting("Reading the source…"));
        }
        ImportPhase::Parse => {
            ui.add(
                progress(record).text(format!("{} / {} Bible books", record.done, record.total)),
            );
        }
        ImportPhase::Index | ImportPhase::Complete => {
            ui.label("Parse complete");
        }
        ImportPhase::Failed => {}
    }
    ui.add_space(space::M);
    section(ui, "Search index");
    match (&record.phase, &record.index) {
        (ImportPhase::Download | ImportPhase::Parse, _) => {
            ui.label("Search index: waiting for Parse");
        }
        (ImportPhase::Index, _) if record.total > 0 => {
            ui.add(
                progress(record).text(format!("Search index: {} / {}", record.done, record.total)),
            );
        }
        (ImportPhase::Index, _) => {
            ui.add(waiting("Search index: starting…"));
        }
        (ImportPhase::Failed, _) => {}
        // Not built by the Import: the background rebuild's live state.
        (ImportPhase::Complete, ImportIndexStatus::Deferred | ImportIndexStatus::Failed(_)) => {
            match (index, &record.index) {
                (Some(IndexStatus::Ready), _) => {
                    ui.label("Search index: built");
                }
                (Some(IndexStatus::Indexing { done, total }), _) if *total > 0 => {
                    ui.add(crate::components::progress_bar(
                        *done as f32 / *total as f32,
                        format!("Search index: {done} / {total}"),
                    ));
                }
                (Some(IndexStatus::Unavailable), _) => {
                    ui.label(UNAVAILABLE_INDEX);
                }
                (Some(IndexStatus::Failed(e)), _) | (_, ImportIndexStatus::Failed(e)) => {
                    ui.colored_label(theme::c().warn, format!("Search index failed: {e}"));
                }
                _ => {
                    ui.label(DEFERRED_INDEX);
                }
            }
        }
        (ImportPhase::Complete, ImportIndexStatus::Built) => {
            ui.label("Search index: built");
        }
    }
}

#[allow(clippy::cast_precision_loss)]
fn progress(record: &ImportRecord) -> egui::ProgressBar {
    crate::components::progress_bar(record.done as f32 / record.total.max(1) as f32, "")
}

/// A phase under way whose length is not known yet: an animated, empty progress bar (#223).
fn waiting(what: impl Into<egui::WidgetText>) -> egui::ProgressBar {
    crate::components::progress_bar(0.0, what).animate(true)
}

/// What was brought in, dropped markup, Unmapped/Absent counts and licence warnings.
fn summary(ui: &mut Ui, id: &str, report: &ImportReport) {
    section(ui, "Summary");
    ui.label(
        RichText::new(&report.title)
            .font(theme::display(text::SUBTITLE))
            .color(theme::c().ink),
    );
    ui.label(source_line(report));
    ui.label(summary_counts(report));
    if let Some(counts) = report.verse_map {
        ui.label(format!("Unmapped verses: {}", counts.unmapped));
        ui.label(format!("Absent verses: {}", counts.absent));
    }
    ui.add_space(space::M);
    section(ui, "Licence");
    if report.license_missing {
        ui.colored_label(theme::c().warn, LICENCE_MISSING);
    } else {
        ui.label(LICENCE_RECORDED);
    }
    ui.add_space(space::M);
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
