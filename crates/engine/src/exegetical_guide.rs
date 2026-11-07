//! The Exegetical Guide (#144): a Passage word by word in its original language. Its Key is a
//! Passage in the Standard versification; its sections are pure data, like every Guide's.
//!
//! - **Which Bible:** for each Standard Verse, the first Bible in Preferred order in the
//!   Verse's original language (Hebrew or Aramaic for the Old Testament, Greek for the New:
//!   TAHOT, TAGNT) whose own Verses for it hold Tagged words. The Standard Verse reaches the
//!   Bible's own numbering through the Bible's Verse map, so English Ps 3:1 reads TAHOT's
//!   Ps 3:2. A Bible's Verse reached from two Standard Verses is listed once.
//! - **Word by word:** every Tagged word of those Verses in text order: the word as written,
//!   its transliteration, the Lemma and Strong's number of its main part (the first part
//!   below STEPBible's affix numbers, as Word Info picks it), the Gloss of all its parts, and
//!   its Morphology in plain English, one part after another.
//! - **Lexicon summaries:** each distinct main Strong's number in order of first use, with the
//!   short entry of the first Lexicon in Preferred order that has it (one opening with an
//!   editorial note gives way to the next, as in Key words).
//! - **Grammatical notes:** each Morphology term the words use (aorist, Qal, genitive …), in
//!   order of first use, with its plain-English explanation and the words that use it.

use obs_model::{GrammarTerm, MorphemePart, morph};
use obs_scripture::{StrongsNumber, Verse};

use crate::word_info::ShownWord;

/// Longest short entry kept for a Lexicon summary, in characters.
pub const SUMMARY_CHARS: usize = 320;

/// One Tagged word of the Passage in its original language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExegeticalWord {
    /// As written in the Bible.
    pub written: String,
    pub rtl: bool,
    pub transliteration: Option<String>,
    /// The main part's Strong's number (the word's own, not a prefix's).
    pub strongs: StrongsNumber,
    /// The main part's Lemma (original script).
    pub lemma: Option<String>,
    /// The glosses of every part, in order (`and he created`).
    pub gloss: Option<String>,
    /// Plain English, each part's in turn: `Verb, aorist active indicative, third person
    /// singular`.
    pub morphology: Option<String>,
    /// The raw codes, `/` between parts, for a tooltip only.
    pub morph_code: Option<String>,
    /// The parts as tagged, for Grammatical notes.
    pub parts: Vec<MorphemePart>,
}

impl ExegeticalWord {
    /// From one `tagged_word` span; `None` when it has no parts.
    pub fn new(written: &str, parts: &[MorphemePart]) -> Option<Self> {
        let rtl = written
            .chars()
            .any(|c| matches!(c as u32, 0x0590..=0x05FF | 0xFB1D..=0xFB4F));
        let shown = ShownWord::new(written, rtl, String::new(), parts, |_| None)?;
        let codes: Vec<&str> = parts
            .iter()
            .filter_map(|p| p.morph.as_deref())
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        let plain: Vec<String> = codes
            .iter()
            .filter_map(|c| morph::parse(c).ok())
            .map(|m| m.long())
            .filter(|m| !m.is_empty())
            .collect();
        Some(Self {
            written: written.to_string(),
            rtl,
            transliteration: shown.transliteration.filter(|t| !t.is_empty()),
            strongs: shown.main.strongs,
            lemma: shown.main.lemma,
            gloss: shown.gloss,
            morphology: (!plain.is_empty()).then(|| plain.join(" + ")),
            morph_code: (!codes.is_empty()).then(|| codes.join("/")),
            parts: parts.to_vec(),
        })
    }
}

/// One Verse of the Bible read, with its words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExegeticalVerse {
    /// The Standard Verse that led to it (the first, when several do).
    pub standard: Verse,
    /// The Verse in the Bible's own numbering.
    pub native: Verse,
    pub bible_id: String,
    pub bible_abbreviation: String,
    pub words: Vec<ExegeticalWord>,
}

