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
    decompose_canonical(c, |d| {
        if !is_combining_mark(d) {
            for l in d.to_lowercase() {
                out.push(fold(l));
            }
        }
    });
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
    fn separators() {
        assert_eq!(words("LORD’s house"), ["lord", "s", "house"]);
        assert_eq!(words("well-pleased; (yea)"), ["well", "pleased", "yea"]);
        assert_eq!(words("Jn 3:16"), ["jn", "3", "16"]);
        assert_eq!(words("  "), Vec::<String>::new());
        assert_eq!(words("ὁ λόγος."), ["ο", "λογοσ"]);
    }
}
