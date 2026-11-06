//! The Search Tool (#59; #16 rules 9–11 and 14; #18): one Tool with a Bible / Library / Notes
//! switch. Bible mode is implemented here, Library mode (#60) in [`crate::library_search`]
//! on the same Tab state, jobs and commands, and Notes mode in `notes.rs` (#64).
//!
//! A Bible search runs on Enter over the Bibles chosen in the picker (default: the ★ Bible,
//! first in Preferred order) within a Search scope: a Passage list in the Standard
//! versification, mapped through each Bible's Verse map into verse-ordinal ranges (Unmapped
//! verses only with Whole Bible). The search itself and the snippets run as background jobs
//! over the ready Search indexes (#58). Results are a Verse list in Standard canonical order:
//! one row per Standard Verse with the matching text of each Bible, hits highlighted in Rust
//! ([`obs_search::Highlighter`]). Snippets arrive a page at a time, the first page with the
//! results.
//!
//! Opening a hit (#16 rule 14): the Search Tab's Link set navigates to the Verse; without a
//! Link set, the most recently active Tab of that Bible goes there; otherwise a new Tab opens.
//! Ctrl/Cmd always opens a new Tab.
//!
//! Other Tools hand searches over with the `search.run` command (query, scope and Bibles):
//! Word Info's "Search this word" does, and Word Study's "Show all" and "Translated as" (#55,
//! #56) will (`strong:G5485`, `strong:G5485 "favour"`).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Range, RangeInclusive};
use std::sync::{Arc, Mutex};

use obs_model::{MorphemePart, ResourceType, Span, SpanKind, UnitId};
use obs_scripture::{
    Book, Passage, PassageList, RefStyle, Standard, Verse, VerseMap, Versification, format_passage,
    parse_reference,
};
use obs_search::{Highlighter, SearchHit};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::jobs::{JobEvent, JobEventKind, JobId};
use crate::library::{Library, LibraryEntry};
use crate::search_views::{
    AnalysisOutput, AnalysisSort, ChartBy, ResultsFilter, ResultsView, SearchAnalysis,
};
use crate::session::Session;
use crate::workspace::{OpenRequest, TabId, TabKind, Tool};

/// Rows of snippets fetched per job; the first page comes with the results.
pub const PAGE_ROWS: usize = 30;

/// How many recent queries are kept for each mode.
pub const RECENT_LIMIT: usize = 10;

/// The Search Tool's modes (#16 rule 9).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    #[default]
    Bible,
    Library,
    Notes,
}

impl SearchMode {
    pub const ALL: [SearchMode; 3] = [SearchMode::Bible, SearchMode::Library, SearchMode::Notes];

    pub fn id(self) -> &'static str {
        match self {
            SearchMode::Bible => "bible",
            SearchMode::Library => "library",
            SearchMode::Notes => "notes",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            SearchMode::Bible => "Bible",
            SearchMode::Library => "Library",
            SearchMode::Notes => "Notes",
        }
    }

    fn preference_key(self) -> String {
        format!("search.recent.{}", self.id())
    }
}

/// Which field an inline error sits under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorField {
    Query,
    Scope,
    Bibles,
    /// The Library mode's Resource picker.
    Resources,
    /// The search itself failed.
    Search,
}

/// An error shown under its field, never as a dialog (#16).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineError {
    pub field: ErrorField,
    pub message: String,
    /// The bytes of the field's text it is about, to underline.
    pub span: Option<Range<usize>>,
}

impl InlineError {
    pub(crate) fn new(
        field: ErrorField,
        message: impl Into<String>,
        span: Option<Range<usize>>,
    ) -> Self {
        Self {
            field,
            message: message.into(),
            span,
        }
    }
}

/// Parse and compile a query on the UI thread, so its errors show at once (inline, #16).
/// Returns the trimmed query.
pub(crate) fn check_query(query: &str) -> Result<String, InlineError> {
    let query = query.trim().to_string();
    let parsed = obs_search::parse(&query)
        .map_err(|e| InlineError::new(ErrorField::Query, e.kind.to_string(), Some(e.span)))?;
    obs_search::compile(&parsed, &obs_search::NoLexicon)
        .map_err(|e| InlineError::new(ErrorField::Query, e.kind.to_string(), Some(e.span)))?;
    Ok(query)
}

// ----------------------------------------------------------------------------------- scope

/// What the Scope field says when it covers everything.
pub const WHOLE_BIBLE: &str = "Whole Bible";
pub const OLD_TESTAMENT: &str = "Old Testament";
pub const NEW_TESTAMENT: &str = "New Testament";

/// A Bible search's Search scope, in the Standard versification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Everything, including Unmapped verses.
    WholeBible,
    /// These Passages (merged, sorted); Unmapped verses are left out.
    Passages(PassageList),
}

fn books(first: Book, last: Book) -> Option<Passage> {
    Passage::new(
        Standard.first_verse_of_book(first)?,
        Standard.last_verse_of_book(last)?,
    )
    .ok()
}

/// Read the Scope field: empty or "Whole Bible", "Old Testament" / "OT", "New Testament" /
/// "NT", or a Bible reference in the Standard versification ("Rom 1-8; Gal").
pub fn parse_scope(text: &str) -> Result<Scope, InlineError> {
    let t = text.trim();
    let lower = t.to_lowercase();
    let testament = |first, last| {
        books(first, last)
            .map(|p| {
                let mut l = PassageList::new();
                l.push(p);
                Scope::Passages(l)
            })
            .ok_or_else(|| InlineError::new(ErrorField::Scope, "no such books", None))
    };
    match lower.as_str() {
        "" | "whole bible" | "bible" | "all" => Ok(Scope::WholeBible),
        "old testament" | "ot" => testament(Book::Genesis, Book::Malachi),
        "new testament" | "nt" => testament(Book::Matthew, Book::Revelation),
        _ => parse_reference(t, &Standard, None)
            .map(|l| Scope::Passages(l.merged(&Standard)))
            .map_err(|e| InlineError::new(ErrorField::Scope, e.kind.to_string(), Some(e.span))),
    }
}

/// One choice of the Scope presets menu: its label and the text it puts in the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopePreset {
    pub label: String,
    pub value: String,
}

/// The Scope presets (#16 rule 10): Whole Bible, Old and New Testament, and the current book
/// and chapter of `context` (a Standard Verse) when there is one.
pub fn scope_presets(context: Option<Verse>) -> Vec<ScopePreset> {
    let preset = |label: &str, value: &str| ScopePreset {
        label: label.into(),
        value: value.into(),
    };
    let mut out = vec![
        preset(WHOLE_BIBLE, WHOLE_BIBLE),
        preset(OLD_TESTAMENT, OLD_TESTAMENT),
        preset(NEW_TESTAMENT, NEW_TESTAMENT),
    ];
    if let Some(v) = context {
        if let Some(p) = Passage::book(&Standard, v.book) {
            let value = format_passage(p, &Standard, RefStyle::Long);
            out.push(preset(&format!("Current book ({value})"), &value));
        }
        if let Some(p) = Passage::chapter(&Standard, v.book, v.chapter) {
            let value = format_passage(p, &Standard, RefStyle::Long);
            out.push(preset(&format!("Current chapter ({value})"), &value));
        }
    }
    out
}

/// One Bible Verse, as a search maps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutlineVerse {
    /// In the Bible's own numbering, with the last verse number it covers.
    pub native: Verse,
    pub last: u16,
    /// Its Standard Passage; `None` for an Unmapped verse.
    pub standard: Option<Passage>,
}

