//! Visual filters (#211): named rules that colour the words of Resource text matching a
//! Search query, painted live as the user reads.
//!
//! - A **Visual filter** is a Search query (the Search Tool's grammar: words, phrases,
//!   `strong:`, `lemma:`, `@…` Morphology criteria), optionally text the Resource marks as
//!   Words of Christ or a divine name, an optional Passage list it is limited to (its scope,
//!   in the Standard versification, read like the Search Tool's Scope field), and a style: a
//!   colour with a look (text colour, underline, background or box), the same looks Highlight
//!   styles have. A word is coloured when it matches the query **or** lies in the marked text.
//! - A filter is **on in all Tabs** or not. The Tab header's Filters menu switches each filter
//!   for that Tab: off in this Tab for one on in all Tabs, on in this Tab for the others. The
//!   Tab keeps the filters it switched ([`TabFilters`]), saved in Layouts and the Last session.
//! - Filters live in the User database, as the `reading.visual_filters` preference (JSON),
//!   like the Collections. Until the user changes them the three built-ins are there: Words of
//!   Christ, Divine names and Imperatives (off until switched on).
//! - **Matching** ([`FilterMatcher`], [`block_hits`]) is pure and runs per visible block on
//!   a worker in the UI. The query is read as the Search Tool reads it, one document (Verse,
//!   or paragraph outside Bibles: [`TextUnit::paragraphs`]) at a time: `NOT`, implicit `AND`
//!   and proximity decide whether a document matches before any of its words are coloured. A
//!   filter limited to Passages reads only the Verses in them (so nothing outside Bibles),
//!   and `WITHIN n` counts documents as the Search index numbers them, the ones out of scope
//!   included. A Bible that is not in Hebrew, Aramaic or Greek has no Morphology or
//!   Lemmas of its own (KJV+Strong's tags numbers only), so the `@…` and `lemma:` terms reach
//!   it through **Strong's alignment** (ADR 0015): in each Verse, the words whose Strong's
//!   number is that of a matching word in the same Standard Verse of the first Hebrew (Old
//!   Testament) or Greek (New Testament) Bible in Preferred order, as the Exegetical Guide
//!   picks it.

use std::collections::BTreeSet;
use std::ops::Range;

use obs_model::{MorphemePart, SpanKind, TextUnit};
use obs_scripture::{Passage, PassageList, StrongsNumber};
use obs_search::{Highlighter, LemmaLookup, Query};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bible_search::{Scope, parse_scope};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::workspace::{Effect, TabId, TabKind};
use obs_store::{HighlightLook, Rgb, StyleAppearance};

/// The User database preference that keeps the filters.
pub const VISUAL_FILTERS_PREF: &str = "reading.visual_filters";

/// The User database preference that keeps the number of the next new filter's id, so a
/// deleted filter's id is never given to another: a Layout that switched the deleted filter
/// must not switch the new one.
pub const VISUAL_FILTERS_NEXT_ID_PREF: &str = "reading.visual_filters_next_id";

/// Longest filter name, in characters.
pub const MAX_FILTER_NAME: usize = 60;

/// STEPBible numbers prefixes, suffixes and other grammatical morphemes from 9001 up: they
/// are never aligned (every Hebrew word has its `וְ`).
const AFFIX_NUMBERS_FROM: u32 = 9000;

/// Text a Resource marks, that a filter can colour whatever its query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkedText {
    /// Words the Resource marks as spoken by Jesus (USFM `\wj`, OSIS `<q who="Jesus">`).
    WordsOfChrist,
    /// Divine names the Resource marks (USFM `\nd`, OSIS `<divineName>`: "LORD").
    DivineNames,
}

impl MarkedText {
    pub const ALL: [MarkedText; 2] = [MarkedText::WordsOfChrist, MarkedText::DivineNames];

    /// Its id in commands and the stored JSON.
    pub fn id(self) -> &'static str {
        match self {
            MarkedText::WordsOfChrist => "words_of_christ",
            MarkedText::DivineNames => "divine_names",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }

    /// As the UI names it.
    pub fn label(self) -> &'static str {
        match self {
            MarkedText::WordsOfChrist => "Words of Christ",
            MarkedText::DivineNames => "Divine names",
        }
    }

    fn marks(self, kind: &SpanKind) -> bool {
        matches!(
            (self, kind),
            (MarkedText::WordsOfChrist, SpanKind::Woj)
                | (MarkedText::DivineNames, SpanKind::DivineName)
        )
    }
}

