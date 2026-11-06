//! Find in Resource (#175): every place one Resource's text reads as what the user typed in a
//! Tab's Find bar, as byte ranges of the text shown, so they can be drawn over it.
//!
//! Matching uses the Search normalizer ([`obs_model::fold_for_find`]): NFD, final forms and
//! curly quotes folded, then, by the Find options, combining marks dropped (Ignore
//! diacritics, on by default) and case folded (unless Match case). So "αγαπη" finds ἀγάπη and
//! "בראשית" finds בְּרֵאשִׁ֖ית. Matches are substrings unless Whole word is on.
//!
//! With Ignore diacritics on, a query in Latin letters also finds Greek and Hebrew words by
//! their transliteration in the shared scheme ([`obs_model::transliteration_key`]): "agape"
//! finds ἀγάπη. Such a match is always of whole words: a transliteration cannot be mapped
//! back letter by letter, and as a prefix "the" would find every θεός.

use std::ops::Range;

use obs_model::{fold_for_find, tokenize, transliteration_key};
use unicode_normalization::char::is_combining_mark;

/// The Find bar's options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
    pub ignore_diacritics: bool,
}

impl Default for FindOptions {
    fn default() -> Self {
        Self {
            match_case: false,
            whole_word: false,
            ignore_diacritics: true,
        }
    }
}

/// What the user typed, ready to find in text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finder {
    options: FindOptions,
    /// The query folded like the text.
    needle: String,
    /// The query's words, when it may match Greek and Hebrew words by transliteration.
    transliterated: Option<Vec<String>>,
}

fn is_original_script(c: char) -> bool {
    matches!(c as u32, 0x0370..=0x03FF | 0x1F00..=0x1FFF | 0x0590..=0x05FF | 0xFB1D..=0xFB4F)
}

/// A letter, digit or combining mark: what a whole word cannot continue into.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || is_combining_mark(c)
}

impl Finder {
    /// `None` when there is nothing to find (an empty or all-space query).
    pub fn new(query: &str, options: FindOptions) -> Option<Self> {
        if query.trim().is_empty() {
            return None;
        }
        let needle = fold_for_find(query, options.match_case, !options.ignore_diacritics).text;
        if needle.is_empty() {
            return None;
        }
        let transliterated = (options.ignore_diacritics
            && !query.chars().any(is_original_script)
            && query.chars().any(char::is_alphabetic))
        .then(|| tokenize(query).into_iter().map(|t| t.text).collect())
        .filter(|w: &Vec<String>| !w.is_empty());
        Some(Self {
            options,
            needle,
            transliterated,
        })
    }

    pub fn options(&self) -> FindOptions {
        self.options
    }

    /// Where `text` matches, as sorted, non-overlapping byte ranges of `text`.
    pub fn find(&self, text: &str) -> Vec<Range<usize>> {
        let folded = fold_for_find(
            text,
            self.options.match_case,
            !self.options.ignore_diacritics,
        );
        let mut out = Vec::new();
        let hay = folded.text.as_str();
        let mut from = 0;
        while let Some(at) = hay.get(from..).and_then(|h| h.find(&self.needle)) {
            let start = from + at;
            let end = start + self.needle.len();
            match folded
                .original(start, end)
                .filter(|r| !self.options.whole_word || whole_word(text, r))
            {
                Some(r) => {
                    out.push(r);
                    from = end;
                }
                None => {
                    from = start + hay[start..].chars().next().map_or(1, char::len_utf8);
                }
            }
        }
        if let Some(words) = &self.transliterated
            && text.chars().any(is_original_script)
        {
            out.extend(transliterated_matches(text, words));
            out.sort_by_key(|r| (r.start, r.end));
            // Keep the first of overlapping matches.
            let mut kept: Vec<Range<usize>> = Vec::with_capacity(out.len());
            for r in out {
                if kept.last().is_none_or(|k| k.end <= r.start) {
                    kept.push(r);
                }
            }
            out = kept;
        }
        out
    }
}

/// Runs of Greek or Hebrew words whose transliterations are `words`.
fn transliterated_matches(text: &str, words: &[String]) -> Vec<Range<usize>> {
    let tokens: Vec<(Range<usize>, Option<String>)> = tokenize(text)
        .into_iter()
        .map(|t| {
            let word = &text[t.range.clone()];
            let key = word
                .chars()
                .any(is_original_script)
                .then(|| transliteration_key(word));
            (t.range, key)
        })
        .collect();
    let n = words.len();
    let mut out = Vec::new();
    if tokens.len() < n {
        return out;
    }
    for i in 0..=tokens.len() - n {
        let all = (0..n).all(|k| tokens[i + k].1.as_ref() == Some(&words[k]));
        if all {
            out.push(tokens[i].0.start..tokens[i + n - 1].0.end);
        }
    }
    out
}

/// Whether `range` of `text` is not part of a longer word on either side.
fn whole_word(text: &str, range: &Range<usize>) -> bool {
    let before = text[..range.start].chars().next_back();
    let after = text[range.end..].chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}
