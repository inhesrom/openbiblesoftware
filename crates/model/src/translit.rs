//! One shared ASCII transliteration for Greek and Hebrew, used for Lexicon Key indexes and the
//! `lemma:` search operator (`lemma:agape`, `lemma:chesed`).
//!
//! The scheme favours what a lay user would type, not scholarly precision: lowercase ASCII,
//! no diacritics, aleph and ayin omitted.
//!
//! - **Greek:** α a, β b, γ g (n before γ κ ξ χ), δ d, ε e, ζ z, η e, θ th, ι i, κ k, λ l, μ m,
//!   ν n, ξ x, ο o, π p, ρ r (rh with rough breathing), σ/ς s, τ t, υ y (u after a vowel),
//!   φ ph, χ ch, ψ ps, ω o; a rough breathing on the first vowel or diphthong adds `h`.
//! - **Hebrew:** consonants א – , ב b/v, ג g, ד d, ה h, ו v, ז z, ח ch, ט t, י y, כ k/kh, ל l,
//!   מ m, נ n, ס s, ע –, פ p/f, צ ts, ק q, ר r, שׁ sh, שׂ s, ת t (b/k/p with dagesh, else v/kh/f);
//!   vowels patah/qamats/hataf-patah a, segol/tsere/hataf-segol e, hiriq i, holam/hataf-qamats o,
//!   qubuts u, sheva silent; holam-vav o, shureq u; yod after i/e is silent; furtive patah
//!   is read before its final consonant (ruach).
//! - Anything else goes through [`crate::normalize`].

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::key::{Key, KeyEntry};
use crate::normalize::{normalize, tokenize};

/// Bump when output changes; indexes built from it rebuild.
pub const TRANSLITERATION_VERSION: u32 = 1;

/// One row of a Resource's transliteration index (#20 rule 5): a Lemma Key and its
/// transliteration in index form (see [`transliteration_key`]).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LemmaTransliteration {
    pub transliteration: String,
    /// The Lemma Key, in original script.
    pub lemma: String,
}

/// A Lemma's transliteration as the index stores it: [`transliterate`], split into words by
/// [`tokenize`] and joined with single spaces. The `lemma:` search operator reduces what the
/// user types the same way, so `lemma:agape` finds ἀγάπη.
pub fn transliteration_key(lemma: &str) -> String {
    let words: Vec<String> = tokenize(&transliterate(lemma))
        .into_iter()
        .map(|t| t.text)
        .collect();
    words.join(" ")
}

/// The transliteration index of a Resource: one row per distinct Lemma Key, sorted by
/// transliteration. Lemmas that transliterate to nothing are left out.
pub fn transliteration_index<'a>(
    keys: impl IntoIterator<Item = &'a KeyEntry>,
) -> Vec<LemmaTransliteration> {
    let mut rows: Vec<LemmaTransliteration> = keys
        .into_iter()
        .filter_map(|entry| match &entry.key {
            Key::Lemma(lemma) => Some(LemmaTransliteration {
                transliteration: transliteration_key(lemma),
                lemma: lemma.clone(),
            }),
            _ => None,
        })
        .filter(|row| !row.transliteration.is_empty())
        .collect();
    rows.sort();
    rows.dedup();
    rows
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    Greek,
    Hebrew,
    Other,
}

fn script(c: char) -> Script {
    match c as u32 {
        0x0370..=0x03FF | 0x1F00..=0x1FFF => Script::Greek,
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => Script::Hebrew,
        _ => Script::Other,
    }
}

/// A base character with the combining marks that follow it.
struct Cluster {
    base: char,
    marks: Vec<char>,
}

impl Cluster {
    fn has(&self, mark: char) -> bool {
        self.marks.contains(&mark)
    }
}

