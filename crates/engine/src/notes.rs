//! The Notes Tool (#64; #18 rules 7 and 10; #17 rules 7, 13, 18 and 19) and the Search Tool's
//! Notes mode (#16 rules 10 and 13).
//!
//! - **Cards.** [`Session::note_cards`] lists the Annotations of the Notebook chosen in the
//!   Notes Tool's column (or all), in **Bible order** (Standard Passage; Text-range anchors in a
//!   Bible by their own reference, then other Resources, then Notes without an Anchor) or by
//!   **last modified**, narrowed by the Tab header's filter field. Each card carries what the
//!   UI shows: reference, quote, Notebook, and whether its Resource is missing (#17 rule 13).
//! - **Following a card** (#18 rule 10): a Passage anchor goes to the most recently active
//!   Bible Tab, preferring its origin Bible, and that Tab's Link set follows; a Text-range
//!   anchor goes to the most recently active Tab of its own Resource; otherwise a new Tab
//!   opens. Ctrl/Cmd always opens a new Tab.
//! - **Show in text** (#17 rule 18): each Notebook's toggle in the Notes Tool's column is #63's
//!   `notebook.show_in_text` command (`id`, `show`) over the Notebook's `show_in_text` column
//!   ([`Session::set_show_in_text`], undoable); [`Session::hidden_in_text`] lists the Notebooks
//!   with it off.
//! - **Bible references in Note bodies** are found when shown ([`body_references`]), read in
//!   the Standard versification (ADR 0002, #17 rule 7); clicking one runs
//!   `notes.show_passage`.
//! - **Notes search** reads the #16 query grammar against `notes_fts` (one `body` column of
//!   normalized words), newest change first, within the Search Tab's Notebook filter and
//!   optional Passage list. It runs synchronously: the index is written in the same
//!   transaction as each edit, so a word typed a moment ago is found.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::Arc;

use obs_model::ResourceType;
use obs_scripture::{Book, Passage, PassageList, RefStyle, Standard, Verse, format_passage};
use obs_search::{Compiled, Highlighter, NoLexicon};
use serde_json::{Value, json};

use crate::annotations::{Anchor, Annotation, Notebook};
use crate::bible_search::{ErrorField, InlineError, Scope, SearchMode, Snippet, parse_scope};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::library::Library;
use crate::session::Session;
use crate::workspace::{Effect, OpenRequest, TabId, TabKind, Tool};

/// Longest Notes-search snippet, in characters.
pub const SNIPPET_CHARS: usize = 220;

/// Which Annotations the Notes Tool lists.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum NotesFilter {
    #[default]
    All,
    Notebook(String),
    /// **Needs attention** (#17 rule 12): the Orphaned annotations, of every Notebook.
    NeedsAttention,
}

/// The order of the Notes Tool's cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CardOrder {
    /// Canonical order of their anchors (the default, #18 rule 7).
    #[default]
    Bible,
    /// Most recently changed first.
    Modified,
}

impl CardOrder {
    pub fn id(self) -> &'static str {
        match self {
            CardOrder::Bible => "bible",
            CardOrder::Modified => "modified",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [CardOrder::Bible, CardOrder::Modified]
            .into_iter()
            .find(|o| o.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            CardOrder::Bible => "Bible order",
            CardOrder::Modified => "Last modified",
        }
    }
}

/// The Notes Tool's state (it has one Tab at most).
#[derive(Debug, Clone, Default)]
pub struct NotesTool {
    pub filter: NotesFilter,
    pub order: CardOrder,
    /// A card to bring into view, and whether to start editing it (a Note), set by
    /// `notes.open`; the UI takes it.
    pub reveal: Option<(String, bool)>,
}

/// One card of the Notes Tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub annotation: Annotation,
    /// The Notebook's name.
    pub notebook: String,
    /// Where it is anchored: "Genesis 30:1 (TB)", "Esth 10:4 (LXX)", "No anchor".
    pub reference: String,
    /// The quoted text of its Text range, white space collapsed.
    pub quote: Option<String>,
    /// For a Text-range anchor whose Resource left the library: "LXX is not in the library".
    pub missing: Option<String>,
    /// For an Orphaned annotation: why it needs attention.
    pub attention: Option<String>,
}

