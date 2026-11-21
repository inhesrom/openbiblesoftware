//! Book batches (ADR 0023, #344, #347): many Markdown books imported by one job, each its own
//! Book Resource, under one Import report that lists every book brought in and every book that
//! could not be, with why.
//!
//! A batch reads its books from a [`BookStream`]: a folder on this computer (Import… →
//! Markdown folder…) or, for #347, the Christian Open Library. The job takes one book per
//! step, so Cancel stops between books and the books already in stay in the Library. Each
//! book that comes in is sent back at once ([`JobContext::emit`]) and joins the Library then,
//! with its own Import record, as a single Import would.

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use obs_formats::{BookFile, BookFiles, MarkdownBookOptions, group_book_files};
use serde::{Deserialize, Serialize};

use crate::import::{ImportIndexStatus, ImportIndexer, ImportPhase, ImportRecord};
use crate::jobs::{Job, JobContext, Step};
use crate::{Effect, JobEvent, JobEventKind, LibraryEntry, OpenRequest, Session, TabKind};

/// One book of a batch, read and ready to import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedBook {
    /// What the report calls it: its main file's path in the source (else its first Part's).
    pub name: String,
    pub main: Option<BookFile>,
    pub parts: Vec<BookFile>,
    /// The Resource id to write it as: a book already in the Library is written as its next
    /// Resource revision. `None`: a new id made from the book's name.
    pub resource_id: Option<String>,
    /// Added to the Book's Attribution (where it was copied from, ADR 0024).
    pub note: Option<String>,
    /// Where a Christian Open Library book came from, kept on its Import record.
    pub col: Option<crate::col::ColOrigin>,
}

/// A book a batch could not bring in, and why (the plain error, never a stack).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookFailure {
    pub name: String,
    pub error: String,
}

/// A book a batch brought in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookOutcome {
    pub name: String,
    pub resource_id: String,
    pub title: String,
    /// It replaced the book's earlier revision in the Library.
    #[serde(default)]
    pub updated: bool,
}

/// What a batch's Import report lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookBatchSummary {
    pub imported: Vec<BookOutcome>,
    pub failed: Vec<BookFailure>,
}

/// Where a batch's books come from. `next_book` is called once per job step.
pub trait BookStream: Send + 'static {
    /// How many books it holds, when known before reading them.
    fn total(&self) -> Option<usize>;
    /// The next book (`Ok(None)` when there are no more). An `Err` inside is one book that
    /// could not be read, and the batch goes on; an outer `Err` stops the batch (the books
    /// already in stay).
    fn next_book(
        &mut self,
        ctx: &JobContext,
    ) -> Result<Option<Result<LoadedBook, BookFailure>>, String>;
}

#[cfg(not(target_arch = "wasm32"))]
/// Folders a folder batch never reads: hidden ones (`.git`, `.obsidian`) and Obsidian's
/// trash.
fn skipped_dir(name: &str) -> bool {
    name.starts_with('.') || name == "_trash"
}

/// The most Markdown files a folder batch reads (COL's whole library is about 17,600).
pub const MAX_FOLDER_FILES: usize = 100_000;