/// One **Visual filter**: a Search query (and/or marked text) and the style its matches are
/// painted in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualFilter {
    /// Stable id (the built-ins' are `words-of-christ`, `divine-names`, `imperatives`).
    pub id: String,
    pub name: String,
    /// A Search query; may be empty when `marked` is set.
    #[serde(default)]
    pub query: String,
    /// Text the Resource marks that is coloured too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marked: Option<MarkedText>,
    /// Passages it is limited to (the Search Tool's Scope field); empty: everywhere.
    #[serde(default)]
    pub scope: String,
    pub look: HighlightLook,
    pub color: Rgb,
    /// On in every Tab unless a Tab switched it off.
    #[serde(default)]
    pub all_tabs: bool,
}

impl VisualFilter {
    /// How its matches are drawn: the colour and look, each Page tone's colour derived when
    /// drawn (ADR 0012), so a theme switch needs no relayout.
    pub fn appearance(&self) -> StyleAppearance {
        StyleAppearance::new(self.look, self.color)
    }
}

/// The Strong's numbers the built-in Divine names filter finds: YHWH (and its Elohim
/// reading), Yah, Adonai, Elohim, El, Eloah and Shaddai, and θεός.
const DIVINE_NAME_NUMBERS: &str = "strong:H3068 OR strong:H3069 OR strong:H3050 OR strong:H136 \
     OR strong:H430 OR strong:H410 OR strong:H433 OR strong:H7706 OR strong:G2316";

/// The filters a new User database has: off until the user switches them on.
pub fn built_in_filters() -> Vec<VisualFilter> {
    vec![
        VisualFilter {
            id: "words-of-christ".into(),
            name: "Words of Christ".into(),
            query: String::new(),
            marked: Some(MarkedText::WordsOfChrist),
            scope: String::new(),
            look: HighlightLook::TextColor,
            color: Rgb::new(200, 40, 40),
            all_tabs: false,
        },
        VisualFilter {
            id: "divine-names".into(),
            name: "Divine names".into(),
            query: DIVINE_NAME_NUMBERS.into(),
            marked: Some(MarkedText::DivineNames),
            scope: String::new(),
            look: HighlightLook::Box,
            color: Rgb::new(201, 162, 39),
            all_tabs: false,
        },
        VisualFilter {
            id: "imperatives".into(),
            name: "Imperatives".into(),
            query: "@imperative".into(),
            marked: None,
            scope: String::new(),
            look: HighlightLook::Underline,
            color: Rgb::new(20, 130, 120),
            all_tabs: false,
        },
    ]
}

/// The filters a Tab switched from their default (see the module docs). Kept per Tab, saved in
/// Layouts and the Last session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabFilters {
    /// Ids of filters switched in this Tab: on here for a filter not on in all Tabs, off here
    /// for one that is.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub switched: BTreeSet<String>,
}

impl TabFilters {
    pub fn is_default(&self) -> bool {
        self.switched.is_empty()
    }

    /// Whether `filter` shows in this Tab.
    pub fn shows(&self, filter: &VisualFilter) -> bool {
        filter.all_tabs != self.switched.contains(&filter.id)
    }
}

/// The filters as the Session keeps them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VisualFilters {
    pub(crate) list: Vec<VisualFilter>,
    /// Bumped by every change to `list`.
    pub(crate) revision: u64,
    /// The number of the next new filter's id (`filter-<n>`): ids are never reused.
    pub(crate) next_id: u64,
}

impl Default for VisualFilters {
    fn default() -> Self {
        Self {
            list: built_in_filters(),
            revision: 0,
            next_id: 1,
        }
    }
}

// ------------------------------------------------------------------------------- matching

/// A filter's Search query as a condition on one document, read as the Search Tool reads it:
/// a document (a Verse in a Bible, a paragraph elsewhere) is coloured only when it satisfies the
/// whole query, and then only in the terms that made it so. `bless NOT persecute` colours
/// nothing in a Verse that says "persecute"; `bless curse` nothing in a Verse without both.
#[derive(Debug, Clone)]
enum Condition {
    /// One term (a word, phrase, `strong:`, `lemma:` or `@…`): the words it marks.
    Term(Highlighter),
    /// Every one of them (`love neighbour`); a `NOT` among them colours nothing.
    All(Vec<Condition>),
    /// Any of them (`faith OR hope`).
    Any(Vec<Condition>),
    /// `NOT x`, `-x`: satisfied where `x` is not.
    Not(Box<Condition>),
    /// `NEAR/n`, `WITHIN n words` (either order) and `BEFORE/n` (`left` first): words or
    /// phrases of each side with at most `words` words between them.
    Near {
        left: Highlighter,
        right: Highlighter,
        words: u32,
        ordered: bool,
    },
    /// `WITHIN n verses`: each side in a document at most `documents - 1` documents from one
    /// the other side is in.
    Within {
        left: Box<Condition>,
        right: Box<Condition>,
        documents: u32,
    },
}

