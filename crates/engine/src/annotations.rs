//! The annotation service (#61): Notebooks, Notes and Highlights in the User database, the
//! rules that turn a selection into Anchors, and undo.
//!
//! - **Anchors** (ADR 0001, 0002, 0006). A selection in a Bible becomes a Passage anchor on
//!   the Standard Passage its Verses map to, carrying the selection as its origin Text range.
//!   A selection that touches only Unmapped verses (or no Verse at all) becomes a Text-range
//!   anchor, with the Bible's own reference in `native_ref`. Other Resources always get
//!   Text-range anchors. Every Text range captures its quote, ~32 characters of context on
//!   each side and the Resource's content hash.
//! - **Selections across text units** (in a Bible: across Bible books) make one Highlight per
//!   unit, all in one undo step. A Note on such a selection gets one Passage anchor over the
//!   whole selection, without a Text range.
//! - **Undo** is inverse operations: every command writes a list of row [`Change`]s in one
//!   transaction and records it; undo writes the inverted list, redo the list again. Each
//!   write first checks the rows are as expected, so history never lands on rows that
//!   changed underneath it.
//! - **Notebooks**: the default Notebook always exists and cannot be deleted. Deleting another
//!   moves its Annotations to the default (the pre-selected choice) or deletes them.

use obs_model::{ResourceType, UnitId, UnitKind};
use obs_scripture::{Passage, Standard, Verse, Versification, parse_reference};
use obs_store::annotation::{inverse, new_id_at, now_ms, quote_with_context};
use obs_store::{ResourceStore, StoreError};
use serde_json::{Value, json};

pub use obs_store::{
    Anchor, Annotation, CONTEXT_CHARS, Change, Content, HighlightStyle, Notebook, Row, TextRange,
};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::session::Session;

/// Undo steps kept; older ones are forgotten.
pub const UNDO_LIMIT: usize = 500;

/// Longest Notebook name, in characters.
pub const MAX_NOTEBOOK_NAME: usize = 100;

/// User database preference holding the id of the Notebook last chosen for a new Annotation
/// (#18 rule 8: new Annotations go into the Notebook last used).
pub const LAST_NOTEBOOK_PREF: &str = "annotations.last_notebook";

// ------------------------------------------------------------------------------------ undo

/// One undoable step: the row Changes one command made, applied together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoStep {
    /// What the step did, for "Undo …" menu labels: "Highlight", "Delete Notebook".
    pub label: String,
    pub changes: Vec<Change>,
}

/// The undo and redo stacks of a Session.
#[derive(Debug, Clone, Default)]
pub struct UndoHistory {
    undo: Vec<UndoStep>,
    redo: Vec<UndoStep>,
}

impl UndoHistory {
    /// The step Undo would revert.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    /// The step Redo would apply again.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    fn push(&mut self, step: UndoStep) {
        if self.undo.len() >= UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(step);
        self.redo.clear();
    }
}

// ------------------------------------------------------------------------------- selection

/// A position in a Resource: a text unit and a UTF-8 byte offset into its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPos {
    pub unit: UnitId,
    pub byte: usize,
}

/// What the user selected in one Resource, in document coordinates. The ends may be given in
/// either order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub resource_id: String,
    pub start: TextPos,
    pub end: TextPos,
}

/// One text unit's part of a selection, with what it maps to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// The selected text in this unit, with quote and context.
    pub range: TextRange,
    /// In a Bible: the Standard Passage spanning the mapped Verses the range touches; `None`
    /// when it touches only Unmapped verses or no Verse.
    pub standard: Option<Passage>,
    /// Whether the Resource is a Bible.
    pub in_bible: bool,
}

impl Segment {
    /// The Anchor this part of a selection gets on its own.
    pub fn anchor(&self) -> Anchor {
        match self.standard {
            Some(passage) if self.in_bible => Anchor::Passage {
                passage,
                origin: Some(self.range.clone()),
            },
            _ => Anchor::TextRange {
                range: self.range.clone(),
            },
        }
    }
}

/// The Anchor of a Note on a selection: a single unit's own Anchor; across units (Bible
/// books) one Passage anchor over the whole selection without a Text range, or, when no
/// part maps to the Standard versification, the first part's Text range.
pub fn note_anchor(segments: &[Segment]) -> Option<Anchor> {
    let first = segments.first()?;
    if segments.len() == 1 {
        return Some(first.anchor());
    }
    let mapped: Vec<Passage> = segments.iter().filter_map(|s| s.standard).collect();
    match (
        mapped.iter().map(|p| p.start()).min(),
        mapped.iter().map(|p| p.end()).max(),
    ) {
        (Some(start), Some(end)) if first.in_bible => {
            Passage::new(start, end)
                .ok()
                .map(|passage| Anchor::Passage {
                    passage,
                    origin: None,
                })
        }
        _ => Some(Anchor::TextRange {
            range: first.range.clone(),
        }),
    }
}

