//! Text Comparison (#18 rules 11–14): the Passage shown as Verse blocks, one row per Bible,
//! aligned on Standard Verses through each Bible's Verse map (#10 rule 9), and a word-level
//! LCS diff of each row against the ★ base Bible over the search normalizer's tokens.

use std::ops::Range;

use obs_model::ResourceType;
use obs_scripture::{Passage, Standard, Verse, VerseMap, Versification};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::library::Library;
use crate::session::Session;
use crate::workspace::{TabId, TabKind, Tool};

/// How many Bibles a new Text Comparison shows: the first ones in Preferred order.
pub const DEFAULT_BIBLES: usize = 5;

/// One Text Comparison Tab's choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    /// The Bibles compared, as resource ids in row order. `None` until the user picks: the
    /// first [`DEFAULT_BIBLES`] Bibles in Preferred order.
    pub bibles: Option<Vec<String>>,
    /// The ★ base Bible the others are diffed against (none chosen: the first row).
    pub base: Option<String>,
    /// A typed range narrowing the whole chapter, shown as a removable chip.
    pub range: Option<Passage>,
    /// Mark the words that differ from the base.
    pub differences: bool,
}

impl Default for Comparison {
    fn default() -> Self {
        Self {
            bibles: None,
            base: None,
            range: None,
            differences: true,
        }
    }
}

impl Comparison {
    /// The Bibles compared, in row order (only those still in the library).
    pub fn bibles(&self, library: &Library) -> Vec<String> {
        match &self.bibles {
            None => default_bibles(library),
            Some(ids) => ids
                .iter()
                .filter(|id| {
                    library
                        .get(id)
                        .is_some_and(|e| e.manifest.resource_type == ResourceType::Bible)
                })
                .cloned()
                .collect(),
        }
    }

    /// The row of the ★ base Bible among `bibles`.
    pub fn base_index(&self, bibles: &[String]) -> Option<usize> {
        if bibles.is_empty() {
            return None;
        }
        Some(
            self.base
                .as_ref()
                .and_then(|b| bibles.iter().position(|id| id == b))
                .unwrap_or(0),
        )
    }

    /// The Standard Verses shown: the typed range, else the whole chapter of `location`.
    pub fn extent(&self, location: Verse) -> Vec<Verse> {
        let passage = self.range.or_else(|| {
            Passage::chapter(&Standard, location.book, location.chapter)
                .or_else(|| Passage::book(&Standard, location.book))
        });
        passage.map_or_else(Vec::new, |p| p.verses(&Standard).collect())
    }
}

/// The first [`DEFAULT_BIBLES`] Bibles in Preferred order.
pub fn default_bibles(library: &Library) -> Vec<String> {
    library
        .of_type(ResourceType::Bible)
        .take(DEFAULT_BIBLES)
        .map(|e| e.manifest.resource_id.clone())
        .collect()
}

/// Whether a range typed into the reference box narrows the comparison: anything longer than
/// one Verse and other than one whole chapter.
pub fn narrows(passage: Passage) -> bool {
    let whole_chapter = passage.start().book == passage.end().book
        && passage.start().chapter == passage.end().chapter
        && Passage::chapter(&Standard, passage.start().book, passage.start().chapter)
            == Some(passage);
    !passage.is_single_verse() && !whole_chapter
}

/// Whether `verse` lies in a chapter the range touches (a jump elsewhere clears the range).
pub(crate) fn in_range_chapters(range: Passage, verse: Verse) -> bool {
    let key = |v: Verse| (v.book, v.chapter);
    key(range.start()) <= key(verse) && key(verse) <= key(range.end())
}

// ------------------------------------------------------------------------------- word diff

/// How one row's text differs from the base row's, as byte positions in the row's text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WordDiff {
    /// Words that differ from the base, or are not in it (tinted).
    pub changed: Vec<Range<usize>>,
    /// Where base words are missing from this row (marked ‸).
    pub missing: Vec<usize>,
}