/// A Bible's Verses in document order (index = verse ordinal = search document id) with
/// their Standard Passages.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BibleOutline {
    pub verses: Vec<OutlineVerse>,
}

impl BibleOutline {
    /// From the Bible's verse list (document order) and its Verse map (none: it numbers
    /// like the Standard).
    pub fn new(verses: &[(Verse, u16)], map: Option<&VerseMap>) -> Self {
        let aligned = map.filter(|m| m.entries().len() == verses.len());
        let verses = verses
            .iter()
            .enumerate()
            .map(|(i, &(native, last))| {
                let identity = Passage::new(native, Verse::new(native.book, native.chapter, last))
                    .ok()
                    .or(Some(Passage::verse(native)));
                let standard = match (aligned, map) {
                    (Some(m), _) => m.entries().get(i).and_then(|e| e.standard),
                    (None, Some(m)) => m.entry(native).map_or(identity, |e| e.standard),
                    (None, None) => identity,
                };
                OutlineVerse {
                    native,
                    last,
                    standard,
                }
            })
            .collect();
        Self { verses }
    }

    /// The verse-ordinal ranges `scope` covers in this Bible: `None` for everything (Whole
    /// Bible, Unmapped verses included); otherwise the Verses whose Standard Passage overlaps
    /// the scope (possibly none).
    pub fn ordinal_ranges(&self, scope: &Scope) -> Option<Vec<RangeInclusive<i64>>> {
        let Scope::Passages(list) = scope else {
            return None;
        };
        let mut out: Vec<RangeInclusive<i64>> = Vec::new();
        for (i, v) in self.verses.iter().enumerate() {
            let inside = v.standard.is_some_and(|p| list.overlaps(p));
            if !inside {
                continue;
            }
            let i = i64::try_from(i).unwrap_or(i64::MAX);
            match out.last_mut() {
                Some(r) if *r.end() + 1 == i => *r = *r.start()..=i,
                _ => out.push(i..=i),
            }
        }
        Some(out)
    }
}

// ------------------------------------------------------------------------------------ rows

/// One Bible's matching Verse in a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowHit {
    /// Index into [`BibleResults::bibles`].
    pub bible: usize,
    /// The Bible Verse in its own numbering, and the last verse number it covers.
    pub native: Verse,
    pub last: u16,
    /// Where its text is: unit and byte range (the Text range space).
    pub unit: UnitId,
    pub range: Range<u32>,
}

/// One row of the Verse list: a Standard Verse and every chosen Bible that matched there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerseRow {
    /// `None` for an Unmapped verse (its row holds one Bible's hit).
    pub standard: Option<Verse>,
    pub hits: Vec<RowHit>,
}

impl VerseRow {
    /// Where the row sits in canonical order.
    pub fn position(&self) -> Option<Verse> {
        self.standard
            .or_else(|| self.hits.first().map(|h| h.native))
    }
}

/// Hits of each Bible (index = Bible) → the Verse list in Standard canonical order. Several
/// Bible Verses that map to one Standard Verse share its row; an Unmapped verse gets a row of
/// its own at its own number, after any mapped row there.
pub fn verse_rows(hits: &[(&[SearchHit], &BibleOutline)]) -> Vec<VerseRow> {
    let mut rows: BTreeMap<(Verse, u8, usize, Verse), VerseRow> = BTreeMap::new();
    for (bible, (hits, outline)) in hits.iter().enumerate() {
        for h in hits.iter() {
            let outlined = usize::try_from(h.document)
                .ok()
                .and_then(|i| outline.verses.get(i));
            let keyed = match &h.key {
                Some(obs_model::Key::Passage(p)) => Some(*p),
                _ => None,
            };
            let (native, last, standard) = match (outlined, keyed) {
                (Some(o), _) => (o.native, o.last, o.standard.map(|p| p.start())),
                (None, Some(p)) => (p.start(), p.end().verse, Some(p.start())),
                (None, None) => continue,
            };
            let key = match standard {
                Some(s) => (s, 0, 0, s),
                None => (native, 1, bible, native),
            };
            rows.entry(key)
                .or_insert_with(|| VerseRow {
                    standard,
                    hits: Vec::new(),
                })
                .hits
                .push(RowHit {
                    bible,
                    native,
                    last,
                    unit: h.unit.clone(),
                    range: h.range.clone(),
                });
        }
    }
    rows.into_values().collect()
}

/// A Bible that was searched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchedBible {
    pub id: String,
    pub abbreviation: String,
    pub rtl: bool,
    /// How many of its Verses matched.
    pub hits: usize,
}

/// What a Bible search found.
#[derive(Debug, Clone, PartialEq)]
pub struct BibleResults {
    pub query: String,
    /// The Scope field as searched ([`WHOLE_BIBLE`] when it was empty).
    pub scope: String,
    pub bibles: Vec<SearchedBible>,
    pub rows: Vec<VerseRow>,
    pub highlighter: Highlighter,
    /// Chosen Bibles not searched: still indexing (abbreviations), or failed (with why).
    pub indexing: Vec<String>,
    pub failed: Vec<(String, String)>,
    /// Bibles searched through an older index while theirs is rebuilt.
    pub stale: Vec<String>,
    /// How long the search took, in milliseconds.
    pub elapsed_ms: u64,
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

impl BibleResults {
    /// Matching Bible Verses over every Bible.
    pub fn hit_count(&self) -> usize {
        self.bibles.iter().map(|b| b.hits).sum()
    }

    /// The summary line: `312 verses · 599 hits · KJV 298 · WEB 301`.
    pub fn summary(&self) -> String {
        let mut parts = vec![plural(self.rows.len(), "verse", "verses")];
        if self.bibles.len() > 1 {
            parts.push(plural(self.hit_count(), "hit", "hits"));
        }
        for b in &self.bibles {
            parts.push(format!("{} {}", b.abbreviation, b.hits));
        }
        parts.join(" · ")
    }

    /// The row's reference: the Standard Verse, or an Unmapped verse in its Bible's numbering.
    pub fn row_reference(&self, row: &VerseRow) -> String {
        match row.standard {
            Some(v) => format_passage(Passage::verse(v), &Standard, RefStyle::Long),
            None => {
                let hit = row.hits.first();
                let v = hit.map_or(Verse::new(Book::Genesis, 1, 1), |h| h.native);
                let abbr = hit
                    .and_then(|h| self.bibles.get(h.bible))
                    .map_or("", |b| b.abbreviation.as_str());
                format!(
                    "{} ({abbr}, no Standard equivalent)",
                    format_passage(Passage::verse(v), &Standard, RefStyle::Long)
                )
            }
        }
    }
}

/// A hit's text as shown: whitespace collapsed, headings left out, with the highlighted byte
/// ranges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snippet {
    pub text: String,
    pub highlights: Vec<Range<usize>>,
}

/// The snippet of `unit_text[range]`: headings inside the range are left out (as the index
/// leaves them out), runs of white space become one space, and `highlighter` marks the hits.
/// `spans` are the unit's spans overlapping the range (Tagged words and headings are used).
pub fn snippet(
    unit_text: &str,
    range: Range<usize>,
    spans: &[Span],
    highlighter: &Highlighter,
) -> Snippet {
    snippet_and_tagged(unit_text, range, spans, highlighter).0
}

/// The Tagged words of a snippet: byte ranges of its text, and morpheme parts.
pub(crate) type TaggedWords<'s> = Vec<(Range<usize>, &'s [MorphemePart])>;

