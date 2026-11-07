//! The built-in Strong's index (#243): every Strong's number, extended ones included, with its
//! lemma in Hebrew or Greek script and a short gloss, shipped inside the app so a Bible's
//! Interlinear lines read with no Lexicon installed. Lexicons in the library come first
//! ([`crate::interlinear::cell_texts`]); this is the fallback.
//!
//! `assets/strongs/index.tsv` is written by `cargo xtask strongs-index` from STEPBible's
//! TBESH and TBESG (CC BY 4.0, credited in Help → Credits): one row per disambiguated number,
//! `number <TAB> lemma <TAB> gloss`, `#` lines are comments. A gloss of the form
//! "first: beginning" names the word, then the sense this number is for.

use std::collections::HashMap;
use std::sync::OnceLock;

use obs_scripture::StrongsNumber;

/// The built-in index file.
const INDEX: &str = include_str!("../assets/strongs/index.tsv");

/// One number's row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Row<'a> {
    lemma: &'a str,
    gloss: &'a str,
}

impl Row<'_> {
    /// The sense of a "word: sense" gloss, or the whole gloss.
    fn sense(&self) -> &str {
        self.gloss
            .split_once(": ")
            .map_or(self.gloss, |(_, sense)| sense)
            .trim()
    }

    /// The word of a "word: sense" gloss, or the whole gloss.
    fn word(&self) -> &str {
        self.gloss
            .split_once(": ")
            .map_or(self.gloss, |(word, _)| word)
            .trim()
    }
}

/// What the index gives a word: its lemma and a gloss for an interlinear line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexWord {
    /// In Hebrew or Greek script, without accents; may be empty.
    pub lemma: String,
    /// May be empty.
    pub gloss: String,
}

/// Strong's numbers with their lemma and gloss.
#[derive(Debug, Default)]
pub struct StrongsIndex<'a> {
    rows: Vec<(StrongsNumber, Row<'a>)>,
    /// Row of each number as listed.
    exact: HashMap<StrongsNumber, usize>,
    /// Rows of each base number (its own and its extended ones), in file order.
    by_base: HashMap<StrongsNumber, Vec<usize>>,
}

/// The index built into the app, read the first time it is asked for.
pub fn builtin() -> &'static StrongsIndex<'static> {
    static INDEX_ONCE: OnceLock<StrongsIndex<'static>> = OnceLock::new();
    INDEX_ONCE.get_or_init(|| StrongsIndex::parse(INDEX))
}

impl<'a> StrongsIndex<'a> {
    /// Read an index file; rows that do not parse are skipped.
    pub fn parse(text: &'a str) -> Self {
        let mut index = Self::default();
        for line in text.lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut fields = line.split('\t');
            let (Some(number), Some(lemma), Some(gloss)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let Ok(number) = number.parse::<StrongsNumber>() else {
                continue;
            };
            let i = index.rows.len();
            index.rows.push((number, Row { lemma, gloss }));
            index.exact.entry(number).or_insert(i);
            index.by_base.entry(number.base()).or_default().push(i);
        }
        index
    }

    /// How many numbers it lists.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The lemma and gloss of `number`, under a word written `written` (the English a
    /// reverse interlinear translates it with, which picks the sense).
    ///
    /// An extended number listed (`H1254A`, `H7225G`) gives its own lemma and sense. A base
    /// number, or an extended one not listed, gives its first row's lemma; its gloss is the
    /// sense one of its rows names when the written word uses it ("beginning" under "In the
    /// beginning" for H7225, whose rows are "first: beginning" and "first: best"), else the
    /// word their glosses share ("first"), else the first row's gloss.
    pub fn word(&self, number: StrongsNumber, written: &str) -> Option<IndexWord> {
        if number.is_extended()
            && let Some(&i) = self.exact.get(&number)
        {
            let row = &self.rows[i].1;
            return Some(IndexWord {
                lemma: row.lemma.to_string(),
                gloss: row.sense().to_string(),
            });
        }
        let rows: Vec<&Row<'a>> = self
            .by_base
            .get(&number.base())?
            .iter()
            .map(|&i| &self.rows[i].1)
            .collect();
        let first = rows.first()?;
        let lemma = rows
            .iter()
            .map(|r| r.lemma)
            .find(|l| !l.is_empty())
            .unwrap_or_default();
        let gloss = match rows.as_slice() {
            [only] => only.sense(),
            _ => rows
                .iter()
                .map(|r| r.sense())
                .find(|sense| uses_sense(written, sense))
                .unwrap_or_else(|| first.word()),
        };
        Some(IndexWord {
            lemma: lemma.to_string(),
            gloss: gloss.to_string(),
        })
    }
}