impl ExegeticalVerse {
    /// `Psalms 3:2 (TAHOT)`: the Bible's own numbering.
    pub fn reference(&self) -> String {
        format!("{} ({})", verse_label(self.native), self.bible_abbreviation)
    }

    /// Whether the Bible numbers the Verse differently from the Standard versification.
    pub fn renumbered(&self) -> bool {
        self.native != self.standard
    }
}

/// `Psalms 3:2`, `Psalms 3:title`.
pub fn verse_label(v: Verse) -> String {
    let verse = if v.is_title() {
        "title".to_string()
    } else {
        v.verse.to_string()
    };
    format!("{} {}:{verse}", v.book.name(), v.chapter)
}

/// The Word by word section: the Passage's Verses in its original language.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WordByWord {
    pub verses: Vec<ExegeticalVerse>,
}

impl WordByWord {
    pub fn words(&self) -> impl Iterator<Item = &ExegeticalWord> {
        self.verses.iter().flat_map(|v| v.words.iter())
    }

    pub fn word_count(&self) -> usize {
        self.verses.iter().map(|v| v.words.len()).sum()
    }
}

/// One Lexicon summary: a Strong's number of the Passage and the preferred Lexicon's short
/// entry for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LexiconSummary {
    pub strongs: StrongsNumber,
    /// The Passage's Lemma, else the entry's headword.
    pub lemma: Option<String>,
    pub transliteration: Option<String>,
    /// Words of the Passage with this number.
    pub in_passage: u32,
    /// The entry's first lines after its headword, at most [`SUMMARY_CHARS`] characters.
    pub summary: String,
    /// The whole entry, for opening the Lexicon at it.
    pub entry: crate::word_study::LexiconEntry,
}

impl LexiconSummary {
    /// Whether the Lemma is Hebrew (drawn right to left).
    pub fn rtl(&self) -> bool {
        self.lemma.as_deref().is_some_and(|l| {
            l.chars()
                .any(|c| matches!(c as u32, 0x0590..=0x05FF | 0xFB1D..=0xFB4F))
        })
    }
}

/// One Grammatical note: a Morphology term the Passage uses, explained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarNote {
    pub term: GrammarTerm,
    /// What the term is called in the note's heading ("Tense", "Stem").
    pub label: &'static str,
    pub explanation: &'static str,
    /// The words that use it, as written, each once, in order.
    pub words: Vec<String>,
}

/// STEPBible numbers prefixes and suffixes from H9001 / G9001 up (see [`crate::word_info`]).
const AFFIX_NUMBERS_FROM: u32 = 9000;

/// The Grammatical notes of `words`: each term once, in order of first use.
pub fn grammar_notes(words: &WordByWord) -> Vec<GrammarNote> {
    let mut notes: Vec<GrammarNote> = Vec::new();
    for w in words.words() {
        for part in &w.parts {
            let Some(m) = part.morph.as_deref().and_then(|c| morph::parse(c).ok()) else {
                continue;
            };
            for term in m.terms() {
                let explanation = term.explanation();
                // Hebrew and Aramaic share their notes (Daniel, Ezra): one note per meaning.
                let found = notes.iter_mut().find(|n| {
                    n.term.kind == term.kind
                        && n.term.name == term.name
                        && n.explanation == explanation
                });
                let note = match found {
                    Some(n) => n,
                    None => {
                        notes.push(GrammarNote {
                            term,
                            label: term.kind.label(term.language),
                            explanation,
                            words: Vec::new(),
                        });
                        let last = notes.len() - 1;
                        &mut notes[last]
                    }
                };
                if !note.words.contains(&w.written) {
                    note.words.push(w.written.clone());
                }
            }
        }
    }
    notes
}