/// [`snippet`], and the Tagged words in it: their byte ranges of the snippet's text and their
/// morpheme parts (for the Analysis view, #146).
pub(crate) fn snippet_and_tagged<'s>(
    unit_text: &str,
    range: Range<usize>,
    spans: &'s [Span],
    highlighter: &Highlighter,
) -> (Snippet, TaggedWords<'s>) {
    let start = range.start.min(unit_text.len());
    let end = range.end.clamp(start, unit_text.len());
    let holes: Vec<Range<usize>> = spans
        .iter()
        .filter(|s| matches!(s.kind, SpanKind::Heading { .. }))
        .map(|s| s.start as usize..s.end as usize)
        .collect();
    let in_hole = |b: usize| holes.iter().any(|h| h.contains(&b));
    // map[i]: the snippet byte where unit byte `start + i` lands.
    let mut map = vec![0usize; end - start + 1];
    let mut text = String::with_capacity(end - start);
    let mut space = false;
    if let Some(slice) = unit_text.get(start..end) {
        for (i, c) in slice.char_indices() {
            let hole = in_hole(start + i);
            let skip = hole || c.is_whitespace();
            if !skip && space {
                text.push(' ');
                space = false;
            }
            map[i..i + c.len_utf8()].fill(text.len());
            if skip {
                // A heading left out separates words like white space.
                space = !text.is_empty();
                continue;
            }
            text.push(c);
        }
    }
    let len = text.len();
    if let Some(last) = map.last_mut() {
        *last = len;
    }
    let to_snippet = |b: usize| map[b.clamp(start, end) - start].min(len);
    let tagged: Vec<(Range<usize>, &[MorphemePart])> = spans
        .iter()
        .filter_map(|s| match &s.kind {
            SpanKind::TaggedWord { parts } => {
                let (a, b) = (to_snippet(s.start as usize), to_snippet(s.end as usize));
                (a < b).then_some((a..b, parts.as_slice()))
            }
            _ => None,
        })
        .collect();
    let highlights = highlighter.ranges(&text, tagged.iter().cloned());
    (Snippet { text, highlights }, tagged)
}

// ------------------------------------------------------------------------- per-Tab state

/// One Search Tab's state: the mode, the fields, and the last search's results.
#[derive(Debug, Clone, Default)]
pub struct SearchTab {
    pub mode: SearchMode,
    /// The query as typed (and last run).
    pub query: String,
    /// The Scope field: a Passage list, or empty for the Whole Bible.
    pub scope: String,
    /// The Bibles chosen in the picker, in order; `None` until the user picks: the ★ Bible.
    pub bibles: Option<Vec<String>>,
    /// Library mode's Search scope: Resource types and single Resources (#60).
    pub library: crate::library_search::LibraryScope,
    pub error: Option<InlineError>,
    /// What a Saved search, Search history entry, Favorite or Visit named in its Search scope
    /// that is gone (#179), said above the results until the next search.
    pub notice: Option<String>,
    /// Bumped each time a Saved search, Search history entry, Favorite or Visit sets every
    /// field, even to what they held: the UI then shows those fields over anything typed.
    pub restored: u64,
    pub results: Option<Arc<BibleResults>>,
    /// The last Library search's results (#60).
    pub library_results: Option<Arc<crate::library_search::LibraryResults>>,
    /// A search is running.
    pub running: bool,
    /// The row chosen with ↑/↓ (Enter opens it).
    pub selected: Option<usize>,
    /// Notes mode: the Notebook searched (`None`: all), and the Notes found (#64).
    pub notebook: Option<String>,
    pub notes: Option<Arc<crate::notes::NotesResults>>,
    /// Snippets by page ([`PAGE_ROWS`] rows): for each row, one per hit (Library rows have
    /// one hit each).
    pub(crate) pages: BTreeMap<usize, Arc<Vec<Vec<Snippet>>>>,
    pub(crate) loading: BTreeSet<usize>,
    /// Bumped by every search; older jobs' results are dropped.
    generation: u64,
    /// How Bible mode's results are shown: Verses, Grid, Chart or Analysis (#146).
    pub view: ResultsView,
    /// The Grid cell or Chart bar the Verse list is narrowed to (#146); `results` then holds
    /// the narrowed results and `unfiltered` every result.
    pub filter: Option<ResultsFilter>,
    pub(crate) unfiltered: Option<Arc<BibleResults>>,
    /// What the Chart's bars count.
    pub chart: ChartBy,
    /// The Analysis table, once made, and its order.
    pub analysis: Option<Arc<SearchAnalysis>>,
    pub analysis_sort: AnalysisSort,
    pub(crate) analysis_order: Arc<Vec<usize>>,
    /// The job making the Analysis.
    pub(crate) analysis_job: Option<JobId>,
}

