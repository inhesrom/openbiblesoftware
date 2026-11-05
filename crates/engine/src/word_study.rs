//! Word Study (#55; decisions #15 and #20): the Guide whose Key is a Strong's number.
//!
//! - **Box.** The Word Study's header box takes a Strong's number (`G5485`, `H1254`, the
//!   extended `H1254A`) or English words. A number moves the Guide there; words open the
//!   English-word chooser: the Strong's numbers the best tagged English Bible (the most
//!   Strong's numbers rendered; Preferred order breaks ties) renders with those words, each
//!   with its count ("love → G25 ×…, G26 ×…"). When exactly one number renders them the Guide goes
//!   straight to it, also when the answer was already computed.
//! - **Header** (always on top, not a section): Lemma, transliteration, Strong's number, short
//!   Gloss and the Lexicon it came from (the first Lexicon in Preferred order with an entry),
//!   part of speech and total occurrences (the Bibles' rendering indexes; see
//!   [`header`]).
//! - **Lexicon entries** section ([`crate::GuideSection::LexiconEntries`]): every Lexicon in
//!   Preferred order that has the number, extended number first, then the base number
//!   ([`obs_store::ResourceStore::lookup_strongs`]). The UI expands the first entry.
//!
//! The header and the chooser are computed on the job queue like Guide sections, so asking
//! never waits. The rendering index lives in each Bible's `.obr` (built by `ResourceBuilder`);
//! for a file that predates it, it is derived from the Bible's text here (not in
//! `ObrReader::load`), once per revision under a per-Bible lock, and kept in memory.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use obs_model::{Direction, Key, RenderingIndex, ResourceType, transliterate};
use obs_scripture::{StrongsLanguage, StrongsNumber};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::library::LibraryEntry;
use crate::word_info::{LexiconGloss, has_original_script};
use crate::workspace::{Guide, OpenRequest, PanelId, TabId, TabKind};
use crate::{JobEvent, JobEventKind, JobId, Library, Session};

/// What was typed into a Word Study's box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordStudyInput {
    Strongs(StrongsNumber),
    /// English words, trimmed (one word or a short phrase).
    Words(String),
}

/// Read the Word Study box: a Strong's number, else English words.
pub fn parse_input(text: &str) -> Result<WordStudyInput, String> {
    let text = text.trim();
    if let Ok(n) = text.parse::<StrongsNumber>() {
        return Ok(WordStudyInput::Strongs(n));
    }
    if obs_model::tokenize(text).is_empty() {
        return Err("Type a Strong's number (G5485, H1254A) or an English word".into());
    }
    let looks_numbered = text
        .chars()
        .next()
        .is_some_and(|c| matches!(c, 'G' | 'g' | 'H' | 'h'))
        && text.chars().skip(1).all(|c| c.is_ascii_alphanumeric())
        && text.chars().nth(1).is_some_and(|c| c.is_ascii_digit());
    if looks_numbered {
        return Err(format!(
            "“{text}” is not a Strong's number (expected e.g. G5485 or H1254A)"
        ));
    }
    Ok(WordStudyInput::Words(text.to_string()))
}

/// Where a total occurrence count comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrences {
    pub count: u32,
    pub resource_id: String,
    pub abbreviation: String,
    /// The number counted: the Word Study's own, or its base number in a Bible that tags
    /// only base numbers (the KJV counts H1254 for H1254A).
    pub counted: StrongsNumber,
}

/// The Word Study header: Lemma, transliteration, Strong's number, Gloss, part of speech and
/// total occurrences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordStudyHeader {
    pub strongs: StrongsNumber,
    /// The headword of the first Lexicon entry (original script).
    pub lemma: Option<String>,
    pub transliteration: Option<String>,
    /// The first Lexicon in Preferred order with an entry: its short gloss.
    pub gloss: Option<LexiconGloss>,
    /// Plain English (`verb`), from the Bibles' Morphology.
    pub part_of_speech: Option<String>,
    pub occurrences: Option<Occurrences>,
}

/// One Strong's number the chooser offers for English words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub strongs: StrongsNumber,
    /// Tagged words in the Bible rendered with the words and tagged with this number.
    pub count: u32,
    pub lemma: Option<String>,
    pub gloss: Option<String>,
}

