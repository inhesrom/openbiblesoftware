//! The Word Study's Translated as, Occurrences and Forms sections (#56; decision #15).
//!
//! - **Translated as** ([`translated_as`]): the English renderings of the number in the best
//!   Strong's-tagged English Bible that renders it, chosen as the English-word chooser chooses
//!   (#55: the most Strong's numbers rendered, Preferred order breaking ties), grouped and
//!   counted ("grace ×130, favour ×6"). It reads the Bible's rendering index. A Bible that
//!   tags only base numbers (the KJV) answers an extended number with its base number.
//! - **Occurrences** ([`occurrences`]): where the number occurs in the original-language
//!   Bible (the first in Preferred order in the number's language that tags it: TAHOT for H,
//!   TAGNT for G), else in the first tagged Bible that does. Every Verse, in Standard canonical
//!   order, with the forms the number takes there, so the UI can count per Bible book and
//!   filter by form without asking again. English previews are read separately, only for the
//!   rows shown ([`crate::Session::occurrence_previews`]), from the Translated as Bible with
//!   the word highlighted.
//! - **Forms** ([`forms`]): the distinct Morphology of the number in that original-language
//!   Bible, in plain English ([`obs_model::morph`]) with counts, most frequent first. Codes
//!   that read the same in English are one form. The labels are the ones the Occurrences rows
//!   carry, so clicking a form filters Occurrences.
//!
//! Each original-language Bible's tagged words are gathered once per revision into a
//! [`Concordance`] (one SQL pass over its `tagged_word` spans) and shared by later jobs.

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::{Arc, Mutex};

use obs_model::{ResourceType, morph};
use obs_scripture::{Book, StrongsLanguage, StrongsNumber, Verse};

use serde_json::json;

use crate::Library;
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, always};
use crate::key_words::{Identity, is_original_language};
use crate::library::LibraryEntry;
use crate::word_study::RenderingIndexes;
use crate::workspace::{TabId, TabKind};

/// Occurrences rows shown in the Guide (#15: "the first 50 Verses").
pub const OCCURRENCE_ROWS: usize = 50;

/// One English rendering of a Strong's number and how many Tagged words have it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderingRow {
    /// Normalized words (`grace`, `the world`).
    pub rendering: String,
    pub count: u32,
}

/// What the Translated as section shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TranslatedAs {
    /// The tagged English Bible the renderings come from (resource id, abbreviation).
    pub bible_id: String,
    pub bible_abbreviation: String,
    /// The number counted: the Word Study's own, or its base number in a Bible that tags only
    /// base numbers.
    pub counted: Option<StrongsNumber>,
    /// Most frequent first, then alphabetically.
    pub renderings: Vec<RenderingRow>,
}

impl TranslatedAs {
    /// Tagged words counted over every rendering.
    pub fn total(&self) -> u32 {
        self.renderings.iter().map(|r| r.count).sum()
    }

    /// The Search query that finds the Verses with this rendering: `strong:G5485 "favour"`.
    pub fn search_query(&self, rendering: &str) -> Option<String> {
        let n = self.counted?;
        Some(format!("strong:{n} \"{}\"", rendering.replace('"', "")))
    }
}

/// The Bible an Occurrences preview is read from, and the number highlighted in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PreviewBible {
    pub id: String,
    pub abbreviation: String,
    /// The number as that Bible tags it (an extended number's base in the KJV).
    pub highlight: StrongsNumber,
    pub rtl: bool,
}

/// One Verse in which the number occurs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OccurrenceVerse {
    /// The Standard Verse; for an Unmapped verse, the Bible's own.
    pub verse: Verse,
    /// The Verse in the counted Bible's own numbering.
    pub native: Verse,
    pub unmapped: bool,
    /// One entry per occurrence in the Verse: its form, an index into
    /// [`OccurrenceList::forms`] (`None` without readable Morphology).
    pub forms: Vec<Option<u16>>,
}

/// What the Occurrences section shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OccurrenceList {
    /// The Bible the occurrences are counted in (resource id, abbreviation).
    pub bible_id: String,
    pub bible_abbreviation: String,
    /// The number counted (see [`TranslatedAs::counted`]).
    pub counted: Option<StrongsNumber>,
    /// Every Verse with the number, in Standard canonical order.
    pub verses: Vec<OccurrenceVerse>,
    /// Form labels (plain-English Morphology), as [`Form::label`] has them.
    pub forms: Vec<String>,
    /// Where the English previews come from.
    pub preview: Option<PreviewBible>,
}

