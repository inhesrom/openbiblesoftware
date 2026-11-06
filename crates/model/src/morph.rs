//! Morphology codes in plain English (#42).
//!
//! STEPBible tags every Hebrew and Aramaic word of TAHOT with an OSHB-style code (`HVqp3ms`,
//! `HC/Vqw3ms`, `AR/Ncmsa`) and every Greek word of TAGNT with a Robinson-style code
//! (`V-AAI-3S`, `N-GSF`, `S-1SNSM`). [`parse`] reads either into a [`Morph`]: the word's
//! [`MorphPart`]s (a Hebrew word can be prefix + word + suffix) with their grammar as typed
//! fields. [`Morph::short`] and [`Morph::long`] render it ("Qal perfect, 3rd masc. sing."),
//! and [`Morph::code`] keeps the raw code for tooltips.
//!
//! Names follow STEPBible's own expansions (TEHMC, TEGMC), which these codes come from.

use std::fmt;

/// The language a code describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MorphLanguage {
    Hebrew,
    Aramaic,
    Greek,
}

/// Part of speech of one part of a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartOfSpeech {
    Noun,
    Adjective,
    Verb,
    Pronoun,
    Article,
    Preposition,
    Conjunction,
    Adverb,
    Particle,
    Interjection,
    /// A Hebrew pronominal or other suffix.
    Suffix,
    /// A transliterated foreign word (Greek `ARAM`, `HEB`).
    Foreign,
}

impl PartOfSpeech {
    pub fn name(self) -> &'static str {
        match self {
            PartOfSpeech::Noun => "noun",
            PartOfSpeech::Adjective => "adjective",
            PartOfSpeech::Verb => "verb",
            PartOfSpeech::Pronoun => "pronoun",
            PartOfSpeech::Article => "article",
            PartOfSpeech::Preposition => "preposition",
            PartOfSpeech::Conjunction => "conjunction",
            PartOfSpeech::Adverb => "adverb",
            PartOfSpeech::Particle => "particle",
            PartOfSpeech::Interjection => "interjection",
            PartOfSpeech::Suffix => "suffix",
            PartOfSpeech::Foreign => "transliterated word",
        }
    }

    /// Words that carry grammar rather than content; Key words leaves them out (#15).
    pub fn is_function_word(self) -> bool {
        matches!(
            self,
            PartOfSpeech::Article
                | PartOfSpeech::Preposition
                | PartOfSpeech::Conjunction
                | PartOfSpeech::Particle
                | PartOfSpeech::Pronoun
                | PartOfSpeech::Suffix
                | PartOfSpeech::Interjection
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gender {
    Masculine,
    Feminine,
    Neuter,
    /// Hebrew "common" / "both": either gender.
    Common,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Number {
    Singular,
    Plural,
    Dual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Case {
    Nominative,
    Genitive,
    Dative,
    Accusative,
    Vocative,
}

/// Hebrew and Aramaic noun state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    Absolute,
    Construct,
    /// Aramaic determined (emphatic) state.
    Determined,
}

/// One part of a word: its grammar as far as the code gives it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MorphPart {
    /// `None` only for a Hebrew segment the code leaves empty.
    pub pos: Option<PartOfSpeech>,
    /// What sort of noun, pronoun, particle or suffix: "proper", "gentilic", "personal",
    /// "definite article", "relative", "directional" …
    pub kind: Option<&'static str>,
    /// Hebrew and Aramaic verb stem: "Qal", "Niphal", "Peal" …
    pub stem: Option<&'static str>,
    /// Hebrew verb form ("perfect", "consecutive imperfect") or Greek tense ("aorist").
    pub tense: Option<&'static str>,
    /// Greek voice ("active", "middle deponent").
    pub voice: Option<&'static str>,
    /// Mood: "indicative", "jussive", "cohortative", "imperative", "subjunctive" …
    pub mood: Option<&'static str>,
    /// Non-finite verb form: "participle", "passive participle", "infinitive" …
    pub form: Option<&'static str>,
    pub person: Option<u8>,
    pub gender: Option<Gender>,
    pub number: Option<Number>,
    pub case: Option<Case>,
    pub state: Option<State>,
    /// Further notes: "comparative", "Attic form", "transcribed from Aramaic" …
    pub extras: Vec<&'static str>,
}

/// A parsed morphology code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Morph {
    code: String,
    pub language: MorphLanguage,
    pub parts: Vec<MorphPart>,
    /// A Strong's number this Greek word is joined with (`CONJ + G1437`).
    pub joined_with: Option<String>,
}

/// Why a code could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorphError {
    pub code: String,
    pub reason: String,
}