/// The English-word chooser: the Strong's numbers a tagged English Bible renders with the
/// typed words, most frequent first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chooser {
    pub words: String,
    /// The Bible the counts are from (resource id, abbreviation); `None` when no tagged
    /// English Bible has the words.
    pub bible: Option<(String, String)>,
    pub choices: Vec<Choice>,
    /// Whether the library has any Strong's-tagged English Bible at all.
    pub has_tagged_bible: bool,
}

/// One Lexicon entry for a Word Study's number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexiconEntry {
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    /// The Key the entry was found by (the extended number, or its base number).
    pub key: Key,
    /// The entry's unit, for opening the Lexicon at it.
    pub unit: String,
    /// The headword (the unit's title).
    pub headword: Option<String>,
    /// The entry's text; paragraphs are separated by newlines.
    pub text: String,
    /// Strong's numbers the entry links to (its one-part `tagged_word` spans), in order,
    /// without the Word Study's own number.
    pub links: Vec<StrongsNumber>,
    pub direction: Direction,
}

impl LexiconEntry {
    /// Stable id of the entry within a Word Study (Lexicon and unit).
    pub fn id(&self) -> String {
        format!("{}/{}", self.resource_id, self.unit)
    }
}

/// Whether a Bible's language is the original language of `number`'s half of the Strong's
/// system (Hebrew or Aramaic for H, Greek for G).
fn is_original_language(entry: &LibraryEntry, number: StrongsNumber) -> bool {
    let lang = entry
        .manifest
        .language
        .split('-')
        .next()
        .unwrap_or_default();
    match number.language() {
        StrongsLanguage::Hebrew => matches!(lang, "hbo" | "he" | "arc"),
        StrongsLanguage::Greek => matches!(lang, "grc" | "el"),
    }
}

/// A Bible revision: resource id, revision, content hash.
type Identity = (String, u32, String);

/// One Bible revision's index, filled by the first job that needs it. Its lock is held while
/// the index is read or derived, so a second job asking for the same Bible waits for that
/// one instead of deriving it again.
type Slot = Arc<Mutex<Option<Arc<RenderingIndex>>>>;

/// Each Bible's rendering index, read from its `.obr` (or derived from its text for a file
/// that predates the index) once per revision and shared by later jobs.
#[derive(Debug, Default)]
pub struct RenderingIndexes {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    loaded: Mutex<BTreeMap<Identity, Slot>>,
}

impl RenderingIndexes {
    /// A Bible's rendering index.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn get(&self, entry: &LibraryEntry) -> Result<Arc<RenderingIndex>, String> {
        let m = &entry.manifest;
        let identity = (m.resource_id.clone(), m.revision, m.content_hash.clone());
        self.get_with(identity, || read_or_derive(entry))
    }

    /// The index for `identity`: the one already in memory, else `load`'s, kept for later
    /// calls. Only one `load` runs per identity at a time; a failed one is not kept.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn get_with(
        &self,
        identity: Identity,
        load: impl FnOnce() -> Result<RenderingIndex, String>,
    ) -> Result<Arc<RenderingIndex>, String> {
        let slot = {
            let mut loaded = self.loaded.lock().unwrap_or_else(|p| p.into_inner());
            match loaded.get(&identity) {
                Some(slot) => Arc::clone(slot),
                None => {
                    // An older revision of the same Bible is not asked for again.
                    loaded.retain(|(id, _, _), _| *id != identity.0);
                    let slot = Slot::default();
                    loaded.insert(identity, Arc::clone(&slot));
                    slot
                }
            }
        };
        let mut slot = slot.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(index) = slot.as_ref() {
            return Ok(Arc::clone(index));
        }
        let index = Arc::new(load()?);
        *slot = Some(Arc::clone(&index));
        Ok(index)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn get(&self, _: &LibraryEntry) -> Result<Arc<RenderingIndex>, String> {
        Err("Opening Resources is not supported here".into())
    }

    /// Every Strong's-tagged English Bible (one whose index has renderings) with its index,
    /// the best tagged first: the most Strong's numbers rendered, Preferred order breaking
    /// ties. Not the most Tagged words: the Starter eBible ASV and WEB tag about twice as
    /// many words as the KJV but on fewer numbers (no G25 or G26 at all; "the" is H3605
    /// ten thousand times), so they would outrank it and answer "love" poorly.
    fn tagged_english_bibles<'a>(
        &'a self,
        library: &'a Library,
    ) -> Vec<(&'a LibraryEntry, Arc<RenderingIndex>)> {
        let mut bibles: Vec<(&LibraryEntry, Arc<RenderingIndex>, usize)> = self
            .tagged_bibles(library)
            .filter(|(_, i)| !i.renderings.is_empty())
            .map(|(e, i)| {
                let numbers = i
                    .renderings
                    .iter()
                    .map(|r| r.strongs)
                    .collect::<BTreeSet<_>>()
                    .len();
                (e, i, numbers)
            })
            .collect();
        // Stable: equally tagged Bibles keep their Preferred order.
        bibles.sort_by_key(|b| std::cmp::Reverse(b.2));
        bibles.into_iter().map(|(e, i, _)| (e, i)).collect()
    }

    /// The Strong's-tagged Bibles in Preferred order with their indexes, loaded one at a time
    /// as the caller goes. A Bible that cannot be read, or has no Tagged words, is skipped.
    fn tagged_bibles<'a>(
        &'a self,
        library: &'a Library,
    ) -> impl Iterator<Item = (&'a LibraryEntry, Arc<RenderingIndex>)> + 'a {
        library
            .of_type(ResourceType::Bible)
            .filter_map(|e| Some((e, self.get(e).ok()?)))
            .filter(|(_, index)| !index.totals.is_empty())
    }
}