impl Card {
    /// Whether it is an Orphaned annotation (listed under Needs attention).
    pub fn needs_attention(&self) -> bool {
        self.annotation.is_orphaned()
    }

    /// Whether following it can show its text. An Orphaned annotation's offsets point at
    /// whatever is there now, so it is not followed: it is re-attached instead.
    pub fn followable(&self) -> bool {
        self.annotation.anchor.is_some() && self.missing.is_none() && !self.annotation.is_orphaned()
    }
}

/// A Note found by a Notes search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteHit {
    pub id: String,
    pub notebook: String,
    pub reference: String,
    /// The body, white space collapsed and cut around the first hit, with the hits marked.
    pub snippet: Snippet,
    pub modified_at: i64,
}

/// What a Notes search found, newest change first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotesResults {
    pub hits: Vec<NoteHit>,
}

impl NotesResults {
    pub fn summary(&self) -> String {
        match self.hits.len() {
            1 => "1 note".to_string(),
            n => format!("{n} notes"),
        }
    }
}

/// The Bible references in a Note body: byte ranges and their Passages in the Standard
/// versification, in text order.
pub fn body_references(body: &str) -> Vec<(Range<usize>, PassageList)> {
    obs_formats::scripture_references(body)
}

/// A Bible's own reference as `native_ref` stores it (`Esth.10.4`, `Esth.10.4-Esth.10.6`).
fn native_passage(native: &str) -> Option<Passage> {
    let verse = |s: &str| {
        let mut parts = s.split('.');
        let book = Book::from_osis(parts.next()?)?;
        let chapter = parts.next()?.parse().ok()?;
        let verse = parts.next()?.parse().ok()?;
        Some(Verse::new(book, chapter, verse))
    };
    match native.split_once('-') {
        Some((a, b)) => Passage::new(verse(a)?, verse(b)?).ok(),
        None => verse(native).map(Passage::verse),
    }
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn abbreviation(library: &Library, id: &str) -> Option<String> {
    library.get(id).map(|e| e.manifest.abbreviation.clone())
}

/// Where an Annotation is anchored, as the Notes Tool and Notes search label it.
pub fn anchor_label(library: &Library, anchor: Option<&Anchor>) -> String {
    match anchor {
        None => "No anchor".to_string(),
        Some(Anchor::Passage { passage, origin }) => {
            let text = format_passage(*passage, &Standard, RefStyle::Long);
            match origin
                .as_ref()
                .and_then(|o| abbreviation(library, &o.resource_id))
            {
                Some(abbr) => format!("{text} ({abbr})"),
                None => text,
            }
        }
        Some(Anchor::TextRange { range }) => {
            let abbr = library
                .abbreviation_of(&range.resource_id)
                .map_or_else(|| range.resource_id.clone(), str::to_string);
            match range.native_ref.as_deref() {
                Some(native) => {
                    let text = native_passage(native).map_or_else(
                        || native.to_string(),
                        |p| format_passage(p, &Standard, RefStyle::Short),
                    );
                    format!("{text} ({abbr})")
                }
                None => format!("{abbr}, {}", range.unit),
            }
        }
    }
}

/// The Bible-order sort key: Passage anchors (and Text ranges with their Bible's own
/// reference) by Verse, then other Text ranges by Resource and position, then Notes without
/// an Anchor by creation.
fn bible_key(library: &Library, a: &Annotation) -> (u8, Option<(Verse, Verse)>, String, u32) {
    match &a.anchor {
        Some(Anchor::Passage { passage, .. }) => {
            (0, Some((passage.start(), passage.end())), String::new(), 0)
        }
        Some(Anchor::TextRange { range }) => {
            match range.native_ref.as_deref().and_then(native_passage) {
                Some(p) => (0, Some((p.start(), p.end())), String::new(), range.start),
                None => (
                    1,
                    None,
                    format!(
                        "{}\u{0}{}",
                        abbreviation(library, &range.resource_id)
                            .unwrap_or_else(|| range.resource_id.clone()),
                        range.unit
                    ),
                    range.start,
                ),
            }
        }
        None => (2, None, String::new(), 0),
    }
}

/// `snippet.text` cut to about [`SNIPPET_CHARS`] characters around the first highlight, with
/// "…" where text was left out.
fn window(snippet: Snippet) -> Snippet {
    let Snippet { text, highlights } = snippet;
    if text.chars().count() <= SNIPPET_CHARS {
        return Snippet { text, highlights };
    }
    let first = highlights.first().map_or(0, |r| r.start);
    // Start a few dozen characters before the first hit, at a word boundary.
    let lead = text[..first]
        .char_indices()
        .rev()
        .nth(60)
        .map_or(0, |(i, _)| i);
    let start = if lead == 0 {
        0
    } else {
        text[lead..].find(' ').map_or(lead, |s| lead + s + 1)
    };
    let end = text[start..]
        .char_indices()
        .nth(SNIPPET_CHARS)
        .map_or(text.len(), |(i, _)| start + i);
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    let shift = out.len();
    out.push_str(&text[start..end]);
    let highlights = highlights
        .into_iter()
        .filter(|r| r.start >= start && r.end <= end)
        .map(|r| r.start - start + shift..r.end - start + shift)
        .collect();
    if end < text.len() {
        out.push('…');
    }
    Snippet {
        text: out,
        highlights,
    }
}

/// Whether a Note's anchor lies in a Search scope. Notes without a Passage anchor are only in
/// the Whole Bible scope.
fn in_scope(scope: &Scope, a: &Annotation) -> bool {
    match scope {
        Scope::WholeBible => true,
        Scope::Passages(list) => a
            .anchor
            .as_ref()
            .and_then(Anchor::passage)
            .is_some_and(|p| list.overlaps(p)),
    }
}

impl Session {
    /// How many Annotations each Notebook holds (empty Notebooks left out).
    pub fn notebook_counts(&self) -> Result<Vec<(String, u64)>, String> {
        match &self.user {
            Some(u) => u.notebook_counts(),
            None => Ok(Vec::new()),
        }
    }

    /// The ids of Notebooks whose Show in text is off (#17 rule 18).
    pub fn hidden_in_text(&self) -> BTreeSet<String> {
        self.notebooks()
            .unwrap_or_default()
            .into_iter()
            .filter(|n| !n.show_in_text)
            .map(|n| n.id)
            .collect()
    }

    /// The Notes Tool's cards: the chosen Notebook's Annotations (or all) in the chosen
    /// order, those matching every word of `filter` (in the body, quote, reference or
    /// Notebook name, ignoring case).
    pub fn note_cards(&self, filter: &str) -> Result<Vec<Card>, String> {
        if self.user.is_none() {
            return Ok(Vec::new());
        }
        let notebooks: BTreeMap<String, String> = self
            .notebooks()?
            .into_iter()
            .map(|n: Notebook| (n.id, n.name))
            .collect();
        let only = match &self.notes.filter {
            NotesFilter::All | NotesFilter::NeedsAttention => None,
            NotesFilter::Notebook(id) if notebooks.contains_key(id) => Some(id.as_str()),
            // The Notebook went away (deleted, or undone): show all.
            NotesFilter::Notebook(_) => None,
        };
        let attention_only = self.notes.filter == NotesFilter::NeedsAttention;
        let words: Vec<String> = filter.split_whitespace().map(str::to_lowercase).collect();
        let mut cards: Vec<Card> = self
            .annotations(only)?
            .into_iter()
            .filter(|a| !attention_only || a.is_orphaned())
            .map(|a| self.card(a, &notebooks))
            .filter(|c| {
                if words.is_empty() {
                    return true;
                }
                let hay = format!(
                    "{}\n{}\n{}\n{}",
                    c.annotation.body(),
                    c.quote.as_deref().unwrap_or_default(),
                    c.reference,
                    c.notebook
                )
                .to_lowercase();
                words.iter().all(|w| hay.contains(w.as_str()))
            })
            .collect();
        match self.notes.order {
            CardOrder::Bible => self.sort_in_bible_order(&mut cards),
            CardOrder::Modified => cards.sort_by(|a, b| {
                (b.annotation.modified_at, &b.annotation.id)
                    .cmp(&(a.annotation.modified_at, &a.annotation.id))
            }),
        }
        Ok(cards)
    }

    /// The Notes Tool's card for `a`; `notebooks` maps Notebook ids to names.
    pub(crate) fn card(&self, a: Annotation, notebooks: &BTreeMap<String, String>) -> Card {
        let range = a.anchor.as_ref().and_then(Anchor::text_range);
        let name = |id: &str| {
            self.library
                .abbreviation_of(id)
                .map_or_else(|| id.to_string(), str::to_string)
        };
        let missing = match &a.anchor {
            Some(Anchor::TextRange { range }) if self.library.get(&range.resource_id).is_none() => {
                Some(format!(
                    "{} is not in the library",
                    name(&range.resource_id)
                ))
            }
            _ => None,
        };
        let attention = match &a.anchor {
            Some(Anchor::TextRange { range }) if range.orphaned => Some(format!(
                "⚠ This text was not found in {} after it changed. Select the text in a \
                 reading pane and choose Re-attach here{}.",
                name(&range.resource_id),
                if a.is_note() {
                    ", keep the Note without an anchor, or delete it"
                } else {
                    ", or delete it"
                }
            )),
            _ => None,
        };
        Card {
            notebook: notebooks.get(&a.notebook_id).cloned().unwrap_or_default(),
            reference: anchor_label(&self.library, a.anchor.as_ref()),
            quote: range.map(|r| collapse(&r.quote)),
            missing,
            attention,
            annotation: a,
        }
    }

    /// Sort cards in Bible order (#18 rule 7).
    pub(crate) fn sort_in_bible_order(&self, cards: &mut [Card]) {
        let library = &self.library;
        cards.sort_by_cached_key(|c| {
            (
                bible_key(library, &c.annotation),
                c.annotation.created_at,
                c.annotation.id.clone(),
            )
        });
    }

    /// Open (or focus) the Notes Tool. With `id`, bring that Annotation's card into view (the
    /// list switches to All if its Notebook is not the one chosen) and, for a Note, start
    /// editing it. Returns the Notes Tab.
    pub fn open_in_notes(&mut self, id: Option<&str>, req: OpenRequest) -> Result<TabId, String> {
        let annotation = id.map(|id| self.annotation(id)).transpose()?;
        let tab = self.workspace.open(
            TabKind::Tool(Tool::Notes),
            Tool::Notes.title().to_string(),
            req,
        );
        if let Some(a) = annotation {
            let listed = match &self.notes.filter {
                NotesFilter::All => true,
                NotesFilter::Notebook(n) => *n == a.notebook_id,
                NotesFilter::NeedsAttention => a.is_orphaned(),
            };
            if !listed {
                self.notes.filter = NotesFilter::All;
            }
            self.notes.reveal = Some((a.id.clone(), a.is_note()));
        }
        Ok(tab)
    }

    /// Show a Standard Passage (#18 rule 10): in the most recently active Bible Tab, preferring
    /// Bible `prefer` when it is open, whose Link set follows; otherwise in a new Tab of
    /// `prefer` (or the ★ Bible). `new_tab` (Ctrl/Cmd) always opens a new Tab. Returns how it
    /// was shown (`tab` or `new`) and the Tab.
    pub fn show_passage(
        &mut self,
        passage: Passage,
        prefer: Option<&str>,
        new_tab: bool,
    ) -> Result<(&'static str, TabId), String> {
        let library = &self.library;
        let is_bible = |id: &str| {
            library
                .get(id)
                .is_some_and(|e| e.manifest.resource_type == ResourceType::Bible)
        };
        let prefer = prefer.filter(|id| is_bible(id)).map(str::to_string);
        if !new_tab {
            let existing = prefer
                .as_ref()
                .and_then(|id| {
                    let kind = TabKind::Resource(id.clone());
                    self.workspace.most_recent(|t| t.kind == kind)
                })
                .or_else(|| {
                    self.workspace
                        .most_recent(|t| matches!(&t.kind, TabKind::Resource(id) if is_bible(id)))
                });
            if let Some(t) = existing {
                self.workspace.focus(t);
                self.workspace.navigate_standard(t, passage.start());
                return Ok(("tab", t));
            }
        }
        let bible = prefer
            .or_else(|| {
                self.library
                    .preferred(ResourceType::Bible)
                    .map(|e| e.manifest.resource_id.clone())
            })
            .ok_or("Add a Bible to your library to read this Passage")?;
        let t = self.open_resource_tab(&bible, new_tab)?;
        // The new Tab's Verse map is known once the UI has loaded it.
        self.workspace.push_effect(Effect::NavigateStandard {
            tab: t,
            verse: passage.start(),
        });
        Ok(("new", t))
    }

    fn open_resource_tab(&mut self, id: &str, split: bool) -> Result<TabId, String> {
        let out = self
            .run("resource.open", &json!({ "id": id, "split": split }))
            .map_err(|e| e.to_string())?;
        out["tab"]
            .as_u64()
            .map(TabId)
            .ok_or_else(|| "no Tab opened".to_string())
    }

    /// Follow an Annotation's anchor (#18 rule 10). Returns how (`tab` or `new`) and the Tab.
    pub fn follow_annotation(
        &mut self,
        id: &str,
        new_tab: bool,
    ) -> Result<(&'static str, TabId), String> {
        let a = self.annotation(id)?;
        match a.anchor {
            None => Err("This Note is not anchored to any text".into()),
            Some(Anchor::Passage { passage, origin }) => self.show_passage(
                passage,
                origin.as_ref().map(|o| o.resource_id.as_str()),
                new_tab,
            ),
            Some(Anchor::TextRange { range }) => {
                let name = self
                    .library
                    .abbreviation_of(&range.resource_id)
                    .map_or_else(|| range.resource_id.clone(), str::to_string);
                if self.library.get(&range.resource_id).is_none() {
                    return Err(format!("{name} is not in the library"));
                }
                // Its offsets point at whatever is there now: never shown as if it were its
                // text (#17 rule 12).
                if range.orphaned {
                    return Err(format!(
                        "This text was not found in {name} after it changed: re-attach it"
                    ));
                }
                let kind = TabKind::Resource(range.resource_id.clone());
                let existing = (!new_tab)
                    .then(|| self.workspace.most_recent(|t| t.kind == kind))
                    .flatten();
                let (how, t) = match existing {
                    Some(t) => {
                        self.workspace.focus(t);
                        ("tab", t)
                    }
                    None => ("new", self.open_resource_tab(&range.resource_id, new_tab)?),
                };
                self.workspace.push_effect(Effect::RevealText {
                    tab: t,
                    unit: range.unit.clone(),
                    byte: range.start as usize,
                });
                Ok((how, t))
            }
        }
    }

    /// Run a Notes search in a Search Tab (its query, Notebook filter and Passage list).
    pub(crate) fn run_notes_search(&mut self, tab: TabId) -> Option<InlineError> {
        let st = self.search_tab(tab)?.clone();
        let query = st.query.trim().to_string();
        let parsed = match obs_search::parse(&query) {
            Ok(q) => q,
            Err(e) => {
                return Some(InlineError::new(
                    ErrorField::Query,
                    e.kind.to_string(),
                    Some(e.span),
                ));
            }
        };
        let compiled = match obs_search::compile_notes(&parsed) {
            Ok(c) => c,
            Err(e) => {
                return Some(InlineError::new(
                    ErrorField::Query,
                    e.kind.to_string(),
                    Some(e.span),
                ));
            }
        };
        let scope = match parse_scope(&st.scope) {
            Ok(s) => s,
            Err(e) => return Some(e),
        };
        let Some(user) = self.user.as_deref() else {
            return Some(InlineError::new(
                ErrorField::Search,
                "Notes are kept in the User database, which is not open",
                None,
            ));
        };
        let ids = match compiled {
            Compiled::Match(m) => match user.search_notes(&m) {
                Ok(ids) => ids,
                Err(e) => return Some(InlineError::new(ErrorField::Search, e, None)),
            },
            Compiled::Nothing => Vec::new(),
        };
        let (all, notebooks) = match (user.annotations(None), user.notebooks()) {
            (Ok(a), Ok(n)) => (a, n),
            (Err(e), _) | (_, Err(e)) => {
                return Some(InlineError::new(ErrorField::Search, e, None));
            }
        };
        let notebook = st
            .notebook
            .filter(|id| notebooks.iter().any(|n| n.id == *id));
        let names: BTreeMap<&str, &str> = notebooks
            .iter()
            .map(|n| (n.id.as_str(), n.name.as_str()))
            .collect();
        let by_id: BTreeMap<&str, &Annotation> = all.iter().map(|a| (a.id.as_str(), a)).collect();
        let highlighter = Highlighter::new(&parsed, &NoLexicon);
        let hits: Vec<NoteHit> = ids
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied())
            .filter(|a| notebook.as_deref().is_none_or(|n| a.notebook_id == n))
            .filter(|a| in_scope(&scope, a))
            .map(|a| {
                let text = collapse(a.body());
                let highlights = highlighter.ranges(&text, Vec::new());
                NoteHit {
                    id: a.id.clone(),
                    notebook: names
                        .get(a.notebook_id.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                    reference: anchor_label(&self.library, a.anchor.as_ref()),
                    snippet: window(Snippet { text, highlights }),
                    modified_at: a.modified_at,
                }
            })
            .collect();
        self.remember_search(SearchMode::Notes, &query);
        if let Some(st) = self.search_tab_mut(tab) {
            st.selected = (!hits.is_empty()).then_some(0);
            st.notes = Some(Arc::new(NotesResults { hits }));
        }
        None
    }
}

// -------------------------------------------------------------------------------- commands

const ID: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::String,
    required: true,
    help: "Annotation id",
};
const SPLIT: ParamSpec = ParamSpec {
    name: "split",
    kind: ParamKind::Boolean,
    required: false,
    help: "Open in a new Tab split off to the right (Ctrl/Cmd held)",
};

