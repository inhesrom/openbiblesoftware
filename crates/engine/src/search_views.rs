//! Search results views (#146): a Bible search's results seen four ways, switched above the
//! results.
//!
//! - **Verses**: the Verse list ([`crate::bible_search`]).
//! - **Grid**: a heat map of matching Verses by Bible book × Bible ([`HitGrid`]).
//! - **Chart**: a bar chart of matching Verses by Bible book, or by chapter within one book
//!   ([`chart_bars`]).
//! - **Analysis**: a sortable table with one row per matched word: Reference, Bible, the
//!   matched word, its Lemma and Morphology, and its context ([`SearchAnalysis`]). It is made
//!   in the background from every hit's text when first shown, and exports as CSV.
//!
//! Clicking a Grid cell or a Chart bar sets a Results filter: the Verse list then shows only
//! that Bible book (or chapter), and for a Grid cell only that Bible's hits. The Grid, Chart
//! and Analysis always count the whole result set.

use std::ops::Range;
use std::sync::Arc;

use obs_model::MorphemePart;
use obs_scripture::{Book, Standard, Versification};
use obs_search::Highlighter;
use serde_json::{Value, json};

use crate::bible_search::{
    BibleResults, SearchMode, SearchTab, SearchedBible, Snippet, TAB, VerseRow, failed, tab_param,
};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::workspace::TabId;

// ---------------------------------------------------------------------------------- views

/// How a Bible search's results are shown.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ResultsView {
    #[default]
    Verses,
    Grid,
    Chart,
    Analysis,
}

impl ResultsView {
    pub const ALL: [ResultsView; 4] = [
        ResultsView::Verses,
        ResultsView::Grid,
        ResultsView::Chart,
        ResultsView::Analysis,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ResultsView::Verses => "verses",
            ResultsView::Grid => "grid",
            ResultsView::Chart => "chart",
            ResultsView::Analysis => "analysis",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|v| v.id().eq_ignore_ascii_case(id.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            ResultsView::Verses => "Verses",
            ResultsView::Grid => "Grid",
            ResultsView::Chart => "Chart",
            ResultsView::Analysis => "Analysis",
        }
    }
}

// --------------------------------------------------------------------------------- filter

/// What a Grid cell or Chart bar narrows the Verse list to: one Bible book (or one chapter of
/// it), and for a Grid cell one Bible's hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultsFilter {
    pub book: Book,
    pub chapter: Option<u16>,
    /// Index into [`BibleResults::bibles`].
    pub bible: Option<usize>,
}

impl ResultsFilter {
    pub fn book(book: Book) -> Self {
        Self {
            book,
            chapter: None,
            bible: None,
        }
    }

    /// Whether a row of the Verse list lies in the filter's book (and chapter).
    pub fn contains(&self, row: &VerseRow) -> bool {
        row.position()
            .is_some_and(|v| v.book == self.book && self.chapter.is_none_or(|c| v.chapter == c))
    }

    /// How the filter reads above the Verse list: `John 3`, `1 John · KJV`.
    pub fn label(&self, results: &BibleResults) -> String {
        let mut s = self.book.name().to_string();
        if let Some(c) = self.chapter {
            s.push_str(&format!(" {c}"));
        }
        if let Some(b) = self.bible.and_then(|b| results.bibles.get(b)) {
            s.push_str(&format!(" · {}", b.abbreviation));
        }
        s
    }

    /// The results narrowed to the filter: its rows (with only its Bible's hits), the Bibles'
    /// counts recounted.
    pub fn apply(&self, results: &BibleResults) -> BibleResults {
        let rows: Vec<VerseRow> = results
            .rows
            .iter()
            .filter(|r| self.contains(r))
            .filter_map(|r| {
                let hits: Vec<_> = r
                    .hits
                    .iter()
                    .filter(|h| self.bible.is_none_or(|b| h.bible == b))
                    .cloned()
                    .collect();
                (!hits.is_empty()).then_some(VerseRow {
                    standard: r.standard,
                    hits,
                })
            })
            .collect();
        let bibles = results
            .bibles
            .iter()
            .enumerate()
            .map(|(i, b)| SearchedBible {
                hits: rows
                    .iter()
                    .map(|r| r.hits.iter().filter(|h| h.bible == i).count())
                    .sum(),
                ..b.clone()
            })
            .collect();
        BibleResults {
            rows,
            bibles,
            ..results.clone()
        }
    }
}