/// The Standard Passage of one Bible Verse (own numbering, covering up to verse `last`):
/// its Verse map entry (`mapped`), or, for a Bible without a map, the Verse itself when the
/// Standard versification has it. `None` for an Unmapped verse.
fn standard_of(mapped: Option<Option<Passage>>, native: Verse, last: u16) -> Option<Passage> {
    match mapped {
        Some(standard) => standard,
        None => {
            let end = Verse::new(native.book, native.chapter, last.max(native.verse));
            (Standard.contains(native) && Standard.contains(end))
                .then(|| Passage::new(native, end).ok())
                .flatten()
        }
    }
}

/// The bytes of `range` in `text` from its first letter or digit to its last: a Verse
/// without the white space and punctuation at its ends. Empty (at the start) when there is
/// none.
fn words_of(text: &str, range: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let Some(piece) = text.get(range.clone()) else {
        return range.start..range.start;
    };
    let word = |c: char| c.is_alphanumeric();
    match (piece.find(word), piece.rfind(word)) {
        (Some(a), Some(b)) => {
            let last = piece[b..].chars().next().map_or(0, char::len_utf8);
            range.start + a..range.start + b + last
        }
        _ => range.start..range.start,
    }
}

fn store_err(e: StoreError) -> String {
    e.to_string()
}

/// Split a selection into one [`Segment`] per text unit, capturing quote and context and, in a
/// Bible, the Standard Passage each part maps to. Footnote bodies between the ends are
/// skipped (they are not shown in the text), and whitespace at the ends of each part is left
/// out.
pub fn segments(store: &dyn ResourceStore, sel: &Selection) -> Result<Vec<Segment>, String> {
    let manifest = store.manifest();
    let in_bible = manifest.resource_type == ResourceType::Bible;
    let ids = store.unit_ids().map_err(store_err)?;
    let at = |p: &TextPos| {
        ids.iter()
            .position(|u| *u == p.unit)
            .ok_or_else(|| format!("{} has no text unit `{}`", manifest.abbreviation, p.unit))
    };
    let (mut a, mut b) = ((at(&sel.start)?, &sel.start), (at(&sel.end)?, &sel.end));
    if (b.0, b.1.byte) < (a.0, a.1.byte) {
        std::mem::swap(&mut a, &mut b);
    }
    let mut out = Vec::new();
    for (i, id) in ids.iter().enumerate().take(b.0 + 1).skip(a.0) {
        let (kind, text) = store
            .unit_text(id)
            .map_err(store_err)?
            .ok_or_else(|| format!("text unit `{id}` vanished"))?;
        if i != a.0 && i != b.0 && kind == UnitKind::Note {
            continue;
        }
        let text = text.as_str();
        let mut s = if i == a.0 { a.1.byte } else { 0 };
        let mut e = if i == b.0 { b.1.byte } else { text.len() };
        for at in [s, e] {
            if !text.is_char_boundary(at) {
                return Err(format!("byte {at} is not a character position in `{id}`"));
            }
        }
        let piece = text.get(s..e).unwrap_or_default();
        s += piece.len() - piece.trim_start().len();
        e -= piece.len() - piece.trim_end().len();
        if s >= e {
            continue;
        }
        let Some((prefix, quote, suffix)) = quote_with_context(text, s, e) else {
            continue;
        };
        // The Verses of this unit that the range touches, and what they map to.
        let touched = if in_bible {
            store.verses_overlapping(id, s..e).map_err(store_err)?
        } else {
            Vec::new()
        };
        let standard: Vec<Passage> = touched
            .iter()
            .filter_map(|(v, mapped)| standard_of(*mapped, v.verse, v.last))
            .collect();
        let span = match (
            standard.iter().map(|p| p.start()).min(),
            standard.iter().map(|p| p.end()).max(),
        ) {
            (Some(start), Some(end)) => Passage::new(start, end).ok(),
            _ => None,
        };
        // Whole Verses: the range reaches from the first touched Verse's first word to the
        // last one's last word (white space and punctuation at a Verse's ends do not count).
        let whole_verses = span.is_some()
            && match (touched.first(), touched.last()) {
                (Some((first, _)), Some((last, _))) => {
                    let core = |v: &obs_model::BibleVerse| words_of(text, v.range());
                    s <= core(first).start && e >= core(last).end
                }
                _ => false,
            };
        let native_ref = match (touched.first(), touched.last()) {
            (Some((first, _)), Some((last, _))) if span.is_none() => {
                let end = Verse::new(last.verse.book, last.verse.chapter, last.last);
                Some(if first.verse == end {
                    first.verse.osis()
                } else {
                    format!("{}-{}", first.verse.osis(), end.osis())
                })
            }
            _ => None,
        };
        let (start, end) = (
            u32::try_from(s).map_err(|_| "selection is too long")?,
            u32::try_from(e).map_err(|_| "selection is too long")?,
        );
        out.push(Segment {
            range: TextRange {
                resource_id: manifest.resource_id.clone(),
                unit: id.clone(),
                start,
                end,
                quote: quote.to_string(),
                prefix: prefix.to_string(),
                suffix: suffix.to_string(),
                content_hash: manifest.content_hash.clone(),
                native_ref,
                whole_verses,
            },
            standard: span,
            in_bible,
        });
    }
    if out.is_empty() {
        return Err("Select some text first".into());
    }
    Ok(out)
}

