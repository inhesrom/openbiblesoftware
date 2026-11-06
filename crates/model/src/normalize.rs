//! The shared text normalizer and tokenizer, used at index time and at query time (#16 rule 4).
//!
//! NFD → drop combining marks (Greek accents and breathings, Hebrew niqqud and cantillation,
//! Latin diacritics) → lowercase → fold final forms (ς→σ, ך→כ, ם→מ, ן→נ, ף→פ, ץ→צ) → unify
//! curly quotes and apostrophes → NFC. Search is therefore case- and diacritic-insensitive.
//!
//! Words are runs of letters and digits (with their marks). Everything else separates words,
//! including apostrophes, hyphens, Hebrew maqaf (U+05BE) and sof pasuq (U+05C3).

use std::ops::Range;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::{decompose_canonical, is_combining_mark};

/// Bump when normalization or tokenization output changes; Search indexes rebuild.
pub const NORMALIZER_VERSION: u32 = 1;

fn fold(c: char) -> char {
    match c {
        'ς' => 'σ',
        'ך' => 'כ',
        'ם' => 'מ',
        'ן' => 'נ',
        'ף' => 'פ',
        'ץ' => 'צ',
        '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' | '\u{2032}' | '\u{02BC}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{2033}' => '"',
        other => other,
    }
}

/// Append the normalized (pre-NFC) form of one character to `out`.
fn push_folded(c: char, out: &mut String) {
    push_find_folded(c, false, false, out);
}

/// Append one character as Find in Resource compares it (#175): the Search normalizer's
/// steps (NFD, final forms and quotes folded), keeping combining marks only with
/// `keep_marks` (Ignore diacritics off) and case only with `keep_case` (Match case on). With
/// both off this is exactly what [`normalize`] does before its final NFC.
fn push_find_folded(c: char, keep_case: bool, keep_marks: bool, out: &mut String) {
    decompose_canonical(c, |d| {
        if is_combining_mark(d) {
            if keep_marks {
                out.push(d);
            }
        } else if keep_case {
            out.push(fold(d));
        } else {
            for l in d.to_lowercase() {
                out.push(fold(l));
            }
        }
    });
}

/// Text folded for Find in Resource (#175), with where each of its clusters came from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindFolded {
    /// The folded text.
    pub text: String,
    /// One entry per cluster of the original (a character with the combining marks after
    /// it): where its folding starts in `text` and where it starts in the original, both in
    /// bytes, in order. A last entry holds both lengths. A match counts only from one
    /// cluster's start to another's, so it never takes half of a letter with its marks.
    pub clusters: Vec<(u32, u32)>,
}

impl FindFolded {
    /// The original byte range of the folded range `start..end`, if both ends are cluster
    /// starts. A cluster that folds to nothing is left out at both ends.
    pub fn original(&self, start: usize, end: usize) -> Option<std::ops::Range<usize>> {
        let (start, end) = (u32::try_from(start).ok()?, u32::try_from(end).ok()?);
        // The last cluster starting at `start`, the first starting at `end`.
        let s = self.clusters.partition_point(|c| c.0 <= start).checked_sub(1)?;
        let e = self.clusters.partition_point(|c| c.0 < end);
        let (fs, os) = *self.clusters.get(s)?;
        let (fe, oe) = *self.clusters.get(e)?;
        (fs == start && fe == end).then_some(os as usize..oe as usize)
    }
}

/// Fold `text` as Find in Resource compares it (see `push_find_folded`): the Search
/// normalizer, keeping case with `keep_case` and combining marks with `keep_marks`. Kept
/// marks are put in canonical order within their cluster, so typed and stored text compare
/// alike whatever order their marks were written in.
pub fn fold_for_find(text: &str, keep_case: bool, keep_marks: bool) -> FindFolded {
    let mut out = FindFolded {
        text: String::with_capacity(text.len()),
        clusters: Vec::with_capacity(text.len() / 2),
    };
    let offset = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        out.clusters.push((offset(out.text.len()), offset(i)));
        let mut end = i + c.len_utf8();
        while let Some(&(j, m)) = chars.peek() {
            if !is_combining_mark(m) {
                break;
            }
            end = j + m.len_utf8();
            chars.next();
        }
        if c.is_ascii() && end == i + 1 {
            out.text
                .push(if keep_case { c } else { c.to_ascii_lowercase() });
        } else if keep_marks && end > i + c.len_utf8() {
            for d in text[i..end].nfd() {
                push_find_folded(d, keep_case, true, &mut out.text);
            }
        } else {
            for d in text[i..end].chars() {
                push_find_folded(d, keep_case, keep_marks, &mut out.text);
            }
        }
    }
    out.clusters
        .push((offset(out.text.len()), offset(text.len())));
    out
}

/// Normalize text for search: see the module docs.
pub fn normalize(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for c in text.chars() {
        push_folded(c, &mut folded);
    }
    folded.nfc().collect()
}

/// One word found by [`tokenize`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// The normalized word.
    pub text: String,
    /// Byte range of the word in the original (un-normalized) text.
    pub range: Range<usize>,
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() && c != '\u{02BC}'
}