impl Condition {
    fn new(q: &Query, lemmas: &dyn LemmaLookup) -> Self {
        let all = |qs: &[Query]| qs.iter().map(|q| Condition::new(q, lemmas)).collect();
        match q {
            Query::And(qs) => Condition::All(all(qs)),
            Query::Or(qs) => Condition::Any(all(qs)),
            Query::Not { query, .. } => Condition::Not(Box::new(Condition::new(query, lemmas))),
            Query::Near {
                left,
                right,
                words,
                ordered,
                ..
            } => Condition::Near {
                left: Highlighter::new(left, lemmas),
                right: Highlighter::new(right, lemmas),
                words: *words,
                ordered: *ordered,
            },
            Query::Within {
                left,
                right,
                documents,
                ..
            } => Condition::Within {
                left: Box::new(Condition::new(left, lemmas)),
                right: Box::new(Condition::new(right, lemmas)),
                documents: *documents,
            },
            Query::Word { .. }
            | Query::Phrase { .. }
            | Query::Stem { .. }
            | Query::Strong { .. }
            | Query::Lemma { .. }
            | Query::Morph { .. } => Condition::Term(Highlighter::new(q, lemmas)),
        }
    }

    /// Whether any term asks for Morphology or a Lemma.
    fn uses_original_language_data(&self) -> bool {
        match self {
            Condition::Term(h) => h.uses_original_language_data(),
            Condition::All(cs) | Condition::Any(cs) => {
                cs.iter().any(Condition::uses_original_language_data)
            }
            Condition::Not(c) => c.uses_original_language_data(),
            Condition::Near { left, right, .. } => {
                left.uses_original_language_data() || right.uses_original_language_data()
            }
            Condition::Within { left, right, .. } => {
                left.uses_original_language_data() || right.uses_original_language_data()
            }
        }
    }

    /// The most documents away a `WITHIN n verses` looks, nested ones added: in
    /// `(a WITHIN 2 verses b) WITHIN 2 verses c`, `c` is read with `b` a Verse away and `b`
    /// with `a` a Verse further.
    fn reach(&self) -> usize {
        match self {
            Condition::Term(_) | Condition::Near { .. } => 0,
            Condition::All(cs) | Condition::Any(cs) => {
                cs.iter().map(Condition::reach).max().unwrap_or(0)
            }
            Condition::Not(c) => c.reach(),
            Condition::Within {
                left,
                right,
                documents,
            } => (documents.saturating_sub(1) as usize)
                .saturating_add(left.reach().max(right.reach())),
        }
    }

    /// For each of `docs`: `None` where it does not satisfy the condition, else the bytes
    /// (of the unit) of the terms that satisfied it.
    fn eval(&self, docs: &[Document], cx: &mut MatchContext<'_>) -> Vec<Option<Vec<Range<usize>>>> {
        let found = |ranges: Vec<Range<usize>>| (!ranges.is_empty()).then_some(ranges);
        match self {
            Condition::Term(h) => docs.iter().map(|d| found(cx.term(h, d))).collect(),
            Condition::All(cs) => {
                let mut out: Vec<Option<Vec<Range<usize>>>> = vec![Some(Vec::new()); docs.len()];
                for c in cs {
                    if out.iter().all(Option::is_none) {
                        break;
                    }
                    for (o, r) in out.iter_mut().zip(c.eval(docs, cx)) {
                        match (o.as_mut(), r) {
                            (Some(o), Some(r)) => o.extend(r),
                            _ => *o = None,
                        }
                    }
                }
                out
            }
            Condition::Any(cs) => {
                let mut out: Vec<Option<Vec<Range<usize>>>> = vec![None; docs.len()];
                for c in cs {
                    for (o, r) in out.iter_mut().zip(c.eval(docs, cx)) {
                        if let Some(r) = r {
                            o.get_or_insert_with(Vec::new).extend(r);
                        }
                    }
                }
                out
            }
            Condition::Not(c) => c
                .eval(docs, cx)
                .into_iter()
                .map(|r| r.is_none().then(Vec::new))
                .collect(),
            Condition::Near {
                left,
                right,
                words,
                ordered,
            } => docs
                .iter()
                .map(|d| found(cx.near(left, right, *words, *ordered, d)))
                .collect(),
            Condition::Within {
                left,
                right,
                documents,
            } => {
                let reach = documents.saturating_sub(1) as usize;
                let l = left.eval(docs, cx);
                let r = right.eval(docs, cx);
                // A side counts in document `i` when the other side is in a neighbour of it at
                // most `reach` documents away, counted by ordinal so that documents left out
                // of a filter's scope still count.
                let partnered = |i: usize, other: &[Option<Vec<Range<usize>>>]| {
                    (0..docs.len()).any(|j| {
                        other[j].is_some()
                            && docs[i].ordinal.abs_diff(docs[j].ordinal) <= reach
                            && docs[i].neighbours(&docs[j])
                    })
                };
                (0..docs.len())
                    .map(|i| {
                        let mut out: Option<Vec<Range<usize>>> = None;
                        if let Some(ranges) = &l[i]
                            && partnered(i, &r)
                        {
                            out.get_or_insert_with(Vec::new)
                                .extend(ranges.iter().cloned());
                        }
                        if let Some(ranges) = &r[i]
                            && partnered(i, &l)
                        {
                            out.get_or_insert_with(Vec::new)
                                .extend(ranges.iter().cloned());
                        }
                        out
                    })
                    .collect()
            }
        }
    }
}