impl fmt::Display for MorphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "morphology code `{}`: {}", self.code, self.reason)
    }
}

impl std::error::Error for MorphError {}

fn err(code: &str, reason: impl Into<String>) -> MorphError {
    MorphError {
        code: code.to_string(),
        reason: reason.into(),
    }
}

/// Parse a STEPBible Hebrew/Aramaic (OSHB-style) or Greek (Robinson-style) code.
pub fn parse(code: &str) -> Result<Morph, MorphError> {
    let c = code.trim();
    if c.is_empty() {
        return Err(err(code, "empty"));
    }
    // Hebrew codes start with the language letter and then a part-of-speech letter (or `/`);
    // Greek codes are capitals with `-` separators.
    let mut chars = c.chars();
    let first = chars.next();
    let second = chars.next();
    let hebrew = matches!(first, Some('H' | 'A'))
        && (second.is_none_or(|s| s == '/' || s.is_ascii_lowercase() || is_hebrew_pos(s)))
        && !c.starts_with("ADV")
        && !c.starts_with("ARAM")
        && !c.contains('-');
    if hebrew {
        hebrew::parse(c).map_err(|r| err(code, r))
    } else {
        greek::parse(c).map_err(|r| err(code, r))
    }
}

fn is_hebrew_pos(c: char) -> bool {
    matches!(c, 'A' | 'C' | 'D' | 'N' | 'P' | 'R' | 'S' | 'T' | 'V')
}

impl Morph {
    /// The code as given, for tooltips.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// The part that carries the word's meaning: the last part that is not a suffix (for a
    /// Hebrew word with prefixes, the word itself).
    pub fn main_part(&self) -> Option<&MorphPart> {
        self.parts
            .iter()
            .rev()
            .find(|p| p.pos.is_some_and(|pos| pos != PartOfSpeech::Suffix))
            .or_else(|| self.parts.first())
    }

    /// Part of speech of the main part.
    pub fn pos(&self) -> Option<PartOfSpeech> {
        self.main_part().and_then(|p| p.pos)
    }

    /// Whether the word is a function word (article, preposition, particle, pronoun …).
    pub fn is_function_word(&self) -> bool {
        self.pos().is_some_and(PartOfSpeech::is_function_word)
    }

    /// Compact English: `Qal perfect, 3rd masc. sing.`; parts of a Hebrew word are joined
    /// with ` + `.
    pub fn short(&self) -> String {
        let parts: Vec<String> = self
            .parts
            .iter()
            .filter(|p| p.pos.is_some())
            .map(|p| p.render(false))
            .collect();
        let mut s = parts.join(" + ");
        if let Some(j) = &self.joined_with {
            s.push_str(&format!(" + {j}"));
        }
        s
    }

    /// Spelled-out English: `Verb, Qal perfect, third person masculine singular`.
    pub fn long(&self) -> String {
        let parts: Vec<String> = self
            .parts
            .iter()
            .filter(|p| p.pos.is_some())
            .map(|p| p.render(true))
            .collect();
        let mut s = parts.join("; then ");
        if let Some(j) = &self.joined_with {
            s.push_str(&format!("; joined with {j}"));
        }
        s
    }
}

fn person_word(p: u8, long: bool) -> &'static str {
    match (p, long) {
        (1, false) => "1st",
        (2, false) => "2nd",
        (3, false) => "3rd",
        (1, true) => "first person",
        (2, true) => "second person",
        (_, true) => "third person",
        _ => "3rd",
    }
}

impl Gender {
    fn word(self, long: bool) -> &'static str {
        match (self, long) {
            (Gender::Masculine, false) => "masc.",
            (Gender::Feminine, false) => "fem.",
            (Gender::Neuter, false) => "neut.",
            (Gender::Common, false) => "com.",
            (Gender::Masculine, true) => "masculine",
            (Gender::Feminine, true) => "feminine",
            (Gender::Neuter, true) => "neuter",
            (Gender::Common, true) => "common",
        }
    }
}