// ----------------------------------------------------------------------------------- grid

/// The Grid: matching Verses by Bible book (rows, canonical order, only books with a hit) and
/// Bible (columns, as searched).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HitGrid {
    pub books: Vec<Book>,
    /// The Bibles' abbreviations.
    pub bibles: Vec<String>,
    /// `counts[book][bible]`.
    pub counts: Vec<Vec<usize>>,
    /// The largest count of any cell.
    pub max: usize,
}

impl HitGrid {
    pub fn new(results: &BibleResults) -> Self {
        let n = results.bibles.len();
        let mut by_book: Vec<Vec<usize>> = vec![vec![0; n]; Book::ALL.len()];
        for row in &results.rows {
            let Some(at) = row.position() else {
                continue;
            };
            for h in &row.hits {
                if let Some(c) = by_book[at.book.index()].get_mut(h.bible) {
                    *c += 1;
                }
            }
        }
        let mut books = Vec::new();
        let mut counts = Vec::new();
        for book in Book::ALL {
            let row = &by_book[book.index()];
            if row.iter().any(|c| *c > 0) {
                books.push(book);
                counts.push(row.clone());
            }
        }
        let max = counts.iter().flatten().copied().max().unwrap_or(0);
        Self {
            books,
            bibles: results
                .bibles
                .iter()
                .map(|b| b.abbreviation.clone())
                .collect(),
            counts,
            max,
        }
    }

    /// How strongly a cell is coloured: its count over the largest, 0 to 1.
    pub fn heat(&self, count: usize) -> f32 {
        if self.max == 0 {
            0.0
        } else {
            count as f32 / self.max as f32
        }
    }

    /// A cell's count (0 when the book has no hit in that Bible).
    pub fn count(&self, book: Book, bible: usize) -> usize {
        self.books
            .iter()
            .position(|b| *b == book)
            .and_then(|i| self.counts[i].get(bible))
            .copied()
            .unwrap_or(0)
    }

    /// The books whose hottest cell is at least `heat`, hottest first.
    pub fn hottest(&self, heat: f32) -> Vec<Book> {
        let mut out: Vec<(usize, Book)> = self
            .books
            .iter()
            .zip(&self.counts)
            .map(|(b, c)| (c.iter().copied().max().unwrap_or(0), *b))
            .filter(|(c, _)| self.heat(*c) >= heat)
            .collect();
        out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        out.into_iter().map(|(_, b)| b).collect()
    }
}

// ---------------------------------------------------------------------------------- chart

/// What the Chart's bars count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChartBy {
    /// One bar per Bible book with a hit.
    #[default]
    Book,
    /// One bar per chapter of a Bible book.
    Chapter(Book),
}

/// One bar of the Chart: what it is, how many matching Verses it has, and the Results filter
/// a click on it sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartBar {
    pub label: String,
    pub count: usize,
    pub filter: ResultsFilter,
}

/// The Chart's bars: matching Verses (rows of the Verse list) by Bible book, or by chapter of
/// one book (every chapter of the book, so the gaps show).
pub fn chart_bars(results: &BibleResults, by: ChartBy) -> Vec<ChartBar> {
    match by {
        ChartBy::Book => {
            let mut counts = vec![0usize; Book::ALL.len()];
            for row in &results.rows {
                if let Some(v) = row.position() {
                    counts[v.book.index()] += 1;
                }
            }
            Book::ALL
                .into_iter()
                .filter(|b| counts[b.index()] > 0)
                .map(|b| ChartBar {
                    label: b.name().to_string(),
                    count: counts[b.index()],
                    filter: ResultsFilter::book(b),
                })
                .collect()
        }
        ChartBy::Chapter(book) => {
            let mut counts: Vec<usize> = vec![0; usize::from(Standard.chapter_count(book))];
            for row in &results.rows {
                if let Some(v) = row.position().filter(|v| v.book == book && v.chapter > 0) {
                    let i = usize::from(v.chapter - 1);
                    if counts.len() <= i {
                        counts.resize(i + 1, 0);
                    }
                    counts[i] += 1;
                }
            }
            counts
                .into_iter()
                .enumerate()
                .map(|(i, count)| {
                    let chapter = u16::try_from(i + 1).unwrap_or(u16::MAX);
                    ChartBar {
                        label: format!("{} {chapter}", book.abbrev()),
                        count,
                        filter: ResultsFilter {
                            book,
                            chapter: Some(chapter),
                            bible: None,
                        },
                    }
                })
                .collect()
        }
    }
}

