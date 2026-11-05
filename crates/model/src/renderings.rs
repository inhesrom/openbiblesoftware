//! The English rendering → Strong's index of a Strong's-tagged Bible (#20 rule 5, #15).
//!
//! Built by [`crate::ResourceBuilder`] from a Bible's `tagged_word` spans, so a Bible brought
//! in by Import gets it as well as the Starter KJV. It is derived data, outside the content
//! hash, and lives in the Bible's own `.obr`.
//!
//! - **Renderings**: for each Tagged word whose text is in a translation's own script (not
//!   Hebrew or Greek), its words normalized ([`crate::normalize`]) and joined with single
//!   spaces (`the world`), counted per Strong's number of each of its parts. The Word Study
//!   box's English-word chooser reads it ("love → G25 ×…, G26 ×…"), and Translated as (#56)
//!   reads it the other way round.
//! - **Totals**: how many Tagged-word parts carry each Strong's number, and the part of
//!   speech its Morphology most often gives (Bibles with readable codes only: TAHOT, TAGNT).

use std::collections::BTreeMap;

use obs_scripture::StrongsNumber;

use crate::morph::{self, PartOfSpeech};
use crate::normalize::tokenize;
use crate::unit::{SpanKind, TextUnit};

/// One rendering of a Strong's number in a Bible, with how many Tagged words have it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rendering {
    /// Normalized words of the Tagged word's text, joined with single spaces.
    pub rendering: String,
    pub strongs: StrongsNumber,
    pub count: u32,
}

/// How often a Strong's number occurs in a Bible's Tagged words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StrongsTotal {
    pub strongs: StrongsNumber,
    pub count: u32,
    /// The part of speech most of its occurrences are parsed as, when the Bible's Morphology
    /// codes are ones [`morph::parse`] reads.
    pub part_of_speech: Option<PartOfSpeech>,
}

/// A Bible's rendering index: renderings sorted by (rendering, number), totals by number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenderingIndex {
    pub renderings: Vec<Rendering>,
    pub totals: Vec<StrongsTotal>,
}

/// Every part of speech, in the order ties are broken.
const PARTS_OF_SPEECH: [PartOfSpeech; 12] = [
    PartOfSpeech::Noun,
    PartOfSpeech::Verb,
    PartOfSpeech::Adjective,
    PartOfSpeech::Adverb,
    PartOfSpeech::Pronoun,
    PartOfSpeech::Preposition,
    PartOfSpeech::Conjunction,
    PartOfSpeech::Particle,
    PartOfSpeech::Interjection,
    PartOfSpeech::Article,
    PartOfSpeech::Foreign,
    PartOfSpeech::Suffix,
];

impl PartOfSpeech {
    /// Stable id, as stored in a `.obr` (the same text as [`PartOfSpeech::name`]).
    pub fn from_name(name: &str) -> Option<Self> {
        PARTS_OF_SPEECH.into_iter().find(|p| p.name() == name)
    }
}

/// Whether `c` is a Hebrew or Greek letter: words in those scripts are the original text,
/// not a rendering of it.
fn is_original_script(c: char) -> bool {
    matches!(
        c as u32,
        0x0370..=0x03FF | 0x1F00..=0x1FFF | 0x0590..=0x05FF | 0xFB1D..=0xFB4F
    )
}

/// `text` normalized into a rendering: its words joined with single spaces. `None` when it
/// has no word, or is Hebrew or Greek.
pub fn rendering_key(text: &str) -> Option<String> {
    let words: Vec<String> = tokenize(text).into_iter().map(|t| t.text).collect();
    let phrase = words.join(" ");
    let english =
        phrase.chars().any(char::is_alphabetic) && !phrase.chars().any(is_original_script);
    english.then_some(phrase)
}