/// The Occurrences list as one form filters it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FilteredOccurrences<'a> {
    /// Occurrences per Bible book, in canonical order.
    pub books: Vec<(Book, u32)>,
    /// The Verses, in order.
    pub verses: Vec<&'a OccurrenceVerse>,
    /// Occurrences counted.
    pub count: u32,
}

impl OccurrenceList {
    /// Occurrences (Tagged-word parts) in every Verse.
    pub fn count(&self) -> u32 {
        self.verses.iter().map(|v| v.forms.len() as u32).sum()
    }

    /// The list for one form (by [`Form::label`]); every occurrence when `form` is `None`.
    /// A form the list does not have leaves nothing.
    pub fn filtered(&self, form: Option<&str>) -> FilteredOccurrences<'_> {
        let wanted = match form {
            None => None,
            Some(label) => match self.forms.iter().position(|f| f == label) {
                Some(i) => Some(u16::try_from(i).ok()),
                None => return FilteredOccurrences::default(),
            },
        };
        let mut out = FilteredOccurrences::default();
        for v in &self.verses {
            let n = match wanted {
                None => v.forms.len() as u32,
                Some(f) => v.forms.iter().filter(|x| **x == f).count() as u32,
            };
            if n == 0 {
                continue;
            }
            out.count += n;
            out.verses.push(v);
            match out.books.last_mut() {
                Some((b, c)) if *b == v.verse.book => *c += n,
                _ => out.books.push((v.verse.book, n)),
            }
        }
        out
    }

    /// The number Show all in Search looks for: the one counted (an extended number's base in
    /// a Bible that tags only base numbers), else the Word Study's own.
    fn search_number(&self, number: StrongsNumber) -> StrongsNumber {
        self.counted.unwrap_or(number)
    }

    /// The Search query for every occurrence: `strong:G5485`, with the number counted
    /// (Search matches an extended number exactly).
    pub fn search_query(&self, number: StrongsNumber) -> String {
        format!("strong:{}", self.search_number(number))
    }

    /// The Bibles Show all in Search searches: the preview Bible first (its text is what the
    /// rows show), then the counted Bible. The preview Bible is left out when it tags the
    /// number differently (the KJV's H1254 for TAHOT's H1254A): the query finds nothing there.
    pub fn search_bibles(&self, number: StrongsNumber) -> String {
        let wanted = self.search_number(number);
        let mut ids: Vec<&str> = Vec::new();
        if let Some(p) = &self.preview
            && p.highlight == wanted
        {
            ids.push(&p.id);
        }
        if !self.bible_id.is_empty() && !ids.contains(&self.bible_id.as_str()) {
            ids.push(&self.bible_id);
        }
        ids.join(",")
    }
}

/// One grammatical form of a number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// Plain English (`Qal perfect, 3rd masc. sing.`), or the raw code when it cannot be read.
    pub label: String,
    /// Spelled out (`Verb, Qal perfect, third person masculine singular`), for the tooltip.
    pub long: String,
    /// The Morphology codes that read as this form.
    pub codes: Vec<String>,
    pub count: u32,
}

/// What the Forms section shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Forms {
    pub bible_id: String,
    pub bible_abbreviation: String,
    pub counted: Option<StrongsNumber>,
    /// Most frequent first, then by label.
    pub forms: Vec<Form>,
    /// Occurrences without Morphology.
    pub without_form: u32,
}

/// One Bible's Tagged words by Strong's number: for each number, the Verses (indexes into
/// `verses`) and Morphology codes (indexes into `codes`) of its parts, in document order.
#[derive(Debug, Default)]
pub struct Concordance {
    /// Each Bible Verse (own numbering) with its Standard Verse (`None`: Unmapped).
    verses: Vec<(Verse, Option<Verse>)>,
    words: HashMap<StrongsNumber, Vec<(u32, u32)>>,
    codes: Vec<String>,
}

/// One unit's Verses: (start, end, Verse index), by start.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
type UnitVerses = [(u32, u32, u32)];

/// No Morphology code.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
const NO_CODE: u32 = u32::MAX;