impl Number {
    fn word(self, long: bool) -> &'static str {
        match (self, long) {
            (Number::Singular, false) => "sing.",
            (Number::Plural, false) => "pl.",
            (Number::Dual, false) => "dual",
            (Number::Singular, true) => "singular",
            (Number::Plural, true) => "plural",
            (Number::Dual, true) => "dual",
        }
    }
}

impl Case {
    fn word(self, long: bool) -> &'static str {
        match (self, long) {
            (Case::Nominative, false) => "nom.",
            (Case::Genitive, false) => "gen.",
            (Case::Dative, false) => "dat.",
            (Case::Accusative, false) => "acc.",
            (Case::Vocative, false) => "voc.",
            (Case::Nominative, true) => "nominative",
            (Case::Genitive, true) => "genitive",
            (Case::Dative, true) => "dative",
            (Case::Accusative, true) => "accusative",
            (Case::Vocative, true) => "vocative",
        }
    }
}

impl State {
    fn word(self, long: bool) -> &'static str {
        match (self, long) {
            (State::Absolute, false) => "abs.",
            (State::Construct, false) => "constr.",
            (State::Determined, false) => "det.",
            (State::Absolute, true) => "absolute",
            (State::Construct, true) => "construct",
            (State::Determined, true) => "determined",
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

impl MorphPart {
    /// `Qal perfect, 3rd masc. sing.` (short) or `Verb, Qal perfect, third person masculine
    /// singular` (long).
    fn render(&self, long: bool) -> String {
        let pos = self.pos.map_or("", PartOfSpeech::name);
        // The head: what the part is.
        let mut head: Vec<&str> = Vec::new();
        if let Some(s) = self.stem {
            head.push(s);
        }
        if let Some(t) = self.tense {
            head.push(t);
        }
        if let Some(v) = self.voice {
            head.push(v);
        }
        if let Some(m) = self.mood {
            head.push(m);
        }
        if let Some(f) = self.form {
            head.push(f);
        }
        let is_verb = self.pos == Some(PartOfSpeech::Verb);
        if !is_verb {
            if let Some(k) = self.kind {
                head.push(k);
            }
            head.push(pos);
        }
        // The details: person, gender, number, case, state.
        let mut detail: Vec<&str> = Vec::new();
        if let Some(p) = self.person {
            detail.push(person_word(p, long));
        }
        if let Some(c) = self.case {
            detail.push(c.word(long));
        }
        if let Some(g) = self.gender {
            detail.push(g.word(long));
        }
        if let Some(n) = self.number {
            detail.push(n.word(long));
        }
        // An infinitive's state is already in its name.
        if let Some(s) = self.state
            && !self.form.is_some_and(|f| f.starts_with("infinitive"))
        {
            detail.push(s.word(long));
        }
        let mut out = String::new();
        if long {
            out.push_str(&capitalize(pos));
            if is_verb || !head.is_empty() {
                let h: Vec<&str> = if is_verb {
                    head.clone()
                } else {
                    head[..head.len() - 1].to_vec()
                };
                if !h.is_empty() {
                    out.push_str(", ");
                    out.push_str(&h.join(" "));
                }
            }
        } else {
            out.push_str(&head.join(" "));
        }
        if !detail.is_empty() {
            out.push_str(", ");
            out.push_str(&detail.join(" "));
        }
        for e in &self.extras {
            out.push_str(" (");
            out.push_str(e);
            out.push(')');
        }
        out
    }
}

impl fmt::Display for Morph {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.short())
    }
}

mod hebrew {
    use super::*;

    fn gender(c: char) -> Result<Option<Gender>, String> {
        Ok(Some(match c {
            'm' => Gender::Masculine,
            'f' => Gender::Feminine,
            'b' | 'c' => Gender::Common,
            // OSHB's "not applicable" (demonstratives, `Nxxxa`).
            'x' => return Ok(None),
            _ => return Err(format!("gender `{c}`")),
        }))
    }

    fn number(c: char) -> Result<Option<Number>, String> {
        Ok(Some(match c {
            's' => Number::Singular,
            'p' => Number::Plural,
            'd' => Number::Dual,
            'x' => return Ok(None),
            _ => return Err(format!("number `{c}`")),
        }))
    }

    fn state(c: char) -> Result<Option<State>, String> {
        Ok(Some(match c {
            'a' => State::Absolute,
            'c' => State::Construct,
            'd' => State::Determined,
            _ => return Err(format!("state `{c}`")),
        }))
    }