// ------------------------------------------------------------------------------- analysis

/// A column of the Analysis table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AnalysisColumn {
    #[default]
    Reference,
    Bible,
    Word,
    Lemma,
    Morphology,
    Context,
}

impl AnalysisColumn {
    pub const ALL: [AnalysisColumn; 6] = [
        AnalysisColumn::Reference,
        AnalysisColumn::Bible,
        AnalysisColumn::Word,
        AnalysisColumn::Lemma,
        AnalysisColumn::Morphology,
        AnalysisColumn::Context,
    ];

    pub fn id(self) -> &'static str {
        match self {
            AnalysisColumn::Reference => "reference",
            AnalysisColumn::Bible => "bible",
            AnalysisColumn::Word => "word",
            AnalysisColumn::Lemma => "lemma",
            AnalysisColumn::Morphology => "morphology",
            AnalysisColumn::Context => "context",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.id().eq_ignore_ascii_case(id.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            AnalysisColumn::Reference => "Reference",
            AnalysisColumn::Bible => "Bible",
            AnalysisColumn::Word => "Word",
            AnalysisColumn::Lemma => "Lemma",
            AnalysisColumn::Morphology => "Morphology",
            AnalysisColumn::Context => "Context",
        }
    }
}

/// The Analysis table's order: a column, ascending or descending. Ties keep canonical order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnalysisSort {
    pub column: AnalysisColumn,
    pub descending: bool,
}

/// One matched word of one hit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisRow {
    /// The row of the Verse list (unfiltered) and the hit in it.
    pub row: usize,
    pub hit: usize,
    pub reference: String,
    pub bible: String,
    /// The Bible reads right to left (its word and context).
    pub rtl: bool,
    pub word: String,
    /// The matched Tagged words' Lemmas (a word's own, else its Lexicon headword), and their
    /// Morphology in plain English (a code the decoder cannot read stays as written); empty
    /// for an untagged word.
    pub lemma: String,
    pub morphology: String,
    /// The Morphology codes as the Bible tags them, for a tooltip.
    pub morph_codes: String,
    /// The hit's text as the Verse list shows it, and where the word is in it.
    pub context: Arc<str>,
    pub word_range: Range<usize>,
}

impl AnalysisRow {
    /// The context before the word.
    pub fn before(&self) -> &str {
        self.context
            .get(..self.word_range.start)
            .unwrap_or_default()
    }

    /// The context after the word.
    pub fn after(&self) -> &str {
        self.context.get(self.word_range.end..).unwrap_or_default()
    }

    /// A short Context for the table: up to `before` words before the matched word and
    /// `after` words after it (punctuation touching the word comes along), `…` where words
    /// are left out, and where the word is in it.
    pub fn excerpt(&self, before: usize, after: usize) -> (String, Range<usize>) {
        let (head, tail) = (self.before(), self.after());
        let mut head: Vec<&str> = head.split_whitespace().collect();
        let mut tail: Vec<&str> = tail.split_whitespace().collect();
        let glued_head = (!self.before().ends_with(char::is_whitespace))
            .then(|| head.pop())
            .flatten()
            .unwrap_or_default();
        let glued_tail = if self.after().starts_with(char::is_whitespace) || tail.is_empty() {
            ""
        } else {
            tail.remove(0)
        };
        let mut out = String::new();
        if head.len() > before {
            out.push_str("… ");
        }
        for w in &head[head.len().saturating_sub(before)..] {
            out.push_str(w);
            out.push(' ');
        }
        out.push_str(glued_head);
        let start = out.len();
        out.push_str(&self.word);
        let end = out.len();
        out.push_str(glued_tail);
        for w in &tail[..tail.len().min(after)] {
            out.push(' ');
            out.push_str(w);
        }
        if tail.len() > after {
            out.push_str(" …");
        }
        (out, start..end)
    }