/// A Bible's index from its `.obr`, or derived from its text when the file predates it.
#[cfg(not(target_arch = "wasm32"))]
fn read_or_derive(entry: &LibraryEntry) -> Result<RenderingIndex, String> {
    use obs_store::ResourceStore;
    let reader = obs_store::ObrReader::open(&entry.path).map_err(|e| e.to_string())?;
    if let Some(index) = reader.rendering_index().map_err(|e| e.to_string())? {
        return Ok(index);
    }
    let mut units = Vec::new();
    for id in reader.unit_ids().map_err(|e| e.to_string())? {
        if let Some(u) = reader.unit(&id).map_err(|e| e.to_string())? {
            units.push(u);
        }
    }
    Ok(RenderingIndex::build(&units))
}

/// The Lexicon entries for `number`: every Lexicon in Preferred order that has it (extended
/// number first, then the base number), each entry once. A Lexicon that cannot be read is
/// skipped; the section fails only when every Lexicon failed.
#[cfg(not(target_arch = "wasm32"))]
pub fn lexicon_entries(
    library: &Library,
    number: StrongsNumber,
) -> Result<Vec<LexiconEntry>, String> {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for entry in library.of_type(ResourceType::Lexicon) {
        match lexicon_entries_in(entry, number) {
            Ok(mut found) => out.append(&mut found),
            Err(e) => errors.push(format!("{}: {e}", entry.manifest.abbreviation)),
        }
    }
    if out.is_empty() && !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(out)
}

#[cfg(target_arch = "wasm32")]
pub fn lexicon_entries(_: &Library, _: StrongsNumber) -> Result<Vec<LexiconEntry>, String> {
    Err("Opening Resources is not supported here".into())
}

#[cfg(not(target_arch = "wasm32"))]
fn lexicon_entries_in(
    entry: &LibraryEntry,
    number: StrongsNumber,
) -> Result<Vec<LexiconEntry>, obs_store::StoreError> {
    use obs_model::SpanKind;
    use obs_store::ResourceStore;
    let reader = obs_store::ObrReader::open(&entry.path)?;
    let m = reader.manifest().clone();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for found in reader.lookup_strongs(number)? {
        if !seen.insert((found.unit.clone(), found.range)) {
            continue;
        }
        let Some(unit) = reader.unit(&found.unit)? else {
            continue;
        };
        let range = found.range.map_or(0..unit.text.len(), |(s, e)| {
            (s as usize).min(unit.text.len())..(e as usize).min(unit.text.len())
        });
        let text = unit
            .text
            .get(range.clone())
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut links: Vec<StrongsNumber> = Vec::new();
        for span in &unit.spans {
            if let SpanKind::TaggedWord { parts } = &span.kind
                && (span.start as usize) >= range.start
                && (span.end as usize) <= range.end
            {
                for p in parts {
                    if p.strongs != number && !links.contains(&p.strongs) {
                        links.push(p.strongs);
                    }
                }
            }
        }
        out.push(LexiconEntry {
            resource_id: m.resource_id.clone(),
            title: m.title.clone(),
            abbreviation: m.abbreviation.clone(),
            key: found.key.clone(),
            unit: found.unit.to_string(),
            headword: unit.title.clone().filter(|t| !t.trim().is_empty()),
            text,
            links,
            direction: m.direction,
        });
    }
    Ok(out)
}