    fn person(c: char) -> Result<Option<u8>, String> {
        match c {
            '1' => Ok(Some(1)),
            '2' => Ok(Some(2)),
            '3' => Ok(Some(3)),
            'x' => Ok(None),
            _ => Err(format!("person `{c}`")),
        }
    }

    /// Stems as STEPBible names them (TEHMC); OSHB's rarer stems for completeness.
    fn stem(aramaic: bool, c: char) -> Result<&'static str, String> {
        let s = if aramaic {
            match c {
                'q' => "Peal",
                'Q' => "Peil",
                'p' => "Pael",
                'P' => "Pual",
                'a' => "Aphel",
                'h' => "Haphel",
                'H' => "Hophal",
                'e' => "Shaphel",
                'i' => "Hitpeel",
                'u' => "Hitpael",
                'M' => "Hitpaal",
                'v' => "Ishtaphel",
                's' => "Saphel",
                't' => "Hishtaphel",
                'w' => "Hithaphel",
                'o' => "Polel",
                'z' => "Ithpoel",
                'r' => "Hithpolel",
                'f' => "Hithpalpel",
                'b' => "Hephal",
                'c' => "Tiphel",
                'm' => "Poel",
                'l' => "Palpel",
                'L' => "Ithpalpel",
                'O' => "Ithpolel",
                'G' => "Ittaphal",
                _ => return Err(format!("Aramaic stem `{c}`")),
            }
        } else {
            match c {
                'q' => "Qal",
                'N' => "Niphal",
                'p' => "Piel",
                'P' => "Pual",
                'h' => "Hiphil",
                'H' => "Hophal",
                't' => "Hithpael",
                'u' => "Hothpaal",
                'D' => "Nithpael",
                'O' => "Polal",
                'c' => "Tiphil",
                'v' => "Hishtaphel",
                'o' => "Polel",
                'r' => "Hithpolel",
                'm' => "Poel",
                'M' => "Poal",
                'k' => "Palel",
                'K' => "Pulal",
                'Q' => "Qal passive",
                'l' => "Pilpel",
                'L' => "Polpal",
                'f' => "Hithpalpel",
                'j' => "Pealal",
                'i' => "Pilel",
                'w' => "Nithpalel",
                'y' => "Nithpoel",
                'z' => "Hithpoel",
                _ => return Err(format!("Hebrew stem `{c}`")),
            }
        };
        Ok(s)
    }

    fn verb(aramaic: bool, rest: &[char]) -> Result<MorphPart, String> {
        let mut p = MorphPart {
            pos: Some(PartOfSpeech::Verb),
            ..MorphPart::default()
        };
        let (&s, rest) = rest.split_first().ok_or("verb without a stem")?;
        p.stem = Some(stem(aramaic, s)?);
        let (&f, rest) = rest.split_first().ok_or("verb without a form")?;
        let finite = rest.first().is_some_and(char::is_ascii_digit);
        match (f, finite) {
            ('p', _) => {
                p.tense = Some("perfect");
            }
            ('q', _) => p.tense = Some("consecutive perfect"),
            ('i', _) => p.tense = Some("imperfect"),
            ('n', _) => {
                p.tense = Some("imperfect");
                p.mood = Some("indicative");
            }
            ('w', _) => p.tense = Some("consecutive imperfect"),
            ('u', _) => p.tense = Some("conjunctive imperfect"),
            ('j', _) => {
                p.tense = Some("imperfect");
                p.mood = Some("jussive");
            }
            ('h', _) | ('c', true) => {
                p.tense = Some("imperfect");
                p.mood = Some("cohortative");
            }
            ('v', _) => p.mood = Some("imperative"),
            ('r', _) => p.form = Some("participle"),
            ('s', _) => p.form = Some("passive participle"),
            ('a', _) => p.form = Some("infinitive absolute"),
            ('c', false) => p.form = Some("infinitive construct"),
            _ => return Err(format!("verb form `{f}`")),
        }
        match rest {
            [] => {}
            // Infinitives carry a state: `Vqcc`, `Vqaa`.
            [s] if p.form.is_some_and(|f| f.starts_with("infinitive")) => {
                p.state = state(*s)?;
            }
            [pe, g, n] if pe.is_ascii_digit() => {
                p.person = person(*pe)?;
                p.gender = gender(*g)?;
                p.number = number(*n)?;
            }
            [g, n] => {
                p.gender = gender(*g)?;
                p.number = number(*n)?;
            }
            [g, n, s] => {
                p.gender = gender(*g)?;
                p.number = number(*n)?;
                p.state = state(*s)?;
            }
            _ => return Err("verb inflection".into()),
        }
        Ok(p)
    }

    /// Gender, number, state (any trailing subset).
    fn gns(p: &mut MorphPart, rest: &[char]) -> Result<(), String> {
        if let Some(&g) = rest.first() {
            p.gender = gender(g)?;
        }
        if let Some(&n) = rest.get(1) {
            p.number = number(n)?;
        }
        if let Some(&s) = rest.get(2) {
            p.state = state(s)?;
        }
        if rest.len() > 3 {
            return Err("too long".into());
        }
        Ok(())
    }

    fn png(p: &mut MorphPart, rest: &[char]) -> Result<(), String> {
        match rest {
            [] => Ok(()),
            [pe, g, n] => {
                p.person = person(*pe)?;
                p.gender = gender(*g)?;
                p.number = number(*n)?;
                Ok(())
            }
            _ => Err("person, gender, number".into()),
        }
    }

    fn segment(aramaic: bool, seg: &str) -> Result<MorphPart, String> {
        let chars: Vec<char> = seg.chars().collect();
        let Some((&pos, rest)) = chars.split_first() else {
            // Some TAHOT words have an empty grammar segment.
            return Ok(MorphPart::default());
        };
        let mut p = MorphPart::default();
        let kind_of = |rest: &[char]| rest.first().copied();
        match pos {
            'V' => return verb(aramaic, rest),
            'C' => {
                p.pos = Some(PartOfSpeech::Conjunction);
            }
            // STEPBible's `c`: the vav of a consecutive verb.
            'c' => {
                p.pos = Some(PartOfSpeech::Conjunction);
                p.kind = Some("consecutive");
            }
            'D' => p.pos = Some(PartOfSpeech::Adverb),
            'R' => {
                p.pos = Some(PartOfSpeech::Preposition);
                match rest {
                    [] => {}
                    ['d'] => p.extras.push("with the article"),
                    _ => return Err("preposition".into()),
                }
                return Ok(p);
            }
            'N' => {
                p.pos = Some(PartOfSpeech::Noun);
                match kind_of(rest) {
                    Some('c') => p.kind = Some("common"),
                    Some('g') => p.kind = Some("gentilic"),
                    Some('t') => p.kind = Some("title"),
                    // OSHB's `Nxxxa` (the writing on the wall, Dan 5:25): type not given.
                    Some('x') => {}
                    Some('p') => {
                        p.kind = Some("proper");
                        let note = match rest.get(1) {
                            Some('m') => {
                                p.gender = Some(Gender::Masculine);
                                "person's name"
                            }
                            Some('f') => {
                                p.gender = Some(Gender::Feminine);
                                "person's name"
                            }
                            Some('l') => "place name",
                            Some('t') => "title",
                            None => "name",
                            Some(c) => return Err(format!("proper noun type `{c}`")),
                        };
                        p.extras.push(note);
                        if rest.len() > 2 {
                            return Err("proper noun".into());
                        }
                        return Ok(p);
                    }
                    Some(c) => return Err(format!("noun type `{c}`")),
                    None => {}
                }
                if p.kind == Some("common") {
                    p.kind = None;
                }
                gns(&mut p, rest.get(1..).unwrap_or(&[]))?;
                return Ok(p);
            }
            'A' => {
                p.pos = Some(PartOfSpeech::Adjective);
                match kind_of(rest) {
                    Some('a') => {}
                    Some('c') => p.kind = Some("cardinal number"),
                    Some('g') => p.kind = Some("gentilic"),
                    Some('o') => p.kind = Some("ordinal number"),
                    Some(c) => return Err(format!("adjective type `{c}`")),
                    None => {}
                }
                gns(&mut p, rest.get(1..).unwrap_or(&[]))?;
                return Ok(p);
            }
            'P' => {
                p.pos = Some(PartOfSpeech::Pronoun);
                p.kind = Some(match kind_of(rest) {
                    Some('p') => "personal",
                    Some('d') => "demonstrative",
                    Some('f') => "indefinite",
                    Some('i') => "interrogative",
                    Some('r') => "relative",
                    None => return Ok(p),
                    Some(c) => return Err(format!("pronoun type `{c}`")),
                });
                let tail = rest.get(1..).unwrap_or(&[]);
                // Person first (OSHB writes `x` for a demonstrative's person: `Pdxms`).
                if tail
                    .first()
                    .is_some_and(|c| c.is_ascii_digit() || *c == 'x')
                {
                    png(&mut p, tail)?;
                } else {
                    gns(&mut p, tail)?;
                }
                return Ok(p);
            }
            'S' => {
                p.pos = Some(PartOfSpeech::Suffix);
                p.kind = Some(match kind_of(rest) {
                    Some('p') => "pronominal",
                    Some('d') => "directional",
                    Some('h') => "paragogic he",
                    Some('n') => "paragogic nun",
                    None => return Ok(p),
                    Some(c) => return Err(format!("suffix type `{c}`")),
                });
                png(&mut p, rest.get(1..).unwrap_or(&[]))?;
                return Ok(p);
            }
            'T' => {
                p.pos = Some(PartOfSpeech::Particle);
                let k = match kind_of(rest) {
                    Some('a') if aramaic => "definite article",
                    Some('a') => "affirmation",
                    Some('d') => "definite article",
                    Some('c') => "conditional",
                    Some('e') => "exhortation",
                    Some('i') => "interrogative",
                    Some('j') => "interjection",
                    Some('m') => "demonstrative",
                    Some('n') => "negative",
                    Some('o') => "object marker",
                    Some('r') => "relative",
                    None => return Ok(p),
                    Some(c) => return Err(format!("particle type `{c}`")),
                };
                if k == "definite article" {
                    p.pos = Some(PartOfSpeech::Article);
                    p.kind = Some("definite");
                } else if k == "interjection" {
                    p.pos = Some(PartOfSpeech::Interjection);
                } else if k == "object marker" {
                    p.kind = Some("object marker");
                } else {
                    p.kind = Some(k);
                }
                if rest.len() > 1 {
                    return Err("particle".into());
                }
                return Ok(p);
            }
            c => return Err(format!("part of speech `{c}`")),
        }
        if !rest.is_empty() {
            return Err(format!("unexpected `{}`", rest.iter().collect::<String>()));
        }
        Ok(p)
    }

    pub(super) fn parse(code: &str) -> Result<Morph, String> {
        let aramaic = code.starts_with('A');
        let body = &code[1..];
        let mut parts = Vec::new();
        for seg in body.split('/') {
            parts.push(segment(aramaic, seg)?);
        }
        if parts.iter().all(|p| p.pos.is_none()) {
            return Err("no parts".into());
        }
        Ok(Morph {
            code: code.to_string(),
            language: if aramaic {
                MorphLanguage::Aramaic
            } else {
                MorphLanguage::Hebrew
            },
            parts,
            joined_with: None,
        })
    }
}