fn has_user(s: &Session) -> bool {
    s.user.is_some()
}

fn always(_: &Session) -> bool {
    true
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "notes.notebook",
        label: "Show Notebook",
        shortcut: None,
        params: &[ParamSpec {
            name: "value",
            kind: ParamKind::String,
            required: true,
            help: "A Notebook id, `all`, or `attention` (Needs attention: the Orphaned annotations)",
        }],
        enabled: has_user,
        run: run_notebook,
    },
    CommandSpec {
        id: "notes.order",
        label: "Order Notes",
        shortcut: None,
        params: &[ParamSpec {
            name: "value",
            kind: ParamKind::String,
            required: true,
            help: "bible (Bible order) or modified (last modified first)",
        }],
        enabled: always,
        run: run_order,
    },
    CommandSpec {
        id: "notes.open",
        label: "Open in Notes",
        shortcut: None,
        params: &[
            ParamSpec {
                required: false,
                help: "Annotation to show (and, for a Note, edit)",
                ..ID
            },
            SPLIT,
        ],
        enabled: has_user,
        run: run_open,
    },
    CommandSpec {
        id: "notes.follow",
        label: "Go to text",
        shortcut: None,
        params: &[ID, SPLIT],
        enabled: has_user,
        run: run_follow,
    },
    CommandSpec {
        id: "notes.show_passage",
        label: "Show Passage",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "passage",
                kind: ParamKind::String,
                required: true,
                help: "A Bible reference in the Standard numbering, e.g. `Gen 32:24`",
            },
            ParamSpec {
                name: "bible",
                kind: ParamKind::String,
                required: false,
                help: "The Bible to prefer (default: the most recent Bible Tab, else the ★ Bible)",
            },
            SPLIT,
        ],
        enabled: always,
        run: run_show_passage,
    },
    CommandSpec {
        id: "search.notebook",
        label: "Search this Notebook",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "tab",
                kind: ParamKind::Integer,
                required: true,
                help: "The Search Tab",
            },
            ParamSpec {
                name: "value",
                kind: ParamKind::String,
                required: true,
                help: "A Notebook id, or `all`",
            },
        ],
        enabled: always,
        run: run_search_notebook,
    },
];

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn run_notebook(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "notes.notebook";
    let v = p.str("value").unwrap_or_default();
    s.notes.filter = if v == "all" {
        NotesFilter::All
    } else if v == "attention" {
        NotesFilter::NeedsAttention
    } else {
        let known = s.notebooks().map_err(|e| failed(CMD, e))?;
        if !known.iter().any(|n| n.id == v) {
            return Err(failed(CMD, format!("There is no Notebook {v}")));
        }
        NotesFilter::Notebook(v.to_string())
    };
    Ok(json!({ "notebook": v }))
}

