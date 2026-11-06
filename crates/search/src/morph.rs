//! Morphology criteria (#145): the grammar one Tagged word must have, in plain English.
//!
//! A query names them after `@` (or `morph:`): `@verb aorist imperative`, `strong:G26 @noun
//! genitive`. Each criterion is one value of one [`MorphCategory`] (part of speech, tense,
//! voice, mood, case, number, gender, person, stem, state); all of them must hold for the
//! same morpheme part ([`MorphCriteria::matches_code`]), read through the shared decoder
//! ([`obs_model::morph`]), so nobody has to know the codes. The Morphology chooser builds the
//! same criteria from menus ([`MorphCategory::values`], [`MorphCriteria::set`]) and writes
//! them as query text ([`MorphCriteria::query_text`]), so the chooser and typing give the
//! same query.

use obs_model::morph::{self, Case, Gender, Morph, MorphPart, Number, State};

/// One menu of the Morphology chooser: a kind of grammatical fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MorphCategory {
    PartOfSpeech,
    Tense,
    Voice,
    Mood,
    Case,
    Number,
    Gender,
    Person,
    /// Hebrew and Aramaic verb stem (Qal, Piel, Peal …).
    Stem,
    /// Hebrew and Aramaic noun state.
    State,
}

/// Hebrew stems first, then the Aramaic ones not already named, as STEPBible names them
/// (the names [`obs_model::morph::parse`] gives).
const STEMS: &[&str] = &[
    "Qal",
    "Niphal",
    "Piel",
    "Pual",
    "Hiphil",
    "Hophal",
    "Hithpael",
    "Qal passive",
    "Polel",
    "Polal",
    "Hithpolel",
    "Poel",
    "Poal",
    "Palel",
    "Pulal",
    "Pilpel",
    "Polpal",
    "Hithpalpel",
    "Pealal",
    "Pilel",
    "Hothpaal",
    "Nithpael",
    "Tiphil",
    "Hishtaphel",
    "Nithpalel",
    "Nithpoel",
    "Hithpoel",
    "Peal",
    "Peil",
    "Pael",
    "Aphel",
    "Haphel",
    "Shaphel",
    "Hitpeel",
    "Hitpael",
    "Hitpaal",
    "Ishtaphel",
    "Saphel",
    "Hithaphel",
    "Ithpoel",
    "Hephal",
    "Palpel",
    "Ithpalpel",
    "Ithpolel",
    "Ittaphal",
    "Tiphel",
];

impl MorphCategory {
    /// In the order the chooser shows them and query text names them.
    pub const ALL: [MorphCategory; 10] = [
        MorphCategory::PartOfSpeech,
        MorphCategory::Tense,
        MorphCategory::Voice,
        MorphCategory::Mood,
        MorphCategory::Case,
        MorphCategory::Number,
        MorphCategory::Gender,
        MorphCategory::Person,
        MorphCategory::Stem,
        MorphCategory::State,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MorphCategory::PartOfSpeech => "Part of speech",
            MorphCategory::Tense => "Tense",
            MorphCategory::Voice => "Voice",
            MorphCategory::Mood => "Mood",
            MorphCategory::Case => "Case",
            MorphCategory::Number => "Number",
            MorphCategory::Gender => "Gender",
            MorphCategory::Person => "Person",
            MorphCategory::Stem => "Stem",
            MorphCategory::State => "State",
        }
    }

    /// The values the chooser offers, in plain English.
    pub fn values(self) -> &'static [&'static str] {
        match self {
            MorphCategory::PartOfSpeech => &[
                "noun",
                "adjective",
                "verb",
                "pronoun",
                "article",
                "preposition",
                "conjunction",
                "adverb",
                "particle",
                "interjection",
                "suffix",
            ],
            MorphCategory::Tense => &[
                "present",
                "imperfect",
                "future",
                "aorist",
                "perfect",
                "pluperfect",
                "consecutive perfect",
                "consecutive imperfect",
                "conjunctive imperfect",
            ],
            MorphCategory::Voice => &["active", "middle", "passive", "deponent"],
            MorphCategory::Mood => &[
                "indicative",
                "subjunctive",
                "optative",
                "imperative",
                "jussive",
                "cohortative",
                "infinitive",
                "participle",
            ],
            MorphCategory::Case => &["nominative", "genitive", "dative", "accusative", "vocative"],
            MorphCategory::Number => &["singular", "plural", "dual"],
            MorphCategory::Gender => &["masculine", "feminine", "neuter", "common"],
            MorphCategory::Person => &["first", "second", "third"],
            MorphCategory::Stem => STEMS,
            MorphCategory::State => &["absolute", "construct", "determined"],
        }
    }
}

/// Short forms a query may use for a value.
const ALIASES: &[(&str, &str)] = &[
    ("1st", "first"),
    ("2nd", "second"),
    ("3rd", "third"),
    ("sg", "singular"),
    ("sing", "singular"),
    ("pl", "plural"),
    ("masc", "masculine"),
    ("fem", "feminine"),
    ("neut", "neuter"),
    ("nom", "nominative"),
    ("gen", "genitive"),
    ("dat", "dative"),
    ("acc", "accusative"),
    ("voc", "vocative"),
    ("adj", "adjective"),
    ("pron", "pronoun"),
    ("prep", "preposition"),
    ("conj", "conjunction"),
    ("adv", "adverb"),
    ("wayyiqtol", "consecutive imperfect"),
    ("weqatal", "consecutive perfect"),
];