mod greek {
    use super::*;

    fn case(c: char) -> Result<Case, String> {
        Ok(match c {
            'N' => Case::Nominative,
            'G' => Case::Genitive,
            'D' => Case::Dative,
            'A' => Case::Accusative,
            'V' => Case::Vocative,
            _ => return Err(format!("case `{c}`")),
        })
    }

    fn number(c: char) -> Result<Number, String> {
        Ok(match c {
            'S' => Number::Singular,
            'P' => Number::Plural,
            _ => return Err(format!("number `{c}`")),
        })
    }

    fn gender(c: char) -> Result<Gender, String> {
        Ok(match c {
            'M' => Gender::Masculine,
            'F' => Gender::Feminine,
            'N' => Gender::Neuter,
            _ => return Err(format!("gender `{c}`")),
        })
    }

    fn person(c: char) -> Result<u8, String> {
        match c {
            '1' => Ok(1),
            '2' => Ok(2),
            '3' => Ok(3),
            _ => Err(format!("person `{c}`")),
        }
    }

    /// Case, number and (optionally) gender: `GSF`, `DP`.
    fn cng(p: &mut MorphPart, t: &str) -> Result<(), String> {
        let c: Vec<char> = t.chars().collect();
        match c[..] {
            [ca, n] => {
                p.case = Some(case(ca)?);
                p.number = Some(number(n)?);
            }
            [ca, n, g] => {
                p.case = Some(case(ca)?);
                p.number = Some(number(n)?);
                p.gender = Some(gender(g)?);
            }
            _ => return Err(format!("case, number, gender `{t}`")),
        }
        Ok(())
    }