fn run_order(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "notes.order";
    let v = p.str("value").unwrap_or_default();
    s.notes.order = CardOrder::from_id(v)
        .ok_or_else(|| failed(CMD, format!("“{v}” is not an order (bible, modified)")))?;
    Ok(json!({ "order": v }))
}

fn run_open(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let req = OpenRequest {
        panel: None,
        split: p.bool("split").unwrap_or(false),
    };
    let tab = s
        .open_in_notes(p.str("id"), req)
        .map_err(|e| failed("notes.open", e))?;
    Ok(json!({ "tab": tab.0 }))
}

fn run_follow(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let (via, tab) = s
        .follow_annotation(
            p.str("id").unwrap_or_default(),
            p.bool("split").unwrap_or(false),
        )
        .map_err(|e| failed("notes.follow", e))?;
    Ok(json!({ "via": via, "tab": tab.0 }))
}

fn run_show_passage(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "notes.show_passage";
    let text = p.str("passage").unwrap_or_default();
    let list = obs_scripture::parse_reference(text, &Standard, None)
        .map_err(|e| failed(CMD, format!("“{text}”: {}", e.kind)))?;
    let passage = *list
        .passages()
        .first()
        .ok_or_else(|| failed(CMD, format!("“{text}” names no Passage")))?;
    let (via, tab) = s
        .show_passage(passage, p.str("bible"), p.bool("split").unwrap_or(false))
        .map_err(|e| failed(CMD, e))?;
    Ok(json!({ "via": via, "tab": tab.0 }))
}

fn run_search_notebook(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "search.notebook";
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .filter(|t| s.search_tab(*t).is_some())
        .ok_or_else(|| failed(CMD, "not a Search Tab"))?;
    let v = p.str("value").unwrap_or_default();
    let notebook = if v == "all" || v.is_empty() {
        None
    } else {
        let known = s.notebooks().map_err(|e| failed(CMD, e))?;
        if !known.iter().any(|n| n.id == v) {
            return Err(failed(CMD, format!("There is no Notebook {v}")));
        }
        Some(v.to_string())
    };
    if let Some(st) = s.search_tab_mut(tab) {
        st.notebook = notebook;
    }
    let error = crate::bible_search::rerun(s, tab);
    Ok(json!({
        "tab": tab.0,
        "error": error.map(|e| e.message),
    }))
}