impl SearchTab {
    /// The chosen Bibles that are still in the library, in picker order; by default the ★
    /// Bible (first in Preferred order).
    pub fn bibles(&self, library: &Library) -> Vec<String> {
        match &self.bibles {
            None => library
                .preferred(ResourceType::Bible)
                .map(|e| vec![e.manifest.resource_id.clone()])
                .unwrap_or_default(),
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

    /// A hit's snippet, once its page has arrived.
    pub fn snippet(&self, row: usize, hit: usize) -> Option<&Snippet> {
        self.pages
            .get(&(row / PAGE_ROWS))?
            .get(row % PAGE_ROWS)?
            .get(hit)
    }

    /// Whether a row's page of snippets is being fetched.
    pub fn is_loading(&self, row: usize) -> bool {
        self.loading.contains(&(row / PAGE_ROWS))
    }

    /// A counter that changes with every search (for views to notice new results).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Rows of the results shown (Verse rows in Bible mode, paragraph rows in Library mode).
    pub fn row_count(&self) -> usize {
        match (&self.results, &self.library_results) {
            (Some(r), _) => r.rows.len(),
            (None, Some(r)) => r.rows.len(),
            (None, None) => 0,
        }
    }

    fn clear_results(&mut self) {
        self.notice = None;
        self.results = None;
        self.library_results = None;
        self.notes = None;
        self.selected = None;
        self.running = false;
        self.filter = None;
        self.unfiltered = None;
        self.chart = ChartBy::Book;
        self.analysis = None;
        self.analysis_order = Arc::default();
        self.analysis_job = None;
        self.reset_pages();
    }

    /// Forget the snippets fetched: the rows shown changed.
    pub(crate) fn reset_pages(&mut self) {
        self.pages.clear();
        self.loading.clear();
        self.generation += 1;
    }

    /// Whether the Analysis is being made.
    pub fn analysis_running(&self) -> bool {
        self.analysis_job.is_some()
    }
}

/// A search job in flight (Bible or Library mode).
// Jobs run only on the desktop.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JobKind {
    Run,
    Page(usize),
    /// The Analysis table (#146).
    Analysis,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct BibleJob {
    pub(crate) tab: TabId,
    pub(crate) generation: u64,
    pub(crate) kind: JobKind,
}

/// The Session's Bible-search bookkeeping: jobs in flight, recent queries and the Verse
/// outlines read so far.
#[derive(Debug, Default)]
pub struct BibleSearches {
    pub(crate) jobs: BTreeMap<JobId, BibleJob>,
    recent: BTreeMap<SearchMode, Vec<String>>,
    /// Outlines by resource id and content hash, shared with the jobs that read them.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    outlines: Arc<Mutex<BTreeMap<String, Arc<BibleOutline>>>>,
    /// Outlines being read ahead of a search, by job.
    warming: BTreeMap<JobId, String>,
    /// Bibles whose index became searchable, waiting for their outline to be read ahead
    /// (one at a time, the ★ Bible first).
    read_ahead: Vec<String>,
    /// Outline keys that could not be read: not read ahead again until the library changes.
    failed_outlines: BTreeSet<String>,
    /// The library (its [`library_fingerprint`]) when `failed_outlines` was last checked.
    failed_library: u64,
    /// Tests of the Search index lifecycle step through jobs one by one: no read-ahead
    /// jobs between them.
    #[cfg(test)]
    pub(crate) no_read_ahead: bool,
}

impl BibleSearches {
    /// Forget the recent queries read so far (another User database was attached).
    pub(crate) fn forget_recent(&mut self) {
        self.recent.clear();
    }

    /// Whether a search or snippet job is running.
    pub fn is_running(&self) -> bool {
        !self.jobs.is_empty()
    }

    /// Whether an outline is being read ahead or waits to be.
    pub fn is_reading_ahead(&self) -> bool {
        !self.warming.is_empty() || !self.read_ahead.is_empty()
    }

    /// Whether the outline with this key (resource id `@` content hash) has been read.
    pub fn has_outline(&self, key: &str) -> bool {
        self.outlines
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(key)
    }

    /// Whether the outline with this key could not be read (and is not read ahead again
    /// until the library changes).
    pub fn outline_failed(&self, key: &str) -> bool {
        self.failed_outlines.contains(key)
    }
}

/// A Bible's outline cache key: its resource id and content hash.
fn outline_key(entry: &LibraryEntry) -> String {
    format!(
        "{}@{}",
        entry.manifest.resource_id, entry.manifest.content_hash
    )
}

/// A cheap fingerprint of what the library holds (each Resource's id, content and file),
/// to notice that it changed.
fn library_fingerprint(library: &Library) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for e in library.entries() {
        e.manifest.resource_id.hash(&mut h);
        e.manifest.content_hash.hash(&mut h);
        e.path.hash(&mut h);
    }
    h.finish()
}

/// What a search job hands back.
struct RunOutput {
    results: Arc<BibleResults>,
    first_page: Vec<Vec<Snippet>>,
}

/// What a snippet job hands back.
pub(crate) struct PageOutput(pub(crate) Vec<Vec<Snippet>>);

/// What a search asks for: the fields of `search.run`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchRequestFields {
    pub query: String,
    /// The Scope field; `None` keeps the Tab's.
    pub scope: Option<String>,
    /// Bibles to search; `None` keeps the Tab's choice.
    pub bibles: Option<Vec<String>>,
    /// Library mode's Resources; `None` keeps the Tab's choice.
    pub library: Option<crate::library_search::LibraryScope>,
}

impl Session {
    /// A Search Tab's state.
    pub fn search_tab(&self, tab: TabId) -> Option<&SearchTab> {
        self.workspace
            .tab(tab)
            .filter(|t| t.kind == TabKind::Tool(Tool::Search))
            .map(|t| &t.search)
    }

    pub(crate) fn search_tab_mut(&mut self, tab: TabId) -> Option<&mut SearchTab> {
        self.workspace
            .tab_mut(tab)
            .filter(|t| t.kind == TabKind::Tool(Tool::Search))
            .map(|t| &mut t.search)
    }

    /// Whether any Bible search or snippet job is running.
    pub fn bible_search_running(&self) -> bool {
        self.bible_search.is_running()
    }

    /// The recent queries of a mode, newest first (kept in the User database, read once).
    pub fn recent_searches(&mut self, mode: SearchMode) -> Vec<String> {
        if let Some(r) = self.bible_search.recent.get(&mode) {
            return r.clone();
        }
        let recent: Vec<String> = self
            .user
            .as_ref()
            .and_then(|u| u.preference(&mode.preference_key()).ok().flatten())
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        self.bible_search.recent.insert(mode, recent.clone());
        recent
    }

    pub(crate) fn remember_search(&mut self, mode: SearchMode, query: &str) {
        let mut recent = self.recent_searches(mode);
        recent.retain(|q| q != query);
        recent.insert(0, query.to_string());
        recent.truncate(RECENT_LIMIT);
        if let Some(user) = &self.user
            && let Ok(v) = serde_json::to_string(&recent)
            && let Err(e) = user.set_preference(&mode.preference_key(), &v)
        {
            self.workspace
                .push_effect(crate::workspace::Effect::Message(format!(
                    "Could not save recent searches: {e}"
                )));
        }
        self.bible_search.recent.insert(mode, recent);
    }

    /// The Standard Verse a Search Tab's "current book" and "current chapter" presets use:
    /// where its Link set is, else where the most recently active Bible (or other Passage
    /// Tab) is.
    pub fn search_context(&self, tab: TabId) -> Option<Verse> {
        let ws = &self.workspace;
        if let Some(link) = ws.tab(tab).and_then(|t| t.link)
            && let Some(v) = ws.standard_location_of(link, tab)
        {
            return Some(v);
        }
        let recent = ws.most_recent(|t| {
            t.kind.is_reading()
                && t.key_schemes.contains(&obs_model::KeyScheme::Passage)
                && t.history.current().is_some()
        })?;
        let t = ws.tab(recent)?;
        match crate::link::to_standard(t.verse_map.as_deref(), t.history.current()?) {
            crate::link::ToStandard::Verse(v) => Some(v),
            crate::link::ToStandard::Unmapped => None,
        }
    }

    /// The Search Tab a hand-over runs in: `tab` if given, else the most recently active
    /// Search Tab, else a new one.
    pub(crate) fn search_tab_for(&mut self, tab: Option<TabId>, req: OpenRequest) -> TabId {
        let kind = TabKind::Tool(Tool::Search);
        if let Some(t) = tab.filter(|t| self.workspace.tab(*t).is_some_and(|t| t.kind == kind)) {
            self.workspace.focus(t);
            return t;
        }
        if !req.split
            && let Some(t) = self.workspace.most_recent(|t| t.kind == kind)
        {
            self.workspace.focus(t);
            return t;
        }
        self.workspace
            .open(kind, Tool::Search.title().to_string(), req)
    }

    /// Run a search in a Search Tab, in its mode: check the query and the mode's Search scope
    /// (errors go inline), remember the query, and start the search over the ready indexes.
    /// Returns the inline error, if any.
    pub fn run_search(&mut self, tab: TabId, request: SearchRequestFields) -> Option<InlineError> {
        self.cancel_bible_jobs(tab);
        let fields = {
            let st = self.search_tab_mut(tab)?;
            st.query = request.query.clone();
            if let Some(scope) = request.scope {
                st.scope = scope;
            }
            if let Some(b) = request.bibles {
                st.bibles = Some(b);
            }
            if let Some(l) = request.library {
                st.library = l;
            }
            st.clear_results();
            st.error = None;
            st.clone()
        };
        // A search is somewhere History goes back to (#155).
        if !fields.query.trim().is_empty() {
            self.workspace.visited(tab);
        }
        let error = match fields.mode {
            SearchMode::Bible => self.start_bible_search(tab, &fields),
            SearchMode::Library => self.start_library_search(tab, &fields),
            SearchMode::Notes => self.run_notes_search(tab),
        };
        if let Some(st) = self.search_tab_mut(tab) {
            st.error = error.clone();
        }
        if error.is_none() {
            self.record_search_history(&fields);
        }
        error
    }