/// Where a new Note is anchored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteTarget {
    /// No Anchor: a free-standing Note.
    Nothing,
    /// A Standard Passage, without a Text range (a Note made from a reference).
    Passage(Passage),
    Selection(Selection),
}

/// What happens to the Annotations of a Notebook being deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotebookDeletion {
    /// Move them to the default Notebook (the pre-selected choice).
    #[default]
    MoveToDefault,
    DeleteAnnotations,
}

impl NotebookDeletion {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "move" => Some(Self::MoveToDefault),
            "delete" => Some(Self::DeleteAnnotations),
            _ => None,
        }
    }
}

/// A Notebook name as stored: trimmed, non-empty, at most [`MAX_NOTEBOOK_NAME`] characters,
/// without control characters.
fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("A Notebook needs a name".into());
    }
    if name.chars().count() > MAX_NOTEBOOK_NAME {
        return Err(format!(
            "Notebook names are at most {MAX_NOTEBOOK_NAME} characters"
        ));
    }
    if name.chars().any(char::is_control) {
        return Err("Notebook names cannot contain control characters".into());
    }
    Ok(name.to_string())
}

/// The FTS5 query for words typed by the user: every normalized word, quoted, all required.
/// `None` when nothing searchable was typed.
pub fn notes_query(text: &str) -> Option<String> {
    let words: Vec<String> = obs_model::tokenize(text)
        .into_iter()
        .map(|t| format!("\"{}\"", t.text.replace('"', "\"\"")))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

// --------------------------------------------------------------------------------- Session

impl Session {
    fn user_data(&self) -> Result<&dyn crate::persist::UserData, String> {
        self.user
            .as_deref()
            .ok_or_else(|| "There is no User database".to_string())
    }

    /// Bumped by every change to the annotation tables (new, edited, undone or redone) and to
    /// what is shown in text (a Notebook's Show in text, View → Show Highlights), so views can
    /// tell when to reload Annotations.
    pub fn annotations_revision(&self) -> u64 {
        self.annotations_revision
    }

    pub fn undo_history(&self) -> &UndoHistory {
        &self.history
    }

    /// Write `changes` as one undo step labelled `label`.
    fn commit(&mut self, label: &str, changes: Vec<Change>) -> Result<(), String> {
        if changes.is_empty() {
            return Ok(());
        }
        self.user_data()?.apply_changes(&changes)?;
        self.history.push(UndoStep {
            label: label.to_string(),
            changes,
        });
        self.annotations_revision += 1;
        Ok(())
    }

    /// Revert the last step. Returns its label, or `None` if there was nothing to undo. If the
    /// rows changed underneath the history, nothing is written and the history is cleared.
    pub fn undo(&mut self) -> Result<Option<String>, String> {
        let Some(step) = self.history.undo.pop() else {
            return Ok(None);
        };
        if let Err(e) = self.user_data()?.apply_changes(&inverse(&step.changes)) {
            self.history.clear();
            return Err(format!("Could not undo {}: {e}", step.label));
        }
        let label = step.label.clone();
        self.history.redo.push(step);
        self.annotations_revision += 1;
        Ok(Some(label))
    }

    /// Apply the last undone step again. Returns its label, or `None` if there was nothing to
    /// redo.
    pub fn redo(&mut self) -> Result<Option<String>, String> {
        let Some(step) = self.history.redo.pop() else {
            return Ok(None);
        };
        if let Err(e) = self.user_data()?.apply_changes(&step.changes) {
            self.history.clear();
            return Err(format!("Could not redo {}: {e}", step.label));
        }
        let label = step.label.clone();
        self.history.undo.push(step);
        self.annotations_revision += 1;
        Ok(Some(label))
    }

    /// Every Notebook, the default first.
    pub fn notebooks(&self) -> Result<Vec<Notebook>, String> {
        self.user_data()?.notebooks()
    }

    /// The default Notebook.
    pub fn default_notebook(&self) -> Result<Notebook, String> {
        self.notebooks()?
            .into_iter()
            .find(|n| n.is_default)
            .ok_or_else(|| "The User database has no default Notebook".to_string())
    }

    fn notebook(&self, id: &str) -> Result<Notebook, String> {
        self.notebooks()?
            .into_iter()
            .find(|n| n.id == id)
            .ok_or_else(|| format!("There is no Notebook {id}"))
    }

    /// The Notebook new Annotations go into unless another is chosen: the one last used
    /// (a User database preference), or the default when that is unset or gone.
    pub fn last_notebook(&self) -> Result<Notebook, String> {
        let last = self.user_data()?.preference(LAST_NOTEBOOK_PREF)?;
        let notebooks = self.notebooks()?;
        last.and_then(|id| notebooks.iter().find(|n| n.id == id).cloned())
            .or_else(|| notebooks.into_iter().find(|n| n.is_default))
            .ok_or_else(|| "The User database has no default Notebook".to_string())
    }

    /// The Notebook `id`; when `None`, the last used one. See [`Self::remember_notebook`].
    fn notebook_for_new(&self, id: Option<&str>) -> Result<Notebook, String> {
        match id {
            Some(id) => self.notebook(id),
            None => self.last_notebook(),
        }
    }

    /// After a new Annotation went into an explicitly chosen Notebook, remember it as the last
    /// used. Only once the change is applied, so a rejected one leaves the preference alone.
    /// The Annotation is already stored, so failing to remember the choice is not an error.
    fn remember_notebook(&self, chosen: Option<&str>, notebook: &Notebook) {
        if chosen.is_some()
            && let Ok(user) = self.user_data()
        {
            let _ = user.set_preference(LAST_NOTEBOOK_PREF, &notebook.id);
        }
    }

    pub fn annotation(&self, id: &str) -> Result<Annotation, String> {
        self.user_data()?
            .annotation(id)?
            .ok_or_else(|| format!("There is no Annotation {id}"))
    }

    /// Annotations in one Notebook, or in all.
    pub fn annotations(&self, notebook: Option<&str>) -> Result<Vec<Annotation>, String> {
        self.user_data()?.annotations(notebook)
    }

    /// Ids of Notes containing every word of `text`, most recently modified first.
    pub fn search_notes(&self, text: &str) -> Result<Vec<String>, String> {
        match notes_query(text) {
            Some(q) => self.user_data()?.search_notes(&q),
            None => Ok(Vec::new()),
        }
    }

    /// Split a selection into per-unit [`Segment`]s, reading the Resource from the library.
    pub fn selection_segments(&self, sel: &Selection) -> Result<Vec<Segment>, String> {
        let entry = self
            .library
            .get(&sel.resource_id)
            .ok_or_else(|| format!("{} is not in the library", sel.resource_id))?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let reader = obs_store::ObrReader::open(&entry.path).map_err(store_err)?;
            segments(&reader, sel)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = entry;
            Err("Opening Resources is not supported here".into())
        }
    }

    /// Highlight a selection: one Highlight per text unit (Bible book) it covers, all in one
    /// undo step, in `notebook` (remembered as the last used) or else the last used one.
    /// Returns the new Highlights.
    pub fn highlight(
        &mut self,
        sel: &Selection,
        style: HighlightStyle,
        notebook_id: Option<&str>,
    ) -> Result<Vec<Annotation>, String> {
        let notebook = self.notebook_for_new(notebook_id)?;
        let parts = self.selection_segments(sel)?;
        let now = now_ms();
        let made: Vec<Annotation> = parts
            .iter()
            .map(|part| Annotation {
                id: new_id_at(now),
                notebook_id: notebook.id.clone(),
                content: Content::Highlight { style },
                anchor: Some(part.anchor()),
                created_at: now,
                modified_at: now,
            })
            .collect();
        let label = if made.len() == 1 {
            "Highlight"
        } else {
            "Highlights"
        };
        self.commit(
            label,
            made.iter()
                .map(|a| Change::insert(Row::Annotation(a.clone())))
                .collect(),
        )?;
        self.remember_notebook(notebook_id, &notebook);
        Ok(made)
    }

    /// Make a Note with `body`, anchored as `target` says, in `notebook` (remembered as the
    /// last used) or else the last used one.
    pub fn add_note(
        &mut self,
        target: &NoteTarget,
        body: &str,
        notebook_id: Option<&str>,
    ) -> Result<Annotation, String> {
        let notebook = self.notebook_for_new(notebook_id)?;
        let anchor = match target {
            NoteTarget::Nothing => None,
            NoteTarget::Passage(passage) => Some(Anchor::Passage {
                passage: *passage,
                origin: None,
            }),
            NoteTarget::Selection(sel) => note_anchor(&self.selection_segments(sel)?),
        };
        let now = now_ms();
        let note = Annotation {
            id: new_id_at(now),
            notebook_id: notebook.id.clone(),
            content: Content::Note {
                body: body.to_string(),
            },
            anchor,
            created_at: now,
            modified_at: now,
        };
        self.commit("Note", vec![Change::insert(Row::Annotation(note.clone()))])?;
        self.remember_notebook(notebook_id, &notebook);
        Ok(note)
    }

    /// Replace an Annotation by an edited copy as one undo step; nothing happens if `edit`
    /// changes nothing. `touch` marks it modified now.
    fn edit_annotation(
        &mut self,
        id: &str,
        label: &str,
        touch: bool,
        edit: impl FnOnce(&mut Annotation) -> Result<(), String>,
    ) -> Result<Annotation, String> {
        let before = self.annotation(id)?;
        let mut after = before.clone();
        edit(&mut after)?;
        if after == before {
            return Ok(before);
        }
        if touch {
            after.modified_at = now_ms().max(before.modified_at);
        }
        self.commit(
            label,
            vec![Change::update(
                Row::Annotation(before),
                Row::Annotation(after.clone()),
            )],
        )?;
        Ok(after)
    }

    /// Replace a Note's text.
    pub fn edit_note(&mut self, id: &str, body: &str) -> Result<Annotation, String> {
        self.edit_annotation(id, "Edit Note", true, |a| match &mut a.content {
            Content::Note { body: b } => {
                *b = body.to_string();
                Ok(())
            }
            Content::Highlight { .. } => Err("Only a Note has text to edit".into()),
        })
    }

    /// Change a Highlight's style.
    pub fn set_highlight_style(
        &mut self,
        id: &str,
        style: HighlightStyle,
    ) -> Result<Annotation, String> {
        self.edit_annotation(id, "Highlight style", true, |a| match &mut a.content {
            Content::Highlight { style: s } => {
                *s = style;
                Ok(())
            }
            Content::Note { .. } => Err("Only a Highlight has a style".into()),
        })
    }

    /// Move an Annotation to another Notebook. Its modified time stays: moving does not
    /// change what it says.
    pub fn move_annotation(&mut self, id: &str, notebook: &str) -> Result<Annotation, String> {
        let notebook = self.notebook(notebook)?;
        self.edit_annotation(id, "Move to Notebook", false, |a| {
            a.notebook_id = notebook.id;
            Ok(())
        })
    }

    pub fn delete_annotation(&mut self, id: &str) -> Result<(), String> {
        let a = self.annotation(id)?;
        let label = if a.is_note() {
            "Delete Note"
        } else {
            "Delete Highlight"
        };
        self.commit(label, vec![Change::delete(Row::Annotation(a))])
    }

    /// Check a Notebook name is usable and not taken by another Notebook (ignoring case).
    fn notebook_name(&self, name: &str, except: Option<&str>) -> Result<String, String> {
        let name = clean_name(name)?;
        let taken = self
            .notebooks()?
            .into_iter()
            .any(|n| Some(n.id.as_str()) != except && n.name.to_lowercase() == name.to_lowercase());
        if taken {
            return Err(format!("There is already a Notebook named “{name}”"));
        }
        Ok(name)
    }

    pub fn create_notebook(&mut self, name: &str) -> Result<Notebook, String> {
        let name = self.notebook_name(name, None)?;
        let now = now_ms();
        let nb = Notebook {
            id: new_id_at(now),
            name,
            is_default: false,
            show_in_text: true,
            created_at: now,
            modified_at: now,
        };
        self.commit(
            "New Notebook",
            vec![Change::insert(Row::Notebook(nb.clone()))],
        )?;
        Ok(nb)
    }

    pub fn rename_notebook(&mut self, id: &str, name: &str) -> Result<Notebook, String> {
        let before = self.notebook(id)?;
        let name = self.notebook_name(name, Some(id))?;
        if name == before.name {
            return Ok(before);
        }
        let after = Notebook {
            name,
            modified_at: now_ms().max(before.modified_at),
            ..before.clone()
        };
        self.commit(
            "Rename Notebook",
            vec![Change::update(
                Row::Notebook(before),
                Row::Notebook(after.clone()),
            )],
        )?;
        Ok(after)
    }

    /// Turn a Notebook's **Show in text** on or off (`None`: the other way round). One undo
    /// step, like every Notebook change; its modified time stays, since what it holds did not
    /// change.
    pub fn set_show_in_text(&mut self, id: &str, show: Option<bool>) -> Result<Notebook, String> {
        let before = self.notebook(id)?;
        let show = show.unwrap_or(!before.show_in_text);
        if show == before.show_in_text {
            return Ok(before);
        }
        let after = Notebook {
            show_in_text: show,
            ..before.clone()
        };
        let label = if show { "Show in text" } else { "Hide in text" };
        self.commit(
            label,
            vec![Change::update(
                Row::Notebook(before),
                Row::Notebook(after.clone()),
            )],
        )?;
        Ok(after)
    }

    /// The Annotations to draw in Resource text: those of Notebooks with Show in text on, and
    /// without Highlights while View → Show Highlights is off. Oldest first.
    pub fn annotations_in_text(&self) -> Result<Vec<Annotation>, String> {
        let shown: std::collections::HashSet<String> = self
            .notebooks()?
            .into_iter()
            .filter(|n| n.show_in_text)
            .map(|n| n.id)
            .collect();
        let highlights = self.appearance.show_highlights;
        Ok(self
            .annotations(None)?
            .into_iter()
            .filter(|a| shown.contains(&a.notebook_id) && (highlights || a.is_note()))
            .collect())
    }

    /// Delete a Notebook (never the default), moving its Annotations to the default
    /// Notebook or deleting them, as one undo step. Returns how many Annotations it held.
    pub fn delete_notebook(&mut self, id: &str, then: NotebookDeletion) -> Result<usize, String> {
        let nb = self.notebook(id)?;
        if nb.is_default {
            return Err("The default Notebook cannot be deleted".into());
        }
        let default = self.default_notebook()?;
        let members = self.annotations(Some(id))?;
        let n = members.len();
        let mut changes: Vec<Change> = members
            .into_iter()
            .map(|a| match then {
                NotebookDeletion::MoveToDefault => {
                    let moved = Annotation {
                        notebook_id: default.id.clone(),
                        ..a.clone()
                    };
                    Change::update(Row::Annotation(a), Row::Annotation(moved))
                }
                NotebookDeletion::DeleteAnnotations => Change::delete(Row::Annotation(a)),
            })
            .collect();
        changes.push(Change::delete(Row::Notebook(nb)));
        self.commit("Delete Notebook", changes)?;
        Ok(n)
    }
}