impl RenderingIndex {
    /// Build the index from a Bible's units.
    pub fn build(units: &[TextUnit]) -> Self {
        let mut renderings: BTreeMap<(String, StrongsNumber), u32> = BTreeMap::new();
        let mut totals: BTreeMap<StrongsNumber, (u32, [u32; PARTS_OF_SPEECH.len()])> =
            BTreeMap::new();
        for unit in units {
            for span in &unit.spans {
                let SpanKind::TaggedWord { parts } = &span.kind else {
                    continue;
                };
                let rendering = unit.text.get(span.range()).and_then(rendering_key);
                for part in parts {
                    let (count, pos) = totals.entry(part.strongs).or_default();
                    *count += 1;
                    let parsed = part
                        .morph
                        .as_deref()
                        .and_then(|code| morph::parse(code.trim()).ok())
                        .and_then(|m| m.pos());
                    if let Some(i) =
                        parsed.and_then(|p| PARTS_OF_SPEECH.iter().position(|q| *q == p))
                    {
                        pos[i] += 1;
                    }
                    if let Some(r) = &rendering {
                        *renderings.entry((r.clone(), part.strongs)).or_default() += 1;
                    }
                }
            }
        }
        Self {
            renderings: renderings
                .into_iter()
                .map(|((rendering, strongs), count)| Rendering {
                    rendering,
                    strongs,
                    count,
                })
                .collect(),
            totals: totals
                .into_iter()
                .map(|(strongs, (count, pos))| StrongsTotal {
                    strongs,
                    count,
                    part_of_speech: most_common(&pos),
                })
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.renderings.is_empty() && self.totals.is_empty()
    }

    /// The Strong's numbers rendered with `words` (one word or a phrase, matched as whole
    /// normalized words anywhere in a rendering), each with how many Tagged words render it
    /// so: most frequent first, then by number.
    pub fn numbers_for(&self, words: &str) -> Vec<(StrongsNumber, u32)> {
        let wanted: Vec<String> = tokenize(words).into_iter().map(|t| t.text).collect();
        if wanted.is_empty() {
            return Vec::new();
        }
        let mut sums: BTreeMap<StrongsNumber, u32> = BTreeMap::new();
        for r in &self.renderings {
            let have: Vec<&str> = r.rendering.split(' ').collect();
            if have
                .windows(wanted.len())
                .any(|w| w.iter().zip(&wanted).all(|(a, b)| *a == b))
            {
                *sums.entry(r.strongs).or_default() += r.count;
            }
        }
        let mut out: Vec<(StrongsNumber, u32)> = sums.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    /// How often `number` occurs: an extended number counts only itself; a base number
    /// counts itself and all of its homographs ([`StrongsNumber::matches`]). The part of
    /// speech is that of the most frequent matching number that has one.
    pub fn total(&self, number: StrongsNumber) -> Option<StrongsTotal> {
        let matching: Vec<&StrongsTotal> = self
            .totals
            .iter()
            .filter(|t| number.matches(t.strongs))
            .collect();
        if matching.is_empty() {
            return None;
        }
        let part_of_speech = matching
            .iter()
            .filter(|t| t.part_of_speech.is_some())
            .max_by(|a, b| a.count.cmp(&b.count).then(b.strongs.cmp(&a.strongs)))
            .and_then(|t| t.part_of_speech);
        Some(StrongsTotal {
            strongs: number,
            count: matching.iter().map(|t| t.count).sum(),
            part_of_speech,
        })
    }

    /// How `number` is rendered (matched as in [`RenderingIndex::total`]): each rendering
    /// with its count, most frequent first, then alphabetically.
    pub fn renderings_of(&self, number: StrongsNumber) -> Vec<(String, u32)> {
        let mut sums: BTreeMap<&str, u32> = BTreeMap::new();
        for r in self.renderings.iter().filter(|r| number.matches(r.strongs)) {
            *sums.entry(r.rendering.as_str()).or_default() += r.count;
        }
        let mut out: Vec<(String, u32)> =
            sums.into_iter().map(|(r, n)| (r.to_string(), n)).collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }
}

fn most_common(counts: &[u32; PARTS_OF_SPEECH.len()]) -> Option<PartOfSpeech> {
    let (i, n) = counts
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(&a.0)))?;
    (*n > 0).then(|| PARTS_OF_SPEECH[i])
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::unit::{MorphemePart, Span, UnitId, UnitKind};