    fn verb(p: &mut MorphPart, tvm: &str, rest: &mut Vec<&str>) -> Result<(), String> {
        let mut c: Vec<char> = tvm.chars().collect();
        let second = c.first() == Some(&'2');
        if second {
            c.remove(0);
        }
        let [t, v, m] = c[..] else {
            return Err(format!("tense, voice, mood `{tvm}`"));
        };
        p.tense = Some(match (t, second) {
            ('P', false) => "present",
            ('I', false) => "imperfect",
            ('F', false) => "future",
            ('A', false) => "aorist",
            ('R', false) => "perfect",
            ('L', false) => "pluperfect",
            ('P', true) => "2nd present",
            ('F', true) => "2nd future",
            ('A', true) => "2nd aorist",
            ('R', true) => "2nd perfect",
            ('L', true) => "2nd pluperfect",
            _ => return Err(format!("tense `{t}`")),
        });
        p.voice = match v {
            'A' => Some("active"),
            'M' => Some("middle"),
            'P' => Some("passive"),
            'D' => Some("middle deponent"),
            'O' => Some("passive deponent"),
            'N' => Some("middle or passive deponent"),
            'E' => Some("middle or passive"),
            'Q' => Some("impersonal active"),
            // No voice stated.
            'X' => None,
            _ => return Err(format!("voice `{v}`")),
        };
        match m {
            'I' | 'S' | 'O' | 'M' => {
                p.mood = Some(match m {
                    'I' => "indicative",
                    'S' => "subjunctive",
                    'O' => "optative",
                    _ => "imperative",
                });
                let pn = if rest.is_empty() { "" } else { rest.remove(0) };
                let c: Vec<char> = pn.chars().collect();
                let [pe, n] = c[..] else {
                    return Err(format!("person and number `{pn}`"));
                };
                p.person = Some(person(pe)?);
                p.number = Some(number(n)?);
            }
            'N' => p.form = Some("infinitive"),
            'P' | 'R' => {
                // Robinson's `R`: a participle used as an imperative (Tischendorf).
                p.form = Some(if m == 'R' {
                    "imperatival participle"
                } else {
                    "participle"
                });
                if rest.is_empty() {
                    return Err("participle without case".into());
                }
                let t = rest.remove(0);
                cng(p, t)?;
            }
            _ => return Err(format!("mood `{m}`")),
        }
        Ok(())
    }