// -------------------------------------------------------------------------------- commands

const RESOURCE: ParamSpec = ParamSpec {
    name: "resource",
    kind: ParamKind::String,
    required: false,
    help: "Resource id the selection is in",
};
const START_UNIT: ParamSpec = ParamSpec {
    name: "start_unit",
    kind: ParamKind::String,
    required: false,
    help: "Text unit where the selection starts",
};
const START: ParamSpec = ParamSpec {
    name: "start",
    kind: ParamKind::Integer,
    required: false,
    help: "UTF-8 byte offset of the selection start in its unit",
};
const END_UNIT: ParamSpec = ParamSpec {
    name: "end_unit",
    kind: ParamKind::String,
    required: false,
    help: "Text unit where the selection ends (default: the start unit)",
};
const END: ParamSpec = ParamSpec {
    name: "end",
    kind: ParamKind::Integer,
    required: false,
    help: "UTF-8 byte offset just past the selection end in its unit",
};
const NOTEBOOK: ParamSpec = ParamSpec {
    name: "notebook",
    kind: ParamKind::String,
    required: false,
    help: "Notebook id (default: the Notebook last used, else the default Notebook)",
};
const ID: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::String,
    required: true,
    help: "Annotation id",
};
const NOTEBOOK_ID: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::String,
    required: true,
    help: "Notebook id",
};
const STYLE: ParamSpec = ParamSpec {
    name: "style",
    kind: ParamKind::String,
    required: false,
    help: "Highlight style: yellow (default), green, blue, pink or red_underline",
};
const NAME: ParamSpec = ParamSpec {
    name: "name",
    kind: ParamKind::String,
    required: true,
    help: "Notebook name",
};