/// Words too common to tell senses apart.
const COMMON_WORDS: &[&str] = &["the", "and", "for", "with", "from", "that", "this", "one"];

/// Whether the English `written` uses `sense` ("beginning" in "In the beginning", "heaven"
/// in "the heavens"): a word of three letters or more of either is the other's, or begins
/// it when both have four or more.
fn uses_sense(written: &str, sense: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphabetic())
            .map(str::to_lowercase)
            .filter(|w| w.chars().count() >= 3 && !COMMON_WORDS.contains(&w.as_str()))
            .collect()
    };
    let written = words(written);
    words(sense).iter().any(|s| {
        written.iter().any(|w| {
            w == s
                || (s.len() >= 4 && w.len() >= 4 && (w.starts_with(s.as_str()) || s.starts_with(w.as_str())))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(s: &str) -> StrongsNumber {
        s.parse().unwrap()
    }

    fn word(number: &str, written: &str) -> (String, String) {
        let w = builtin().word(n(number), written).unwrap();
        (w.lemma, w.gloss)
    }

    #[test]
    fn the_builtin_index_lists_hebrew_and_greek() {
        let index = builtin();
        assert!(index.len() > 20_000, "{}", index.len());
        assert!(index.exact.contains_key(&n("H1254A")));
        assert!(index.exact.contains_key(&n("G26")));
    }

    /// #243 acceptance: extended numbers resolve to their own lemma and gloss.
    #[test]
    fn extended_numbers_resolve() {
        assert_eq!(word("H1254A", "created"), ("בָּרָא".into(), "to create".into()));
        assert_eq!(word("H1254B", "fat"), ("בָּרָא".into(), "to fatten".into()));
        assert_eq!(word("H7225H", "best"), ("רֵאשִׁית".into(), "best".into()));
        assert_eq!(word("H430G", "God"), ("אֱלֹהִים".into(), "God".into()));
        // Lowercase and zero-padded as some Bibles write them.
        assert_eq!(word("h1254a", "x"), word("H1254A", "x"));
        assert_eq!(word("H01254A", "x"), word("H1254A", "x"));
    }

    #[test]
    fn a_base_number_takes_the_sense_its_translation_uses() {
        // #243 acceptance: Gen 1:1 "In the beginning" (H7225).
        assert_eq!(word("H7225", "In the beginning"), ("רֵאשִׁית".into(), "beginning".into()));
        // No sense used: the word the senses share.
        assert_eq!(word("H776", "the earth").1, "land");
        assert_eq!(word("H157", "loved").1, "to love");
        // One row: its gloss.
        assert_eq!(word("H1254", "created"), ("בָּרָא".into(), "to create".into()));
        assert_eq!(word("H8064", "the heavens").1, "heaven");
        assert_eq!(word("G26", "love"), ("ἀγάπη".into(), "love".into()));
        assert_eq!(word("G3056", "the Word"), ("λόγος".into(), "word".into()));
        // An extended number the index does not list reads as its base number.
        assert_eq!(word("H8064Z", "heavens").1, "heaven");
    }

    #[test]
    fn unknown_numbers_have_nothing() {
        assert_eq!(builtin().word(n("H99999"), "x"), None);
    }

    #[test]
    fn a_small_index_parses_and_skips_bad_rows() {
        let index = StrongsIndex::parse(
            "# comment\nH1G\tאָב\tfather\nnot a row\nX9\ta\tb\nH1H\tאָב\t(Huram)-abi\n",
        );
        assert_eq!(index.len(), 2);
        assert_eq!(index.word(n("H1"), "his father").unwrap().gloss, "father");
        assert_eq!(index.word(n("H1H"), "x").unwrap().gloss, "(Huram)-abi");
    }
}