/// A word-level LCS diff of `text` against `base`, over normalized tokens (case, punctuation
/// and diacritics ignored, #18 rule 14). A run of words in `text` with no counterpart in the
/// base is changed; a run of base words with nothing in `text` between the same matches is
/// missing, marked before the next word of `text`.
pub fn word_diff(base: &str, text: &str) -> WordDiff {
    let a = obs_model::tokenize(base);
    let b = obs_model::tokenize(text);
    let (n, m) = (a.len(), b.len());
    // lcs[i][j]: LCS length of a[i..] and b[j..].
    let mut lcs = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[at(i, j)] = if a[i].text == b[j].text {
                lcs[at(i + 1, j + 1)] + 1
            } else {
                lcs[at(i + 1, j)].max(lcs[at(i, j + 1)])
            };
        }
    }
    // Where a missing run of base words is marked: before row word `j`, or after the last.
    let missing_at = |j: usize| match b.get(j) {
        Some(t) => t.range.start,
        None => b.last().map_or(0, |t| t.range.end),
    };
    let mut out = WordDiff::default();
    let (mut i, mut j) = (0, 0);
    // Base words and row words skipped since the last match.
    let (mut skipped_base, mut skipped_row) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i].text == b[j].text {
            if skipped_base > 0 && skipped_row == 0 {
                out.missing.push(missing_at(j));
            }
            skipped_base = 0;
            skipped_row = 0;
            i += 1;
            j += 1;
        } else if j >= m || (i < n && lcs[at(i + 1, j)] >= lcs[at(i, j + 1)]) {
            skipped_base += 1;
            i += 1;
        } else {
            out.changed.push(b[j].range.clone());
            skipped_row += 1;
            j += 1;
        }
    }
    if skipped_base > 0 && skipped_row == 0 {
        out.missing.push(missing_at(j));
    }
    out
}

// ------------------------------------------------------------------------------------ rows

/// A Bible as Text Comparison reads it.
pub trait ComparedBible {
    /// Its language (BCP 47), to tell which rows can be compared with the base.
    fn language(&self) -> &str;
    /// Its Verse map to the Standard versification (none: it numbers like the Standard).
    fn verse_map(&self) -> Option<&VerseMap>;
    /// The Bible Verse (own numbering) containing `native`, with the last verse number it
    /// covers (`\v 1-2` → 2).
    fn verse_at(&self, native: Verse) -> Option<(Verse, u16)>;
    /// The text of one Bible Verse, whitespace collapsed.
    fn verse_text(&self, native: Verse) -> Option<String>;
}

/// What one Bible shows in one row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// Its own Verses for this Standard Verse (several when they share the row), with the
    /// last verse number each covers, and their text.
    Text {
        natives: Vec<(Verse, u16)>,
        text: String,
    },
    /// Its Verse spans several rows and is shown merged in the row of Standard Verse `into`.
    Merged { into: Verse, native: Verse },
    /// An Absent verse: "— not in this Bible —".
    Absent,
}

impl Cell {
    /// The Bible's own number when it differs from the row's Standard Verse ("32:1").
    pub fn native_label(&self, row: Verse) -> Option<String> {
        let Cell::Text { natives, .. } = self else {
            return None;
        };
        let (first, _) = *natives.first()?;
        let (last, last_n) = *natives.last()?;
        let end = Verse::new(last.book, last.chapter, last_n);
        if first == row && end == row {
            return None;
        }
        let num = |v: Verse| {
            if v.is_title() {
                "title".to_string()
            } else {
                v.verse.to_string()
            }
        };
        Some(if end == first {
            format!("{}:{}", first.chapter, num(first))
        } else if end.chapter == first.chapter {
            format!("{}:{}–{}", first.chapter, num(first), num(end))
        } else {
            format!(
                "{}:{}–{}:{}",
                first.chapter,
                num(first),
                end.chapter,
                num(end)
            )
        })
    }
}