fn has_user(s: &Session) -> bool {
    s.user.is_some()
}

fn can_undo(s: &Session) -> bool {
    s.user.is_some() && s.history.undo_label().is_some()
}

fn can_redo(s: &Session) -> bool {
    s.user.is_some() && s.history.redo_label().is_some()
}

/// Ctrl+Z and Ctrl+Shift+Z are the undo keys of text fields too, so the Shell leaves them to a
/// focused text field (a Note being written, a reference box) instead of running these
/// commands (#62). Ctrl+1…5 and Ctrl+Shift+N act on the reading-pane selection, which only the
/// Shell knows, so the Shell binds them itself.
pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "edit.undo",
        label: "Undo",
        shortcut: Some("Ctrl+Z"),
        params: &[],
        enabled: can_undo,
        run: run_undo,
    },
    CommandSpec {
        id: "edit.redo",
        label: "Redo",
        shortcut: Some("Ctrl+Shift+Z"),
        params: &[],
        enabled: can_redo,
        run: run_redo,
    },
    CommandSpec {
        id: "annotation.highlight",
        label: "Highlight",
        shortcut: None,
        params: &[
            ParamSpec {
                required: true,
                ..RESOURCE
            },
            ParamSpec {
                required: true,
                ..START_UNIT
            },
            ParamSpec {
                required: true,
                ..START
            },
            END_UNIT,
            ParamSpec {
                required: true,
                ..END
            },
            STYLE,
            NOTEBOOK,
        ],
        enabled: has_user,
        run: run_highlight,
    },
    CommandSpec {
        id: "annotation.note",
        label: "Note",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "body",
                kind: ParamKind::String,
                required: false,
                help: "The Note's text",
            },
            RESOURCE,
            START_UNIT,
            START,
            END_UNIT,
            END,
            ParamSpec {
                name: "passage",
                kind: ParamKind::String,
                required: false,
                help: "Anchor on this Passage (Standard versification) instead of a selection",
            },
            NOTEBOOK,
        ],
        enabled: has_user,
        run: run_note,
    },
    CommandSpec {
        id: "annotation.edit",
        label: "Edit Note",
        shortcut: None,
        params: &[
            ID,
            ParamSpec {
                name: "body",
                kind: ParamKind::String,
                required: true,
                help: "The Note's new text",
            },
        ],
        enabled: has_user,
        run: run_edit,
    },
    CommandSpec {
        id: "annotation.style",
        label: "Highlight style",
        shortcut: None,
        params: &[
            ID,
            ParamSpec {
                required: true,
                ..STYLE
            },
        ],
        enabled: has_user,
        run: run_style,
    },
    CommandSpec {
        id: "annotation.move",
        label: "Move to Notebook",
        shortcut: None,
        params: &[
            ID,
            ParamSpec {
                required: true,
                ..NOTEBOOK
            },
        ],
        enabled: has_user,
        run: run_move,
    },
    CommandSpec {
        id: "annotation.delete",
        label: "Delete Annotation",
        shortcut: None,
        params: &[ID],
        enabled: has_user,
        run: run_delete,
    },
    CommandSpec {
        id: "annotation.get",
        label: "Show Annotation",
        shortcut: None,
        params: &[ID],
        enabled: has_user,
        run: run_get,
    },
    CommandSpec {
        id: "annotation.list",
        label: "List Annotations",
        shortcut: None,
        params: &[ParamSpec {
            help: "Only this Notebook (default: all)",
            ..NOTEBOOK
        }],
        enabled: has_user,
        run: run_list,
    },
    CommandSpec {
        id: "annotation.search_notes",
        label: "Search Notes",
        shortcut: None,
        params: &[ParamSpec {
            name: "text",
            kind: ParamKind::String,
            required: true,
            help: "Words every Note found must contain",
        }],
        enabled: has_user,
        run: run_search,
    },
    CommandSpec {
        id: "notebook.create",
        label: "New Notebook…",
        shortcut: None,
        params: &[NAME],
        enabled: has_user,
        run: run_notebook_create,
    },
    CommandSpec {
        id: "notebook.rename",
        label: "Rename Notebook…",
        shortcut: None,
        params: &[NOTEBOOK_ID, NAME],
        enabled: has_user,
        run: run_notebook_rename,
    },
    CommandSpec {
        id: "notebook.delete",
        label: "Delete Notebook…",
        shortcut: None,
        params: &[
            NOTEBOOK_ID,
            ParamSpec {
                name: "annotations",
                kind: ParamKind::String,
                required: false,
                help: "What happens to its Annotations: move (to the default Notebook; the default) or delete",
            },
        ],
        enabled: has_user,
        run: run_notebook_delete,
    },
    CommandSpec {
        id: "notebook.show_in_text",
        label: "Show in text",
        shortcut: None,
        params: &[
            NOTEBOOK_ID,
            ParamSpec {
                name: "show",
                kind: ParamKind::Boolean,
                required: false,
                help: "Draw its Annotations in Resource text (default: the other way round)",
            },
        ],
        enabled: has_user,
        run: run_notebook_show_in_text,
    },
    CommandSpec {
        id: "notebook.list",
        label: "List Notebooks",
        shortcut: None,
        params: &[],
        enabled: has_user,
        run: run_notebook_list,
    },
];

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn to_json<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// The selection named by the `resource`, `start_unit`, `start`, `end_unit`, `end` params.
fn selection(cmd: &'static str, p: &Params<'_>) -> Result<Selection, CommandError> {
    let missing = |name: &str| failed(cmd, format!("a selection needs `{name}`"));
    let unit = |name: &'static str| -> Result<UnitId, CommandError> {
        let raw = p.str(name).ok_or_else(|| missing(name))?;
        UnitId::new(raw).map_err(|e| failed(cmd, e))
    };
    let byte = |name: &'static str| -> Result<usize, CommandError> {
        let n = p.i64(name).ok_or_else(|| missing(name))?;
        usize::try_from(n).map_err(|_| failed(cmd, format!("`{name}` must be 0 or more")))
    };
    let start_unit = unit("start_unit")?;
    let end_unit = match p.str("end_unit") {
        Some(_) => unit("end_unit")?,
        None => start_unit.clone(),
    };
    Ok(Selection {
        resource_id: p
            .str("resource")
            .ok_or_else(|| missing("resource"))?
            .to_string(),
        start: TextPos {
            unit: start_unit,
            byte: byte("start")?,
        },
        end: TextPos {
            unit: end_unit,
            byte: byte("end")?,
        },
    })
}