impl Concordance {
    /// Gather a Bible's Tagged words.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(store: &dyn obs_store::ResourceStore) -> Result<Self, obs_store::StoreError> {
        let verses = store.verses()?;
        let map = store.verse_map()?;
        let mut by_unit: HashMap<obs_model::UnitId, Vec<(u32, u32, u32)>> = HashMap::new();

        let mut out = Concordance {
            verses: standard_verses(&verses, map.as_ref()),
            ..Concordance::default()
        };
        for (i, v) in verses.iter().enumerate() {
            by_unit
                .entry(v.unit.clone())
                .or_default()
                .push((v.start, v.end, i as u32));
        }
        for list in by_unit.values_mut() {
            list.sort_unstable();
        }
        let mut code_ids: HashMap<String, u32> = HashMap::new();
        let mut last: Option<(obs_model::UnitId, &UnitVerses)> = None;
        let empty: &UnitVerses = &[];
        store.for_each_tagged_word_at(&mut |unit, start, parts| {
            let list = match &last {
                Some((u, l)) if u == unit => *l,
                _ => {
                    let l = by_unit.get(unit).map_or(empty, Vec::as_slice);
                    last = Some((unit.clone(), l));
                    l
                }
            };
            let at = list.partition_point(|(s, _, _)| *s <= start);
            let Some(&(_, end, verse)) = at.checked_sub(1).and_then(|i| list.get(i)) else {
                return;
            };
            if start >= end {
                return;
            }
            for p in parts {
                let code = match p.morph.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
                    Some(c) => match code_ids.get(c) {
                        Some(id) => *id,
                        None => {
                            let id = out.codes.len() as u32;
                            out.codes.push(c.to_string());
                            code_ids.insert(c.to_string(), id);
                            id
                        }
                    },
                    None => NO_CODE,
                };
                out.words.entry(p.strongs).or_default().push((verse, code));
            }
        })?;
        Ok(out)
    }

    /// Where `number` occurs: (Verse, code) in document order, and the number counted. A base
    /// number counts all of its homographs; an extended number the Bible does not tag counts
    /// its base number in a Bible that tags only base numbers.
    pub fn hits(&self, number: StrongsNumber) -> (StrongsNumber, Vec<(u32, u32)>) {
        let mut out: Vec<(u32, u32)> = self
            .words
            .iter()
            .filter(|(k, _)| number.matches(**k))
            .flat_map(|(_, v)| v.iter().copied())
            .collect();
        let mut counted = number;
        if out.is_empty() && number.is_extended() {
            counted = number.base();
            out = self.words.get(&counted).cloned().unwrap_or_default();
        }
        out.sort_by_key(|h| h.0);
        (counted, out)
    }

    fn code(&self, id: u32) -> Option<&str> {
        self.codes.get(id as usize).map(String::as_str)
    }

    /// Whether any Tagged word carries a Morphology code, so the Bible can fill Forms. A
    /// Bible with Strong's numbers only cannot.
    pub fn has_morphology(&self) -> bool {
        !self.codes.is_empty()
    }
}

/// Each Bible Verse (own numbering) with its Standard Verse (`None`: Unmapped), as the
/// Search Tool's outline reads them: the Verse map by position only when it lines up with the
/// verse list, otherwise looked up Verse by Verse.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn standard_verses(
    verses: &[obs_model::BibleVerse],
    map: Option<&obs_scripture::VerseMap>,
) -> Vec<(Verse, Option<Verse>)> {
    let listed: Vec<(Verse, u16)> = verses.iter().map(|v| (v.verse, v.last)).collect();
    crate::bible_search::BibleOutline::new(&listed, map)
        .verses
        .into_iter()
        .map(|v| (v.native, v.standard.map(|p| p.start())))
        .collect()
}

/// The plain-English form of a Morphology code and its spelled-out reading; the raw code when
/// it cannot be read.
pub fn form_of(code: &str) -> (String, String) {
    match morph::parse(code) {
        Ok(m) => {
            let short = m.short();
            let long = m.long();
            if short.is_empty() {
                (code.to_string(), code.to_string())
            } else {
                (short, long)
            }
        }
        Err(_) => (code.to_string(), code.to_string()),
    }
}

type Slot = Arc<Mutex<Option<Arc<Concordance>>>>;