    fn start_bible_search(&mut self, tab: TabId, st: &SearchTab) -> Option<InlineError> {
        let query = match check_query(&st.query) {
            Ok(q) => q,
            Err(e) => return Some(e),
        };
        let scope = match parse_scope(&st.scope) {
            Ok(s) => s,
            Err(e) => return Some(e),
        };
        let bibles = st.bibles(&self.library);
        if bibles.is_empty() {
            let message = if self.library.of_type(ResourceType::Bible).next().is_some() {
                "Choose a Bible to search"
            } else {
                "Add a Bible to your library to search it"
            };
            return Some(InlineError::new(ErrorField::Bibles, message, None));
        }
        self.remember_search(SearchMode::Bible, &query);
        self.submit_bible_search(tab, query, st.scope.trim().to_string(), scope, bibles)
    }

    #[cfg(target_arch = "wasm32")]
    fn submit_bible_search(
        &mut self,
        _: TabId,
        _: String,
        _: String,
        _: Scope,
        _: Vec<String>,
    ) -> Option<InlineError> {
        Some(InlineError::new(
            ErrorField::Search,
            "Searching needs the desktop app",
            None,
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn submit_bible_search(
        &mut self,
        tab: TabId,
        query: String,
        scope_text: String,
        scope: Scope,
        bibles: Vec<String>,
    ) -> Option<InlineError> {
        use crate::search::{NotReady, Opened};
        let mut searched = Vec::new();
        let mut indexing = Vec::new();
        let mut failed = Vec::new();
        let mut stale = Vec::new();
        for id in bibles {
            let Some(entry) = self.library.get(&id) else {
                continue;
            };
            let abbreviation = entry.manifest.abbreviation.clone();
            let bible = desktop::JobBible {
                outline_key: outline_key(entry),
                abbreviation: abbreviation.clone(),
                rtl: entry.manifest.direction == obs_model::Direction::Rtl,
                obr: entry.path.clone(),
                id: id.clone(),
            };
            match self.open_search_index(&id) {
                Ok(Opened::Current(index)) => searched.push((bible, index)),
                Ok(Opened::Stale(index)) => {
                    stale.push(abbreviation);
                    searched.push((bible, index));
                }
                Err(NotReady::Indexing) => indexing.push(abbreviation),
                Err(NotReady::Failed(e)) => failed.push((abbreviation, e)),
                Err(NotReady::NotInLibrary) => {}
            }
        }
        let lexicons: Vec<std::path::PathBuf> = self
            .library
            .of_type(ResourceType::Lexicon)
            .map(|e| e.path.clone())
            .collect();
        let input = desktop::RunInput {
            query,
            scope_text: if scope_text.is_empty() {
                WHOLE_BIBLE.to_string()
            } else {
                scope_text
            },
            scope,
            bibles: searched,
            indexing,
            failed,
            stale,
            lexicons,
            outlines: Arc::clone(&self.bible_search.outlines),
        };
        let generation = self.search_tab(tab).map_or(0, SearchTab::generation);
        let job = self
            .jobs()
            .submit_fn("Bible search", move |ctx| desktop::run(input, ctx));
        self.bible_search.jobs.insert(
            job,
            BibleJob {
                tab,
                generation,
                kind: JobKind::Run,
            },
        );
        if let Some(st) = self.search_tab_mut(tab) {
            st.running = true;
            st.loading.insert(0);
        }
        None
    }

    pub(crate) fn cancel_bible_jobs(&mut self, tab: TabId) {
        let ids: Vec<JobId> = self
            .bible_search
            .jobs
            .iter()
            .filter(|(_, j)| j.tab == tab)
            .map(|(id, _)| *id)
            .collect();
        // Their (now stale) events are still applied, and dropped by generation.
        for id in ids {
            self.jobs().cancel(id);
        }
    }

    /// Fetch the snippets of `rows` (the rows a view shows) if they are not here yet.
    pub fn request_search_snippets(&mut self, tab: TabId, rows: Range<usize>) {
        let Some(st) = self.search_tab(tab) else {
            return;
        };
        if st.results.is_none() && st.library_results.is_some() {
            self.request_library_snippets(tab, rows);
            return;
        }
        let Some(results) = st.results.clone() else {
            return;
        };
        if rows.is_empty() {
            return;
        }
        let last = rows.end.min(results.rows.len());
        if rows.start >= last {
            return;
        }
        let pages: Vec<usize> = (rows.start / PAGE_ROWS..=(last - 1) / PAGE_ROWS)
            .filter(|p| !st.pages.contains_key(p) && !st.loading.contains(p))
            .collect();
        let generation = st.generation;
        for page in pages {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let obrs: Vec<Option<std::path::PathBuf>> = results
                    .bibles
                    .iter()
                    .map(|b| self.library.get(&b.id).map(|e| e.path.clone()))
                    .collect();
                let results = Arc::clone(&results);
                let job = self.jobs().submit_fn("Search snippets", move |ctx| {
                    if ctx.is_cancelled() {
                        return Err("cancelled".to_string());
                    }
                    Ok(PageOutput(desktop::page_snippets(&results, &obrs, page)))
                });
                self.bible_search.jobs.insert(
                    job,
                    BibleJob {
                        tab,
                        generation,
                        kind: JobKind::Page(page),
                    },
                );
                if let Some(st) = self.search_tab_mut(tab) {
                    st.loading.insert(page);
                }
            }
            #[cfg(target_arch = "wasm32")]
            let _ = (page, generation);
        }
    }

    /// Read the outlines of `bibles` in the background now if they are not read yet, so the
    /// first search over them does not wait for it. The Search Tab calls this while it is
    /// shown; Bibles whose index becomes searchable are read ahead anyway
    /// ([`Self::read_outline_ahead`]), this only puts the chosen ones first.
    pub fn warm_bible_outlines(&mut self, bibles: &[String]) {
        for id in bibles {
            self.bible_search.read_ahead.retain(|q| q != id);
            self.submit_outline_read(id);
        }
    }

    /// A Bible's Search index became searchable: queue its outline to be read ahead, so the
    /// first search over it does not pay for that read. Outlines are read one at a time, the
    /// ★ Bible (best in Preferred order) first.
    pub(crate) fn read_outline_ahead(&mut self, id: &str) {
        let is_bible = self
            .library
            .get(id)
            .is_some_and(|e| e.manifest.resource_type == ResourceType::Bible);
        #[cfg(test)]
        if self.bible_search.no_read_ahead {
            return;
        }
        if is_bible && !self.bible_search.read_ahead.iter().any(|q| q == id) {
            self.bible_search.read_ahead.push(id.to_string());
        }
        self.start_next_read_ahead();
    }

    /// Submit the next queued read-ahead if no outline is being read.
    fn start_next_read_ahead(&mut self) {
        while self.bible_search.warming.is_empty() && !self.bible_search.read_ahead.is_empty() {
            let library = &self.library;
            let next = self
                .bible_search
                .read_ahead
                .iter()
                .enumerate()
                .min_by_key(|(_, id)| library.position_in_type(id).unwrap_or(usize::MAX))
                .map(|(i, _)| i)
                .unwrap_or(0);
            let id = self.bible_search.read_ahead.remove(next);
            self.submit_outline_read(&id);
        }
    }

    /// Start reading a Bible's outline in the background unless it is read, being read, or
    /// could not be read (until the library changes).
    fn submit_outline_read(&mut self, id: &str) {
        let fingerprint = library_fingerprint(&self.library);
        if fingerprint != self.bible_search.failed_library {
            self.bible_search.failed_library = fingerprint;
            self.bible_search.failed_outlines.clear();
        }
        let Some(entry) = self.library.get(id) else {
            return;
        };
        let key = outline_key(entry);
        if self.bible_search.failed_outlines.contains(&key)
            || self.bible_search.warming.values().any(|k| *k == key)
        {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let cached = self
                .bible_search
                .outlines
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .contains_key(&key);
            if cached {
                return;
            }
            let outlines = Arc::clone(&self.bible_search.outlines);
            let obr = entry.path.clone();
            let k = key.clone();
            let job = self.jobs().submit_fn("Search outline", move |_| {
                desktop::outline(&outlines, &k, &obr).map(|_| ())
            });
            self.bible_search.warming.insert(job, key);
        }
    }

    /// Apply Bible search events; return the others.
    pub(crate) fn poll_bible_searches(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let mut other = Vec::new();
        // Tabs showing the Analysis whose new results need one.
        let mut analyse = Vec::new();
        for event in events {
            if self.bible_search.warming.contains_key(&event.id) {
                if event.kind.is_terminal()
                    && let Some(key) = self.bible_search.warming.remove(&event.id)
                {
                    if matches!(event.kind, JobEventKind::Failed(_)) {
                        self.bible_search.failed_outlines.insert(key);
                    }
                    self.start_next_read_ahead();
                }
                continue;
            }
            let Some(job) = self.bible_search.jobs.get(&event.id).copied() else {
                other.push(event);
                continue;
            };
            if !event.kind.is_terminal() {
                continue;
            }
            self.bible_search.jobs.remove(&event.id);
            let Some(st) = self.search_tab_mut(job.tab) else {
                continue;
            };
            if job.kind == JobKind::Analysis {
                if st.analysis_job == Some(event.id) {
                    st.analysis_job = None;
                    match event.kind {
                        JobEventKind::Finished(out) => {
                            if let Ok(out) = out.downcast::<AnalysisOutput>() {
                                st.analysis_order = Arc::new(out.0.order(st.analysis_sort));
                                st.analysis = Some(out.0);
                            }
                        }
                        JobEventKind::Failed(e) => {
                            st.error = Some(InlineError::new(ErrorField::Search, e, None));
                        }
                        _ => {}
                    }
                }
                continue;
            }
            if st.generation != job.generation {
                continue;
            }
            let page = match job.kind {
                JobKind::Run | JobKind::Analysis => 0,
                JobKind::Page(p) => p,
            };
            st.loading.remove(&page);
            match (event.kind, job.kind) {
                (JobEventKind::Finished(out), JobKind::Run) => {
                    st.running = false;
                    match out.downcast::<RunOutput>() {
                        Ok(out) => {
                            let RunOutput {
                                results,
                                first_page,
                            } = *out;
                            st.selected = (!results.rows.is_empty()).then_some(0);
                            st.results = Some(results);
                            st.pages.insert(0, Arc::new(first_page));
                            if st.view == ResultsView::Analysis {
                                analyse.push(job.tab);
                            }
                        }
                        Err(out) => {
                            if let Ok(out) =
                                out.downcast::<crate::library_search::LibraryRunOutput>()
                            {
                                let crate::library_search::LibraryRunOutput {
                                    results,
                                    first_page,
                                } = *out;
                                st.selected = (!results.rows.is_empty()).then_some(0);
                                st.library_results = Some(results);
                                st.pages.insert(0, Arc::new(first_page));
                            }
                        }
                    }
                }
                (JobEventKind::Finished(out), JobKind::Page(p)) => {
                    if let Ok(out) = out.downcast::<PageOutput>() {
                        st.pages.insert(p, Arc::new(out.0));
                    }
                }
                (JobEventKind::Failed(e), JobKind::Run) => {
                    st.running = false;
                    st.error = Some(InlineError::new(ErrorField::Search, e, None));
                }
                (JobEventKind::Cancelled, JobKind::Run) => st.running = false,
                _ => {}
            }
        }
        for tab in analyse {
            self.start_analysis(tab);
        }
        other
    }

    /// Open a hit (#16 rule 14): the Search Tab's Link set navigates to the Verse; without
    /// one (or for an Unmapped verse), the most recently active Tab of that Bible goes to it;
    /// otherwise a new Tab opens there. `new_tab` (Ctrl/Cmd) always opens a new Tab.
    /// Returns how it was opened (`link`, `tab` or `new`) and the Tab that shows it.
    pub fn open_search_hit(
        &mut self,
        tab: TabId,
        row: usize,
        hit: usize,
        new_tab: bool,
    ) -> Result<(&'static str, Option<TabId>), String> {
        self.open_search_hit_in(tab, row, hit, new_tab, false)
    }

    /// [`Self::open_search_hit`]; with `all`, `row` counts every result whatever the Results
    /// filter (as the Analysis table's rows do, #146) and the Verse list's choice is kept.
    pub fn open_search_hit_in(
        &mut self,
        tab: TabId,
        row: usize,
        hit: usize,
        new_tab: bool,
        all: bool,
    ) -> Result<(&'static str, Option<TabId>), String> {
        let st = self.search_tab(tab).ok_or("not a Search Tab")?;
        if st.results.is_none() && st.library_results.is_some() {
            return self.open_library_hit(tab, row, new_tab);
        }
        let results = if all {
            st.all_results().cloned()
        } else {
            st.results.clone()
        }
        .ok_or("no results")?;
        let r = results.rows.get(row).ok_or("no such row")?;
        let h = r.hits.get(hit).or(r.hits.first()).ok_or("no such hit")?;
        let id = results
            .bibles
            .get(h.bible)
            .map(|b| b.id.clone())
            .ok_or("no such Bible")?;
        if !all && let Some(st) = self.search_tab_mut(tab) {
            st.selected = Some(row);
        }
        let link = self.workspace.tab(tab).and_then(|t| t.link);
        if !new_tab
            && let (Some(link), Some(standard)) = (link, r.standard)
            && self.workspace.members(link).into_iter().any(|m| {
                m != tab
                    && self
                        .workspace
                        .tab(m)
                        .is_some_and(|t| t.key_schemes.contains(&obs_model::KeyScheme::Passage))
            })
        {
            self.workspace.navigate_standard(tab, standard);
            return Ok(("link", None));
        }
        let kind = TabKind::Resource(id.clone());
        if !new_tab && let Some(t) = self.workspace.most_recent(|t| t.kind == kind) {
            self.workspace.focus(t);
            self.workspace.navigate(t, h.native);
            return Ok(("tab", Some(t)));
        }
        if self.library.get(&id).is_none() {
            return Err(format!("{id} is no longer in the library"));
        }
        let out = self
            .run("resource.open", &json!({ "id": id, "split": new_tab }))
            .map_err(|e| e.to_string())?;
        let t = out["tab"].as_u64().map(TabId).ok_or("no Tab opened")?;
        self.workspace.navigate(t, h.native);
        Ok(("new", Some(t)))
    }
}

// --------------------------------------------------------------------------------- desktop

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod desktop {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use obs_search::{HitOrder, SearchIndex, SearchOptions};
    use obs_store::{ObrReader, ResourceStore, SqliteSearchIndex};

    use super::*;
    use crate::jobs::JobContext;

    /// A Bible a search job searches.
    pub(super) struct JobBible {
        pub id: String,
        pub abbreviation: String,
        pub rtl: bool,
        pub obr: PathBuf,
        /// Resource id and content hash: the outline cache key.
        pub outline_key: String,
    }

    pub(super) struct RunInput {
        pub query: String,
        pub scope_text: String,
        pub scope: Scope,
        pub bibles: Vec<(JobBible, SqliteSearchIndex)>,
        pub indexing: Vec<String>,
        pub failed: Vec<(String, String)>,
        pub stale: Vec<String>,
        pub lexicons: Vec<PathBuf>,
        pub outlines: Outlines,
    }

    pub(super) type Outlines = Arc<Mutex<BTreeMap<String, Arc<BibleOutline>>>>;

    /// A Bible's outline, read once per content hash: from its Verse map (which lists every
    /// Verse) when it has one, else from its verse list.
    pub(super) fn outline(
        outlines: &Outlines,
        key: &str,
        obr: &std::path::Path,
    ) -> Result<Arc<BibleOutline>, String> {
        let cached = outlines
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(key)
            .cloned();
        if let Some(o) = cached {
            return Ok(o);
        }
        let reader = ObrReader::open(obr).map_err(|e| e.to_string())?;
        let o = match reader.verse_map().map_err(|e| e.to_string())? {
            Some(map) => {
                let verses: Vec<(Verse, u16)> =
                    map.entries().iter().map(|e| (e.verse, e.last)).collect();
                BibleOutline::new(&verses, Some(&map))
            }
            None => {
                let verses: Vec<(Verse, u16)> = reader
                    .verses()
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .map(|v| (v.verse, v.last))
                    .collect();
                BibleOutline::new(&verses, None)
            }
        };
        let o = Arc::new(o);
        outlines
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key.to_string(), Arc::clone(&o));
        Ok(o)
    }

    /// The search job: compile the query (Lemma transliterations through the Lexicons), map
    /// the scope through each Bible's Verse map, search each index, build the Verse list and
    /// the first page of snippets.
    /// `lemma:` transliterations resolved through the Lexicons' transliteration columns.
    pub(crate) fn lemma_lookup(lexicons: Vec<PathBuf>) -> impl Fn(&str) -> Vec<String> {
        move |translit: &str| -> Vec<String> {
            let mut out = Vec::new();
            for path in &lexicons {
                if let Ok(reader) = ObrReader::open(path)
                    && let Ok(found) = reader.lemmas_for_transliteration(translit)
                {
                    out.extend(found);
                }
            }
            out
        }
    }

    pub(super) fn run(input: RunInput, ctx: &JobContext) -> Result<RunOutput, String> {
        let started = std::time::Instant::now();
        let lookup = lemma_lookup(input.lexicons.clone());
        let parsed = obs_search::parse(&input.query).map_err(|e| e.to_string())?;
        let compiled = obs_search::compile(&parsed, &lookup).map_err(|e| e.to_string())?;
        let highlighter = Highlighter::new(&parsed, &lookup);
        let mut per_bible: Vec<(Vec<SearchHit>, Arc<BibleOutline>)> = Vec::new();
        let mut failed = input.failed.clone();
        let mut bibles = Vec::new();
        for (b, index) in &input.bibles {
            if ctx.is_cancelled() {
                return Err("cancelled".into());
            }
            let found = outline(&input.outlines, &b.outline_key, &b.obr)
                .map_err(|e| format!("{}: {e}", b.abbreviation))
                .and_then(|o| {
                    let hits = match o.ordinal_ranges(&input.scope) {
                        Some(r) if r.is_empty() => Vec::new(),
                        documents => index
                            .search(
                                &compiled,
                                &SearchOptions {
                                    documents: documents.unwrap_or_default(),
                                    order: HitOrder::Document,
                                    limit: None,
                                },
                            )
                            .map_err(|e| format!("{}: {e}", b.abbreviation))?,
                    };
                    Ok((hits, o))
                });
            match found {
                Ok((hits, o)) => {
                    bibles.push(SearchedBible {
                        id: b.id.clone(),
                        abbreviation: b.abbreviation.clone(),
                        rtl: b.rtl,
                        hits: hits.len(),
                    });
                    per_bible.push((hits, o));
                }
                Err(e) => failed.push((b.abbreviation.clone(), e)),
            }
        }
        let refs: Vec<(&[SearchHit], &BibleOutline)> = per_bible
            .iter()
            .map(|(h, o)| (h.as_slice(), o.as_ref()))
            .collect();
        let rows = verse_rows(&refs);
        let results = Arc::new(BibleResults {
            query: input.query.clone(),
            scope: input.scope_text.clone(),
            bibles,
            rows,
            highlighter,
            indexing: input.indexing.clone(),
            failed,
            stale: input.stale.clone(),
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        });
        let obrs: Vec<Option<PathBuf>> = input
            .bibles
            .iter()
            .filter(|(b, _)| results.bibles.iter().any(|r| r.id == b.id))
            .map(|(b, _)| Some(b.obr.clone()))
            .collect();
        let first_page = page_snippets(&results, &obrs, 0);
        Ok(RunOutput {
            results,
            first_page,
        })
    }

    /// The snippets of one page of rows, reading each hit's text and spans from its Bible.
    pub(super) fn page_snippets(
        results: &BibleResults,
        obrs: &[Option<PathBuf>],
        page: usize,
    ) -> Vec<Vec<Snippet>> {
        let mut readers: HashMap<usize, Option<ObrReader>> = HashMap::new();
        let mut texts: HashMap<(usize, UnitId), Option<String>> = HashMap::new();
        let start = page * PAGE_ROWS;
        let end = (start + PAGE_ROWS).min(results.rows.len());
        let mut out = Vec::with_capacity(end.saturating_sub(start));
        for row in results.rows.get(start..end).unwrap_or_default() {
            let mut snippets = Vec::with_capacity(row.hits.len());
            for h in &row.hits {
                let reader = readers.entry(h.bible).or_insert_with(|| {
                    obrs.get(h.bible)
                        .and_then(Option::as_ref)
                        .and_then(|p| ObrReader::open(p).ok())
                });
                let Some(reader) = reader.as_ref() else {
                    snippets.push(Snippet::default());
                    continue;
                };
                let text = texts
                    .entry((h.bible, h.unit.clone()))
                    .or_insert_with(|| reader.unit_text(&h.unit).ok().flatten().map(|t| t.1));
                let Some(text) = text.as_deref() else {
                    snippets.push(Snippet::default());
                    continue;
                };
                let range = h.range.start as usize..h.range.end as usize;
                let spans = reader
                    .spans_starting_in(&h.unit, range.clone())
                    .unwrap_or_default();
                snippets.push(snippet(text, range, &spans, &results.highlighter));
            }
            out.push(snippets);
        }
        out
    }
}

// -------------------------------------------------------------------------------- commands

pub(crate) const TAB: ParamSpec = ParamSpec {
    name: "tab",
    kind: ParamKind::Integer,
    required: true,
    help: "The Search Tab",
};
const SPLIT: ParamSpec = ParamSpec {
    name: "split",
    kind: ParamKind::Boolean,
    required: false,
    help: "Open in a new Tab split off to the right (Ctrl/Cmd held)",
};

pub(crate) fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

pub(crate) fn tab_param(
    command: &'static str,
    s: &Session,
    p: &Params<'_>,
) -> Result<TabId, CommandError> {
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .ok_or_else(|| failed(command, "no tab"))?;
    if s.search_tab(tab).is_none() {
        return Err(failed(
            command,
            format!("Tab {} is not a Search Tab", tab.0),
        ));
    }
    Ok(tab)
}

pub(crate) fn error_json(e: Option<&InlineError>) -> Value {
    e.map_or(Value::Null, |e| {
        json!({
            "field": format!("{:?}", e.field).to_lowercase(),
            "message": e.message,
        })
    })
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "search.run",
        label: "Search",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "text",
                kind: ParamKind::String,
                required: true,
                help: "The query (#16 grammar), e.g. \"love thy neighbour\" or strong:H1254",
            },
            ParamSpec {
                name: "tab",
                kind: ParamKind::Integer,
                required: false,
                help: "The Search Tab to run in (default: the most recently active one, else a new one)",
            },
            ParamSpec {
                name: "scope",
                kind: ParamKind::String,
                required: false,
                help: "Passage list, Whole Bible, Old Testament or New Testament (default: the Tab's)",
            },
            ParamSpec {
                name: "bibles",
                kind: ParamKind::String,
                required: false,
                help: "Comma-separated Bible resource ids (default: the Tab's choice, else the ★ Bible)",
            },
            ParamSpec {
                name: "resources",
                kind: ParamKind::String,
                required: false,
                help: "Library mode: comma-separated Resource types (commentary, lexicon, dictionary) and resource ids (default: the Tab's choice, else every Commentary, Lexicon and Dictionary)",
            },
            ParamSpec {
                name: "mode",
                kind: ParamKind::String,
                required: false,
                help: "bible, library or notes (default: the Tab's)",
            },
            SPLIT,
        ],
        enabled: always,
        run: search_run,
    },
    CommandSpec {
        id: "search.mode",
        label: "Search mode",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "value",
                kind: ParamKind::String,
                required: true,
                help: "bible, library or notes",
            },
        ],
        enabled: always,
        run: search_mode,
    },
    CommandSpec {
        id: "search.scope",
        label: "Search scope",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "value",
                kind: ParamKind::String,
                required: true,
                help: "Passage list, Whole Bible, Old Testament or New Testament",
            },
        ],
        enabled: always,
        run: search_scope,
    },
    CommandSpec {
        id: "search.bible",
        label: "Search this Bible",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "id",
                kind: ParamKind::String,
                required: true,
                help: "A Bible's resource id: added to the search, or removed if it is in it",
            },
        ],
        enabled: always,
        run: search_bible,
    },
    CommandSpec {
        id: "search.open",
        label: "Open search hit",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "row",
                kind: ParamKind::Integer,
                required: true,
                help: "Result row (0-based)",
            },
            ParamSpec {
                name: "hit",
                kind: ParamKind::Integer,
                required: false,
                help: "Which Bible of the row (0-based; default the first)",
            },
            ParamSpec {
                name: "all",
                kind: ParamKind::Boolean,
                required: false,
                help: "The row counts every result, whatever the Results filter (the Analysis table's rows)",
            },
            SPLIT,
        ],
        enabled: always,
        run: search_open,
    },
];