fn style(cmd: &'static str, p: &Params<'_>) -> Result<HighlightStyle, CommandError> {
    match p.str("style") {
        None => Ok(HighlightStyle::Yellow),
        Some(id) => HighlightStyle::from_id(id).ok_or_else(|| {
            failed(
                cmd,
                format!(
                    "“{id}” is not a Highlight style (yellow, green, blue, pink, red_underline)"
                ),
            )
        }),
    }
}

fn run_undo(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    let label = s.undo().map_err(|e| failed("edit.undo", e))?;
    Ok(json!({ "undone": label }))
}

fn run_redo(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    let label = s.redo().map_err(|e| failed("edit.redo", e))?;
    Ok(json!({ "redone": label }))
}

fn run_highlight(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "annotation.highlight";
    let sel = selection(CMD, &p)?;
    let style = style(CMD, &p)?;
    let made = s
        .highlight(&sel, style, p.str("notebook"))
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({
        "ids": made.iter().map(|a| a.id.clone()).collect::<Vec<_>>(),
        "annotations": to_json(&made),
    }))
}

fn run_note(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "annotation.note";
    let selected = ["resource", "start_unit", "start", "end_unit", "end"]
        .iter()
        .any(|n| p.str(n).is_some() || p.i64(n).is_some());
    let target = if selected {
        if p.str("passage").is_some() {
            return Err(failed(CMD, "give a selection or a `passage`, not both"));
        }
        NoteTarget::Selection(selection(CMD, &p)?)
    } else if let Some(text) = p.str("passage") {
        let list = parse_reference(text, &Standard, None).map_err(|e| {
            failed(
                CMD,
                format!("{} at bytes {}..{}", e.kind, e.span.start, e.span.end),
            )
        })?;
        match list.passages() {
            [one] => NoteTarget::Passage(*one),
            _ => return Err(failed(CMD, "a Note is anchored to one Passage")),
        }
    } else {
        NoteTarget::Nothing
    };
    let note = s
        .add_note(
            &target,
            p.str("body").unwrap_or_default(),
            p.str("notebook"),
        )
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "id": note.id, "annotation": to_json(&note) }))
}