/// The distinct main Strong's numbers of `words` in order of first use, with how often each
/// is used and the first Lemma given for it.
pub fn passage_numbers(words: &WordByWord) -> Vec<(StrongsNumber, u32, Option<String>)> {
    let mut out: Vec<(StrongsNumber, u32, Option<String>)> = Vec::new();
    for w in words.words() {
        if w.strongs.number() >= AFFIX_NUMBERS_FROM {
            continue;
        }
        match out.iter_mut().find(|(n, _, _)| *n == w.strongs) {
            Some((_, count, lemma)) => {
                *count += 1;
                if lemma.is_none() {
                    lemma.clone_from(&w.lemma);
                }
            }
            None => out.push((w.strongs, 1, w.lemma.clone())),
        }
    }
    out
}

/// An entry's short form: its paragraphs after the headword line, whitespace collapsed, cut
/// at a word to [`SUMMARY_CHARS`].
pub fn short_entry(text: &str) -> String {
    let mut paragraphs = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = paragraphs.next().unwrap_or_default();
    let rest: Vec<&str> = paragraphs.collect();
    let body = if rest.is_empty() {
        first.to_string()
    } else {
        rest.join(" ")
    };
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::guide::cut(&collapsed, SUMMARY_CHARS)
}

#[cfg(not(target_arch = "wasm32"))]
mod library {
    use std::collections::{BTreeMap, HashSet};
    use std::ops::Range;
    use std::sync::Arc;

    use obs_model::{ResourceType, SpanKind, UnitId, transliterate};
    use obs_scripture::{Passage, Standard, StrongsLanguage, Testament, VerseMap};
    use obs_store::{ObrReader, ResourceStore, StoreError};

    use super::*;
    use crate::key_words::{BibleCache, is_original_language};
    use crate::word_info::has_original_script;
    use crate::{Library, LibraryEntry};

    /// The original language of a Bible book's Verses.
    fn language(v: Verse) -> StrongsLanguage {
        match v.book.testament() {
            Testament::Old => StrongsLanguage::Hebrew,
            Testament::New => StrongsLanguage::Greek,
        }
    }

    /// One Bible opened for the section, with its Verse map and the units read so far.
    struct Opened {
        reader: ObrReader,
        map: Arc<Option<VerseMap>>,
        texts: BTreeMap<UnitId, String>,
        /// Its Verses already listed, by unit and start.
        listed: HashSet<(UnitId, u32)>,
        /// The Tagged words of the whole Passage in one unit, by start, read at once: a span
        /// query costs as much as the unit's spans before it, so one per Verse made a chapter
        /// late in a book take a fifth of a second (#183).
        passage_words: Option<PassageWords>,
    }

    /// The Tagged words of one unit's text range, sorted by start.
    type PassageWords = (UnitId, Range<u32>, Vec<obs_model::Span>);

    impl Opened {
        fn open(
            entry: &LibraryEntry,
            cache: &BibleCache,
            passage: Passage,
        ) -> Result<Self, String> {
            let reader = ObrReader::open(&entry.path)
                .map_err(|e| format!("{}: {e}", entry.manifest.abbreviation))?;
            let map = cache.verse_map(&reader)?;
            let mut opened = Opened {
                reader,
                map,
                texts: BTreeMap::new(),
                listed: HashSet::new(),
                passage_words: None,
            };
            // A Passage that cannot be read at once is read Verse by Verse.
            if passage.start() != passage.end() {
                opened.passage_words = opened.read_passage_words(passage).ok().flatten();
            }
            Ok(opened)
        }

        /// The Bible's own Verse for a Standard one (the first, when it has several).
        fn native(&self, standard: Verse) -> Result<Option<obs_model::BibleVerse>, StoreError> {
            let native = match self.map.as_ref() {
                Some(m) => m.to_native(standard).into_iter().next(),
                None => Some(standard),
            };
            native.map_or(Ok(None), |v| self.reader.verse(v))
        }