fn search_run(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.run";
    let text = p.str("text").unwrap_or_default().to_string();
    let mode = match p.str("mode") {
        None => None,
        Some(m) => Some(
            SearchMode::from_id(m).ok_or_else(|| failed(ID, format!("no search mode “{m}”")))?,
        ),
    };
    let tab = p.i64("tab").and_then(|n| u64::try_from(n).ok()).map(TabId);
    let req = OpenRequest {
        panel: None,
        split: p.bool("split").unwrap_or(false),
    };
    let tab = s.search_tab_for(tab, req);
    if let Some(mode) = mode
        && let Some(st) = s.search_tab_mut(tab)
    {
        st.mode = mode;
    }
    let bibles = p.str("bibles").map(|b| {
        b.split(',')
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .map(String::from)
            .collect::<Vec<_>>()
    });
    let library = match p.str("resources") {
        None => None,
        Some(r) => Some(
            crate::library_search::LibraryScope::parse(r, &s.library).map_err(|e| failed(ID, e))?,
        ),
    };
    let error = s.run_search(
        tab,
        SearchRequestFields {
            query: text,
            scope: p.str("scope").map(String::from),
            bibles,
            library,
        },
    );
    Ok(json!({ "tab": tab.0, "error": error_json(error.as_ref()) }))
}