    /// A cell's text (the Context column is the whole hit text).
    pub fn cell(&self, column: AnalysisColumn) -> &str {
        match column {
            AnalysisColumn::Reference => &self.reference,
            AnalysisColumn::Bible => &self.bible,
            AnalysisColumn::Word => &self.word,
            AnalysisColumn::Lemma => &self.lemma,
            AnalysisColumn::Morphology => &self.morphology,
            AnalysisColumn::Context => &self.context,
        }
    }
}

/// The Analysis table of one Bible search, in canonical order.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchAnalysis {
    /// The results it was made from (the unfiltered ones).
    pub results: Arc<BibleResults>,
    pub rows: Vec<AnalysisRow>,
}

impl SearchAnalysis {
    /// The row indices in `sort` order. Text columns compare case-insensitively, empty
    /// cells last either way; Context compares the words after the match (then before it),
    /// as a concordance does; Reference is canonical order (Verse-list row). Ties keep
    /// canonical order whichever way the column sorts.
    pub fn order(&self, sort: AnalysisSort) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.rows.len()).collect();
        if sort.column == AnalysisColumn::Reference {
            if sort.descending {
                order.sort_by(|&a, &b| self.rows[b].row.cmp(&self.rows[a].row).then(a.cmp(&b)));
            }
            return order;
        }
        let key = |r: &AnalysisRow| -> String {
            match sort.column {
                AnalysisColumn::Reference => String::new(),
                AnalysisColumn::Context => {
                    format!("{}\u{1}{}", r.after().trim_start(), r.before()).to_lowercase()
                }
                c => r.cell(c).to_lowercase(),
            }
        };
        let keys: Vec<String> = self.rows.iter().map(key).collect();
        order.sort_by(|&a, &b| {
            let by = keys[a].cmp(&keys[b]);
            let by = if sort.descending { by.reverse() } else { by };
            keys[a]
                .is_empty()
                .cmp(&keys[b].is_empty())
                .then(by)
                .then(a.cmp(&b))
        });
        order
    }

    /// The table as CSV in `order`: a header row, then one record per matched word. UTF-8
    /// with a byte-order mark and CRLF line ends (RFC 4180), so spreadsheets open the Greek
    /// and Hebrew as written.
    pub fn csv(&self, order: &[usize]) -> String {
        let mut out = String::from("\u{feff}");
        let header: Vec<&str> = AnalysisColumn::ALL.iter().map(|c| c.label()).collect();
        push_record(&mut out, &header);
        for &i in order {
            if let Some(r) = self.rows.get(i) {
                let cells: Vec<&str> = AnalysisColumn::ALL.iter().map(|c| r.cell(*c)).collect();
                push_record(&mut out, &cells);
            }
        }
        out
    }
}

fn push_record(out: &mut String, cells: &[&str]) {
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&csv_field(cell));
    }
    out.push_str("\r\n");
}

/// One CSV field: quoted when it holds a comma, quote or line break (quotes doubled). A
/// field a spreadsheet would read as a formula (`=`, `+`, `-`, `@` first) is kept as text
/// with a leading apostrophe.
pub fn csv_field(cell: &str) -> String {
    let cell = if cell.starts_with(['=', '+', '-', '@']) {
        format!("'{cell}")
    } else {
        cell.to_string()
    };
    if cell.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", cell.replace('"', "\"\""))
    } else {
        cell
    }
}

/// One matched word in a hit's snippet: the whole word(s) the highlight lies in, and the
/// Lemma and Morphology of the Tagged words there (the parts the query matched, else all).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedWord {
    pub range: Range<usize>,
    pub lemmas: Vec<String>,
    /// Strong's numbers of parts without a Lemma, for the Lexicon to name.
    pub unnamed: Vec<obs_scripture::StrongsNumber>,
    /// The parts' Morphology codes as tagged.
    pub morph_codes: Vec<String>,
}