/// The short gloss of the first Lexicon in Preferred order that has `number`.
#[cfg(not(target_arch = "wasm32"))]
fn first_gloss(library: &Library, number: StrongsNumber) -> Option<LexiconGloss> {
    library.of_type(ResourceType::Lexicon).find_map(|entry| {
        let found = lexicon_entries_in(entry, number).ok()?;
        let first = found.into_iter().next()?;
        Some(LexiconGloss {
            resource_id: first.resource_id,
            abbreviation: first.abbreviation,
            lemma: first.headword,
            text: crate::word_info::lexicon_excerpt(&first.text)?,
        })
    })
}

#[cfg(target_arch = "wasm32")]
fn first_gloss(_: &Library, _: StrongsNumber) -> Option<LexiconGloss> {
    None
}

/// The header for `number`. Total occurrences come from the first Bible in Preferred order
/// in the number's original language that tags it (TAHOT for H, TAGNT for G), else the first
/// tagged Bible that does; an extended number falls back to its base number in a Bible that
/// tags only base numbers. The part of speech is the counting Bible's, else the first one
/// any tagged Bible gives.
pub fn header(
    library: &Library,
    indexes: &RenderingIndexes,
    number: StrongsNumber,
) -> WordStudyHeader {
    let gloss = first_gloss(library, number);
    let lemma = gloss.as_ref().and_then(|g| g.lemma.clone());
    let transliteration = lemma
        .as_deref()
        .filter(|l| has_original_script(l))
        .map(transliterate)
        .filter(|t| !t.is_empty());
    let found = |index: &RenderingIndex| {
        index.total(number).or_else(|| {
            number
                .is_extended()
                .then(|| {
                    index
                        .totals
                        .iter()
                        .find(|t| t.strongs == number.base())
                        .copied()
                })
                .flatten()
        })
    };
    let original = library
        .of_type(ResourceType::Bible)
        .filter(|e| is_original_language(e, number))
        .filter_map(|e| Some((e, indexes.get(e).ok()?)))
        .find_map(|(e, i)| Some((e, found(&i)?)));
    let mut any_pos = None;
    let mut counted = original;
    if counted.is_none() || counted.is_some_and(|(_, t)| t.part_of_speech.is_none()) {
        for (e, i) in indexes.tagged_bibles(library) {
            let Some(t) = found(&i) else {
                continue;
            };
            counted = counted.or(Some((e, t)));
            if t.part_of_speech.is_some() {
                any_pos = t.part_of_speech;
                break;
            }
        }
    }
    let part_of_speech = counted
        .and_then(|(_, t)| t.part_of_speech)
        .or(any_pos)
        .map(|p| p.name().to_string());
    WordStudyHeader {
        strongs: number,
        lemma,
        transliteration,
        gloss,
        part_of_speech,
        occurrences: counted.map(|(e, t)| Occurrences {
            count: t.count,
            resource_id: e.manifest.resource_id.clone(),
            abbreviation: e.manifest.abbreviation.clone(),
            counted: t.strongs,
        }),
    }
}