/// One document a filter's query is read against: a Verse's bytes in the unit and its
/// Standard Passage, or a paragraph outside Bibles.
#[derive(Debug, Clone)]
struct Document {
    range: Range<usize>,
    standard: Option<Passage>,
    /// Its position among the documents [`block_hits`] was given, which `WITHIN n` counts.
    ordinal: usize,
}

impl Document {
    /// Whether `WITHIN n verses` can join the two: Verses of one book, or paragraphs.
    fn neighbours(&self, other: &Document) -> bool {
        match (self.standard, other.standard) {
            (Some(a), Some(b)) => a.start().book == b.start().book,
            (None, None) => true,
            _ => false,
        }
    }
}

/// What reading a query against the documents of one block needs.
struct MatchContext<'a> {
    unit: &'a TextUnit,
    /// The block's Bible has no original-language data: `@…` and `lemma:` terms take the
    /// words aligned with the original-language Bible's.
    align: bool,
    original: &'a mut dyn FnMut(Passage) -> AlignedWords,
    /// Original words, looked up once per Verse whichever filters need them.
    aligned: std::collections::HashMap<Passage, AlignedWords>,
}

impl MatchContext<'_> {
    /// The bytes term `h` marks in `doc`: its own words, and in an aligned Bible the words
    /// whose Strong's number is that of a word `h` marks in the original-language Verse.
    fn term(&mut self, h: &Highlighter, doc: &Document) -> Vec<Range<usize>> {
        let Some(slice) = self.unit.text.get(doc.range.clone()) else {
            return Vec::new();
        };
        let tagged = tagged_in(self.unit, doc.range.clone());
        let base = doc.range.start;
        let local = tagged
            .iter()
            .map(|(r, parts)| (r.start - base..r.end - base, *parts));
        let mut out: Vec<Range<usize>> = h
            .ranges(slice, local)
            .into_iter()
            .map(|r| r.start + base..r.end + base)
            .collect();
        if self.align
            && h.uses_original_language_data()
            && let Some(standard) = doc.standard
        {
            let original = &mut self.original;
            let words = self
                .aligned
                .entry(standard)
                .or_insert_with(|| original(standard));
            let numbers = aligned_numbers(h, words.iter().map(Vec::as_slice));
            if !numbers.is_empty() {
                for (range, parts) in &tagged {
                    if parts.iter().any(|p| {
                        p.strongs.number() < AFFIX_NUMBERS_FROM
                            && numbers.contains(&p.strongs.base())
                    }) {
                        out.push(range.clone());
                    }
                }
            }
        }
        out
    }

    /// The occurrences of `left` and `right` in `doc` that have one of the other side at most
    /// `words` words away (after it when `ordered`), counted as the Search index counts them.
    fn near(
        &self,
        left: &Highlighter,
        right: &Highlighter,
        words: u32,
        ordered: bool,
        doc: &Document,
    ) -> Vec<Range<usize>> {
        let Some(slice) = self.unit.text.get(doc.range.clone()) else {
            return Vec::new();
        };
        let tokens = obs_model::tokenize(slice);
        // Each occurrence as its first and last word.
        let occurrences = |h: &Highlighter| -> Vec<(usize, usize, Range<usize>)> {
            h.ranges(slice, std::iter::empty())
                .into_iter()
                .filter_map(|r| {
                    let first = tokens.iter().position(|t| t.range.start >= r.start)?;
                    let last = tokens.iter().rposition(|t| t.range.end <= r.end)?;
                    (first <= last).then_some((first, last, r))
                })
                .collect()
        };
        let a = occurrences(left);
        let b = occurrences(right);
        let within = |(_, end, _): &(usize, usize, Range<usize>),
                      (start, _, _): &(usize, usize, Range<usize>)| {
            *start > *end && *start - *end - 1 <= words as usize
        };
        let paired = |x: &(usize, usize, Range<usize>), y: &(usize, usize, Range<usize>)| {
            within(x, y) || (!ordered && within(y, x))
        };
        let base = doc.range.start;
        let mut out = Vec::new();
        for x in &a {
            if b.iter().any(|y| paired(x, y)) {
                out.push(x.2.start + base..x.2.end + base);
            }
        }
        for y in &b {
            if a.iter().any(|x| paired(x, y)) {
                out.push(y.2.start + base..y.2.end + base);
            }
        }
        out
    }
}