    fn extra(p: &mut MorphPart, head: &str, t: &str) -> Result<(), String> {
        let adjective = p.pos == Some(PartOfSpeech::Adjective);
        let name = |p: &mut MorphPart, k: &'static str| p.kind = Some(k);
        match t {
            "C" if adjective => p.extras.push("comparative"),
            "C" => p.extras.push("contracted form"),
            "S" => p.extras.push("superlative"),
            "N" if head == "PRT" => p.kind = Some("negative"),
            "I" if head == "PRT" => p.kind = Some("interrogative"),
            "N" => p.extras.push("negative"),
            "I" => p.extras.push("interrogative"),
            "K" => p.extras.push("with καί joined (crasis)"),
            "ABB" => p.extras.push("abbreviated"),
            "NUI" => p.extras.push("indeclinable numeral"),
            "ATT" => p.extras.push("Attic form"),
            "AP" => p.extras.push("apocopated form"),
            "A" => p.extras.push("Aeolic form"),
            "IRR" => p.extras.push("irregular form"),
            "LI" => p.extras.push("indeclinable letter"),
            "ARAM" => p.extras.push("transcribed from Aramaic"),
            "HEB" => p.extras.push("transcribed from Hebrew"),
            "T" if head == "V" => p.extras.push("transitive"),
            "P" => name(p, "personal name"),
            "L" => name(p, "place name"),
            "T" => name(p, "title"),
            "G" => name(p, "gentilic"),
            "PG" => name(p, "gentilic of a person"),
            "LG" => name(p, "gentilic of a place"),
            "TG" => name(p, "gentilic of a title"),
            _ => return Err(format!("suffix `{t}`")),
        }
        Ok(())
    }