/// One criterion: a value of one category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MorphCriterion {
    pub category: MorphCategory,
    /// One of [`MorphCategory::values`].
    pub value: &'static str,
}

/// How a value is written in query text: lowercase, words joined by `-`
/// (`consecutive-imperfect`, `qal-passive`).
fn query_word(value: &str) -> String {
    value.to_lowercase().replace(' ', "-")
}

impl MorphCriterion {
    /// The criterion a query word names (`aorist`, `Qal`, `consecutive-imperfect`, `gen`),
    /// ignoring case.
    pub fn from_word(word: &str) -> Option<Self> {
        let w = word.trim().to_lowercase().replace(' ', "-");
        let w = ALIASES
            .iter()
            .find(|(a, _)| *a == w)
            .map_or(w, |(_, v)| query_word(v));
        MorphCategory::ALL.iter().find_map(|&category| {
            category
                .values()
                .iter()
                .find(|v| query_word(v) == w)
                .map(|&value| MorphCriterion { category, value })
        })
    }

    /// The criterion as query text.
    pub fn query_word(self) -> String {
        query_word(self.value)
    }

    /// Whether one part of a decoded code has this value.
    pub fn matches(self, p: &MorphPart) -> bool {
        let v = self.value;
        match self.category {
            MorphCategory::PartOfSpeech => p.pos.is_some_and(|pos| pos.name() == v),
            // A Greek second aorist is an aorist.
            MorphCategory::Tense => p
                .tense
                .is_some_and(|t| t.strip_prefix("2nd ").unwrap_or(t) == v),
            // `middle deponent` is middle and deponent; `middle or passive` is both.
            MorphCategory::Voice => p
                .voice
                .is_some_and(|voice| voice.split(' ').any(|w| w == v)),
            MorphCategory::Mood => match v {
                "infinitive" => p.form.is_some_and(|f| f.starts_with("infinitive")),
                "participle" => p.form.is_some_and(|f| f.contains("participle")),
                _ => p.mood == Some(v),
            },
            MorphCategory::Case => p.case.is_some_and(|c| case_name(c) == v),
            MorphCategory::Number => p.number.is_some_and(|n| number_name(n) == v),
            MorphCategory::Gender => p.gender.is_some_and(|g| gender_name(g) == v),
            MorphCategory::Person => p.person.is_some_and(|n| person_name(n) == v),
            MorphCategory::Stem => p.stem.is_some_and(|s| s == v),
            MorphCategory::State => p.state.is_some_and(|s| state_name(s) == v),
        }
    }
}

fn case_name(c: Case) -> &'static str {
    match c {
        Case::Nominative => "nominative",
        Case::Genitive => "genitive",
        Case::Dative => "dative",
        Case::Accusative => "accusative",
        Case::Vocative => "vocative",
    }
}

fn number_name(n: Number) -> &'static str {
    match n {
        Number::Singular => "singular",
        Number::Plural => "plural",
        Number::Dual => "dual",
    }
}

fn gender_name(g: Gender) -> &'static str {
    match g {
        Gender::Masculine => "masculine",
        Gender::Feminine => "feminine",
        Gender::Neuter => "neuter",
        Gender::Common => "common",
    }
}

fn person_name(n: u8) -> &'static str {
    match n {
        1 => "first",
        2 => "second",
        _ => "third",
    }
}

fn state_name(s: State) -> &'static str {
    match s {
        State::Absolute => "absolute",
        State::Construct => "construct",
        State::Determined => "determined",
    }
}

/// Criteria that one morpheme part must all meet, kept in category order so that the same
/// criteria in any order are equal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct MorphCriteria(Vec<MorphCriterion>);

impl MorphCriteria {
    pub fn new(criteria: impl IntoIterator<Item = MorphCriterion>) -> Self {
        let mut c: Vec<MorphCriterion> = criteria.into_iter().collect();
        c.sort();
        c.dedup();
        Self(c)
    }