/// Each Bible's [`Concordance`], gathered once per revision (one job at a time per Bible;
/// another job asking waits for it rather than gathering it again).
#[derive(Debug, Default)]
pub struct Concordances {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    loaded: Mutex<BTreeMap<Identity, Slot>>,
}

impl Concordances {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn get(&self, entry: &LibraryEntry) -> Result<Arc<Concordance>, String> {
        let identity = crate::key_words::identity(&entry.manifest);
        let slot = {
            let mut loaded = self.loaded.lock().unwrap_or_else(|p| p.into_inner());
            match loaded.get(&identity) {
                Some(slot) => Arc::clone(slot),
                None => {
                    loaded.retain(|(id, _, _), _| *id != identity.0);
                    let slot = Slot::default();
                    loaded.insert(identity, Arc::clone(&slot));
                    slot
                }
            }
        };
        let mut slot = slot.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(c) = slot.as_ref() {
            return Ok(Arc::clone(c));
        }
        let reader = obs_store::ObrReader::open(&entry.path).map_err(|e| e.to_string())?;
        let c = Arc::new(Concordance::read(&reader).map_err(|e| e.to_string())?);
        *slot = Some(Arc::clone(&c));
        Ok(c)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn get(&self, _: &LibraryEntry) -> Result<Arc<Concordance>, String> {
        Err("Opening Resources is not supported here".into())
    }

    /// A Bible's concordance if a job has already gathered it; never reads, nor waits for a
    /// job gathering it (for the UI thread).
    pub fn loaded(&self, entry: &LibraryEntry) -> Option<Arc<Concordance>> {
        let identity = crate::key_words::identity(&entry.manifest);
        let slot = {
            let loaded = self.loaded.lock().unwrap_or_else(|p| p.into_inner());
            Arc::clone(loaded.get(&identity)?)
        };
        match slot.try_lock() {
            Ok(c) => c.clone(),
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner().clone(),
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }
}

/// A Bible, the number counted in it, and its renderings with counts.
type BibleRenderings<'a> = (&'a LibraryEntry, StrongsNumber, Vec<(String, u32)>);

/// The best tagged English Bible that renders `number` (the chooser's rule, #55), with its
/// renderings and the number counted.
fn english_renderings<'a>(
    library: &'a Library,
    indexes: &'a RenderingIndexes,
    number: StrongsNumber,
) -> Option<BibleRenderings<'a>> {
    indexes
        .tagged_english_bibles(library)
        .into_iter()
        .find_map(|(e, i)| {
            let rows = i.renderings_of(number);
            if !rows.is_empty() {
                return Some((e, number, rows));
            }
            if number.is_extended() {
                let base = number.base();
                let rows = i.renderings_of(base);
                if !rows.is_empty() {
                    return Some((e, base, rows));
                }
            }
            None
        })
}

/// The Translated as section for `number`.
pub fn translated_as(
    library: &Library,
    indexes: &RenderingIndexes,
    number: StrongsNumber,
) -> TranslatedAs {
    let Some((bible, counted, rows)) = english_renderings(library, indexes, number) else {
        return TranslatedAs::default();
    };
    TranslatedAs {
        bible_id: bible.manifest.resource_id.clone(),
        bible_abbreviation: bible.manifest.abbreviation.clone(),
        counted: Some(counted),
        renderings: rows
            .into_iter()
            .map(|(rendering, count)| RenderingRow { rendering, count })
            .collect(),
    }
}

/// Whether a Bible is in the original language of `number`'s half of the Strong's system.
fn original(entry: &LibraryEntry, language: StrongsLanguage) -> bool {
    is_original_language(&entry.manifest.language, language)
}

/// The Bible occurrences are counted in, with its concordance and `number`'s hits there:
/// the first Bible in Preferred order in the number's original language whose hits carry
/// Morphology (TAGNT after a Greek Bible with Strong's numbers only, so Forms and the
/// Occurrences form filter read the same Bible), else the first in that language that tags
/// it, else (unless `original_only`) the first tagged Bible that does. A Bible's rendering
/// index is checked before its concordance is gathered. A Bible that cannot be read is passed
/// over; it is an error only when nothing answered.
#[allow(clippy::type_complexity)]
fn counted_in<'a>(
    library: &'a Library,
    indexes: &RenderingIndexes,
    concordances: &Concordances,
    number: StrongsNumber,
    original_only: bool,
) -> Result<
    Option<(
        &'a LibraryEntry,
        Arc<Concordance>,
        StrongsNumber,
        Vec<(u32, u32)>,
    )>,
    String,