/// The matched words of a snippet: one per highlight, widened to the Tagged words it touches
/// (a Hebrew root highlighted without its prefix is still one word), duplicates merged.
pub fn matched_words(
    snippet: &Snippet,
    tagged: &[(Range<usize>, &[MorphemePart])],
    highlighter: &Highlighter,
) -> Vec<MatchedWord> {
    let mut out: Vec<MatchedWord> = Vec::new();
    for h in &snippet.highlights {
        let touched: Vec<&(Range<usize>, &[MorphemePart])> = tagged
            .iter()
            .filter(|(r, _)| r.start < h.end && h.start < r.end)
            .collect();
        let start = touched
            .iter()
            .map(|(r, _)| r.start)
            .fold(h.start, usize::min);
        let end = touched.iter().map(|(r, _)| r.end).fold(h.end, usize::max);
        if out.last().is_some_and(|w| w.range == (start..end)) {
            continue;
        }
        let mut lemmas = Vec::new();
        let mut unnamed = Vec::new();
        let mut morph_codes = Vec::new();
        for (_, parts) in touched {
            let matched = highlighter.matching_parts(parts);
            let any = matched.iter().any(|m| *m);
            for (p, m) in parts.iter().zip(matched) {
                if any && !m {
                    continue;
                }
                match p.lemma.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
                    Some(l) => lemmas.push(l.to_string()),
                    None => unnamed.push(p.strongs),
                }
                if let Some(m) = p.morph.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
                    morph_codes.push(m.to_string());
                }
            }
        }
        out.push(MatchedWord {
            range: start..end,
            lemmas,
            unnamed,
            morph_codes,
        });
    }
    out
}

impl SearchTab {
    /// Every result of the last Bible search, whatever the Results filter.
    pub fn all_results(&self) -> Option<&Arc<BibleResults>> {
        self.unfiltered.as_ref().or(self.results.as_ref())
    }

    /// The Analysis rows in the chosen order, once the Analysis has been made.
    pub fn analysis_rows(&self) -> Option<(&SearchAnalysis, &[usize])> {
        self.analysis
            .as_deref()
            .map(|a| (a, self.analysis_order.as_slice()))
    }
}

// -------------------------------------------------------------------------------- session

impl Session {
    /// Show a Search Tab's results another way; the Analysis is made the first time.
    pub fn set_results_view(&mut self, tab: TabId, view: ResultsView) {
        let Some(st) = self.search_tab_mut(tab) else {
            return;
        };
        st.view = view;
        if view == ResultsView::Analysis {
            self.start_analysis(tab);
        }
    }

    /// Narrow the Verse list to a Grid cell or Chart bar (`None`: show every result), and show
    /// it.
    pub fn set_results_filter(&mut self, tab: TabId, filter: Option<ResultsFilter>) {
        let Some(st) = self.search_tab_mut(tab) else {
            return;
        };
        let Some(all) = st.unfiltered.take().or_else(|| st.results.take()) else {
            return;
        };
        match filter {
            Some(f) => {
                st.results = Some(Arc::new(f.apply(&all)));
                st.unfiltered = Some(all);
            }
            None => st.results = Some(all),
        }
        st.filter = filter;
        st.view = ResultsView::Verses;
        st.reset_pages();
        st.selected = st
            .results
            .as_ref()
            .and_then(|r| (!r.rows.is_empty()).then_some(0));
    }

    /// Order the Analysis table by a column.
    pub fn sort_analysis(&mut self, tab: TabId, sort: AnalysisSort) {
        let Some(st) = self.search_tab_mut(tab) else {
            return;
        };
        st.analysis_sort = sort;
        if let Some(a) = &st.analysis {
            st.analysis_order = Arc::new(a.order(sort));
        }
    }