/// Transliterate Greek and Hebrew to lowercase ASCII; other text is normalized.
pub fn transliterate(text: &str) -> String {
    let mut out = String::new();
    // Split the NFD text into runs of one script; marks stay with their base.
    let mut run: Vec<Cluster> = Vec::new();
    let mut run_script = Script::Other;
    let flush = |run: &mut Vec<Cluster>, s: Script, out: &mut String| {
        if run.is_empty() {
            return;
        }
        match s {
            Script::Greek => greek_word(run, out),
            Script::Hebrew => hebrew_word(run, out),
            Script::Other => {
                let raw: String = run
                    .iter()
                    .flat_map(|c| std::iter::once(c.base).chain(c.marks.iter().copied()))
                    .collect();
                out.push_str(&normalize(&raw));
            }
        }
        run.clear();
    };
    for c in text.nfd() {
        if is_combining_mark(c) {
            if let Some(last) = run.last_mut() {
                last.marks.push(c);
            }
            continue;
        }
        // Non-letters (spaces, maqaf, sof pasuq, punctuation) end a word and pass through.
        let s = if c.is_alphabetic() {
            script(c)
        } else {
            Script::Other
        };
        if s == Script::Other {
            flush(&mut run, run_script, &mut out);
            out.push_str(&normalize(&c.to_string()));
            continue;
        }
        if s != run_script {
            flush(&mut run, run_script, &mut out);
            run_script = s;
        }
        run.push(Cluster {
            base: c,
            marks: Vec::new(),
        });
    }
    flush(&mut run, run_script, &mut out);
    out
}

const ROUGH: char = '\u{0314}';

fn is_greek_vowel(c: char) -> bool {
    matches!(c, 'α' | 'ε' | 'η' | 'ι' | 'ο' | 'υ' | 'ω')
}

fn greek_word(clusters: &[Cluster], out: &mut String) {
    let lower: Vec<char> = clusters
        .iter()
        .map(|c| c.base.to_lowercase().next().unwrap_or(c.base))
        .collect();
    let first_is_vowel = lower.first().is_some_and(|&c| is_greek_vowel(c));
    let rough_start = clusters.iter().take(2).any(|c| c.has(ROUGH));
    if first_is_vowel && rough_start {
        out.push('h');
    }
    for (i, &c) in lower.iter().enumerate() {
        let next = lower.get(i + 1).copied();
        let prev = i.checked_sub(1).and_then(|p| lower.get(p).copied());
        let s = match c {
            'α' => "a",
            'β' => "b",
            'γ' if matches!(next, Some('γ' | 'κ' | 'ξ' | 'χ')) => "n",
            'γ' => "g",
            'δ' => "d",
            'ε' => "e",
            'ζ' => "z",
            'η' => "e",
            'θ' => "th",
            'ι' => "i",
            'κ' => "k",
            'λ' => "l",
            'μ' => "m",
            'ν' => "n",
            'ξ' => "x",
            'ο' => "o",
            'π' => "p",
            'ρ' if i == 0 && clusters.first().is_some_and(|c| c.has(ROUGH)) => "rh",
            'ρ' => "r",
            'σ' | 'ς' | 'ϲ' => "s",
            'τ' => "t",
            'υ' if prev.is_some_and(|p| matches!(p, 'α' | 'ε' | 'η' | 'ο')) => "u",
            'υ' => "y",
            'φ' => "ph",
            'χ' => "ch",
            'ψ' => "ps",
            'ω' => "o",
            _ => "",
        };
        out.push_str(s);
    }
}

const DAGESH: char = '\u{05BC}';
const SHIN_DOT: char = '\u{05C1}';
const SIN_DOT: char = '\u{05C2}';
const PATAH: char = '\u{05B7}';

fn hebrew_vowel(cluster: &Cluster) -> Option<&'static str> {
    cluster.marks.iter().find_map(|m| match m {
        '\u{05B1}' | '\u{05B5}' | '\u{05B6}' => Some("e"),
        '\u{05B2}' | '\u{05B7}' | '\u{05B8}' => Some("a"),
        '\u{05B3}' | '\u{05B9}' | '\u{05BA}' | '\u{05C7}' => Some("o"),
        '\u{05B4}' => Some("i"),
        '\u{05BB}' => Some("u"),
        _ => None,
    })
}