> {
    let language = number.language();
    let bibles: Vec<&LibraryEntry> = library.of_type(ResourceType::Bible).collect();
    let originals = bibles.iter().filter(|e| original(e, language));
    let others = bibles
        .iter()
        .filter(|e| !original_only && !original(e, language));
    let mut errors = Vec::new();
    // The first Bible with hits, if none in the original language has Morphology for them.
    let mut uncoded = None;
    for entry in originals.chain(others) {
        if uncoded.is_some() && !original(entry, language) {
            break;
        }
        let tags = match indexes.get(entry) {
            Ok(index) => {
                index.total(number).is_some()
                    || (number.is_extended() && index.total(number.base()).is_some())
            }
            Err(e) => {
                errors.push(format!("{}: {e}", entry.manifest.abbreviation));
                continue;
            }
        };
        if !tags {
            continue;
        }
        match concordances.get(entry) {
            Ok(c) => {
                let (counted, hits) = c.hits(number);
                if hits.is_empty() {
                    continue;
                }
                let coded = hits.iter().any(|(_, code)| c.code(*code).is_some());
                if coded && original(entry, language) {
                    return Ok(Some((entry, c, counted, hits)));
                }
                if uncoded.is_none() {
                    uncoded = Some((*entry, c, counted, hits));
                }
            }
            Err(e) => errors.push(format!("{}: {e}", entry.manifest.abbreviation)),
        }
    }
    if uncoded.is_some() {
        return Ok(uncoded);
    }
    if errors.is_empty() {
        Ok(None)
    } else {
        Err(errors.join("; "))
    }
}

/// The Occurrences section for `number`.
pub fn occurrences(
    library: &Library,
    indexes: &RenderingIndexes,
    concordances: &Concordances,
    number: StrongsNumber,
) -> Result<OccurrenceList, String> {
    let Some((bible, c, counted, hits)) =
        counted_in(library, indexes, concordances, number, false)?
    else {
        return Ok(OccurrenceList::default());
    };
    let mut forms: Vec<String> = Vec::new();
    let mut form_of_code: HashMap<u32, Option<u16>> = HashMap::new();
    // (Verse shown, Unmapped) → (the counted Bible's own Verse, the forms there).
    type Row = (Verse, Vec<Option<u16>>);
    let mut by_verse: BTreeMap<(Verse, bool), Row> = BTreeMap::new();
    for (verse, code) in hits {
        let Some(&(native, standard)) = c.verses.get(verse as usize) else {
            continue;
        };
        let form = *form_of_code.entry(code).or_insert_with(|| {
            let label = form_of(c.code(code)?).0;
            let i = match forms.iter().position(|f| *f == label) {
                Some(i) => i,
                None => {
                    forms.push(label);
                    forms.len() - 1
                }
            };
            u16::try_from(i).ok()
        });
        let key = (standard.unwrap_or(native), standard.is_none());
        by_verse
            .entry(key)
            .or_insert_with(|| (native, Vec::new()))
            .1
            .push(form);
    }
    let verses = by_verse
        .into_iter()
        .map(|((verse, unmapped), (native, forms))| OccurrenceVerse {
            verse,
            native,
            unmapped,
            forms,
        })
        .collect();
    let m = &bible.manifest;
    let preview = match english_renderings(library, indexes, number) {
        Some((e, highlight, _)) => PreviewBible {
            id: e.manifest.resource_id.clone(),
            abbreviation: e.manifest.abbreviation.clone(),
            highlight,
            rtl: e.manifest.direction == obs_model::Direction::Rtl,
        },
        None => PreviewBible {
            id: m.resource_id.clone(),
            abbreviation: m.abbreviation.clone(),
            highlight: counted,
            rtl: m.direction == obs_model::Direction::Rtl,
        },
    };
    Ok(OccurrenceList {
        bible_id: m.resource_id.clone(),
        bible_abbreviation: m.abbreviation.clone(),
        counted: Some(counted),
        verses,
        forms,
        preview: Some(preview),
    })
}