/// How a row's words are marked against the base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Marks {
    /// Nothing to mark (differences off, or nothing to compare).
    None,
    /// This is the ★ base row.
    Base,
    /// A row in another language than the base: "not compared".
    NotCompared,
    Diff(WordDiff),
}

/// One Bible's row in a Verse block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComparedCell {
    pub cell: Cell,
    pub marks: Marks,
}

/// One Verse block: a Standard Verse and one row per Bible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerseBlock {
    pub verse: Verse,
    pub cells: Vec<ComparedCell>,
}

/// The primary language subtag, lowercased ("en-US" → "en").
fn primary_language(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// What `bible` shows in the row of Standard Verse `row`. A merged Verse whose first row lies
/// before `first` (the top of the extent) shows its text here instead.
pub fn cell_for(bible: &dyn ComparedBible, row: Verse, first: Verse) -> Cell {
    let text_of = |natives: Vec<(Verse, u16)>| {
        let text = natives
            .iter()
            .filter_map(|(v, _)| bible.verse_text(*v))
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        Cell::Text { natives, text }
    };
    // Where a Verse starting at Standard `start` is shown: its own row, or the top row when
    // it starts above the extent.
    let shown_at = |start: Verse| start.max(first);
    match bible.verse_map() {
        Some(map) => {
            let entries: Vec<_> = map.entries_for(row).collect();
            let here: Vec<(Verse, u16)> = entries
                .iter()
                .filter(|e| e.standard.is_some_and(|p| shown_at(p.start()) == row))
                .map(|e| (e.verse, e.last))
                .collect();
            if !here.is_empty() {
                return text_of(here);
            }
            match entries.first() {
                Some(e) => Cell::Merged {
                    into: e.standard.map_or(row, |p| shown_at(p.start())),
                    native: e.verse,
                },
                None => Cell::Absent,
            }
        }
        None => match bible.verse_at(row) {
            Some((v, last)) if shown_at(v) == row => text_of(vec![(v, last)]),
            Some((v, _)) => Cell::Merged {
                into: shown_at(v),
                native: v,
            },
            None => Cell::Absent,
        },
    }
}

/// The Verse blocks for `extent` (Standard Verses), one row per Bible, marked against the
/// Bible at `base` when `differences` is on.
pub fn verse_blocks(
    extent: &[Verse],
    bibles: &[&dyn ComparedBible],
    base: Option<usize>,
    differences: bool,
) -> Vec<VerseBlock> {
    let Some(&first) = extent.first() else {
        return Vec::new();
    };
    let base_language = base
        .and_then(|b| bibles.get(b))
        .map(|b| primary_language(b.language()));
    extent
        .iter()
        .map(|&verse| {
            let cells: Vec<Cell> = bibles.iter().map(|b| cell_for(*b, verse, first)).collect();
            let base_text = base.and_then(|b| match cells.get(b) {
                Some(Cell::Text { text, .. }) => Some(text.clone()),
                _ => None,
            });
            let cells = cells
                .into_iter()
                .enumerate()
                .map(|(i, cell)| {
                    let marks = match (&cell, differences) {
                        (_, false) => Marks::None,
                        _ if Some(i) == base => Marks::Base,
                        (Cell::Text { text, .. }, true) => {
                            let same_language = base_language.as_deref()
                                == bibles.get(i).map(|b| primary_language(b.language())).as_deref();
                            match &base_text {
                                _ if !same_language => Marks::NotCompared,
                                Some(base) => Marks::Diff(word_diff(base, text)),
                                None => Marks::None,
                            }
                        }
                        _ => Marks::None,
                    };
                    ComparedCell { cell, marks }
                })
                .collect();
            VerseBlock { verse, cells }
        })
        .collect()
}

// -------------------------------------------------------------------------------- commands

const TAB: ParamSpec = ParamSpec {
    name: "tab",
    kind: ParamKind::Integer,
    required: true,
    help: "The Text Comparison Tab",
};
const ID: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::String,
    required: true,
    help: "Resource id of a Bible in the library",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "compare.bible",
        label: "Add or remove a compared Bible",
        shortcut: None,
        params: &[TAB, ID],
        enabled: always,
        run: toggle_bible,
    },
    CommandSpec {
        id: "compare.base",
        label: "Make the ★ base Bible",
        shortcut: None,
        params: &[TAB, ID],
        enabled: always,
        run: set_base,
    },
    CommandSpec {
        id: "compare.differences",
        label: "Show differences",
        shortcut: None,
        params: &[TAB],
        enabled: always,
        run: toggle_differences,
    },
    CommandSpec {
        id: "compare.clear_range",
        label: "Show the whole chapter",
        shortcut: None,
        params: &[TAB],
        enabled: always,
        run: clear_range,
    },
];

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// The Text Comparison Tab named by the `tab` parameter.
fn comparison_tab(command: &'static str, s: &Session, p: &Params<'_>) -> Result<TabId, CommandError> {
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .ok_or_else(|| failed(command, "no tab"))?;
    match s.workspace.tab(tab) {
        Some(t) if t.kind == TabKind::Tool(Tool::TextComparison) => Ok(tab),
        Some(_) => Err(failed(command, "not a Text Comparison Tab")),
        None => Err(failed(command, format!("no Tab {}", tab.0))),
    }
}