        /// The Tagged words from the Passage's first Verse to its last, when both are in one
        /// unit.
        fn read_passage_words(&self, passage: Passage) -> Result<Option<PassageWords>, StoreError> {
            let (Some(first), Some(last)) =
                (self.native(passage.start())?, self.native(passage.end())?)
            else {
                return Ok(None);
            };
            if first.unit != last.unit || first.start > last.end {
                return Ok(None);
            }
            let range = first.start..last.end;
            let mut spans: Vec<obs_model::Span> = self
                .reader
                .spans_in(&first.unit, range.start as usize..range.end as usize)?
                .into_iter()
                .filter(|s| {
                    s.start >= range.start
                        && s.end <= range.end
                        && matches!(s.kind, SpanKind::TaggedWord { .. })
                })
                .collect();
            spans.sort_by_key(|s| (s.start, s.end));
            Ok(Some((first.unit, range, spans)))
        }

        /// The Tagged words of one of the Bible's own Verses, in text order.
        fn words(
            &mut self,
            verse: &obs_model::BibleVerse,
        ) -> Result<Vec<ExegeticalWord>, StoreError> {
            let in_verse = |s: &obs_model::Span| {
                s.start >= verse.start
                    && s.end <= verse.end
                    && matches!(s.kind, SpanKind::TaggedWord { .. })
            };
            let mut spans: Vec<obs_model::Span> = match &self.passage_words {
                Some((unit, range, spans))
                    if *unit == verse.unit
                        && range.start <= verse.start
                        && verse.end <= range.end =>
                {
                    let from = spans.partition_point(|s| s.start < verse.start);
                    spans[from..]
                        .iter()
                        .take_while(|s| s.start < verse.end)
                        .filter(|s| in_verse(s))
                        .cloned()
                        .collect()
                }
                _ => self
                    .reader
                    .spans_in(&verse.unit, verse.range())?
                    .into_iter()
                    .filter(in_verse)
                    .collect(),
            };
            if spans.is_empty() {
                return Ok(Vec::new());
            }
            spans.sort_by_key(|s| (s.start, s.end));
            if !self.texts.contains_key(&verse.unit) {
                let text = self
                    .reader
                    .unit_text(&verse.unit)?
                    .map(|(_, t)| t)
                    .unwrap_or_default();
                self.texts.insert(verse.unit.clone(), text);
            }
            let text = &self.texts[&verse.unit];
            Ok(spans
                .iter()
                .filter_map(|s| {
                    let SpanKind::TaggedWord { parts } = &s.kind else {
                        return None;
                    };
                    ExegeticalWord::new(text.get(s.range())?, parts)
                })
                .collect())
        }
    }

    /// Word by word for `passage` from the library (see the module docs for which Bible).
    /// A Bible that cannot be read is passed over; the section fails only when nothing was
    /// found and some Bible failed.
    pub fn library_word_by_word(
        library: &Library,
        passage: Passage,
        cache: &BibleCache,
    ) -> Result<WordByWord, String> {
        let originals: Vec<&LibraryEntry> = library
            .of_type(ResourceType::Bible)
            .filter(|e| {
                [StrongsLanguage::Hebrew, StrongsLanguage::Greek]
                    .into_iter()
                    .any(|l| is_original_language(&e.manifest.language, l))
            })
            .collect();
        let mut opened: Vec<Option<Result<Opened, String>>> =
            originals.iter().map(|_| None).collect();
        let mut out = WordByWord::default();
        for standard in passage.verses(&Standard) {
            let want = language(standard);
            for (i, entry) in originals.iter().enumerate() {
                if !is_original_language(&entry.manifest.language, want) {
                    continue;
                }
                let bible = opened[i].get_or_insert_with(|| Opened::open(entry, cache, passage));
                let Ok(bible) = bible else {
                    continue;
                };
                match verses_for(bible, standard) {
                    Ok(Some(found)) => {
                        out.verses.extend(found);
                        break;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        opened[i] = Some(Err(format!("{}: {e}", entry.manifest.abbreviation)));
                    }
                }
            }
        }
        if out.verses.is_empty() {
            let errors: Vec<String> = opened
                .into_iter()
                .flatten()
                .filter_map(Result::err)
                .collect();
            if !errors.is_empty() {
                return Err(errors.join("; "));
            }
        }
        Ok(out)
    }

