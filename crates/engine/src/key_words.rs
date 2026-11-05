//! The Passage Guide's Key words section (#54; decision #15): the distinct Strong's numbers
//! of the Passage in the ★ Strong's-tagged Bible, rarest first, without function words.
//!
//! - **★ Strong's-tagged Bible:** the first Bible in Preferred order with Tagged words in the
//!   Passage (a Bible without that Passage is passed over, #15 rule 2).
//! - **Each row:** Lemma, transliteration, Gloss, Part of speech, count in the Passage and total
//!   in that Bible. The Lemma comes from the Tagged word, else from the same number in the
//!   same Passage of an original-language Bible (Greek for G numbers, Hebrew or Aramaic for
//!   H; TAGNT, TAHOT), else from the first Lexicon's headword. The Gloss is that of the first
//!   Lexicon in Preferred order that has the number (one whose entry opens with an editorial
//!   note instead of a gloss is passed over), else the Tagged word's own.
//! - **Function words** (articles, conjunctions, prepositions, particles, pronouns and
//!   Hebrew affixes) are left out by Part of speech: the Tagged word's Morphology, else the
//!   Morphology of the same number in the original-language Bible's Passage (TAHOT/TAGNT give
//!   one for every word, KJV+Strong's none). STEPBible's affix numbers (H9001 and up) always
//!   are.
//! - **Totals** count every Tagged-word part with the number in the whole Bible, once per
//!   Bible revision (cached in memory by [`BibleCache`]).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use obs_model::{MorphemePart, PartOfSpeech, morph, transliterate};
use obs_scripture::{Passage, StrongsLanguage, StrongsNumber, VerseMap};
#[cfg(not(target_arch = "wasm32"))]
use obs_store::{ResourceStore, StoreError};

use crate::word_info::has_original_script;

/// STEPBible numbers Hebrew prefixes and suffixes from H9001 up (see [`crate::word_info`]).
const AFFIX_NUMBERS_FROM: u32 = 9000;

/// One Key word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyWord {
    pub strongs: StrongsNumber,
    /// Original script.
    pub lemma: Option<String>,
    pub transliteration: Option<String>,
    pub gloss: Option<String>,
    /// Plain English (`verb`, `noun`), when known.
    pub part_of_speech: Option<&'static str>,
    /// Tagged-word parts with this number in the Passage.
    pub in_passage: u32,
    /// … and in the whole Bible.
    pub in_bible: u32,
}

impl KeyWord {
    /// Whether the Lemma is Hebrew (drawn right to left).
    pub fn rtl(&self) -> bool {
        self.lemma.as_deref().is_some_and(|l| {
            l.chars()
                .any(|c| matches!(c as u32, 0x0590..=0x05FF | 0xFB1D..=0xFB4F))
        })
    }
}

/// What the Key words section shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyWords {
    /// The ★ Strong's-tagged Bible the words and totals come from.
    pub bible_id: String,
    pub bible_abbreviation: String,
    /// Rarest first: by total in the Bible, then by count in the Passage (more first).
    pub words: Vec<KeyWord>,
    /// Distinct function words left out.
    pub function_words: usize,
}

/// Which revision of a Resource something was computed from.
pub(crate) type Identity = (String, u32, String);

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn identity(m: &obs_model::Manifest) -> Identity {
    (m.resource_id.clone(), m.revision, m.content_hash.clone())
}

/// Per-Bible data kept across section jobs: Verse maps and Strong's totals, one revision of
/// each Bible at a time.
#[derive(Debug, Default)]
pub struct BibleCache {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    maps: Mutex<BTreeMap<Identity, Arc<Option<VerseMap>>>>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    totals: Mutex<BTreeMap<Identity, Arc<HashMap<StrongsNumber, u32>>>>,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn keep<T: Clone>(
    map: &Mutex<BTreeMap<Identity, T>>,
    id: &Identity,
    make: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if let Some(v) = map.lock().unwrap_or_else(|p| p.into_inner()).get(id) {
        return Ok(v.clone());
    }
    // Computed without the lock held, so other Bibles' jobs are not held up.
    let v = make()?;
    let mut m = map.lock().unwrap_or_else(|p| p.into_inner());
    m.retain(|(rid, _, _), _| *rid != id.0);
    m.insert(id.clone(), v.clone());
    Ok(v)
}

#[cfg(not(target_arch = "wasm32"))]
impl BibleCache {
    pub(crate) fn verse_map(
        &self,
        store: &dyn ResourceStore,
    ) -> Result<Arc<Option<VerseMap>>, String> {
        keep(&self.maps, &identity(store.manifest()), || {
            store.verse_map().map(Arc::new).map_err(|e| e.to_string())
        })
    }