    pub(super) fn parse(code: &str) -> Result<Morph, String> {
        // `CONJ + G1437`: a word joined with another Strong's number.
        let (main, joined) = match code.split_once(" + ") {
            Some((m, j)) => (m.trim(), Some(j.trim().to_string())),
            None => (code, None),
        };
        let mut toks: Vec<&str> = main.split('-').collect();
        if toks.iter().any(|t| t.is_empty()) {
            return Err("empty part".into());
        }
        let head = toks.remove(0);
        let mut p = MorphPart::default();
        let pronoun = |k: &'static str| (Some(PartOfSpeech::Pronoun), Some(k));
        let (pos, kind) = match head {
            "N" => (Some(PartOfSpeech::Noun), None),
            "A" => (Some(PartOfSpeech::Adjective), None),
            "NUI" => (Some(PartOfSpeech::Adjective), Some("indeclinable numeral")),
            "T" => (Some(PartOfSpeech::Article), Some("definite")),
            "V" => (Some(PartOfSpeech::Verb), None),
            "R" => pronoun("relative"),
            "C" => pronoun("reciprocal"),
            "D" => pronoun("demonstrative"),
            "K" => pronoun("correlative"),
            "I" => pronoun("interrogative"),
            "X" => pronoun("indefinite"),
            "Q" => pronoun("correlative or interrogative"),
            "F" => pronoun("reflexive"),
            "S" => pronoun("possessive"),
            "P" => pronoun("personal"),
            "ADV" => (Some(PartOfSpeech::Adverb), None),
            "CONJ" => (Some(PartOfSpeech::Conjunction), None),
            "COND" => (Some(PartOfSpeech::Conjunction), Some("conditional")),
            "PRT" => (Some(PartOfSpeech::Particle), None),
            "PREP" => (Some(PartOfSpeech::Preposition), None),
            "INJ" => (Some(PartOfSpeech::Interjection), None),
            "ARAM" => (Some(PartOfSpeech::Foreign), Some("Aramaic")),
            "HEB" => (Some(PartOfSpeech::Foreign), Some("Hebrew")),
            _ => return Err(format!("part of speech `{head}`")),
        };
        p.pos = pos;
        p.kind = kind;
        match head {
            "V" => {
                if toks.is_empty() {
                    return Err("verb without tense".into());
                }
                let tvm = toks.remove(0);
                verb(&mut p, tvm, &mut toks)?;
            }
            "N" if toks
                .first()
                .is_some_and(|t| matches!(*t, "PRI" | "OI" | "LI")) =>
            {
                let t = toks.remove(0);
                match t {
                    "PRI" => p.kind = Some("indeclinable proper"),
                    "OI" => p.kind = Some("indeclinable"),
                    _ => p.extras.push("indeclinable letter"),
                }
            }
            "A" if toks.first() == Some(&"NUI") => {
                toks.remove(0);
                p.kind = Some("indeclinable numeral");
            }
            "N" | "A" | "T" | "R" | "C" | "D" | "K" | "I" | "X" | "Q" => {
                if let Some(t) = toks.first()
                    && t.len() >= 2
                    && t.chars().next().is_some_and(|c| "NGDAV".contains(c))
                    && t.chars().nth(1).is_some_and(|c| "SP".contains(c))
                {
                    let t = toks.remove(0);
                    cng(&mut p, t)?;
                } else {
                    return Err("case, number, gender".into());
                }
            }
            "P" | "F" => {
                if !toks.is_empty() {
                    let t = toks.remove(0);
                    let (pe, rest) = match t.chars().next() {
                        Some(c) if c.is_ascii_digit() => (Some(person(c)?), &t[1..]),
                        _ => (None, t),
                    };
                    p.person = pe;
                    cng(&mut p, rest)?;
                }
            }
            "S" => {
                // Person and the possessor's number, then the case, number, gender.
                // Tischendorf leaves out the possessor's number (`S-1NSM`).
                let t = if toks.is_empty() { "" } else { toks.remove(0) };
                let c: Vec<char> = t.chars().collect();
                p.person = Some(person(*c.first().unwrap_or(&' '))?);
                match c.len() {
                    5 => {
                        p.extras.push(match number(c[1])? {
                            Number::Singular => "one possessor",
                            _ => "several possessors",
                        });
                        cng(&mut p, &t[2..])?;
                    }
                    4 => cng(&mut p, &t[1..])?,
                    _ => return Err(format!("possessive `{t}`")),
                }
            }
            _ => {}
        }
        for t in toks {
            extra(&mut p, head, t)?;
        }
        Ok(Morph {
            code: code.to_string(),
            language: MorphLanguage::Greek,
            parts: vec![p],
            joined_with: joined,
        })
    }
}