/// The Forms section for `number`: from the original-language Bible only.
pub fn forms(
    library: &Library,
    indexes: &RenderingIndexes,
    concordances: &Concordances,
    number: StrongsNumber,
) -> Result<Forms, String> {
    let Some((bible, c, counted, hits)) = counted_in(library, indexes, concordances, number, true)?
    else {
        return Ok(Forms::default());
    };
    let mut by_label: BTreeMap<String, Form> = BTreeMap::new();
    let mut without_form = 0;
    for (_, code) in hits {
        let Some(code) = c.code(code) else {
            without_form += 1;
            continue;
        };
        let (label, long) = form_of(code);
        let form = by_label.entry(label.clone()).or_insert_with(|| Form {
            label,
            long,
            codes: Vec::new(),
            count: 0,
        });
        form.count += 1;
        if !form.codes.iter().any(|c| c == code) {
            form.codes.push(code.to_string());
        }
    }
    let mut forms: Vec<Form> = by_label.into_values().collect();
    forms.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    Ok(Forms {
        bible_id: bible.manifest.resource_id.clone(),
        bible_abbreviation: bible.manifest.abbreviation.clone(),
        counted: Some(counted),
        forms,
        without_form,
    })
}

/// One Occurrences row's preview: the Verse's text in the preview Bible with the word
/// highlighted. Empty text when the Bible does not have the Verse, or (with `error`) when it
/// could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OccurrencePreview {
    pub text: String,
    pub highlights: Vec<Range<usize>>,
    /// The Verse in the preview Bible's own numbering, for opening it there.
    pub native: Option<Verse>,
    /// Why the preview could not be read (the Bible's file failed), so the row does not say
    /// the Bible lacks the Verse.
    pub error: Option<String>,
}

impl OccurrencePreview {
    /// A preview that could not be read.
    pub fn failed(error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::default()
        }
    }
}

/// Read previews of Standard `verses` from `bible`.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_previews(
    entry: &LibraryEntry,
    highlight: StrongsNumber,
    verses: &[Verse],
) -> Result<Vec<(Verse, OccurrencePreview)>, String> {
    use obs_search::Highlighter;
    use obs_search::query::{NoLexicon, Query};
    use obs_store::ResourceStore;
    let reader = obs_store::ObrReader::open(&entry.path).map_err(|e| e.to_string())?;
    let map = reader.verse_map().map_err(|e| e.to_string())?;
    let highlighter = Highlighter::new(
        &Query::Strong {
            number: highlight,
            span: 0..0,
        },
        &NoLexicon,
    );
    let mut texts: HashMap<obs_model::UnitId, Option<String>> = HashMap::new();
    let mut out = Vec::with_capacity(verses.len());
    for &standard in verses {
        let natives = match &map {
            Some(m) => m.to_native(standard),
            None => vec![standard],
        };
        let mut found: Vec<obs_model::BibleVerse> = Vec::new();
        for n in &natives {
            if let Some(v) = reader.verse(*n).map_err(|e| e.to_string())?
                && found.first().is_none_or(|f| f.unit == v.unit)
                && !found.iter().any(|f| f.start == v.start)
            {
                found.push(v);
            }
        }
        let (Some(first), Some(start), Some(end)) = (
            found.first(),
            found.iter().map(|v| v.start).min(),
            found.iter().map(|v| v.end).max(),
        ) else {
            out.push((standard, OccurrencePreview::default()));
            continue;
        };
        let unit = first.unit.clone();
        if !texts.contains_key(&unit) {
            let text = reader
                .unit_text(&unit)
                .map_err(|e| e.to_string())?
                .map(|(_, text)| text);
            texts.insert(unit.clone(), text);
        }
        let Some(text) = texts.get(&unit).and_then(Option::as_deref) else {
            out.push((standard, OccurrencePreview::default()));
            continue;
        };
        let range = start as usize..end as usize;
        let spans = reader
            .spans_starting_in(&unit, range.clone())
            .map_err(|e| e.to_string())?;
        let snippet = crate::bible_search::snippet(text, range, &spans, &highlighter);
        out.push((
            standard,
            OccurrencePreview {
                text: snippet.text,
                highlights: snippet.highlights,
                native: Some(first.verse),
                error: None,
            },
        ));
    }
    Ok(out)
}