fn run_edit(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "annotation.edit";
    let id = p.str("id").unwrap_or_default();
    let a = s
        .edit_note(id, p.str("body").unwrap_or_default())
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "annotation": to_json(&a) }))
}

fn run_style(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "annotation.style";
    let style = style(CMD, &p)?;
    let a = s
        .set_highlight_style(p.str("id").unwrap_or_default(), style)
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "annotation": to_json(&a) }))
}

fn run_move(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "annotation.move";
    let a = s
        .move_annotation(
            p.str("id").unwrap_or_default(),
            p.str("notebook").unwrap_or_default(),
        )
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "annotation": to_json(&a) }))
}

fn run_delete(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "annotation.delete";
    let id = p.str("id").unwrap_or_default();
    s.delete_annotation(id).map_err(|e| failed(CMD, e))?;
    Ok(json!({ "deleted": id }))
}

fn run_get(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let a = s
        .annotation(p.str("id").unwrap_or_default())
        .map_err(|e| failed("annotation.get", e))?;
    Ok(to_json(&a))
}

fn run_list(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "annotation.list";
    if let Some(id) = p.str("notebook") {
        s.notebook(id).map_err(|e| failed(CMD, e))?;
    }
    let list = s
        .annotations(p.str("notebook"))
        .map_err(|e| failed(CMD, e))?;
    Ok(to_json(&list))
}