    /// Start making the Analysis of a Tab's results unless it is made or being made.
    pub(crate) fn start_analysis(&mut self, tab: TabId) {
        let Some(st) = self.search_tab(tab) else {
            return;
        };
        let Some(results) = st.all_results().cloned() else {
            return;
        };
        if st.analysis_running()
            || st
                .analysis
                .as_ref()
                .is_some_and(|a| Arc::ptr_eq(&a.results, &results))
        {
            return;
        }
        let generation = st.generation();
        #[cfg(not(target_arch = "wasm32"))]
        {
            let obrs: Vec<Option<std::path::PathBuf>> = results
                .bibles
                .iter()
                .map(|b| self.library.get(&b.id).map(|e| e.path.clone()))
                .collect();
            let lexicons: Vec<std::path::PathBuf> = self
                .library
                .of_type(obs_model::ResourceType::Lexicon)
                .map(|e| e.path.clone())
                .collect();
            let job = self.jobs().submit_fn("Search analysis", move |ctx| {
                desktop::analysis(results, &obrs, &lexicons, ctx)
            });
            self.bible_search.jobs.insert(
                job,
                crate::bible_search::BibleJob {
                    tab,
                    generation,
                    kind: crate::bible_search::JobKind::Analysis,
                },
            );
            if let Some(st) = self.search_tab_mut(tab) {
                st.analysis_job = Some(job);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (generation, results);
    }

    /// Write a Search Tab's Analysis table as CSV, in its shown order. Returns the records
    /// written (without the header).
    pub fn export_analysis_csv(
        &mut self,
        tab: TabId,
        path: &std::path::Path,
    ) -> Result<usize, String> {
        let st = self.search_tab(tab).ok_or("not a Search Tab")?;
        let (analysis, order) = st.analysis_rows().ok_or(if st.analysis_running() {
            "The Analysis is still being made"
        } else {
            "Show the Analysis of a Bible search first"
        })?;
        std::fs::write(path, analysis.csv(order))
            .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
        Ok(order.len())
    }
}

/// What the Analysis job hands back.
pub(crate) struct AnalysisOutput(pub(crate) Arc<SearchAnalysis>);

#[cfg(not(target_arch = "wasm32"))]
mod desktop {
    use std::collections::HashMap;
    use std::collections::hash_map::Entry;
    use std::path::PathBuf;

    use obs_model::{UnitId, morph};
    use obs_scripture::StrongsNumber;
    use obs_store::{ObrReader, ResourceStore};

    use super::*;
    use crate::bible_search::snippet_and_tagged;
    use crate::jobs::JobContext;

    /// Read every hit's text and spans and list its matched words. Parts without a Lemma
    /// borrow the headword of the first Lexicon (in Preferred order) that has their number.
    /// A searched Bible that is gone from the Library or cannot be read fails the job: an
    /// Analysis missing its hits would count, sort and export as if it were whole.
    pub(super) fn analysis(
        results: Arc<BibleResults>,
        obrs: &[Option<PathBuf>],
        lexicons: &[PathBuf],
        ctx: &JobContext,
    ) -> Result<AnalysisOutput, String> {
        let mut readers: HashMap<usize, ObrReader> = HashMap::new();
        let mut texts: HashMap<(usize, UnitId), String> = HashMap::new();
        let lexicons: Vec<ObrReader> = lexicons
            .iter()
            .filter_map(|p| ObrReader::open(p).ok())
            .collect();
        let mut headwords: HashMap<StrongsNumber, Option<String>> = HashMap::new();
        let mut rows = Vec::new();
        for (i, row) in results.rows.iter().enumerate() {
            if i % 256 == 0 && ctx.is_cancelled() {
                return Err("cancelled".into());
            }
            let reference = results.row_reference(row);
            for (k, h) in row.hits.iter().enumerate() {
                let (bible, rtl) = results
                    .bibles
                    .get(h.bible)
                    .map(|b| (b.abbreviation.clone(), b.rtl))
                    .unwrap_or_default();
                let reader = match readers.entry(h.bible) {
                    Entry::Occupied(e) => e.into_mut(),
                    Entry::Vacant(e) => {
                        let path = obrs.get(h.bible).and_then(Option::as_ref).ok_or_else(|| {
                            format!(
                                "{bible} is no longer in the Library: search again to \
                                 analyse these results"
                            )
                        })?;
                        e.insert(
                            ObrReader::open(path)
                                .map_err(|e| format!("Could not open {bible}: {e}"))?,
                        )
                    }
                };
                let text = match texts.entry((h.bible, h.unit.clone())) {
                    Entry::Occupied(e) => e.into_mut(),
                    Entry::Vacant(e) => {
                        let text = reader
                            .unit_text(&h.unit)
                            .map_err(|e| format!("Could not read {bible} at {reference}: {e}"))?
                            .ok_or_else(|| {
                                format!(
                                    "{bible} has no text at {reference}: search again to \
                                     analyse these results"
                                )
                            })?
                            .1;
                        e.insert(text)
                    }
                };
                let range = h.range.start as usize..h.range.end as usize;
                let spans = reader
                    .spans_starting_in(&h.unit, range.clone())
                    .map_err(|e| {
                        format!("Could not read the tags of {bible} at {reference}: {e}")
                    })?;
                let (snippet, tagged) =
                    snippet_and_tagged(text, range, &spans, &results.highlighter);
                let context: Arc<str> = Arc::from(snippet.text.as_str());
                for w in matched_words(&snippet, &tagged, &results.highlighter) {
                    let mut lemmas = w.lemmas;
                    for n in w.unnamed {
                        let found = headwords
                            .entry(n)
                            .or_insert_with(|| headword(&lexicons, n))
                            .clone();
                        lemmas.push(found.unwrap_or_else(|| n.to_string()));
                    }
                    lemmas.dedup();
                    rows.push(AnalysisRow {
                        row: i,
                        hit: k,
                        reference: reference.clone(),
                        bible: bible.clone(),
                        rtl,
                        word: context.get(w.range.clone()).unwrap_or_default().to_string(),
                        lemma: lemmas.join(" "),
                        morphology: w
                            .morph_codes
                            .iter()
                            .map(|c| plain_morphology(c))
                            .collect::<Vec<_>>()
                            .join("; "),
                        morph_codes: w.morph_codes.join(" "),
                        context: Arc::clone(&context),
                        word_range: w.range,
                    });
                }
            }
        }
        Ok(AnalysisOutput(Arc::new(SearchAnalysis { results, rows })))
    }

    /// A Morphology code in plain English (`N-ASF` → `noun, acc. fem. sing.`), as Word Info
    /// shows it; a code the decoder cannot read stays as written.
    fn plain_morphology(code: &str) -> String {
        morph::parse(code)
            .ok()
            .map(|m| m.short())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| code.to_string())
    }

    /// The headword of the first Lexicon entry for `number`.
    fn headword(lexicons: &[ObrReader], number: StrongsNumber) -> Option<String> {
        lexicons.iter().find_map(|reader| {
            let found = reader.lookup_strongs(number).ok()?.into_iter().next()?;
            reader
                .unit(&found.unit)
                .ok()
                .flatten()?
                .title
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
        })
    }
}

// ------------------------------------------------------------------------------- commands

fn results_tab(command: &'static str, s: &Session, p: &Params<'_>) -> Result<TabId, CommandError> {
    let tab = tab_param(command, s, p)?;
    let st = s
        .search_tab(tab)
        .ok_or_else(|| failed(command, "not a Search Tab"))?;
    if st.mode != SearchMode::Bible || st.all_results().is_none() {
        return Err(failed(command, "the Tab has no Bible search results"));
    }
    Ok(tab)
}

/// A Bible book named by its OSIS id (`1John`), name (`1 John`) or abbreviation (`1 Jn`).
fn parse_book(text: &str) -> Option<Book> {
    let t = text.trim();
    Book::from_osis(t).or_else(|| {
        Book::ALL
            .into_iter()
            .find(|b| b.name().eq_ignore_ascii_case(t) || b.abbrev().eq_ignore_ascii_case(t))
    })
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "search.view",
        label: "Search results view",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "value",
                kind: ParamKind::String,
                required: true,
                help: "verses, grid, chart or analysis",
            },
        ],
        enabled: always,
        run: search_view,
    },
    CommandSpec {
        id: "search.filter",
        label: "Filter search results",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "book",
                kind: ParamKind::String,
                required: false,
                help: "Bible book (OSIS id, name or abbreviation); without it the filter is cleared",
            },
            ParamSpec {
                name: "chapter",
                kind: ParamKind::Integer,
                required: false,
                help: "A chapter of the book",
            },
            ParamSpec {
                name: "bible",
                kind: ParamKind::String,
                required: false,
                help: "Only this searched Bible's hits (resource id)",
            },
        ],
        enabled: always,
        run: search_filter,
    },
    CommandSpec {
        id: "search.chart",
        label: "Search chart by",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "value",
                kind: ParamKind::String,
                required: true,
                help: "books, or a Bible book to chart by chapter",
            },
        ],
        enabled: always,
        run: search_chart,
    },
    CommandSpec {
        id: "search.sort",
        label: "Sort search analysis",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "column",
                kind: ParamKind::String,
                required: true,
                help: "reference, bible, word, lemma, morphology or context",
            },
            ParamSpec {
                name: "descending",
                kind: ParamKind::Boolean,
                required: false,
                help: "Sort descending (default: ascending, or the other way if the column is already sorted)",
            },
        ],
        enabled: always,
        run: search_sort,
    },
    CommandSpec {
        id: "search.export_csv",
        label: "Export search analysis as CSV",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "path",
                kind: ParamKind::String,
                required: true,
                help: "The .csv file to write",
            },
        ],
        enabled: always,
        run: search_export_csv,
    },
];