/// The Strong's numbers (base forms, affixes left out) of the words among `words` (the
/// morphemes of each Tagged word of one original-language Verse) that term `h` marks.
fn aligned_numbers<'a>(
    h: &Highlighter,
    words: impl IntoIterator<Item = &'a [MorphemePart]>,
) -> BTreeSet<StrongsNumber> {
    let mut out = BTreeSet::new();
    for parts in words {
        for (part, matched) in parts.iter().zip(h.matching_parts(parts)) {
            if matched && part.strongs.number() < AFFIX_NUMBERS_FROM {
                out.insert(part.strongs.base());
            }
        }
    }
    out
}

/// One filter ready to match text.
#[derive(Debug, Clone)]
pub struct FilterMatcher {
    pub id: String,
    pub appearance: StyleAppearance,
    /// Its query; `None` when it has none (it colours marked text only).
    condition: Option<Condition>,
    marked: Option<MarkedText>,
    /// The Standard Passages it is limited to; `None`: everywhere.
    scope: Option<PassageList>,
}

impl FilterMatcher {
    /// `filter` compiled; `lemmas` resolves transliterated `lemma:` terms as a search does.
    pub fn new(filter: &VisualFilter, lemmas: &dyn LemmaLookup) -> Result<Self, String> {
        let condition = if filter.query.trim().is_empty() {
            None
        } else {
            let parsed = obs_search::parse(&filter.query).map_err(|e| e.kind.to_string())?;
            Some(Condition::new(&parsed, lemmas))
        };
        let scope = match parse_scope(&filter.scope).map_err(|e| e.message)? {
            Scope::WholeBible => None,
            Scope::Passages(list) => Some(list),
        };
        Ok(Self {
            id: filter.id.clone(),
            appearance: filter.appearance(),
            condition,
            marked: filter.marked,
            scope,
        })
    }

    /// Whether it is limited to some Passages.
    pub fn is_scoped(&self) -> bool {
        self.scope.is_some()
    }

    /// Whether text at a Standard Passage (`None`: outside any mapped Verse) is in scope.
    pub fn in_scope(&self, standard: Option<Passage>) -> bool {
        match (&self.scope, standard) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(list), Some(p)) => list
                .iter()
                .any(|s| s.start() <= p.end() && p.start() <= s.end()),
        }
    }

    /// Whether its query asks for Morphology or a Lemma, which only an original-language
    /// Bible carries: other Bibles are coloured through Strong's alignment.
    pub fn needs_alignment(&self) -> bool {
        self.condition
            .as_ref()
            .is_some_and(Condition::uses_original_language_data)
    }

    /// How many documents (Verses, or paragraphs outside Bibles) on either side of a block
    /// its query reads (`WITHIN n`, nested ones added), so that the documents passed to
    /// [`block_hits`] can include them.
    pub fn context_documents(&self) -> usize {
        self.condition.as_ref().map_or(0, Condition::reach)
    }
}

/// One document inside or near a block, as the Search index reads the Resource: a Bible
/// Verse (its bytes in the unit and its Standard Passage, `None` for an Unmapped verse), or
/// outside Bibles a paragraph (`standard` is `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentSlice {
    pub range: Range<usize>,
    pub standard: Option<Passage>,
}

/// The paragraphs of `unit` (outside Bibles) that hold a word and overlap `block`, and up to
/// `context` more on either side ([`FilterMatcher::context_documents`]), in order: the
/// documents [`block_hits`] reads a block outside Bibles against.
pub fn paragraph_slices(
    unit: &TextUnit,
    block: Range<usize>,
    context: usize,
) -> Vec<DocumentSlice> {
    let paragraphs: Vec<Range<usize>> = unit
        .paragraphs()
        .into_iter()
        .filter(|p| {
            unit.text
                .get(p.clone())
                .is_some_and(|t| !obs_model::tokenize(t).is_empty())
        })
        .collect();
    let inside = |p: &Range<usize>| p.start < block.end && block.start < p.end;
    let (Some(first), Some(last)) = (
        paragraphs.iter().position(inside),
        paragraphs.iter().rposition(inside),
    ) else {
        return Vec::new();
    };
    let wanted = first.saturating_sub(context)
        ..last
            .saturating_add(context)
            .saturating_add(1)
            .min(paragraphs.len());
    paragraphs[wanted]
        .iter()
        .map(|p| DocumentSlice {
            range: p.clone(),
            standard: None,
        })
        .collect()
}

/// Bytes of a unit that one filter colours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterHit {
    /// Index into the filters matched.
    pub filter: usize,
    pub range: Range<usize>,
}