    /// How often each Strong's number is tagged in the whole Bible.
    pub fn totals(
        &self,
        store: &dyn ResourceStore,
    ) -> Result<Arc<HashMap<StrongsNumber, u32>>, String> {
        keep(&self.totals, &identity(store.manifest()), || {
            let mut counts: HashMap<StrongsNumber, u32> = HashMap::new();
            store
                .for_each_tagged_word(&mut |parts| {
                    for p in parts {
                        *counts.entry(p.strongs).or_default() += 1;
                    }
                })
                .map_err(|e| e.to_string())?;
            Ok(Arc::new(counts))
        })
    }
}

/// The Bible Verses (own numbering) that make up a Standard `passage`, in order, each once.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn passage_verses(
    store: &dyn ResourceStore,
    map: Option<&VerseMap>,
    passage: Passage,
) -> Result<Vec<obs_model::BibleVerse>, StoreError> {
    let mut out: Vec<obs_model::BibleVerse> = Vec::new();
    for standard in passage.verses(&obs_scripture::Standard) {
        let natives = match map {
            Some(m) => m.to_native(standard),
            None => vec![standard],
        };
        for native in natives {
            if let Some(v) = store.verse(native)?
                && !out.iter().any(|o| o.unit == v.unit && o.start == v.start)
            {
                out.push(v);
            }
        }
    }
    Ok(out)
}

/// The parts of every Tagged word inside a Bible's Verses for `passage`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn passage_parts(
    store: &dyn ResourceStore,
    map: Option<&VerseMap>,
    passage: Passage,
) -> Result<Vec<MorphemePart>, StoreError> {
    let mut parts = Vec::new();
    for v in passage_verses(store, map, passage)? {
        for span in store.spans_in(&v.unit, v.range())? {
            if span.start < v.start || span.end > v.end {
                continue;
            }
            if let obs_model::SpanKind::TaggedWord { parts: p } = span.kind {
                parts.extend(p);
            }
        }
    }
    Ok(parts)
}

/// What one number looks like in the Passage, gathered from every Bible read.
#[derive(Default)]
struct Seen {
    count: u32,
    lemma: Option<String>,
    pos: Option<PartOfSpeech>,
    gloss: Option<String>,
}