fn search_view(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.view";
    let tab = tab_param(ID, s, &p)?;
    let v = p.str("value").unwrap_or_default();
    let view =
        ResultsView::from_id(v).ok_or_else(|| failed(ID, format!("no results view “{v}”")))?;
    s.set_results_view(tab, view);
    Ok(json!({ "tab": tab.0, "view": view.id() }))
}

fn search_filter(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.filter";
    let tab = results_tab(ID, s, &p)?;
    let filter = match p.str("book").filter(|b| !b.trim().is_empty()) {
        None => None,
        Some(b) => {
            let book = parse_book(b).ok_or_else(|| failed(ID, format!("no Bible book “{b}”")))?;
            let chapter = p
                .i64("chapter")
                .map(|c| u16::try_from(c).map_err(|_| failed(ID, format!("no chapter {c}"))))
                .transpose()?;
            let bible = match p.str("bible") {
                None => None,
                Some(id) => Some(
                    s.search_tab(tab)
                        .and_then(SearchTab::all_results)
                        .and_then(|r| r.bibles.iter().position(|b| b.id == id))
                        .ok_or_else(|| failed(ID, format!("{id} was not searched")))?,
                ),
            };
            Some(ResultsFilter {
                book,
                chapter,
                bible,
            })
        }
    };
    s.set_results_filter(tab, filter);
    let st = s.search_tab(tab);
    let rows = st.map_or(0, SearchTab::row_count);
    let label = st
        .and_then(|st| Some(st.filter?.label(st.all_results()?)))
        .map_or(Value::Null, Value::from);
    Ok(json!({ "tab": tab.0, "rows": rows, "filter": label }))
}