/// What an original-language Bible says at a Standard Passage, for Strong's alignment: the
/// morphemes of each of its Tagged words there (none when it lacks the Passage).
pub type AlignedWords = Vec<Vec<MorphemePart>>;

/// The Tagged words of `unit` inside `bytes`, with their morphemes.
fn tagged_in(unit: &TextUnit, bytes: Range<usize>) -> Vec<(Range<usize>, &[MorphemePart])> {
    unit.spans
        .iter()
        .filter(|s| bytes.start <= s.start as usize && s.end as usize <= bytes.end)
        .filter_map(|s| match &s.kind {
            SpanKind::TaggedWord { parts } if !parts.is_empty() && s.start < s.end => {
                Some((s.start as usize..s.end as usize, parts.as_slice()))
            }
            _ => None,
        })
        .collect()
}

/// What `filters` colour in `block` (bytes of `unit`), sorted by filter then start.
///
/// - `documents`: the documents of the unit the block holds, consecutive and in order, and
///   any on either side a `WITHIN n` query reads ([`FilterMatcher::context_documents`]):
///   Bible Verses, or outside Bibles paragraphs ([`paragraph_slices`]). Each is read as the
///   Search Tool reads it; `WITHIN n` counts their positions in this list. A filter limited
///   to Passages reads only the Verses in them (nothing outside Bibles), its marked text
///   too. Only the block's bytes are coloured.
/// - `align`: the block is in a Bible without original-language data of its own; `original`
///   gives the aligned original-language words at a Standard Passage (see the module docs).
pub fn block_hits(
    filters: &[FilterMatcher],
    unit: &TextUnit,
    block: Range<usize>,
    documents: &[DocumentSlice],
    align: bool,
    original: &mut dyn FnMut(Passage) -> AlignedWords,
) -> Vec<FilterHit> {
    let text = unit.text.as_str();
    let block = block.start.min(text.len())..block.end.min(text.len());
    let clip = |r: &Range<usize>| r.start.max(block.start)..r.end.min(block.end);
    let mut cx = MatchContext {
        unit,
        align,
        original,
        aligned: Default::default(),
    };
    let mut out = Vec::new();
    for (k, f) in filters.iter().enumerate() {
        let docs: Vec<Document> = documents
            .iter()
            .enumerate()
            .filter(|(_, d)| f.in_scope(d.standard))
            .map(|(ordinal, d)| Document {
                range: d.range.start.min(text.len())..d.range.end.min(text.len()),
                standard: d.standard,
                ordinal,
            })
            .collect();
        let mut ranges: Vec<Range<usize>> = Vec::new();
        if let Some(condition) = &f.condition {
            for found in condition.eval(&docs, &mut cx).into_iter().flatten() {
                ranges.extend(found.iter().map(clip));
            }
        }
        if let Some(marked) = f.marked {
            let segments: Vec<Range<usize>> = if f.is_scoped() {
                docs.iter().map(|d| clip(&d.range)).collect()
            } else {
                vec![block.clone()]
            };
            for seg in segments.iter().filter(|s| s.start < s.end) {
                ranges.extend(
                    unit.spans
                        .iter()
                        .filter(|s| marked.marks(&s.kind))
                        .map(|s| s.start as usize..s.end as usize)
                        .filter(|r| r.start < seg.end && seg.start < r.end)
                        .map(|r| r.start.max(seg.start)..r.end.min(seg.end)),
                );
            }
        }
        out.extend(
            obs_search::highlight::merge(ranges)
                .into_iter()
                .map(|range| FilterHit { filter: k, range }),
        );
    }
    out
}

// --------------------------------------------------------------------------------- session

impl Session {
    /// Every Visual filter, in the order the user made them.
    pub fn visual_filters(&self) -> &[VisualFilter] {
        &self.visual_filters.list
    }

    /// Moves on every change to the filters (not to which a Tab shows).
    pub fn visual_filters_revision(&self) -> u64 {
        self.visual_filters.revision
    }

    /// The filters Tab `tab` shows, in order: none for a Tab that shows no Resource.
    pub fn tab_visual_filters(&self, tab: TabId) -> Vec<VisualFilter> {
        let Some(t) = self.workspace.tab(tab) else {
            return Vec::new();
        };
        if !matches!(t.kind, TabKind::Resource(_)) {
            return Vec::new();
        }
        self.visual_filters
            .list
            .iter()
            .filter(|f| t.visual_filters.shows(f))
            .cloned()
            .collect()
    }

