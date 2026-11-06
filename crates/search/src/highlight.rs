//! Hit highlighting (#16 rule 7): done in Rust on the display text, never with FTS5's
//! `highlight()`, so byte ranges stay in the text the user sees.
//!
//! - **Surface terms** (words, phrases, prefixes) are found by re-tokenizing the display text
//!   with the shared normalizer ([`obs_model::tokenize`]), exactly as the index saw it.
//! - **`strong:` and `lemma:` terms** highlight the Tagged words whose morpheme parts match.
//!   Inside a Hebrew word of several morphemes (TAHOT `וַ/יִּבְרָא`) only the matching
//!   morphemes are highlighted: the stored text has no morpheme boundaries, so each prefix
//!   part is taken to be one letter (with its points) and the root plus any suffix the rest
//!   of the word.
//! - **Morphology criteria** (`@verb aorist`, `strong:G26 @noun genitive`) highlight the
//!   morpheme parts whose code meets them all, and that have the Strong's number or Lemma
//!   the `@` was attached to.
//!
//! Excluded terms (`-works`, `NOT works`) are never highlighted.

use std::ops::Range;

use obs_model::{MorphemePart, tokenize};
use obs_scripture::StrongsNumber;

use crate::morph::MorphCriteria;
use crate::query::{LemmaLookup, Query, is_original_script};

/// STEPBible numbers prefixes, suffixes and other grammatical morphemes from 9001 up.
const AFFIX_NUMBERS_FROM: u32 = 9000;

/// The positive terms of a query, ready to mark matches in display text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Highlighter {
    /// Surface phrases (a word is a one-word phrase) and whether the last word is a prefix.
    phrases: Vec<(Vec<String>, bool)>,
    strongs: Vec<StrongsNumber>,
    /// Lemmas as normalized words (transliterations already resolved).
    lemmas: Vec<Vec<String>>,
    /// Morphology words (`strong:G26 @noun genitive`).
    morph: Vec<MorphWord>,
}

/// One part that a `@…` term describes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MorphWord {
    strong: Option<StrongsNumber>,
    /// The Lemma's alternatives; `None` when no Lemma was given.
    lemmas: Option<Vec<Vec<String>>>,
    criteria: MorphCriteria,
}

impl MorphWord {
    fn matches(&self, p: &MorphemePart) -> bool {
        self.strong
            .is_none_or(|q| q == p.strongs || q == p.strongs.base())
            && self
                .lemmas
                .as_ref()
                .is_none_or(|l| lemma_in(l, p.lemma.as_deref()))
            && p.morph
                .as_deref()
                .is_some_and(|c| self.criteria.matches_code(c.trim()))
    }
}

/// Whether `lemma` holds one of `lemmas` (normalized words).
fn lemma_in(lemmas: &[Vec<String>], lemma: Option<&str>) -> bool {
    let Some(lemma) = lemma else {
        return false;
    };
    let words = words_of(lemma);
    lemmas
        .iter()
        .any(|l| !l.is_empty() && words.windows(l.len()).any(|w| w == l.as_slice()))
}

/// The Lemmas a `lemma:` term means.
fn resolve_lemma(text: &str, lookup: &dyn LemmaLookup) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    if is_original_script(text) {
        out.push(words_of(text));
    } else {
        let translit = words_of(text).join(" ");
        for l in lookup.lemmas_for_transliteration(&translit) {
            out.push(words_of(&l));
        }
    }
    out.retain(|l| !l.is_empty());
    out
}

fn words_of(s: &str) -> Vec<String> {
    tokenize(s).into_iter().map(|t| t.text).collect()
}

impl Highlighter {
    /// The terms of `query` that a hit must (or may) contain. `lemmas` resolves
    /// transliterated `lemma:` terms as the compiler does.
    pub fn new(query: &Query, lemmas: &dyn LemmaLookup) -> Self {
        let mut h = Self::default();
        h.collect(query, lemmas);
        h.phrases.dedup();
        h.strongs.sort_by_key(ToString::to_string);
        h.strongs.dedup();
        h.lemmas.sort();
        h.lemmas.dedup();
        h
    }

    fn collect(&mut self, q: &Query, lookup: &dyn LemmaLookup) {
        match q {
            Query::Word { word, prefix, .. } => self.phrases.push((vec![word.clone()], *prefix)),
            Query::Phrase { words, prefix, .. } => self.phrases.push((words.clone(), *prefix)),
            Query::Strong { number, .. } => self.strongs.push(*number),
            Query::Lemma { text, .. } => self.lemmas.extend(resolve_lemma(text, lookup)),
            Query::Morph {
                strong,
                lemma,
                criteria,
                ..
            } => self.morph.push(MorphWord {
                strong: *strong,
                lemmas: lemma.as_deref().map(|l| resolve_lemma(l, lookup)),
                criteria: criteria.clone(),
            }),
            Query::And(all) | Query::Or(all) => {
                for a in all {
                    self.collect(a, lookup);
                }
            }
            Query::Not { .. } => {}
        }
    }