/// Run the Tab's search again (after its scope, Bibles or Resources changed), if it has a
/// query.
pub(crate) fn rerun(s: &mut Session, tab: TabId) -> Option<InlineError> {
    let query = s.search_tab(tab)?.query.clone();
    if query.trim().is_empty() {
        return None;
    }
    s.run_search(
        tab,
        SearchRequestFields {
            query,
            ..Default::default()
        },
    )
}

fn search_mode(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.mode";
    let tab = tab_param(ID, s, &p)?;
    let v = p.str("value").unwrap_or_default();
    let mode = SearchMode::from_id(v).ok_or_else(|| failed(ID, format!("no search mode “{v}”")))?;
    if let Some(st) = s.search_tab_mut(tab)
        && st.mode != mode
    {
        st.mode = mode;
        st.clear_results();
        st.error = None;
    }
    Ok(json!({ "tab": tab.0, "mode": mode.id() }))
}

fn search_scope(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.scope";
    let tab = tab_param(ID, s, &p)?;
    let v = p.str("value").unwrap_or_default().trim().to_string();
    let v = if v.eq_ignore_ascii_case(WHOLE_BIBLE) {
        String::new()
    } else {
        v
    };
    if let Err(e) = parse_scope(&v) {
        if let Some(st) = s.search_tab_mut(tab) {
            st.scope = v;
            st.error = Some(e.clone());
        }
        return Ok(json!({ "tab": tab.0, "error": error_json(Some(&e)) }));
    }
    if let Some(st) = s.search_tab_mut(tab) {
        st.scope = v;
        if st
            .error
            .as_ref()
            .is_some_and(|e| e.field == ErrorField::Scope)
        {
            st.error = None;
        }
    }
    let error = rerun(s, tab);
    Ok(json!({ "tab": tab.0, "error": error_json(error.as_ref()) }))
}