    /// Read the filters from the User database: the built-ins until the user changes them.
    pub(crate) fn load_visual_filters(&mut self) {
        let stored = self
            .user
            .as_ref()
            .and_then(|u| u.preference(VISUAL_FILTERS_PREF).ok().flatten())
            .and_then(|json| serde_json::from_str::<Vec<VisualFilter>>(&json).ok());
        self.visual_filters.list = stored.unwrap_or_else(built_in_filters);
        let next = self
            .user
            .as_ref()
            .and_then(|u| u.preference(VISUAL_FILTERS_NEXT_ID_PREF).ok().flatten())
            .and_then(|n| n.trim().parse::<u64>().ok())
            .unwrap_or(1);
        self.visual_filters.next_id = next.max(after_ids(&self.visual_filters.list));
        self.visual_filters.revision += 1;
    }

    fn save_visual_filters(&mut self) {
        self.visual_filters.revision += 1;
        let Some(user) = &self.user else {
            return;
        };
        let json = serde_json::to_string(&self.visual_filters.list).unwrap_or_default();
        let next = self.visual_filters.next_id.to_string();
        if let Err(e) = user
            .set_preference(VISUAL_FILTERS_NEXT_ID_PREF, &next)
            .and_then(|()| user.set_preference(VISUAL_FILTERS_PREF, &json))
        {
            self.workspace.push_effect(Effect::Message(format!(
                "Could not save the Visual filters: {e}"
            )));
        }
    }

    /// Forget every Tab's switch of filter `id` (it was deleted, or now shows in all Tabs or
    /// no longer does), so each Tab shows it as the filter now says.
    fn reset_tab_switches(&mut self, id: &str) {
        let tabs: Vec<TabId> = self.workspace.tabs().map(|t| t.id).collect();
        for tab in tabs {
            if let Some(t) = self.workspace.tab_mut(tab) {
                t.visual_filters.switched.remove(id);
            }
        }
    }
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// A filter as commands answer it.
pub fn filter_json(f: &VisualFilter) -> Value {
    json!({
        "id": f.id,
        "name": f.name,
        "query": f.query,
        "marked": f.marked.map(MarkedText::id),
        "scope": f.scope,
        "look": f.look.id(),
        "color": f.color.to_string(),
        "all_tabs": f.all_tabs,
    })
}

/// Check what a filter would match, as the Search Tool checks a query: an error names the
/// field it is about.
pub fn check_filter(f: &VisualFilter) -> Result<(), String> {
    if f.name.trim().is_empty() {
        return Err("A Visual filter needs a name".into());
    }
    if f.name.chars().count() > MAX_FILTER_NAME {
        return Err(format!(
            "A Visual filter's name is at most {MAX_FILTER_NAME} characters"
        ));
    }
    if f.query.trim().is_empty() && f.marked.is_none() {
        return Err("A Visual filter needs a query or marked text to colour".into());
    }
    if !f.query.trim().is_empty() {
        crate::bible_search::check_query(&f.query).map_err(|e| format!("Query: {}", e.message))?;
    }
    parse_scope(&f.scope).map_err(|e| format!("Passages: {}", e.message))?;
    Ok(())
}

/// `filter.save`: make a filter, or change one (by `id`).
fn run_save(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "filter.save";
    let existing = match p.str("id").map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => Some(
            s.visual_filters
                .list
                .iter()
                .position(|f| f.id == id)
                .ok_or_else(|| failed(ID, format!("there is no Visual filter {id}")))?,
        ),
        None => None,
    };
    let mut f = match existing {
        Some(i) => s.visual_filters.list[i].clone(),
        None => VisualFilter {
            id: format!("filter-{}", s.visual_filters.next_id),
            name: String::new(),
            query: String::new(),
            marked: None,
            scope: String::new(),
            look: HighlightLook::Background,
            color: Rgb::new(255, 236, 140),
            all_tabs: false,
        },
    };
    if let Some(name) = p.str("name") {
        f.name = name.trim().to_string();
    }
    if let Some(query) = p.str("query") {
        f.query = query.trim().to_string();
    }
    if let Some(marked) = p.str("marked") {
        f.marked = match marked.trim() {
            "" | "none" => None,
            id => Some(MarkedText::from_id(id).ok_or_else(|| {
                failed(
                    ID,
                    format!("“{id}” is not marked text (words_of_christ, divine_names or none)"),
                )
            })?),
        };
    }
    if let Some(scope) = p.str("scope") {
        f.scope = scope.trim().to_string();
    }
    if let Some(look) = p.str("look") {
        f.look = HighlightLook::from_id(look.trim()).ok_or_else(|| {
            failed(
                ID,
                format!("“{look}” is not a look (background, underline, box or text_color)"),
            )
        })?;
    }
    if let Some(color) = p.str("color") {
        f.color = color.parse().map_err(|e: String| failed(ID, e))?;
    }
    let was_all = f.all_tabs;
    if let Some(all) = p.bool("all_tabs") {
        f.all_tabs = all;
    }
    check_filter(&f).map_err(|e| failed(ID, e))?;
    if existing.is_some() && was_all != f.all_tabs {
        s.reset_tab_switches(&f.id);
    }
    let out = filter_json(&f);
    match existing {
        Some(i) => s.visual_filters.list[i] = f,
        None => {
            s.visual_filters.list.push(f);
            s.visual_filters.next_id += 1;
        }
    }
    s.save_visual_filters();
    Ok(out)
}