    /// Whether nothing would ever be highlighted.
    pub fn is_empty(&self) -> bool {
        self.phrases.is_empty()
            && self.strongs.is_empty()
            && self.lemmas.is_empty()
            && self.morph.is_empty()
    }

    /// Whether a query Strong's number matches a tagged one: an extended number matches
    /// exactly, a base number matches all its homographs (#16).
    fn strong_matches(&self, n: StrongsNumber) -> bool {
        self.strongs.iter().any(|q| *q == n || *q == n.base())
    }

    fn lemma_matches(&self, lemma: Option<&str>) -> bool {
        lemma_in(&self.lemmas, lemma)
    }

    /// Which morpheme parts of a Tagged word match a `strong:`, `lemma:` or `@…` term.
    pub fn matching_parts(&self, parts: &[MorphemePart]) -> Vec<bool> {
        parts
            .iter()
            .map(|p| {
                self.strong_matches(p.strongs)
                    || self.lemma_matches(p.lemma.as_deref())
                    || self.morph.iter().any(|m| m.matches(p))
            })
            .collect()
    }

    /// Byte ranges of `text` to highlight, sorted and merged. `tagged` lists the Tagged words
    /// in `text` (byte ranges of `text`) with their morpheme parts.
    pub fn ranges<'a, I>(&self, text: &str, tagged: I) -> Vec<Range<usize>>
    where
        I: IntoIterator<Item = (Range<usize>, &'a [MorphemePart])>,
    {
        let mut out = Vec::new();
        if !self.phrases.is_empty() {
            let tokens = tokenize(text);
            for (words, prefix) in &self.phrases {
                let n = words.len();
                if n == 0 || tokens.len() < n {
                    continue;
                }
                for i in 0..=tokens.len() - n {
                    let all = (0..n).all(|k| {
                        let t = &tokens[i + k].text;
                        let w = &words[k];
                        t == w || (*prefix && k == n - 1 && t.starts_with(w.as_str()))
                    });
                    if all {
                        out.push(tokens[i].range.start..tokens[i + n - 1].range.end);
                    }
                }
            }
        }
        if !self.strongs.is_empty() || !self.lemmas.is_empty() || !self.morph.is_empty() {
            for (range, parts) in tagged {
                let matched = self.matching_parts(parts);
                if matched.iter().any(|m| *m) {
                    out.extend(morpheme_ranges(text, range, parts, &matched));
                }
            }
        }
        merge(out)
    }
}

/// Sort and merge overlapping or touching ranges.
pub fn merge(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    ranges.retain(|r| r.start < r.end);
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut out: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for r in ranges {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

fn is_hebrew_letter(c: char) -> bool {
    matches!(c as u32, 0x05D0..=0x05EA | 0xFB1D..=0xFB4F)
}

/// Hebrew points and cantillation (not maqaf, paseq, sof pasuq or nun hafukha), and Latin
/// combining marks.
fn is_mark(c: char) -> bool {
    matches!(c as u32, 0x0591..=0x05C7 | 0x0300..=0x036F)
        && !matches!(c as u32, 0x05BE | 0x05C0 | 0x05C3 | 0x05C6)
}

/// Start bytes (within `word`) of each letter with its marks.
fn letter_starts(word: &str) -> Vec<usize> {
    word.char_indices()
        .filter(|(_, c)| !is_mark(*c))
        .map(|(i, _)| i)
        .collect()
}

/// The parts of one Tagged word to highlight. A word of one morpheme, a word whose parts all
/// match, and any word that is not Hebrew are highlighted whole. Otherwise each matching
/// prefix (a part before the root) is its own letter, and a matching root or suffix covers
/// the rest of the word after the prefixes.
pub fn morpheme_ranges(
    text: &str,
    word: Range<usize>,
    parts: &[MorphemePart],
    matched: &[bool],
) -> Vec<Range<usize>> {
    let Some(written) = text.get(word.clone()) else {
        return Vec::new();
    };
    let whole = vec![word.clone()];
    if parts.len() < 2 || matched.iter().all(|m| *m) || !written.chars().any(is_hebrew_letter) {
        return whole;
    }
    let root = parts
        .iter()
        .position(|p| p.strongs.number() < AFFIX_NUMBERS_FROM)
        .unwrap_or(0);
    let letters = letter_starts(written);
    if letters.len() <= root {
        return whole;
    }
    let mut out = Vec::new();
    for (i, m) in matched.iter().enumerate() {
        if !*m {
            continue;
        }
        let (a, b) = if i < root {
            (
                letters[i],
                letters.get(i + 1).copied().unwrap_or(written.len()),
            )
        } else {
            (letters[root], written.len())
        };
        out.push(word.start + a..word.start + b);
    }
    merge(out)
}