/// The chooser for English words: the best tagged Strong's-tagged English Bible (the most
/// Strong's numbers rendered; Preferred order breaking ties, so a poorly tagged Bible the
/// user ranks first does not answer) that renders them, else the next one that does; its
/// numbers with counts, each with the first Lexicon's Lemma and gloss.
pub fn chooser(library: &Library, indexes: &RenderingIndexes, words: &str) -> Chooser {
    let bibles = indexes.tagged_english_bibles(library);
    let has_tagged_bible = !bibles.is_empty();
    let found = bibles.into_iter().find_map(|(e, i)| {
        let numbers = i.numbers_for(words);
        (!numbers.is_empty()).then_some((e, numbers))
    });
    let Some((bible, numbers)) = found else {
        return Chooser {
            words: words.to_string(),
            bible: None,
            choices: Vec::new(),
            has_tagged_bible,
        };
    };
    let choices = numbers
        .into_iter()
        .map(|(strongs, count)| {
            let gloss = first_gloss(library, strongs);
            Choice {
                strongs,
                count,
                lemma: gloss.as_ref().and_then(|g| g.lemma.clone()),
                gloss: gloss.map(|g| g.text),
            }
        })
        .collect();
    Chooser {
        words: words.to_string(),
        bible: Some((
            bible.manifest.resource_id.clone(),
            bible.manifest.abbreviation.clone(),
        )),
        choices,
        has_tagged_bible: true,
    }
}

/// Where a header or chooser is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordStatus<T> {
    Loading,
    Ready(Arc<T>),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Wanted {
    Header(StrongsNumber),
    Chooser(String),
}

enum Computed {
    Header(WordStudyHeader),
    Chooser(Chooser),
}

/// Word Study state for a Session: headers and choosers computed on the job queue, and the
/// Word Study Tabs showing a chooser.
#[derive(Debug, Default)]
pub struct WordStudies {
    headers: BTreeMap<StrongsNumber, WordStatus<WordStudyHeader>>,
    choosers: BTreeMap<String, WordStatus<Chooser>>,
    pending: BTreeMap<JobId, Wanted>,
    wanted: BTreeSet<Wanted>,
    library: Vec<(String, u32, String, std::path::PathBuf)>,
    indexes: Arc<RenderingIndexes>,
    /// Word Study Tabs showing the English-word chooser: the words typed, and the Key the Tab
    /// was at then (the chooser closes when the Tab moves).
    choosing: BTreeMap<TabId, (String, Option<Key>)>,
}

/// Results kept at most (older ones are recomputed when asked for again).
const RESULTS_LIMIT: usize = 128;

impl WordStudies {
    /// Whether a header or chooser job is queued or running.
    pub fn is_busy(&self) -> bool {
        !self.pending.is_empty()
    }

    /// The words a Word Study Tab's chooser is showing, if it is.
    pub fn choosing(&self, tab: TabId) -> Option<&str> {
        self.choosing.get(&tab).map(|(w, _)| w.as_str())
    }

    /// The rendering indexes, shared with jobs.
    pub fn indexes(&self) -> Arc<RenderingIndexes> {
        Arc::clone(&self.indexes)
    }
}

impl Session {
    fn word_study_library_changed(&mut self) {
        let fingerprint: Vec<_> = self
            .library
            .entries()
            .iter()
            .map(|e| {
                (
                    e.manifest.resource_id.clone(),
                    e.manifest.revision,
                    e.manifest.content_hash.clone(),
                    e.path.clone(),
                )
            })
            .collect();
        if fingerprint != self.word_study.library {
            self.word_study.library = fingerprint;
            self.word_study.headers.clear();
            self.word_study.choosers.clear();
            for id in self.word_study.pending.keys() {
                self.jobs().cancel(*id);
            }
            self.word_study.pending.clear();
        }
    }

    fn submit_word_study(&mut self, wanted: Wanted) {
        self.word_study.wanted.insert(wanted.clone());
        if self.word_study.pending.values().any(|w| *w == wanted) {
            return;
        }
        let library = self.library.clone();
        let indexes = self.word_study.indexes();
        let job = match wanted.clone() {
            Wanted::Header(n) => self.jobs().submit_fn("Word Study: header", move |_| {
                Ok(Computed::Header(header(&library, &indexes, n)))
            }),
            Wanted::Chooser(words) => self.jobs().submit_fn("Word Study: chooser", move |_| {
                Ok(Computed::Chooser(chooser(&library, &indexes, &words)))
            }),
        };
        self.word_study.pending.insert(job, wanted);
    }