    fn s(n: &str) -> StrongsNumber {
        n.parse().unwrap()
    }

    /// A Tagged word: its text and its parts (Strong's number, Morphology).
    type Word<'a> = (&'a str, &'a [(&'a str, Option<&'a str>)]);

    fn unit(words: &[Word<'_>]) -> TextUnit {
        let mut text = String::new();
        let mut spans = Vec::new();
        for (word, parts) in words {
            if !text.is_empty() {
                text.push(' ');
            }
            let start = text.len() as u32;
            text.push_str(word);
            spans.push(Span {
                start,
                end: text.len() as u32,
                kind: SpanKind::TaggedWord {
                    parts: parts
                        .iter()
                        .map(|(n, morph)| {
                            let mut p = MorphemePart::new(s(n));
                            p.morph = morph.map(String::from);
                            p
                        })
                        .collect(),
                },
            });
        }
        TextUnit {
            id: UnitId::new("JHN").unwrap(),
            kind: UnitKind::BibleBook,
            title: None,
            text,
            spans,
        }
    }

    #[test]
    fn english_renderings_are_counted_per_number() {
        let u = unit(&[
            ("love", &[("G25", None)]),
            ("Love", &[("G26", None)]),
            ("loved", &[("G25", None)]),
            ("the love", &[("G26", None)]),
            ("love", &[("G5368", None)]),
            ("love", &[("G25", None)]),
        ]);
        let index = RenderingIndex::build(&[u]);
        assert_eq!(
            index.numbers_for("love"),
            [(s("G25"), 2), (s("G26"), 2), (s("G5368"), 1)]
        );
        assert_eq!(index.numbers_for("LOVED"), [(s("G25"), 1)]);
        assert_eq!(index.numbers_for("the love"), [(s("G26"), 1)]);
        assert!(index.numbers_for("charity").is_empty());
        assert!(index.numbers_for("  ").is_empty());
        assert_eq!(
            index.renderings_of(s("G26")),
            [("love".to_string(), 1), ("the love".to_string(), 1)]
        );
        assert_eq!(index.total(s("G25")).unwrap().count, 3);
    }

    #[test]
    fn original_script_is_counted_but_not_a_rendering() {
        let u = unit(&[
            ("בָּרָא", &[("H1254A", Some("HVqp3ms"))]),
            (
                "וַיִּבְרָא",
                &[("H9001", Some("HC")), ("H1254A", Some("HVqw3ms"))],
            ),
            ("בְּרִיא", &[("H1254B", Some("HAamsa"))]),
            ("ἀγάπη", &[("G26", Some("N-NSF"))]),
        ]);
        let index = RenderingIndex::build(&[u]);
        assert!(index.renderings.is_empty());
        let bara = index.total(s("H1254A")).unwrap();
        assert_eq!(bara.count, 2);
        assert_eq!(bara.part_of_speech, Some(PartOfSpeech::Verb));
        // A base number counts every homograph; its part of speech is the commonest one's.
        let base = index.total(s("H1254")).unwrap();
        assert_eq!(base.count, 3);
        assert_eq!(base.part_of_speech, Some(PartOfSpeech::Verb));
        assert_eq!(index.total(s("H1254B")).unwrap().count, 1);
        assert_eq!(index.total(s("H1254C")), None);
        assert_eq!(
            index.total(s("G26")).unwrap().part_of_speech,
            Some(PartOfSpeech::Noun)
        );
    }

    #[test]
    fn unreadable_morphology_leaves_no_part_of_speech() {
        let u = unit(&[("created", &[("H1254", Some("strongMorph:TH8804"))])]);
        let index = RenderingIndex::build(&[u]);
        assert_eq!(index.total(s("H1254")).unwrap().part_of_speech, None);
        assert_eq!(
            index.renderings_of(s("H1254")),
            [("created".to_string(), 1)]
        );
    }

    #[test]
    fn part_of_speech_names_round_trip() {
        for p in PARTS_OF_SPEECH {
            assert_eq!(PartOfSpeech::from_name(p.name()), Some(p));
        }
        assert_eq!(PartOfSpeech::from_name("gerund"), None);
    }
}