fn state(s: &Session, tab: TabId) -> Value {
    let Some(t) = s.workspace.tab(tab) else {
        return Value::Null;
    };
    let c = &t.comparison;
    let bibles = c.bibles(&s.library);
    let base = c.base_index(&bibles).and_then(|i| bibles.get(i)).cloned();
    json!({
        "tab": tab.0,
        "bibles": bibles,
        "base": base,
        "differences": c.differences,
        "range": c.range.map(|r| obs_scripture::format_passage(r, &Standard, obs_scripture::RefStyle::Short)),
    })
}

fn toggle_bible(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "compare.bible";
    let tab = comparison_tab(CMD, s, &p)?;
    let id = p.str("id").unwrap_or_default().to_string();
    if !s
        .library
        .get(&id)
        .is_some_and(|e| e.manifest.resource_type == ResourceType::Bible)
    {
        return Err(failed(CMD, format!("{id} is not a Bible in the library")));
    }
    let mut bibles = s
        .workspace
        .tab(tab)
        .map(|t| t.comparison.bibles(&s.library))
        .unwrap_or_default();
    match bibles.iter().position(|b| *b == id) {
        Some(i) => {
            bibles.remove(i);
        }
        None => bibles.push(id),
    }
    if let Some(t) = s.workspace.tab_mut(tab) {
        t.comparison.bibles = Some(bibles);
    }
    Ok(state(s, tab))
}

fn set_base(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "compare.base";
    let tab = comparison_tab(CMD, s, &p)?;
    let id = p.str("id").unwrap_or_default().to_string();
    let compared = s
        .workspace
        .tab(tab)
        .is_some_and(|t| t.comparison.bibles(&s.library).contains(&id));
    if !compared {
        return Err(failed(CMD, format!("{id} is not being compared")));
    }
    if let Some(t) = s.workspace.tab_mut(tab) {
        t.comparison.base = Some(id);
        t.comparison.differences = true;
    }
    Ok(state(s, tab))
}

fn toggle_differences(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let tab = comparison_tab("compare.differences", s, &p)?;
    if let Some(t) = s.workspace.tab_mut(tab) {
        t.comparison.differences = !t.comparison.differences;
    }
    Ok(state(s, tab))
}

fn clear_range(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let tab = comparison_tab("compare.clear_range", s, &p)?;
    if let Some(t) = s.workspace.tab_mut(tab) {
        t.comparison.range = None;
    }
    Ok(state(s, tab))
}