    /// The Word Study header for `number`. Never waits: one not computed yet is submitted to
    /// the job queue and reported as Loading; ask again on a later frame.
    pub fn word_study_header(&mut self, number: StrongsNumber) -> WordStatus<WordStudyHeader> {
        self.word_study_library_changed();
        if let Some(s) = self.word_study.headers.get(&number) {
            return s.clone();
        }
        self.submit_word_study(Wanted::Header(number));
        WordStatus::Loading
    }

    /// The English-word chooser for `words`, like [`Session::word_study_header`].
    pub fn word_chooser(&mut self, words: &str) -> WordStatus<Chooser> {
        self.word_study_library_changed();
        let key = normalized_words(words);
        if let Some(s) = self.word_study.choosers.get(&key) {
            return s.clone();
        }
        self.submit_word_study(Wanted::Chooser(key));
        WordStatus::Loading
    }

    /// Apply finished header and chooser jobs; return the other events. A chooser with
    /// exactly one number moves the Tabs showing it there. Pending jobs nobody asked for since
    /// the last poll are cancelled.
    pub(crate) fn poll_word_studies(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let wanted = std::mem::take(&mut self.word_study.wanted);
        let choosing: BTreeSet<String> = self
            .word_study
            .choosing
            .values()
            .map(|(w, _)| normalized_words(w))
            .collect();
        for (id, w) in &self.word_study.pending {
            let still =
                wanted.contains(w) || matches!(w, Wanted::Chooser(c) if choosing.contains(c));
            if !still {
                self.jobs().cancel(*id);
            }
        }
        let mut other = Vec::new();
        for event in events {
            if !self.word_study.pending.contains_key(&event.id) {
                other.push(event);
                continue;
            }
            if !event.kind.is_terminal() {
                continue;
            }
            let Some(w) = self.word_study.pending.remove(&event.id) else {
                continue;
            };
            let result = match event.kind {
                JobEventKind::Finished(out) => out
                    .downcast::<Computed>()
                    .map(|c| *c)
                    .map_err(|_| "Unexpected Word Study result".to_string()),
                JobEventKind::Failed(e) => Err(e),
                _ => continue,
            };
            if self.word_study.headers.len() + self.word_study.choosers.len() >= RESULTS_LIMIT {
                self.word_study.headers.clear();
                self.word_study.choosers.clear();
            }
            match (w, result) {
                (_, Ok(Computed::Header(h))) => {
                    self.word_study
                        .headers
                        .insert(h.strongs, WordStatus::Ready(Arc::new(h)));
                }
                (_, Ok(Computed::Chooser(c))) => {
                    let words = normalized_words(&c.words);
                    if let [only] = c.choices.as_slice() {
                        let tabs: Vec<TabId> = self
                            .word_study
                            .choosing
                            .iter()
                            .filter(|(_, (w, _))| normalized_words(w) == words)
                            .map(|(t, _)| *t)
                            .collect();
                        for tab in tabs {
                            self.word_study.choosing.remove(&tab);
                            self.workspace
                                .navigate_entry(tab, Key::StrongsNumber(only.strongs));
                        }
                    }
                    self.word_study
                        .choosers
                        .insert(words, WordStatus::Ready(Arc::new(c)));
                }
                (Wanted::Header(n), Err(e)) => {
                    self.word_study.headers.insert(n, WordStatus::Failed(e));
                }
                (Wanted::Chooser(words), Err(e)) => {
                    self.word_study
                        .choosers
                        .insert(words, WordStatus::Failed(e));
                }
            }
        }
        // A Tab that closed or moved on no longer chooses.
        let ws = &self.workspace;
        self.word_study.choosing.retain(|tab, (_, at)| {
            ws.tab(*tab)
                .is_some_and(|t| t.entry_history.current() == *at)
        });
        other
    }

    /// Show the English-word chooser in a Word Study Tab.
    fn choose_words(&mut self, tab: TabId, words: &str) {
        let at = self
            .workspace
            .tab(tab)
            .and_then(|t| t.entry_history.current());
        self.word_study
            .choosing
            .insert(tab, (words.to_string(), at));
        // Ask now, so a one-number answer can move the Tab even before a frame asks. When the
        // answer is already known (asked before), no job event will come: go there now.
        if let WordStatus::Ready(c) = self.word_chooser(words)
            && let [only] = c.choices.as_slice()
        {
            self.word_study.choosing.remove(&tab);
            self.workspace
                .navigate_entry(tab, Key::StrongsNumber(only.strongs));
        }
    }

