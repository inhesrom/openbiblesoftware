//! Native source selection and live, keyless Import report Tabs (#47).

use egui::{RichText, Ui};
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
    MarkdownBook,
    MarkdownFolder,
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
                    if let Some(books) = &record.books {
                        action = batch(ui, id, &record, books);
                        return;
                    }
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
            Some(ReportAction::OpenBook(book)) => {
                self.run("resource.open", &json!({ "id": book }));
            }
            None => {}
        }
    }

    /// The Import… chooser: a USFM file, a folder of Bible books, a SWORD module's `.conf`
    /// file, or a Markdown book (ADR 0023). The native picker can wait for the user indefinitely, so it runs on its
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
                ui.label(
                    "For a book kept as Markdown, choose its .md file; its Part files come with it.",
                );
                ui.label("A folder of Markdown books imports each book in it, subfolders too.");
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
                    if ui.button("Markdown book…").clicked() {
                        chosen = Some(Pick::MarkdownBook);
                    }
                    if ui.button("Markdown folder…").clicked() {
                        chosen = Some(Pick::MarkdownFolder);
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
                    Pick::MarkdownBook => rfd::FileDialog::new()
                        .set_title("Import a Markdown book")
                        .add_filter("Markdown", &["md", "markdown"])
                        .pick_file(),
                    Pick::MarkdownFolder => rfd::FileDialog::new()
                        .set_title("Import a folder of Markdown books")
                        .pick_folder(),
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
    /// Open one Book a batch brought in.
    OpenBook(String),
}

/// A batch of Books (ADR 0023): how far it has got, then every Book brought in (each opens)
/// and every one that could not be, with why in plain words and the error under Details.
fn batch(
    ui: &mut Ui,
    id: &str,
    record: &ImportRecord,
    books: &obs_engine::books::BookBatchSummary,
) -> Option<ReportAction> {
    let mut action = None;
    let running = matches!(
        record.phase,
        ImportPhase::Download | ImportPhase::Parse | ImportPhase::Index
    );
    section(ui, "Books");
    if running {
        let text = if record.total > 0 {
            format!("{} of {} books", record.done, record.total)
        } else {
            format!("{} books", record.done)
        };
        ui.add(progress(record).text(text));
        if ui.button("Cancel").clicked() {
            action = Some(ReportAction::Run("import.cancel"));
        }
    } else if record.phase == ImportPhase::Failed {
        let error = record.error.as_deref().unwrap_or_default();
        crate::components::error_notice(ui, failure_message(error), error);
    }
    ui.add_space(space::S);
    ui.label(format!(
        "{} imported · {} could not be imported",
        plural(books.imported.len(), "book", "books"),
        books.failed.len()
    ));
    ui.add_space(space::M);
    let row = ui.text_style_height(&egui::TextStyle::Body) + space::XS;
    if !books.imported.is_empty() {
        section(ui, "Imported");
        egui::ScrollArea::vertical()
            .id_salt(("batch-imported", id))
            .max_height(crate::tokens::size::LIST_MAX)
            .auto_shrink([false, true])
            .show_rows(ui, row, books.imported.len(), |ui, rows| {
                for book in &books.imported[rows] {
                    ui.horizontal(|ui| {
                        if crate::components::link(ui, &book.title)
                            .on_hover_text(&book.name)
                            .clicked()
                        {
                            action = Some(ReportAction::OpenBook(book.resource_id.clone()));
                        }
                        if book.updated {
                            ui.label(RichText::new("updated").color(theme::c().muted));
                        }
                    });
                }
            });
        ui.add_space(space::M);
    }
    if !books.failed.is_empty() {
        section(ui, "Could not be imported");
        egui::ScrollArea::vertical()
            .id_salt(("batch-failed", id))
            .max_height(crate::tokens::size::LIST_MAX)
            .auto_shrink([false, true])
            .show_rows(ui, row, books.failed.len(), |ui, rows| {
                for failure in &books.failed[rows] {
                    ui.label(format!(
                        "{} — {}",
                        failure.name,
                        failure_message(&format!("{}: {}", failure.name, failure.error))
                    ))
                    .on_hover_text(&failure.error);
                }
            });
    }
    if !running {
        ui.add_space(space::L);
        ui.horizontal(|ui| {
            if ui.button("Import another…").clicked() {
                action = Some(ReportAction::ImportAnother);
            }
            if ui.button("Close").clicked() {
                action = Some(ReportAction::Close);
            }
        });
    }
    action
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

/// "1 note", "3 notes".
fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

/// What an Import brought in, in the words of what it is (LIBRARY-10): a Bible's Bible books,
/// Verses, notes and Tagged words; a Commentary's, Lexicon's or Dictionary's entries; a
/// dataset's cross-references, people and places, or events. Only a Bible's counts show when
/// they are zero, so a Commentary never reads "0 Bible books · 0 Verses".
pub(super) fn summary_counts(report: &ImportReport) -> String {
    let mut parts = Vec::new();
    let mut nonzero = |n: usize, one: &str, many: &str| {
        if n > 0 {
            parts.push(plural(n, one, many));
        }
    };
    let dataset = report.cross_references
        + report.entities
        + report.mentions
        + report.events
        + report.parallel_sections
        > 0;
    if report.source_format == obs_engine::MARKDOWN_BOOK_FORMAT {
        // A Book (ADR 0023): its chapters and sections are its Divisions.
        nonzero(report.entries, "Division", "Divisions");
        nonzero(report.notes, "note", "notes");
        nonzero(report.scripture_refs, "Bible reference", "Bible references");
    } else if report.entries > 0 {
        nonzero(report.entries, "entry", "entries");
        nonzero(report.notes, "note", "notes");
        nonzero(report.scripture_refs, "Bible reference", "Bible references");
        nonzero(report.tagged_words, "Strong's link", "Strong's links");
    } else if dataset {
        nonzero(
            report.cross_references,
            "Cross-reference",
            "Cross-references",
        );
        nonzero(
            report.parallel_sections,
            "section with Parallel passages",
            "sections with Parallel passages",
        );
        nonzero(report.entities, "person or place", "people and places");
        nonzero(report.mentions, "mention", "mentions");
        nonzero(
            report.events,
            "era, lifespan or event",
            "eras, lifespans and events",
        );
    } else {
        parts.push(plural(report.books.len(), "Bible book", "Bible books"));
        parts.push(plural(report.verses, "Verse", "Verses"));
        parts.push(plural(report.notes, "note", "notes"));
        parts.push(plural(report.tagged_words, "Tagged word", "Tagged words"));
    }
    if parts.is_empty() {
        "Nothing to read was found.".to_string()
    } else {
        parts.join(" · ")
    }
}

/// What was brought in, dropped markup, Unmapped/Absent counts and licence warnings.
fn summary(ui: &mut Ui, id: &str, report: &ImportReport) {
    section(ui, "Summary");
    ui.label(
        RichText::new(&report.title)
            .font(theme::display(text::SUBTITLE))
            .color(theme::c().ink),
    );
    ui.label(format!(
        "{} · {}",
        plural(report.source_files.len(), "source file", "source files"),
        report.source_format
    ));
    ui.label(summary_counts(report));
    if let Some(counts) = report.verse_map {
        ui.label(format!("Unmapped verses: {}", counts.unmapped));
        ui.label(format!("Absent verses: {}", counts.absent));
    }
    ui.add_space(space::M);
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