#[cfg(not(target_arch = "wasm32"))]
/// The Markdown files under `root` (relative paths with `/`), every subfolder but
/// [`skipped_dir`]s; symbolic links are not followed, so nothing outside `root` is read.
pub fn markdown_files(root: &Path) -> std::io::Result<Vec<String>> {
    let mut out = Vec::new();
    let mut folders = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = folders.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if kind.is_dir() {
                if !skipped_dir(&name) {
                    folders.push((entry.path(), rel));
                }
            } else if kind.is_file() && obs_formats::is_markdown_name(&name) {
                out.push(rel);
                if out.len() > MAX_FOLDER_FILES {
                    return Err(std::io::Error::other(format!(
                        "more than {MAX_FOLDER_FILES} Markdown files"
                    )));
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(not(target_arch = "wasm32"))]
/// A folder of Markdown books on this computer, every subfolder included.
pub struct FolderStream {
    root: PathBuf,
    books: std::vec::IntoIter<BookFiles>,
    total: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl FolderStream {
    pub fn open(root: &Path) -> Result<Self, String> {
        let names = markdown_files(root).map_err(|e| format!("{}: {e}", root.display()))?;
        let books = group_book_files(&names);
        if books.is_empty() {
            return Err("no Markdown files in this folder".into());
        }
        Ok(Self {
            root: root.to_path_buf(),
            total: books.len(),
            books: books.into_iter(),
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// Read one file of a book, refusing one too large to import or not UTF-8.
pub(crate) fn read_book_file(root: &Path, name: &str) -> Result<BookFile, String> {
    let path = root.join(name);
    let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if len > obs_formats::MAX_BOOK_FILE_BYTES as u64 {
        return Err(format!("{name} is too large to import"));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("{name}: {e} (a Markdown book must be UTF-8 text)"))?;
    Ok(BookFile {
        name: name.to_string(),
        text,
    })
}

#[cfg(not(target_arch = "wasm32"))]
impl BookStream for FolderStream {
    fn total(&self) -> Option<usize> {
        Some(self.total)
    }

    fn next_book(
        &mut self,
        _: &JobContext,
    ) -> Result<Option<Result<LoadedBook, BookFailure>>, String> {
        let Some(book) = self.books.next() else {
            return Ok(None);
        };
        let name = book.files().next().cloned().unwrap_or_default();
        let read = || -> Result<LoadedBook, String> {
            let main = book
                .main
                .as_ref()
                .map(|n| read_book_file(&self.root, n))
                .transpose()?;
            let parts = book
                .parts
                .iter()
                .map(|n| read_book_file(&self.root, n))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(LoadedBook {
                name: name.clone(),
                main,
                parts,
                resource_id: None,
                note: None,
                col: None,
            })
        };
        Ok(Some(read().map_err(|error| BookFailure { name, error })))
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// A Resource already in the Library that a batch book replaces: its `.obr` and revision.
#[derive(Debug, Clone)]
pub(crate) struct Existing {
    pub path: PathBuf,
    pub revision: u32,
}

/// What the job sends back as each book is done.
pub(crate) enum BookEvent {
    Imported(Box<ImportedBook>),
    Failed(BookFailure),
}

/// A book of a batch that is in.
pub(crate) struct ImportedBook {
    name: String,
    entry: LibraryEntry,
    report: obs_formats::ImportReport,
    index: ImportIndexStatus,
    updated: bool,
    col: Option<crate::col::ColOrigin>,
}

#[cfg(not(target_arch = "wasm32"))]
/// The batch job: one book per step.
struct BatchJob {
    stream: Box<dyn BookStream>,
    dir: PathBuf,
    /// Resource ids in the Library or written by this batch: a new book never takes one.
    taken: BTreeSet<String>,
    existing: HashMap<String, Existing>,
    indexer: Arc<dyn ImportIndexer>,
    done: u64,
    total: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl BatchJob {
    /// A new, unused Resource id from a book's name (`Owen - Mortification.md` →
    /// `owen-mortification-1`), as a single Import makes one.
    fn new_id(&mut self, name: &str) -> String {
        let file = name.rsplit('/').next().unwrap_or(name);
        let slug = obs_formats::slugify(obs_formats::book_stem(file));
        let slug: String = slug.chars().take(44).collect();
        let slug = slug.trim_end_matches('-');
        let mut n = 1u64;
        loop {
            let id = format!("{slug}-{n}");
            if !self.taken.contains(&id) && !self.dir.join(format!("{id}.obr")).exists() {
                self.taken.insert(id.clone());
                return id;
            }
            n += 1;
        }
    }

    fn import(&mut self, book: LoadedBook, ctx: &JobContext) -> Result<BookEvent, String> {
        let (id, revision) = match &book.resource_id {
            Some(id) => (
                id.clone(),
                self.existing.get(id).map(|e| crate::import::Revision {
                    number: e.revision.saturating_add(1),
                    path: e.path.clone(),
                }),
            ),
            None => (self.new_id(&book.name), None),
        };
        let updated = revision.is_some();
        let options = MarkdownBookOptions {
            resource_id: id.clone(),
            source_file: book.name.clone(),
            tool_version: crate::import::tool_version(),
        };
        let (mut resource, report) =
            obs_formats::import_markdown_book(book.main.as_ref(), &book.parts, &options, |_, _| {
                !ctx.is_cancelled()
            })
            .map_err(|e| e.to_string())?;
        if let Some(note) = &book.note {
            let a = &mut resource.manifest.attribution;
            if !a.is_empty() {
                a.push(' ');
            }
            a.push_str(note);
        }
        let target = crate::import::Target {
            dir: &self.dir,
            id: &id,
            revision,
        };
        let mut written = None;
        let (entry, index) = crate::import::write_resource(
            resource,
            &target,
            self.indexer.as_ref(),
            ctx,
            &mut written,
        )
        .map_err(|e| {
            // Nothing half-written stays behind; a revision's earlier file is untouched.
            if let Some(file) = written.take() {
                let _ = std::fs::remove_file(file);
            }
            e.to_string()
        })?;
        self.taken.insert(id);
        Ok(BookEvent::Imported(Box::new(ImportedBook {
            name: book.name,
            entry,
            report,
            index,
            updated,
            col: book.col,
        })))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Job for BatchJob {
    fn label(&self) -> &'static str {
        "Book import"
    }

    fn step(&mut self, ctx: &JobContext) -> Result<Step, String> {
        if self.done == 0 {
            ctx.phase("Parse");
        }
        let Some(next) = self.stream.next_book(ctx)? else {
            return Ok(Step::Done(Box::new(BatchDone)));
        };
        let event = match next {
            Err(failure) => BookEvent::Failed(failure),
            Ok(book) => {
                let name = book.name.clone();
                match self.import(book, ctx) {
                    Ok(event) => event,
                    Err(_) if ctx.is_cancelled() => return Err("Import cancelled".into()),
                    Err(error) => BookEvent::Failed(BookFailure { name, error }),
                }
            }
        };
        ctx.emit(Box::new(event));
        self.done += 1;
        self.total = self.total.max(self.done);
        ctx.progress(self.done, self.total);
        Ok(Step::Continue)
    }
}

#[cfg(not(target_arch = "wasm32"))]
/// A batch's job finished: every book read.
struct BatchDone;

/// The prefix of a batch's Import report id. A Resource id never holds `:`, so a batch's
/// report never shares an id with a Resource's.
pub const BATCH_PREFIX: &str = "books:";

impl Session {
    /// Import every Markdown book under `root`, each its own Book Resource, and open the
    /// batch's Import report. Returns the report id, the Tab and the job.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start_book_folder_import(
        &mut self,
        root: &Path,
    ) -> Result<(String, u64, crate::JobId), String> {
        let stream = FolderStream::open(root)?;
        let source = root.display().to_string();
        let name = root
            .file_name()
            .map_or_else(|| source.clone(), |n| n.to_string_lossy().into_owned());
        self.start_book_batch(Box::new(stream), source, &name)
    }

    /// Start a batch over `stream`, its report described as `source` and titled `name`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn start_book_batch(
        &mut self,
        stream: Box<dyn BookStream>,
        source: String,
        name: &str,
    ) -> Result<(String, u64, crate::JobId), String> {
        let dir = self
            .library
            .directory()
            .map(Path::to_path_buf)
            .or_else(crate::default_library_dir)
            .ok_or("No library directory")?;
        self.library.set_directory(dir.clone());
        let mut n = 1u64;
        let id = loop {
            let id = format!("{BATCH_PREFIX}{n}");
            if self.imports.report(&id).is_none() {
                break id;
            }
            n += 1;
        };
        let taken: BTreeSet<String> = self
            .library
            .entries()
            .iter()
            .map(|e| e.manifest.resource_id.clone())
            .chain(self.imports.reports.keys().cloned())
            .collect();
        let existing = self
            .library
            .entries()
            .iter()
            .map(|e| {
                (
                    e.manifest.resource_id.clone(),
                    Existing {
                        path: e.path.clone(),
                        revision: e.manifest.revision,
                    },
                )
            })
            .collect();
        let total = stream.total().unwrap_or(0) as u64;
        let job = BatchJob {
            stream,
            dir,
            taken,
            existing,
            indexer: Arc::clone(&self.imports.indexer),
            done: 0,
            total,
        };
        let record = ImportRecord {
            resource_id: id.clone(),
            source,
            phase: ImportPhase::Parse,
            done: 0,
            total,
            report: None,
            index: ImportIndexStatus::Deferred,
            error: None,
            origin: None,
            open_origin: None,
            books: Some(BookBatchSummary::default()),
            col: None,
        };
        let job = self.jobs().submit(job);
        self.imports.batches.insert(job, id.clone());
        self.imports.reports.insert(id.clone(), record);
        let tab = self.workspace.open(
            TabKind::ImportReport(id.clone()),
            crate::import::report_title(name),
            OpenRequest::default(),
        );
        Ok((id, tab.0, job))
    }

    /// Apply book batch events: each book joins the Library as it comes in.
    pub(crate) fn poll_book_batches(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let mut other = Vec::new();
        let mut added = Vec::new();
        for event in events {
            let Some(id) = self.imports.batches.get(&event.id).cloned() else {
                other.push(event);
                continue;
            };
            let terminal = event.kind.is_terminal();
            match event.kind {
                JobEventKind::Progress { done, total } => {
                    if let Some(r) = self.imports.reports.get_mut(&id) {
                        r.done = done;
                        r.total = total;
                    }
                }
                JobEventKind::Partial(out) => {
                    if let Ok(book) = out.downcast::<BookEvent>()
                        && let Some(book_id) = self.book_arrived(&id, *book)
                    {
                        added.push(book_id);
                    }
                }
                JobEventKind::Finished(_) => self.batch_ended(&id, ImportPhase::Complete, None),
                JobEventKind::Failed(e) => self.batch_ended(&id, ImportPhase::Failed, Some(e)),
                JobEventKind::Cancelled => {
                    self.batch_ended(&id, ImportPhase::Failed, Some("Import cancelled".into()))
                }
                _ => {}
            }
            if terminal {
                self.imports.batches.remove(&event.id);
            }
        }
        if !added.is_empty() {
            if let Err(e) = self.save_preferred_order() {
                self.workspace.push_effect(Effect::Message(format!(
                    "Could not save the Preferred order: {e}"
                )));
            }
            self.finish_adding(&added);
        }
        other
    }

    /// A batch book is in (or could not be): the Library takes it, with its own Import
    /// record, and the batch's report lists it. Returns its Resource id when it joined.
    fn book_arrived(&mut self, batch: &str, event: BookEvent) -> Option<String> {
        let source = self
            .imports
            .report(batch)
            .map(|r| r.source.clone())
            .unwrap_or_default();
        match event {
            BookEvent::Failed(failure) => {
                if let Some(summary) = self.batch_summary(batch) {
                    summary.failed.push(failure);
                }
                None
            }
            BookEvent::Imported(book) => {
                let ImportedBook {
                    name,
                    entry,
                    report,
                    index,
                    updated,
                    col,
                } = *book;
                let id = entry.manifest.resource_id.clone();
                let title = entry.manifest.title.clone();
                let replaced = self.library.get(&id).is_some();
                self.library.add(entry);
                self.remove_older_search_indexes(&id);
                if replaced {
                    self.workspace.push_effect(Effect::Replaced(id.clone()));
                }
                self.imports.reports.insert(
                    id.clone(),
                    ImportRecord {
                        resource_id: id.clone(),
                        source: format!("{name} in {source}"),
                        phase: ImportPhase::Complete,
                        done: 0,
                        total: 0,
                        report: Some(report),
                        index,
                        error: None,
                        origin: None,
                        open_origin: None,
                        books: None,
                        col,
                    },
                );
                self.save_import_record(&id);
                if let Some(summary) = self.batch_summary(batch) {
                    summary.imported.push(BookOutcome {
                        name,
                        resource_id: id.clone(),
                        title,
                        updated,
                    });
                }
                Some(id)
            }
        }
    }

    fn batch_summary(&mut self, batch: &str) -> Option<&mut BookBatchSummary> {
        self.imports.reports.get_mut(batch)?.books.as_mut()
    }

    fn batch_ended(&mut self, batch: &str, phase: ImportPhase, error: Option<String>) {
        let Some(record) = self.imports.reports.get_mut(batch) else {
            return;
        };
        record.phase = phase;
        record.error = error;
        let (imported, failed) = record
            .books
            .as_ref()
            .map_or((0, 0), |b| (b.imported.len(), b.failed.len()));
        let mut message = format!(
            "Imported {imported} book{}",
            if imported == 1 { "" } else { "s" }
        );
        if failed > 0 {
            message.push_str(&format!("; {failed} could not be imported"));
        }
        self.workspace.push_effect(Effect::Message(message));
    }
}
