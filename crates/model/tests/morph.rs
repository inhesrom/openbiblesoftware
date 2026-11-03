//! The morphology expander (#42) against STEPBible's data: every code TAHOT and TAGNT use,
//! STEPBible's own field-by-field expansions of them (TEHMC, TEGMC), and common forms.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use obs_model::morph::{Case, Gender, Number, State, parse};
use obs_model::{MorphLanguage, PartOfSpeech};

const CODES: &str = include_str!("assets/morph/codes.tsv");
const EXPANSIONS: &str = include_str!("assets/morph/expansions.tsv");

fn rows(data: &str) -> impl Iterator<Item = Vec<&str>> {
    data.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split('\t').collect())
}

#[test]
fn every_code_in_tahot_and_tagnt_expands() {
    let mut n = 0;
    let mut failures = Vec::new();
    for r in rows(CODES) {
        let code = r[1];
        match parse(code) {
            Ok(m) => {
                assert!(!m.short().is_empty() && !m.long().is_empty(), "{code}");
                assert_eq!(m.code(), code);
                let greek = r[0] == "G";
                assert_eq!(m.language == MorphLanguage::Greek, greek, "{code}");
                assert!(m.pos().is_some(), "{code}");
            }
            Err(e) => failures.push(e.to_string()),
        }
        n += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {n} failed: {failures:#?}",
        failures.len()
    );
    assert!(n > 3900, "{n}");
}

/// STEPBible's field values for one of our parts.
fn fields(code: &str) -> Vec<(&'static str, String)> {
    let m = parse(code).unwrap();
    let p = m.main_part().unwrap();
    let greek = m.language == MorphLanguage::Greek;
    let mut f: Vec<(&'static str, String)> = Vec::new();
    let function = match (p.pos.unwrap(), p.kind, greek) {
        (PartOfSpeech::Article | PartOfSpeech::Interjection | PartOfSpeech::Particle, _, false) => {
            "Particle".to_string()
        }
        (PartOfSpeech::Pronoun, Some(k), true) => format!("{} pronoun", capital(k)),
        (PartOfSpeech::Article, _, true) => "Definite article".into(),
        (PartOfSpeech::Particle, Some(k), true) => format!("{} particle", capital(k)),
        (pos, _, _) => capital(pos.name()),
    };
    f.push(("Function", function));
    if let Some(s) = p.stem {
        f.push(("Stem", s.into()));
    }
    if greek {
        if let Some(t) = p.tense {
            f.push(("Tense", t.into()));
        }
        if p.pos == Some(PartOfSpeech::Verb) {
            f.push(("Voice", p.voice.unwrap_or("indefinite voice").into()));
        }
        if let Some(m) = p.mood {
            f.push(("Mood", m.into()));
        }
        if let Some(x) = p.form {
            f.push(("Form", x.into()));
        }
    } else if p.pos == Some(PartOfSpeech::Verb) {
        let form = match (p.tense, p.form, p.mood) {
            (_, Some(x), _) if x.starts_with("infinitive") => "Infinitive",
            (_, Some("participle"), _) => "Participle",
            (_, Some(_), _) => "Participle passive",
            (Some("perfect"), _, _) => "Perfect",
            (Some("consecutive perfect"), _, _) => "Consecutive Perfect",
            (Some("consecutive imperfect"), _, _) => "Consecutive Imperfect",
            (Some("conjunctive imperfect"), _, _) => "Conjunction+Imperfect",
            (Some(_), _, _) => "Imperfect",
            (None, _, _) => "Imperative",
        };
        f.push(("Form", form.into()));
        if let Some(m) = p.mood {
            f.push(("Mood", m.into()));
        }
    }
    if let Some(pe) = p.person {
        f.push(("Person", ["", "1", "2", "3"][pe as usize].into()));
    }
    if let Some(c) = p.case {
        f.push(("Case", format!("{c:?}")));
    }
    if let Some(n) = p.number {
        f.push(("Number", format!("{n:?}")));
    }
    if let Some(g) = p.gender {
        let g = match g {
            Gender::Common => "Either gender".to_string(),
            g => format!("{g:?}"),
        };
        f.push(("Gender", g));
    }
    if let Some(s) = p.state {
        let s = match s {
            State::Determined => "Definite".to_string(),
            s => format!("{s:?}"),
        };
        f.push(("State", s));
    }
    f
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// STEPBible's fields for a code, normalized: lowercase, persons as digits, Hebrew moods
/// taken from the Form's parenthesis.
fn official(fields: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for kv in fields.split(';') {
        let Some((k, v)) = kv.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "Person" => {
                let d = match v.split_whitespace().next().unwrap_or("") {
                    "First" | "1st" => "1",
                    "Second" | "2nd" => "2",
                    _ => "3",
                };
                out.push((k.to_string(), d.to_string()));
            }
            // Proper nouns: a place or title is a kind of name, not a gender.
            "Gender" if v == "Location" || v == "Title" => {}
            // Names STEPBible widens: our Particle and Adverb are theirs.
            "Function" if v == "Particle or Disjunctive" => out.push((k.into(), "particle".into())),
            "Function" if v.starts_with("Adverb") => out.push((k.into(), "adverb".into())),
            "Function" | "Stem" | "Tense" | "Voice" | "Mood" | "Form" | "Case" | "Number"
            | "Gender" | "State" => out.push((k.to_string(), v.to_lowercase())),
            _ => {}
        }
    }
    out
}

/// Where STEPBible's own expansion contradicts its code system (checked by hand against the
/// code letters and the other codes of the same shape).
const SOURCE_QUIRKS: &[&str] = &[
    // TEHMC: `i` (imperfect) with Perfect, `q` (Qal) as Peal, Aramaic `P` as Hitpeel.
    "AVvi3mp",
    "HVvi3mp",
    "HVqq3mp",
    "AVPi3mp",
    "AVPi3ms",
    // TEHMC: `c` (common) gender in a participle expanded as Feminine.
    "HVqrcsc",
    // TEGMC: `PEP` (present middle/passive participle) as active dative; `RMM` (perfect
    // middle imperative) as indicative.
    "V-PEP-ASM",
    "V-RMM-2P",
    "V-RMM-2S",
];

#[test]
fn parses_agree_with_stepbibles_own_expansions() {
    let mut checked = 0;
    let mut wrong = Vec::new();
    for r in rows(EXPANSIONS) {
        let (code, theirs) = (r[1], official(r[2]));
        if SOURCE_QUIRKS.contains(&code) {
            continue;
        }
        let mine: Vec<(String, String)> = fields(code)
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_lowercase()))
            .collect();
        for (k, v) in &mine {
            // Hebrew verbs: STEPBible's Mood is inside Form's parenthesis, which the fixture
            // strips; moods are checked by the forms table instead.
            if k == "Mood" && r[0] == "H" {
                continue;
            }
            // Non-verb Hebrew Forms (Common, Proper …) are kinds; not compared.
            let Some((_, t)) = theirs.iter().find(|(tk, _)| tk == k) else {
                // Fields STEPBible leaves out are fine only for Function-less extras.
                if k != "Function" && !(k == "Voice" && v == "indefinite voice") {
                    wrong.push(format!("{code}: we say {k}={v}, STEPBible has none"));
                }
                continue;
            };
            // TEGMC often drops the "2nd" of a second tense (`V-2LAI` → Pluperfect); the code
            // letter `2` is unambiguous, so ours keeps it.
            let second_tense = k == "Tense" && v.strip_prefix("2nd ") == Some(t.as_str());
            if t != v && !second_tense {
                wrong.push(format!("{code}: {k}: ours {v:?}, STEPBible {t:?}"));
            }
        }
        for (k, t) in &theirs {
            let relevant = matches!(
                k.as_str(),
                "Stem" | "Tense" | "Case" | "Number" | "Gender" | "State" | "Person"
            );
            if relevant && !mine.iter().any(|(mk, _)| mk == k) {
                wrong.push(format!("{code}: missing {k}={t}"));
            }
        }
        checked += 1;
    }
    assert!(
        wrong.is_empty(),
        "{} mismatches:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    assert!(checked > 1900, "{checked}");
}

#[test]
fn common_forms_read_as_expected() {
    let table: &[(&str, &str)] = &[
        // Hebrew.
        ("HVqp3ms", "Qal perfect, 3rd masc. sing."),
        (
            "HC/Vqw3ms",
            "conjunction + Qal consecutive imperfect, 3rd masc. sing.",
        ),
        (
            "Hc/Vqw3ms",
            "consecutive conjunction + Qal consecutive imperfect, 3rd masc. sing.",
        ),
        ("HNcmpa", "noun, masc. pl. abs."),
        ("HR/Ncfsa", "preposition + noun, fem. sing. abs."),
        ("HTd/Ncmpa", "definite article + noun, masc. pl. abs."),
        ("HNpm", "proper noun, masc. (person's name)"),
        ("HNpl", "proper noun (place name)"),
        ("HTo", "object marker particle"),
        ("HVNi3ms", "Niphal imperfect, 3rd masc. sing."),
        (
            "HVhv2ms/Sp1cs",
            "Hiphil imperative, 2nd masc. sing. + pronominal suffix, 1st com. sing.",
        ),
        ("HVqrmsa", "Qal participle, masc. sing. abs."),
        ("HVqc", "Qal infinitive construct"),
        ("HVqa", "Qal infinitive absolute"),
        ("HVqj3ms", "Qal imperfect jussive, 3rd masc. sing."),
        ("HVqc1cs", "Qal imperfect cohortative, 1st com. sing."),
        ("HAcmsa", "cardinal number adjective, masc. sing. abs."),
        ("AVqp3ms", "Peal perfect, 3rd masc. sing."),
        ("ANcmsd", "noun, masc. sing. det."),
        // Greek.
        ("V-AAI-3S", "aorist active indicative, 3rd sing."),
        ("V-2AAI-3S", "2nd aorist active indicative, 3rd sing."),
        ("V-PAP-NSM", "present active participle, nom. masc. sing."),
        ("V-PAN", "present active infinitive"),
        ("V-RPP-ASN", "perfect passive participle, acc. neut. sing."),
        (
            "V-PNI-3S",
            "present middle or passive deponent indicative, 3rd sing.",
        ),
        ("V-FXI-1S", "future indicative, 1st sing."),
        ("N-GSF", "noun, gen. fem. sing."),
        ("N-NSM-P", "personal name noun, nom. masc. sing."),
        ("N-PRI", "indeclinable proper noun"),
        ("A-ASM-C", "adjective, acc. masc. sing. (comparative)"),
        ("T-NSM", "definite article, nom. masc. sing."),
        ("P-1NS", "personal pronoun, 1st nom. sing."),
        (
            "P-1NS-K",
            "personal pronoun, 1st nom. sing. (with καί joined (crasis))",
        ),
        (
            "S-1SNSM",
            "possessive pronoun, 1st nom. masc. sing. (one possessor)",
        ),
        ("F-3ASM", "reflexive pronoun, 3rd acc. masc. sing."),
        ("CONJ", "conjunction"),
        ("PRT-N", "negative particle"),
        ("CONJ + G1437", "conjunction + G1437"),
    ];
    for (code, want) in table {
        assert_eq!(parse(code).unwrap().short(), *want, "{code}");
    }
    assert_eq!(
        parse("HVqp3ms").unwrap().long(),
        "Verb, Qal perfect, third person masculine singular"
    );
    assert_eq!(
        parse("V-PAP-DPF").unwrap().long(),
        "Verb, present active participle, dative feminine plural"
    );
    assert_eq!(
        parse("HC/Ncmsc").unwrap().long(),
        "Conjunction; then Noun, masculine singular construct"
    );
}

#[test]
fn part_of_speech_finds_the_main_word() {
    let pos = |c: &str| parse(c).unwrap().pos().unwrap();
    assert_eq!(pos("HC/Vqw3ms"), PartOfSpeech::Verb);
    assert_eq!(pos("HR/Sp3ms"), PartOfSpeech::Preposition);
    assert_eq!(pos("HTd/Ncmpa"), PartOfSpeech::Noun);
    assert_eq!(pos("HVqp3ms/Sp3ms"), PartOfSpeech::Verb);
    assert_eq!(pos("T-NSM"), PartOfSpeech::Article);
    assert!(parse("HR/Sp3ms").unwrap().is_function_word());
    assert!(parse("HTo").unwrap().is_function_word());
    assert!(parse("CONJ").unwrap().is_function_word());
    assert!(!parse("HC/Vqw3ms").unwrap().is_function_word());
    assert!(!parse("N-GSF").unwrap().is_function_word());
    let m = parse("V-PAP-NSM").unwrap();
    let p = m.main_part().unwrap();
    assert_eq!(
        (p.case, p.number, p.gender),
        (
            Some(Case::Nominative),
            Some(Number::Singular),
            Some(Gender::Masculine)
        )
    );
}

#[test]
fn bad_codes_are_errors_not_panics() {
    for bad in [
        "",
        "HX",
        "HVqz3ms",
        "HNcmsq",
        "V-ZAI-1S",
        "V-AAI",
        "N-QSM",
        "S-1SN",
        "Q",
        "N-GSF-ZZZ",
        "H/",
        "V-AAI-9S",
    ] {
        assert!(parse(bad).is_err(), "{bad:?} parsed");
    }
    // Every prefix of every real code either parses or errors.
    for r in rows(CODES) {
        let code = r[1];
        for i in 0..code.len() {
            if code.is_char_boundary(i) {
                let _ = parse(&code[..i]);
            }
        }
    }
}