/// The number after the highest `filter-<n>` id in `list` (1 when there is none): the least
/// the next id can be, should the kept counter be missing.
fn after_ids(list: &[VisualFilter]) -> u64 {
    list.iter()
        .filter_map(|f| f.id.strip_prefix("filter-")?.parse::<u64>().ok())
        .max()
        .map_or(1, |n| n.saturating_add(1))
}

/// `filter.delete`
fn run_delete(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "filter.delete";
    let id = p.str("id").unwrap_or_default().trim().to_string();
    let Some(i) = s.visual_filters.list.iter().position(|f| f.id == id) else {
        return Err(failed(ID, format!("there is no Visual filter {id}")));
    };
    let gone = s.visual_filters.list.remove(i);
    s.reset_tab_switches(&id);
    s.save_visual_filters();
    Ok(filter_json(&gone))
}

/// `tab.visual_filter`: show or hide a filter in one Resource Tab.
fn run_tab_filter(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "tab.visual_filter";
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .ok_or_else(|| failed(ID, "no tab"))?;
    let id = p.str("id").unwrap_or_default().trim().to_string();
    let Some(filter) = s.visual_filters.list.iter().find(|f| f.id == id).cloned() else {
        return Err(failed(ID, format!("there is no Visual filter {id}")));
    };
    let Some(t) = s.workspace.tab_mut(tab) else {
        return Err(failed(ID, format!("no Tab {}", tab.0)));
    };
    if !matches!(t.kind, TabKind::Resource(_)) {
        return Err(failed(
            ID,
            "Visual filters colour the text of a Resource Tab",
        ));
    }
    let on = p.bool("on").unwrap_or(!t.visual_filters.shows(&filter));
    if on != filter.all_tabs {
        t.visual_filters.switched.insert(id);
    } else {
        t.visual_filters.switched.remove(&id);
    }
    let shown: Vec<String> = s
        .tab_visual_filters(tab)
        .into_iter()
        .map(|f| f.id)
        .collect();
    Ok(json!({ "on": on, "shown": shown }))
}

const FILTER_ID: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::String,
    required: true,
    help: "The Visual filter's id (filter.list)",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "filter.save",
        label: "Save a Visual filter",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "id",
                kind: ParamKind::String,
                required: false,
                help: "The filter to change; without it, a new filter",
            },
            ParamSpec {
                name: "name",
                kind: ParamKind::String,
                required: false,
                help: "Its name (a new filter needs one)",
            },
            ParamSpec {
                name: "query",
                kind: ParamKind::String,
                required: false,
                help: "A Search query: `@imperative`, `strong:G26`, `\"love one another\"`",
            },
            ParamSpec {
                name: "marked",
                kind: ParamKind::String,
                required: false,
                help: "Marked text coloured too: words_of_christ, divine_names or none",
            },
            ParamSpec {
                name: "scope",
                kind: ParamKind::String,
                required: false,
                help: "Passages it is limited to, as the Search scope reads them (`Rom 12`); empty: everywhere",
            },
            ParamSpec {
                name: "look",
                kind: ParamKind::String,
                required: false,
                help: "background, underline, box or text_color",
            },
            ParamSpec {
                name: "color",
                kind: ParamKind::String,
                required: false,
                help: "#rrggbb",
            },
            ParamSpec {
                name: "all_tabs",
                kind: ParamKind::Boolean,
                required: false,
                help: "On in every Tab (true) or only where a Tab switches it on (false)",
            },
        ],
        enabled: always,
        run: run_save,
    },
    CommandSpec {
        id: "filter.delete",
        label: "Delete a Visual filter",
        shortcut: None,
        params: &[FILTER_ID],
        enabled: always,
        run: run_delete,
    },
    CommandSpec {
        id: "filter.list",
        label: "List the Visual filters",
        shortcut: None,
        params: &[],
        enabled: always,
        run: |s, _| {
            Ok(Value::Array(
                s.visual_filters.list.iter().map(filter_json).collect(),
            ))
        },
    },
    CommandSpec {
        id: "tab.visual_filter",
        label: "Show a Visual filter in a Tab",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "tab",
                kind: ParamKind::Integer,
                required: true,
                help: "The Resource Tab",
            },
            FILTER_ID,
            ParamSpec {
                name: "on",
                kind: ParamKind::Boolean,
                required: false,
                help: "Show (true) or hide (false) it in this Tab; without it, toggle",
            },
        ],
        enabled: always,
        run: run_tab_filter,
    },
];