fn search_bible(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.bible";
    let tab = tab_param(ID, s, &p)?;
    let id = p.str("id").unwrap_or_default().to_string();
    if !s
        .library
        .get(&id)
        .is_some_and(|e| e.manifest.resource_type == ResourceType::Bible)
    {
        return Err(failed(ID, format!("{id} is not a Bible in the library")));
    }
    let library = &s.library;
    let Some(st) = s
        .workspace
        .tab_mut(tab)
        .filter(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| &mut t.search)
    else {
        return Err(failed(ID, "not a Search Tab"));
    };
    let mut bibles = st.bibles(library);
    if let Some(i) = bibles.iter().position(|b| *b == id) {
        bibles.remove(i);
    } else {
        bibles.push(id);
    }
    st.bibles = Some(bibles.clone());
    let error = rerun(s, tab);
    Ok(json!({ "tab": tab.0, "bibles": bibles, "error": error_json(error.as_ref()) }))
}

fn search_open(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.open";
    let tab = tab_param(ID, s, &p)?;
    let row = p
        .i64("row")
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| failed(ID, "no row"))?;
    let hit = p
        .i64("hit")
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or(0);
    let (via, opened) = s
        .open_search_hit_in(
            tab,
            row,
            hit,
            p.bool("split").unwrap_or(false),
            p.bool("all").unwrap_or(false),
        )
        .map_err(|e| failed(ID, e))?;
    Ok(json!({ "via": via, "tab": opened.map(|t| t.0) }))
}
