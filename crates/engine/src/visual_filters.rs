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
//!   a worker in the UI. A Bible that is not in Hebrew, Aramaic or Greek has no Morphology or
//!   Lemmas of its own (KJV+Strong's tags numbers only), so the `@…` and `lemma:` terms reach
//!   it through **Strong's alignment** (ADR 0015): in each Verse, the words whose Strong's
//!   number is that of a matching word in the same Standard Verse of the first Hebrew (Old
//!   Testament) or Greek (New Testament) Bible in Preferred order, as the Exegetical Guide
//!   picks it.

use std::collections::BTreeSet;
use std::ops::Range;

use obs_model::{MorphemePart, SpanKind, TextUnit};
use obs_scripture::{Passage, PassageList, StrongsNumber};
use obs_search::{Highlighter, LemmaLookup};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bible_search::{Scope, parse_scope};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::workspace::{Effect, TabId, TabKind};
use obs_store::{HighlightLook, Rgb, StyleAppearance};

/// The User database preference that keeps the filters.
pub const VISUAL_FILTERS_PREF: &str = "reading.visual_filters";

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
}

impl Default for VisualFilters {
    fn default() -> Self {
        Self {
            list: built_in_filters(),
            revision: 0,
        }
    }
}

// ------------------------------------------------------------------------------- matching

/// One filter ready to match text.
#[derive(Debug, Clone)]
pub struct FilterMatcher {
    pub id: String,
    pub appearance: StyleAppearance,
    highlighter: Highlighter,
    marked: Option<MarkedText>,
    /// The Standard Passages it is limited to; `None`: everywhere.
    scope: Option<PassageList>,
}

impl FilterMatcher {
    /// `filter` compiled; `lemmas` resolves transliterated `lemma:` terms as a search does.
    pub fn new(filter: &VisualFilter, lemmas: &dyn LemmaLookup) -> Result<Self, String> {
        let highlighter = if filter.query.trim().is_empty() {
            Highlighter::default()
        } else {
            let parsed = obs_search::parse(&filter.query).map_err(|e| e.kind.to_string())?;
            Highlighter::new(&parsed, lemmas)
        };
        let scope = match parse_scope(&filter.scope).map_err(|e| e.message)? {
            Scope::WholeBible => None,
            Scope::Passages(list) => Some(list),
        };
        Ok(Self {
            id: filter.id.clone(),
            appearance: filter.appearance(),
            highlighter,
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
        self.highlighter.uses_original_language_data()
    }

    /// The Strong's numbers (base forms, affixes left out) of the words among `words` (the
    /// morphemes of each Tagged word of one original-language Verse) that it matches.
    pub fn aligned_numbers<'a>(
        &self,
        words: impl IntoIterator<Item = &'a [MorphemePart]>,
    ) -> BTreeSet<StrongsNumber> {
        let mut out = BTreeSet::new();
        for parts in words {
            for (part, matched) in parts.iter().zip(self.highlighter.matching_parts(parts)) {
                if matched && part.strongs.number() < AFFIX_NUMBERS_FROM {
                    out.insert(part.strongs.base());
                }
            }
        }
        out
    }
}

/// One Bible Verse inside a block: its bytes in the unit and its Standard Passage (`None`
/// for an Unmapped verse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerseSlice {
    pub range: Range<usize>,
    pub standard: Option<Passage>,
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
/// - `verses`: the Bible Verses the block holds (empty outside Bibles). A filter limited to
///   Passages colours only inside its Verses; the others colour the whole block.
/// - `align`: the block is in a Bible without original-language data of its own; `original`
///   gives the aligned original-language words at a Standard Passage (see the module docs).
pub fn block_hits(
    filters: &[FilterMatcher],
    unit: &TextUnit,
    block: Range<usize>,
    verses: &[VerseSlice],
    align: bool,
    original: &mut dyn FnMut(Passage) -> AlignedWords,
) -> Vec<FilterHit> {
    let text = unit.text.as_str();
    let block = block.start.min(text.len())..block.end.min(text.len());
    let clip = |r: &Range<usize>| r.start.max(block.start)..r.end.min(block.end);
    let mut out = Vec::new();
    // Original words are looked up once per Verse, whichever filters need them.
    let mut aligned: std::collections::HashMap<Passage, AlignedWords> = Default::default();
    for (k, f) in filters.iter().enumerate() {
        let segments: Vec<Range<usize>> = if f.is_scoped() {
            verses
                .iter()
                .filter(|v| f.in_scope(v.standard))
                .map(|v| clip(&v.range))
                .filter(|r| r.start < r.end)
                .collect()
        } else {
            vec![block.clone()]
        };
        let mut ranges: Vec<Range<usize>> = Vec::new();
        for seg in &segments {
            let Some(slice) = text.get(seg.clone()) else {
                continue;
            };
            if !f.highlighter.is_empty() {
                let tagged = tagged_in(unit, seg.clone());
                let local = tagged
                    .iter()
                    .map(|(r, parts)| (r.start - seg.start..r.end - seg.start, *parts));
                ranges.extend(
                    f.highlighter
                        .ranges(slice, local)
                        .into_iter()
                        .map(|r| r.start + seg.start..r.end + seg.start),
                );
            }
            if let Some(marked) = f.marked {
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
        if align && f.needs_alignment() {
            for v in verses.iter().filter(|v| f.in_scope(v.standard)) {
                let Some(standard) = v.standard else {
                    continue;
                };
                let seg = clip(&v.range);
                if seg.start >= seg.end {
                    continue;
                }
                let words = aligned
                    .entry(standard)
                    .or_insert_with(|| original(standard));
                let numbers = f.aligned_numbers(words.iter().map(Vec::as_slice));
                if numbers.is_empty() {
                    continue;
                }
                for (range, parts) in tagged_in(unit, seg) {
                    if parts.iter().any(|p| {
                        p.strongs.number() < AFFIX_NUMBERS_FROM
                            && numbers.contains(&p.strongs.base())
                    }) {
                        ranges.push(range);
                    }
                }
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
        self.visual_filters.revision += 1;
    }

    fn save_visual_filters(&mut self) {
        self.visual_filters.revision += 1;
        let Some(user) = &self.user else {
            return;
        };
        let json = serde_json::to_string(&self.visual_filters.list).unwrap_or_default();
        if let Err(e) = user.set_preference(VISUAL_FILTERS_PREF, &json) {
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
            id: next_id(&s.visual_filters.list),
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
        None => s.visual_filters.list.push(f),
    }
    s.save_visual_filters();
    Ok(out)
}

/// A new filter's id: `filter-<n>`, after the highest one in use.
fn next_id(list: &[VisualFilter]) -> String {
    let n = list
        .iter()
        .filter_map(|f| f.id.strip_prefix("filter-")?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("filter-{}", n + 1)
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