    /// Close a Word Study Tab's chooser.
    pub fn stop_choosing(&mut self, tab: TabId) {
        self.word_study.choosing.remove(&tab);
    }
}

/// Words as the chooser caches them: normalized words joined with single spaces.
fn normalized_words(words: &str) -> String {
    obs_model::tokenize(words)
        .into_iter()
        .map(|t| t.text)
        .collect::<Vec<_>>()
        .join(" ")
}

/// `tab.go` for a Word Study: a Strong's number moves the Guide (and its Link set); English
/// words open the chooser.
pub(crate) fn go(s: &mut Session, tab: TabId, text: &str) -> Result<Value, CommandError> {
    const ID: &str = "tab.go";
    match parse_input(text).map_err(|e| failed(ID, e))? {
        WordStudyInput::Strongs(n) => {
            s.stop_choosing(tab);
            s.workspace.navigate_entry(tab, Key::StrongsNumber(n));
            Ok(json!({ "tab": tab.0, "key": n.to_string() }))
        }
        WordStudyInput::Words(words) => {
            s.choose_words(tab, &words);
            Ok(json!({ "tab": tab.0, "choose": words }))
        }
    }
}

/// Open a Word Study at `number`: the focused or most recent Word Study Tab moves there (and
/// its Link set follows); with `new_tab`, or when there is none, a new one opens.
pub(crate) fn open_at(
    s: &mut Session,
    number: StrongsNumber,
    request: OpenRequest,
    new_tab: bool,
) -> TabId {
    let kind = TabKind::Guide(Guide::WordStudy);
    let focused = s.workspace.focused();
    let existing = (!new_tab && !request.split)
        .then(|| {
            s.workspace
                .tabs()
                .filter(|t| t.kind == kind)
                .max_by_key(|t| (Some(t.id) == focused, t.id))
                .map(|t| t.id)
        })
        .flatten();
    let tab = match existing {
        Some(tab) => {
            s.workspace.focus(tab);
            tab
        }
        None => s
            .workspace
            .open(kind, Guide::WordStudy.title().into(), request),
    };
    s.stop_choosing(tab);
    s.workspace.navigate_entry(tab, Key::StrongsNumber(number));
    tab
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn request(p: &Params<'_>) -> OpenRequest {
    OpenRequest {
        panel: p
            .i64("panel")
            .and_then(|n| u64::try_from(n).ok())
            .map(PanelId),
        split: p.bool("split").unwrap_or(false),
    }
}

/// The Strong's number Word Info shows.
pub(crate) fn word_info_number(s: &Session) -> Option<StrongsNumber> {
    s.word_info.shown.as_ref().map(crate::ShownWord::strongs)
}

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "word_study.open",
    label: "Open Word Study",
    shortcut: None,
    params: &[
        ParamSpec {
            name: "strongs",
            kind: ParamKind::String,
            required: true,
            help: "Strong's number, e.g. G5485 or H1254A",
        },
        ParamSpec {
            name: "new_tab",
            kind: ParamKind::Boolean,
            required: false,
            help: "Open a new Word Study Tab even if one is open (Ctrl/Cmd-click)",
        },
        ParamSpec {
            name: "panel",
            kind: ParamKind::Integer,
            required: false,
            help: "Panel whose + was used",
        },
        ParamSpec {
            name: "split",
            kind: ParamKind::Boolean,
            required: false,
            help: "Open in a new Panel split off to the right (Ctrl/Cmd held)",
        },
    ],
    enabled: always,
    run: |s, p| {
        const ID: &str = "word_study.open";
        let text = p.str("strongs").unwrap_or_default();
        let number: StrongsNumber = text.parse().map_err(|e| failed(ID, e))?;
        let new_tab = p.bool("new_tab").unwrap_or(false);
        let tab = open_at(s, number, request(&p), new_tab);
        Ok(json!({ "tab": tab.0, "key": number.to_string() }))
    },
}];