    pub fn criteria(&self) -> &[MorphCriterion] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The chooser's choice in `category` (the first, if a query named several).
    pub fn get(&self, category: MorphCategory) -> Option<&'static str> {
        self.0
            .iter()
            .find(|c| c.category == category)
            .map(|c| c.value)
    }

    /// Choose `value` in `category` (one of [`MorphCategory::values`]), or Any with `None`.
    /// A value the category does not offer is ignored.
    pub fn set(&mut self, category: MorphCategory, value: Option<&str>) {
        self.0.retain(|c| c.category != category);
        if let Some(&value) = value.and_then(|v| category.values().iter().find(|x| **x == v)) {
            self.0.push(MorphCriterion { category, value });
            self.0.sort();
        }
    }

    /// `@verb aorist imperative`; empty for no criteria.
    pub fn query_text(&self) -> String {
        if self.0.is_empty() {
            return String::new();
        }
        let words: Vec<String> = self.0.iter().map(|c| c.query_word()).collect();
        format!("@{}", words.join(" "))
    }

    /// Whether one part meets every criterion.
    pub fn matches_part(&self, p: &MorphPart) -> bool {
        self.0.iter().all(|c| c.matches(p))
    }

    /// Whether some part of a decoded code meets every criterion.
    pub fn matches_morph(&self, m: &Morph) -> bool {
        m.parts.iter().any(|p| self.matches_part(p))
    }

    /// Whether a Morphology code as tagged (`V-AAM-2P`, `HVqp3ms`) meets every criterion. A
    /// code the decoder cannot read meets none.
    pub fn matches_code(&self, code: &str) -> bool {
        morph::parse(code).is_ok_and(|m| self.matches_morph(&m))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn criteria(words: &str) -> MorphCriteria {
        MorphCriteria::new(
            words
                .split_whitespace()
                .map(|w| MorphCriterion::from_word(w).unwrap_or_else(|| panic!("{w}"))),
        )
    }

    #[test]
    fn every_value_is_one_query_word_of_one_category() {
        let mut seen = std::collections::HashSet::new();
        for c in MorphCategory::ALL {
            for v in c.values() {
                let w = query_word(v);
                assert!(seen.insert(w.clone()), "{w} is in two categories");
                assert!(!w.contains(char::is_whitespace));
                let back = MorphCriterion::from_word(&w).unwrap();
                assert_eq!((back.category, back.value), (c, *v));
            }
        }
        for (alias, value) in ALIASES {
            assert!(!seen.contains(*alias), "{alias} shadows a value");
            assert_eq!(MorphCriterion::from_word(alias).unwrap().value, *value);
        }
    }

    #[test]
    fn every_stem_the_decoder_names_is_offered() {
        for lang in ['H', 'A'] {
            for c in ('a'..='z').chain('A'..='Z') {
                if let Ok(m) = morph::parse(&format!("{lang}V{c}p3ms")) {
                    let stem = m.parts[0].stem.unwrap();
                    assert!(STEMS.contains(&stem), "{stem}");
                    assert!(criteria(&query_word(stem)).matches_morph(&m));
                }
            }
        }
    }

    #[test]
    fn criteria_meet_greek_and_hebrew_codes_in_plain_english() {
        let yes: &[(&str, &str)] = &[
            ("verb aorist imperative", "V-AAM-2P"),
            ("aorist imperative", "V-2AAM-2S"),
            ("aorist passive imperative third singular", "V-APM-3S"),
            ("noun genitive", "N-GSF"),
            ("noun gen fem sg", "N-GSF"),
            ("middle deponent", "V-PNI-3S"),
            ("passive", "V-PNI-3S"),
            ("participle", "V-PAP-NSM"),
            ("participle", "HVqrmsa"),
            ("infinitive", "HVqcc"),
            ("verb qal perfect third masculine singular", "HVqp3ms"),
            ("wayyiqtol", "HVqw3ms"),
            ("consecutive-imperfect qal", "HVqw3ms"),
            ("qal-passive", "HVQp3ms"),
            ("construct noun plural", "HNcmpc"),
            ("peal", "AVqp3ms"),
            ("determined", "ANcmsd"),
            ("conjunction", "HC"),
        ];
        for (words, code) in yes {
            assert!(criteria(words).matches_code(code), "{words} ⊄ {code}");
        }
        let no: &[(&str, &str)] = &[
            ("verb aorist imperative", "V-AAI-3S"),
            ("aorist imperative", "V-PAM-2P"),
            ("noun genitive", "N-DSF"),
            ("perfect", "HVqq3ms"),
            ("qal", "HVQp3ms"),
            ("imperative", "V-PAR-NSM"),
            ("verb", "not a code"),
            ("aorist present", "V-AAM-2P"),
        ];
        for (words, code) in no {
            assert!(!criteria(words).matches_code(code), "{words} ⊂ {code}");
        }
    }

    #[test]
    fn the_chooser_writes_criteria_in_category_order() {
        let mut c = MorphCriteria::default();
        assert_eq!(c.query_text(), "");
        c.set(MorphCategory::Mood, Some("imperative"));
        c.set(MorphCategory::Tense, Some("aorist"));
        c.set(MorphCategory::PartOfSpeech, Some("verb"));
        assert_eq!(c.query_text(), "@verb aorist imperative");
        assert_eq!(c, criteria("imperative aorist verb"));
        c.set(MorphCategory::Tense, Some("consecutive imperfect"));
        c.set(MorphCategory::Stem, Some("Qal"));
        c.set(MorphCategory::PartOfSpeech, None);
        c.set(MorphCategory::Case, Some("no such case"));
        assert_eq!(c.query_text(), "@consecutive-imperfect imperative qal");
        assert_eq!(c.get(MorphCategory::Stem), Some("Qal"));
        assert_eq!(c.get(MorphCategory::Case), None);
    }
}