fn search_chart(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.chart";
    let tab = tab_param(ID, s, &p)?;
    let v = p.str("value").unwrap_or_default().trim();
    let by = if v.eq_ignore_ascii_case("books") || v.eq_ignore_ascii_case("book") {
        ChartBy::Book
    } else {
        ChartBy::Chapter(parse_book(v).ok_or_else(|| failed(ID, format!("no Bible book “{v}”")))?)
    };
    if let Some(st) = s.search_tab_mut(tab) {
        st.chart = by;
        st.view = ResultsView::Chart;
    }
    let bars = s
        .search_tab(tab)
        .and_then(SearchTab::all_results)
        .map(|r| chart_bars(r, by))
        .unwrap_or_default();
    let bars: Vec<Value> = bars
        .iter()
        .map(|b| json!({ "label": b.label, "count": b.count }))
        .collect();
    Ok(json!({ "tab": tab.0, "bars": bars }))
}

fn search_sort(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.sort";
    let tab = tab_param(ID, s, &p)?;
    let c = p.str("column").unwrap_or_default();
    let column = AnalysisColumn::from_id(c)
        .ok_or_else(|| failed(ID, format!("no Analysis column “{c}”")))?;
    let current = s
        .search_tab(tab)
        .map(|st| st.analysis_sort)
        .unwrap_or_default();
    let descending = p
        .bool("descending")
        .unwrap_or(current.column == column && !current.descending);
    s.sort_analysis(tab, AnalysisSort { column, descending });
    Ok(json!({ "tab": tab.0, "column": column.id(), "descending": descending }))
}

fn search_export_csv(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "search.export_csv";
    let tab = tab_param(ID, s, &p)?;
    let path = p.str("path").unwrap_or_default().to_string();
    if path.trim().is_empty() {
        return Err(failed(ID, "no path"));
    }
    let records = s
        .export_analysis_csv(tab, std::path::Path::new(&path))
        .map_err(|e| failed(ID, e))?;
    Ok(json!({ "tab": tab.0, "path": path, "records": records }))
}