fn hebrew_word(clusters: &[Cluster], out: &mut String) {
    let mut last_vowel: Option<&str> = None;
    let last = clusters.len().saturating_sub(1);
    for (i, cl) in clusters.iter().enumerate() {
        let vowel = hebrew_vowel(cl);
        let dagesh = cl.has(DAGESH);
        let holam_only = matches!(vowel, Some("o"))
            && cl
                .marks
                .iter()
                .any(|m| matches!(m, '\u{05B9}' | '\u{05BA}'));
        if cl.base == 'ו' {
            if holam_only {
                out.push('o');
                last_vowel = Some("o");
                continue;
            }
            if dagesh && vowel.is_none() && last_vowel.is_none() {
                out.push('u');
                last_vowel = Some("u");
                continue;
            }
        }
        if cl.base == 'י' && vowel.is_none() && !dagesh && matches!(last_vowel, Some("i" | "e")) {
            continue;
        }
        let consonant = match cl.base {
            'א' | 'ע' => "",
            'ב' => {
                if dagesh {
                    "b"
                } else {
                    "v"
                }
            }
            'ג' => "g",
            'ד' => "d",
            'ה' => "h",
            'ו' => "v",
            'ז' => "z",
            'ח' => "ch",
            'ט' => "t",
            'י' => "y",
            'כ' => {
                if dagesh {
                    "k"
                } else {
                    "kh"
                }
            }
            'ך' => "kh",
            'ל' => "l",
            'מ' | 'ם' => "m",
            'נ' | 'ן' => "n",
            'ס' => "s",
            'פ' => {
                if dagesh {
                    "p"
                } else {
                    "f"
                }
            }
            'ף' => "f",
            'צ' | 'ץ' => "ts",
            'ק' => "q",
            'ר' => "r",
            'ש' if cl.has(SIN_DOT) => "s",
            'ש' if cl.has(SHIN_DOT) => "sh",
            'ש' => "sh",
            'ת' => "t",
            _ => "",
        };
        let furtive = i == last && matches!(cl.base, 'ח' | 'ע' | 'ה') && cl.has(PATAH);
        if furtive {
            out.push('a');
            out.push_str(consonant);
            last_vowel = Some("a");
            continue;
        }
        out.push_str(consonant);
        if let Some(v) = vowel {
            out.push_str(v);
            last_vowel = Some(v);
        } else {
            last_vowel = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greek_lemmas() {
        for (greek, expected) in [
            ("ἀγάπη", "agape"),
            ("λόγος", "logos"),
            ("χάρις", "charis"),
            ("ἅγιος", "hagios"),
            ("ἄγγελος", "angelos"),
            ("Ἰησοῦς", "iesous"),
            ("ῥῆμα", "rhema"),
            ("εὐαγγέλιον", "euangelion"),
            ("ψυχή", "psyche"),
            ("Χριστός", "christos"),
            ("θεός", "theos"),
            ("αἷμα", "haima"),
            ("ὕμνος", "hymnos"),
        ] {
            assert_eq!(transliterate(greek), expected, "{greek}");
        }
    }

    #[test]
    fn hebrew_lemmas() {
        for (hebrew, expected) in [
            ("בָּרָא", "bara"),
            ("אֱלֹהִים", "elohim"),
            ("שָׁלוֹם", "shalom"),
            ("חֶסֶד", "chesed"),
            ("רוּחַ", "ruach"),
            ("תּוֹרָה", "torah"),
            ("אַבְרָהָם", "avraham"),
            ("נֶפֶשׁ", "nefesh"),
            ("מֶלֶךְ", "melekh"),
            ("דָּבָר", "davar"),
            ("שָׂרָה", "sarah"),
            ("בְּרִית", "brit"),
        ] {
            assert_eq!(transliterate(hebrew), expected, "{hebrew}");
        }
    }

    #[test]
    fn cantillation_is_ignored_and_words_split() {
        assert_eq!(transliterate("בָּרָ֣א אֱלֹהִ֑ים"), "bara elohim");
        assert_eq!(transliterate("ὁ λόγος"), "ho logos");
        assert_eq!(transliterate("Grace, χάρις"), "grace, charis");
    }

    #[test]
    fn index_rows_come_from_lemma_keys_only() {
        let entry = |key: Key| KeyEntry {
            key,
            unit: crate::UnitId::new("x").unwrap(),
            range: None,
            alias: false,
        };
        let keys = [
            entry(Key::Lemma("ἀγάπη".into())),
            entry(Key::Lemma("ἀγάπη".into())),
            entry(Key::Lemma("אֲבִי־עַלְבוֹן".into())),
            entry(Key::Lemma("···".into())),
            entry(Key::Headword("Love".into())),
            entry(Key::StrongsNumber("G26".parse().unwrap())),
        ];
        let rows = transliteration_index(&keys);
        let pairs: Vec<(&str, &str)> = rows
            .iter()
            .map(|r| (r.transliteration.as_str(), r.lemma.as_str()))
            .collect();
        assert_eq!(pairs, [("agape", "ἀγάπη"), ("avi alvon", "אֲבִי־עַלְבוֹן")]);
        assert_eq!(transliteration_key("Χάρις"), "charis");
    }

    #[test]
    fn output_is_ascii() {
        for s in ["Ἐν ἀρχῇ ἦν ὁ λόγος", "בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם"]
        {
            let t = transliterate(s);
            assert!(t.is_ascii(), "{t}");
        }
    }
}