fn clean(s: Option<&str>) -> Option<String> {
    let s = s?
        .trim()
        .trim_matches(|c: char| matches!(c, ',' | '.' | ';' | ':' | '<' | '>' | '[' | ']'))
        .trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn part_pos(p: &MorphemePart) -> Option<PartOfSpeech> {
    let code = p.morph.as_deref()?.trim();
    morph::parse(code).ok()?.pos()
}

/// Fill in what `seen` lacks from `parts` (the same Passage in another Bible).
fn learn(seen: &mut Seen, number: StrongsNumber, parts: &[MorphemePart]) {
    for p in parts.iter().filter(|p| {
        p.strongs == number || (p.strongs.base() == number.base() && !number.is_extended())
    }) {
        if seen.pos.is_none() {
            seen.pos = part_pos(p);
        }
        if seen.lemma.is_none() {
            seen.lemma = clean(p.lemma.as_deref());
        }
        if seen.gloss.is_none() {
            seen.gloss = clean(p.gloss.as_deref());
        }
    }
}

/// Rank the ★ Bible's Passage words: tally them, fill each from `others` (other Bibles'
/// words in the same Passage) and `lexicon` (Lemma and Gloss of the first Lexicon that has
/// the number), drop function words and sort rarest first.
pub fn rank(
    star: &[MorphemePart],
    others: &[Vec<MorphemePart>],
    totals: &HashMap<StrongsNumber, u32>,
    mut lexicon: impl FnMut(StrongsNumber) -> Option<(Option<String>, String)>,
) -> (Vec<KeyWord>, usize) {
    let mut seen: BTreeMap<StrongsNumber, Seen> = BTreeMap::new();
    for p in star {
        let s = seen.entry(p.strongs).or_default();
        s.count += 1;
        if s.pos.is_none() {
            s.pos = part_pos(p);
        }
        if s.lemma.is_none() {
            s.lemma = clean(p.lemma.as_deref());
        }
        if s.gloss.is_none() {
            s.gloss = clean(p.gloss.as_deref());
        }
    }
    let mut words = Vec::new();
    let mut function_words = 0;
    for (number, mut s) in seen {
        if s.pos.is_none() || s.lemma.is_none() {
            for parts in others {
                learn(&mut s, number, parts);
            }
        }
        let is_function = number.number() >= AFFIX_NUMBERS_FROM
            || s.pos.is_some_and(PartOfSpeech::is_function_word);
        if is_function {
            function_words += 1;
            continue;
        }
        let (lexicon_lemma, lexicon_gloss) = match lexicon(number) {
            Some((l, g)) => (l, Some(g)),
            None => (None, None),
        };
        let lemma = s.lemma.or(lexicon_lemma);
        let transliteration = lemma
            .as_deref()
            .filter(|l| has_original_script(l))
            .map(transliterate)
            .filter(|t| !t.is_empty());
        words.push(KeyWord {
            strongs: number,
            lemma,
            transliteration,
            gloss: lexicon_gloss.or(s.gloss),
            part_of_speech: s.pos.map(PartOfSpeech::name),
            in_passage: s.count,
            in_bible: totals.get(&number).copied().unwrap_or(0).max(s.count),
        });
    }
    words.sort_by(|a, b| {
        a.in_bible
            .cmp(&b.in_bible)
            .then(b.in_passage.cmp(&a.in_passage))
            .then(a.strongs.cmp(&b.strongs))
    });
    (words, function_words)
}

/// The Key words of `passage` from the library.
#[cfg(not(target_arch = "wasm32"))]
pub fn library_key_words(
    library: &crate::Library,
    passage: Passage,
    cache: &BibleCache,
) -> Result<KeyWords, String> {
    use obs_model::ResourceType;
    let open = |e: &crate::LibraryEntry| {
        obs_store::ObrReader::open(&e.path)
            .map_err(|err| format!("{}: {err}", e.manifest.abbreviation))
    };
    let read = |entry: &crate::LibraryEntry| {
        open(entry).and_then(|reader| {
            let map = cache.verse_map(&reader)?;
            let parts = passage_parts(&reader, map.as_ref().as_ref(), passage)
                .map_err(|e| e.to_string())?;
            Ok((reader, parts))
        })
    };
    let mut star: Option<(obs_store::ObrReader, Vec<MorphemePart>)> = None;
    let mut errors = Vec::new();
    for entry in library.of_type(ResourceType::Bible) {
        match read(entry) {
            Ok((_, parts)) if parts.is_empty() => {}
            Ok(found) => {
                star = Some(found);
                break;
            }
            Err(e) => errors.push(e),
        }
    }
    let Some((reader, parts)) = star else {
        if errors.is_empty() {
            return Ok(KeyWords::default());
        }
        return Err(errors.join("; "));
    };
    // Words without Morphology or Lemma (KJV+Strong's) learn them from the same Passage in
    // an original-language Bible (TAGNT, TAHOT): only those are read.
    let lacking: Vec<StrongsLanguage> = parts
        .iter()
        .filter(|p| p.lemma.is_none() || part_pos(p).is_none())
        .map(|p| p.strongs.language())
        .collect();
    let mut others: Vec<Vec<MorphemePart>> = Vec::new();
    for entry in library.of_type(ResourceType::Bible) {
        if entry.manifest.resource_id == reader.manifest().resource_id
            || !lacking
                .iter()
                .any(|l| is_original_language(&entry.manifest.language, *l))
        {
            continue;
        }
        if let Ok((_, p)) = read(entry)
            && !p.is_empty()
        {
            others.push(p);
        }
    }
    let totals = cache.totals(&reader)?;
    let lexicons: Vec<obs_store::ObrReader> = library
        .of_type(ResourceType::Lexicon)
        .filter_map(|e| open(e).ok())
        .collect();
    let (words, function_words) = rank(&parts, &others, &totals, |n| lexicon_entry(&lexicons, n));
    let m = reader.manifest();
    Ok(KeyWords {
        bible_id: m.resource_id.clone(),
        bible_abbreviation: m.abbreviation.clone(),
        words,
        function_words,
    })
}

/// Whether a Bible in `language` (BCP 47) is written in the language Strong's numbers of
/// `strongs` describe: Greek for G, Hebrew or Aramaic for H.
pub fn is_original_language(language: &str, strongs: StrongsLanguage) -> bool {
    let primary = language
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match strongs {
        StrongsLanguage::Greek => matches!(primary.as_str(), "grc" | "el" | "gre" | "ell"),
        StrongsLanguage::Hebrew => {
            matches!(
                primary.as_str(),
                "hbo" | "he" | "heb" | "iw" | "arc" | "oar"
            )
        }
    }
}

/// The Lemma (entry headword) and Gloss of the first Lexicon that has `number`.
#[cfg(not(target_arch = "wasm32"))]
fn lexicon_entry(
    lexicons: &[obs_store::ObrReader],
    number: StrongsNumber,
) -> Option<(Option<String>, String)> {
    for lexicon in lexicons {
        let Ok(entries) = lexicon.lookup_strongs(number) else {
            continue;
        };
        let Some(entry) = entries.first() else {
            continue;
        };
        let Ok(Some(unit)) = lexicon.unit(&entry.unit) else {
            continue;
        };
        let (start, end) = entry.range.map_or((0, unit.text.len()), |(s, e)| {
            (s as usize, (e as usize).min(unit.text.len()))
        });
        // A Lexicon whose entry opens with an editorial note rather than a gloss
        // (Abbott-Smith's `[in LXX …]`) gives way to the next one.
        let Some(gloss) = unit
            .text
            .get(start..end)
            .and_then(crate::word_info::lexicon_excerpt)
            .filter(|g| !g.starts_with(['[', '(']))
        else {
            continue;
        };
        let lemma = unit.title.clone().filter(|t| has_original_script(t));
        return Some((lemma, gloss));
    }
    None
}

#[cfg(target_arch = "wasm32")]
pub fn library_key_words(
    _: &crate::Library,
    _: Passage,
    _: &BibleCache,
) -> Result<KeyWords, String> {
    Err("Opening Resources is not supported here".into())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn part(
        s: &str,
        lemma: Option<&str>,
        morph: Option<&str>,
        gloss: Option<&str>,
    ) -> MorphemePart {
        MorphemePart {
            strongs: s.parse().unwrap(),
            lemma: lemma.map(String::from),
            morph: morph.map(String::from),
            gloss: gloss.map(String::from),
        }
    }

    #[test]
    fn rarest_first_without_function_words_learning_grammar_from_other_bibles() {
        // KJV-style: numbers only.
        let kjv = [
            part("G1063", None, None, None),
            part("G2316", None, None, None),
            part("G25", None, None, None),
            part("G846", None, None, None),
            part("G846", None, None, None),
            part("G3439", None, None, None),
        ];
        // TAGNT-style: the same Passage with Lemma and Morphology.
        let tagnt = vec![
            part("G1063", Some("γάρ"), Some("CONJ"), Some("for")),
            part("G25", Some("ἀγαπάω"), Some("V-AAI-3S"), Some("loved")),
            part("G3588", Some("ὁ"), Some("T-NSM"), Some("the")),
            part("G2316", Some("θεός"), Some("N-NSM-T"), Some("God,")),
            part("G846", Some("αὐτός"), Some("P-ASM"), Some("him")),
        ];
        let totals: HashMap<StrongsNumber, u32> = [("G2316", 1300), ("G25", 143), ("G3439", 9)]
            .into_iter()
            .map(|(n, c)| (n.parse().unwrap(), c))
            .collect();
        let (words, function) = rank(&kjv, &[tagnt], &totals, |n| {
            (n.to_string() == "G3439").then(|| (Some("μονογενής".into()), "only begotten".into()))
        });
        let got: Vec<(String, u32, u32)> = words
            .iter()
            .map(|w| (w.strongs.to_string(), w.in_passage, w.in_bible))
            .collect();
        assert_eq!(
            got,
            [
                ("G3439".into(), 1, 9),
                ("G25".into(), 1, 143),
                ("G2316".into(), 1, 1300)
            ]
        );
        assert_eq!(function, 2, "γάρ and αὐτός");
        assert_eq!(words[0].lemma.as_deref(), Some("μονογενής"));
        assert_eq!(words[0].transliteration.as_deref(), Some("monogenes"));
        assert_eq!(words[0].gloss.as_deref(), Some("only begotten"));
        assert_eq!(words[1].lemma.as_deref(), Some("ἀγαπάω"));
        assert_eq!(words[1].part_of_speech, Some("verb"));
        assert_eq!(words[1].gloss.as_deref(), Some("loved"));
        assert_eq!(
            words[2].gloss.as_deref(),
            Some("God"),
            "punctuation trimmed"
        );
    }

    #[test]
    fn hebrew_affixes_and_tagged_function_words_are_left_out() {
        let tahot = [
            part("H9001", Some("ו"), Some("Hc"), Some("and")),
            part("H559", Some("אָמַר"), Some("HVqw3ms"), Some("he said")),
            part("H413", Some("אֶל"), Some("HR"), Some("to")),
            part("H87", Some("אַבְרָם"), Some("HNpm"), Some("Abram")),
            part("H9031", None, Some("HSp2ms"), Some("yourself")),
        ];
        let (words, function) = rank(&tahot, &[], &HashMap::new(), |_| None);
        let got: Vec<String> = words.iter().map(|w| w.strongs.to_string()).collect();
        // Equally rare: by number.
        assert_eq!(got, ["H87", "H559"]);
        assert_eq!(function, 3);
        assert!(words[0].rtl());
        assert_eq!(words[0].transliteration.as_deref(), Some("avram"));
    }
}