/// Split `text` into normalized words, each with its byte range in `text`, so hits can be
/// highlighted on the display text.
pub fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut current: Option<(usize, String)> = None;
    for (i, c) in text.char_indices() {
        let joins = is_word_char(c) || (current.is_some() && is_combining_mark(c));
        if joins {
            let (_, word) = current.get_or_insert_with(|| (i, String::new()));
            push_folded(c, word);
        } else if let Some((start, word)) = current.take() {
            tokens.push(Token {
                text: word.nfc().collect(),
                range: start..i,
            });
        }
    }
    if let Some((start, word)) = current {
        tokens.push(Token {
            text: word.nfc().collect(),
            range: start..text.len(),
        });
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        tokenize(text).into_iter().map(|t| t.text).collect()
    }

    #[test]
    fn greek_accents_breathings_and_final_sigma() {
        assert_eq!(normalize("Ἐν ἀρχῇ ἦν ὁ λόγος"), "εν αρχη ην ο λογοσ");
        assert_eq!(normalize("ΛΌΓΟΣ"), "λογοσ");
        assert_eq!(normalize("ῥῆμα"), "ρημα");
        assert_eq!(normalize("Ἰησοῦς"), "ιησουσ");
        assert_eq!(normalize("ἀγάπη"), normalize("αγαπη"));
    }

    #[test]
    fn hebrew_niqqud_cantillation_and_final_forms() {
        // Gen 1:1 with full pointing and cantillation (WLC/TAHOT style).
        assert_eq!(normalize("בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים"), "בראשית ברא אלהימ");
        assert_eq!(normalize("אֶרֶץ"), "ארצ");
        assert_eq!(normalize("מֶלֶךְ"), "מלכ");
        assert_eq!(normalize("אָמֵן"), "אמנ");
        assert_eq!(normalize("כַּף"), "כפ");
    }

    #[test]
    fn latin_diacritics_case_and_quotes() {
        assert_eq!(normalize("Naïve CAFÉ façade"), "naive cafe facade");
        assert_eq!(normalize("the LORD’s “word”"), "the lord's \"word\"");
        assert_eq!(normalize("‘thou’"), "'thou'");
    }

    #[test]
    fn idempotent() {
        for s in ["Ἐν ἀρχῇ", "בְּרֵאשִׁ֖ית", "Naïve “x”", "ﬁne", "한국어"]
        {
            let once = normalize(s);
            assert_eq!(normalize(&once), once, "{s}");
        }
    }

    #[test]
    fn tokens_keep_original_byte_ranges() {
        let text = "In the beginning God created";
        let tokens = tokenize(text);
        assert_eq!(tokens.len(), 5);
        assert_eq!(&text[tokens[3].range.clone()], "God");
        assert_eq!(tokens[3].text, "god");

        let heb = "עַל־פְּנֵ֥י הַמָּֽיִם׃ וַיֹּ֥אמֶר";
        let tokens = tokenize(heb);
        assert_eq!(
            tokens.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            ["על", "פני", "המימ", "ויאמר"]
        );
        for t in &tokens {
            assert_eq!(normalize(&heb[t.range.clone()]), t.text);
        }
        assert_eq!(&heb[tokens[0].range.clone()], "עַל");
    }

    #[test]
    fn find_folding_keeps_what_its_options_keep() {
        // Defaults: exactly the Search normalizer.
        for s in ["Ἐν ἀρχῇ ἦν ὁ λόγος", "בְּרֵאשִׁ֖ית בָּרָ֣א", "Naïve CAFÉ “x”"] {
            assert_eq!(fold_for_find(s, false, false).text, normalize(s).nfd().collect::<String>());
        }
        assert_eq!(fold_for_find("Λόγος", true, false).text, "Λογοσ");
        let kept = fold_for_find("ἀγάπη", false, true).text;
        assert_eq!(kept, "ἀγάπη".nfd().collect::<String>());
        // Marks typed in another order compare alike: dagesh and qamats on bet.
        assert_eq!(
            fold_for_find("\u{05D1}\u{05B8}\u{05BC}", false, true).text,
            fold_for_find("\u{05D1}\u{05BC}\u{05B8}", false, true).text
        );
    }

    #[test]
    fn find_folding_maps_clusters_back_to_the_original() {
        let text = "Ἐν ἀρχῇ ἦν";
        let f = fold_for_find(text, false, false);
        assert_eq!(f.text, "εν αρχη ην");
        let at = f.text.find("αρχη").unwrap();
        let r = f.original(at, at + "αρχη".len()).unwrap();
        assert_eq!(&text[r], "ἀρχῇ");
        // Half a cluster is no match: with marks kept, "ε" ends before ἐ's breathing.
        let kept = fold_for_find("ἐν", false, true);
        let e = "ε".len();
        assert_eq!(kept.original(0, e), None);
        let heb = "בְּרֵאשִׁ֖ית";
        let f = fold_for_find(heb, false, false);
        assert_eq!(f.text, "בראשית");
        assert_eq!(f.original(0, f.text.len()), Some(0..heb.len()));
    }

    #[test]
    fn separators() {
        assert_eq!(words("LORD’s house"), ["lord", "s", "house"]);
        assert_eq!(words("well-pleased; (yea)"), ["well", "pleased", "yea"]);
        assert_eq!(words("Jn 3:16"), ["jn", "3", "16"]);
        assert_eq!(words("  "), Vec::<String>::new());
        assert_eq!(words("ὁ λόγος."), ["ο", "λογοσ"]);
    }
}