fn run_search(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let ids = s
        .search_notes(p.str("text").unwrap_or_default())
        .map_err(|e| failed("annotation.search_notes", e))?;
    Ok(json!({ "ids": ids }))
}

fn run_notebook_create(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let nb = s
        .create_notebook(p.str("name").unwrap_or_default())
        .map_err(|e| failed("notebook.create", e))?;
    Ok(json!({ "id": nb.id, "notebook": to_json(&nb) }))
}

fn run_notebook_rename(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let nb = s
        .rename_notebook(
            p.str("id").unwrap_or_default(),
            p.str("name").unwrap_or_default(),
        )
        .map_err(|e| failed("notebook.rename", e))?;
    Ok(json!({ "notebook": to_json(&nb) }))
}

fn run_notebook_delete(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "notebook.delete";
    let then = match p.str("annotations") {
        None => NotebookDeletion::default(),
        Some(v) => NotebookDeletion::from_id(v).ok_or_else(|| {
            failed(
                CMD,
                format!("`annotations` must be move or delete, not “{v}”"),
            )
        })?,
    };
    let id = p.str("id").unwrap_or_default();
    let n = s.delete_notebook(id, then).map_err(|e| failed(CMD, e))?;
    let key = match then {
        NotebookDeletion::MoveToDefault => "moved",
        NotebookDeletion::DeleteAnnotations => "deleted_annotations",
    };
    Ok(json!({ "deleted": id, key: n }))
}

fn run_notebook_show_in_text(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let nb = s
        .set_show_in_text(p.str("id").unwrap_or_default(), p.bool("show"))
        .map_err(|e| failed("notebook.show_in_text", e))?;
    Ok(json!({ "notebook": to_json(&nb) }))
}

fn run_notebook_list(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "notebook.list";
    let user = s.user_data().map_err(|e| failed(CMD, e))?;
    let counts = user.notebook_counts().map_err(|e| failed(CMD, e))?;
    let list: Vec<Value> = s
        .notebooks()
        .map_err(|e| failed(CMD, e))?
        .into_iter()
        .map(|n| {
            let count = counts
                .iter()
                .find(|(id, _)| *id == n.id)
                .map_or(0, |(_, c)| *c);
            json!({
                "id": n.id,
                "name": n.name,
                "default": n.is_default,
                "show_in_text": n.show_in_text,
                "count": count,
            })
        })
        .collect();
    Ok(Value::Array(list))
}