    /// The Bible's own Verses for one Standard Verse that have Tagged words, not yet listed;
    /// `None` when it has no tagged Verse for it (another Bible may).
    fn verses_for(
        bible: &mut Opened,
        standard: Verse,
    ) -> Result<Option<Vec<ExegeticalVerse>>, StoreError> {
        let natives = match bible.map.as_ref() {
            Some(m) => m.to_native(standard),
            None => vec![standard],
        };
        let mut found = false;
        let mut out = Vec::new();
        for native in natives {
            let Some(v) = bible.reader.verse(native)? else {
                continue;
            };
            if bible.listed.contains(&(v.unit.clone(), v.start)) {
                found = true;
                continue;
            }
            let words = bible.words(&v)?;
            if words.is_empty() {
                continue;
            }
            found = true;
            bible.listed.insert((v.unit.clone(), v.start));
            let m = bible.reader.manifest();
            out.push(ExegeticalVerse {
                standard,
                native: v.verse,
                bible_id: m.resource_id.clone(),
                bible_abbreviation: m.abbreviation.clone(),
                words,
            });
        }
        Ok(found.then_some(out))
    }

    /// The Lexicon summaries of `words`: for each distinct main Strong's number, the first
    /// Lexicon in Preferred order with a usable entry. A Lexicon that cannot be opened is
    /// passed over; the section fails only when every Lexicon failed.
    pub fn lexicon_summaries(
        library: &Library,
        words: &WordByWord,
    ) -> Result<Vec<LexiconSummary>, String> {
        let mut lexicons = Vec::new();
        let mut errors = Vec::new();
        for entry in library.of_type(ResourceType::Lexicon) {
            match ObrReader::open(&entry.path) {
                Ok(r) => lexicons.push(r),
                Err(e) => errors.push(format!("{}: {e}", entry.manifest.abbreviation)),
            }
        }
        if lexicons.is_empty() && !errors.is_empty() {
            return Err(errors.join("; "));
        }
        let mut out = Vec::new();
        for (strongs, in_passage, lemma) in passage_numbers(words) {
            let mut first: Option<crate::word_study::LexiconEntry> = None;
            let mut usable = None;
            for lexicon in &lexicons {
                let Ok(found) = crate::word_study::lexicon_entries_from(lexicon, strongs) else {
                    continue;
                };
                let Some(entry) = found.into_iter().next() else {
                    continue;
                };
                // An entry opening with an editorial note (Abbott-Smith's `[in LXX …]`) gives
                // way to the next Lexicon.
                let note = short_entry(&entry.text).starts_with(['[', '(']);
                if !note {
                    usable = Some(entry);
                    break;
                }
                first.get_or_insert(entry);
            }
            let Some(entry) = usable.or(first) else {
                continue;
            };
            let lemma = lemma.or_else(|| entry.headword.clone().filter(|h| has_original_script(h)));
            let transliteration = lemma
                .as_deref()
                .filter(|l| has_original_script(l))
                .map(transliterate)
                .filter(|t| !t.is_empty());
            out.push(LexiconSummary {
                strongs,
                lemma,
                transliteration,
                in_passage,
                summary: short_entry(&entry.text),
                entry,
            });
        }
        Ok(out)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use library::{lexicon_summaries, library_word_by_word};

#[cfg(target_arch = "wasm32")]
pub fn library_word_by_word(
    _: &crate::Library,
    _: obs_scripture::Passage,
    _: &crate::key_words::BibleCache,
) -> Result<WordByWord, String> {
    Err("Opening Resources is not supported here".into())
}

#[cfg(target_arch = "wasm32")]
pub fn lexicon_summaries(
    _: &crate::Library,
    _: &WordByWord,
) -> Result<Vec<LexiconSummary>, String> {
    Err("Opening Resources is not supported here".into())
}