/// A Bible's own Versification, from its verse list.
#[cfg(not(target_arch = "wasm32"))]
fn read_versification(entry: &LibraryEntry) -> Option<obs_scripture::ListedVersification> {
    use obs_store::ResourceStore;
    let reader = obs_store::ObrReader::open(&entry.path).ok()?;
    let verses = reader.verses().ok()?;
    Some(obs_scripture::ListedVersification::new(
        verses.iter().map(|v| (v.verse, v.last)),
    ))
}

#[cfg(target_arch = "wasm32")]
fn read_versification(_: &LibraryEntry) -> Option<obs_scripture::ListedVersification> {
    None
}

#[cfg(target_arch = "wasm32")]
pub fn read_previews(
    _: &LibraryEntry,
    _: StrongsNumber,
    _: &[Verse],
) -> Result<Vec<(Verse, OccurrencePreview)>, String> {
    Err("Opening Resources is not supported here".into())
}

impl crate::Session {
    /// A Bible's own Versification: its open Tab's, else read from its verse list. `None`
    /// when it is not in the library or cannot be read here.
    fn bible_versification(&self, id: &str) -> Option<Arc<obs_scripture::ListedVersification>> {
        let kind = TabKind::Resource(id.to_string());
        if let Some(v) = self
            .workspace
            .most_recent(|t| t.kind == kind)
            .and_then(|t| self.workspace.tab(t))
            .and_then(|t| t.versification.clone())
        {
            return Some(v);
        }
        read_versification(self.library.get(id)?).map(Arc::new)
    }

    /// Open a Bible at a Verse in its own numbering (an Occurrences row): its most recently
    /// active Tab goes there (and its Link set follows); with `new_tab`, or when it has no
    /// Tab, a new Tab opens there.
    pub fn open_bible_verse(
        &mut self,
        id: &str,
        native: Verse,
        new_tab: bool,
    ) -> Result<TabId, String> {
        let kind = TabKind::Resource(id.to_string());
        if !new_tab && let Some(t) = self.workspace.most_recent(|t| t.kind == kind) {
            self.workspace.focus(t);
            self.workspace.navigate(t, native);
            return Ok(t);
        }
        if self.library.get(id).is_none() {
            return Err(format!("{id} is not in the library"));
        }
        let out = self
            .run("resource.open", &json!({ "id": id, "split": new_tab }))
            .map_err(|e| e.to_string())?;
        let t = out["tab"].as_u64().map(TabId).ok_or("no Tab opened")?;
        self.workspace.navigate(t, native);
        Ok(t)
    }
}

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "word_study.open_verse",
    label: "Open a Word Study occurrence",
    shortcut: None,
    params: &[
        ParamSpec {
            name: "bible",
            kind: ParamKind::String,
            required: true,
            help: "Resource id of the Bible",
        },
        ParamSpec {
            name: "verse",
            kind: ParamKind::String,
            required: true,
            help: "The Verse in that Bible's own numbering, e.g. `Lk 2:40`",
        },
        ParamSpec {
            name: "new_tab",
            kind: ParamKind::Boolean,
            required: false,
            help: "Open a new Tab even if the Bible is open (Ctrl/Cmd-click)",
        },
    ],
    enabled: always,
    run: |s, p| {
        const ID: &str = "word_study.open_verse";
        let failed = |message: String| CommandError::Failed {
            command: ID,
            message,
        };
        let bible = p.str("bible").unwrap_or_default().to_string();
        let text = p.str("verse").unwrap_or_default();
        // In the Bible's own numbering (TAHOT's Ps 3:9 is not a Standard Verse).
        let own = s.bible_versification(&bible);
        let parsed = match &own {
            Some(v) => obs_scripture::parse_reference(text, v.as_ref(), None),
            None => obs_scripture::parse_reference(text, &obs_scripture::Standard, None),
        };
        let verse = parsed
            .ok()
            .and_then(|l| l.passages().first().map(|p| p.start()))
            .ok_or_else(|| failed(format!("“{text}” is not a Verse")))?;
        let new_tab = p.bool("new_tab").unwrap_or(false);
        let tab = s.open_bible_verse(&bible, verse, new_tab).map_err(failed)?;
        Ok(json!({ "tab": tab.0 }))
    },
}];
