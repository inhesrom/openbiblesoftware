use obs_scripture::{Book, Passage, PassageList, StrongsNumber, Verse};

use super::*;
use crate::manifest::{KeyScheme, Manifest, ResourceType};
use crate::unit::{MorphemePart, NoteKind, ParaStyle, TextAccumulator};

const JN_3_16: &str = "For God so loved the world, that he gave his only begotten Son.";

fn manifest() -> Manifest {
    let mut m = Manifest::new(
        "kjv-test",
        "King James Version (test)",
        "KJV",
        ResourceType::Bible,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    m
}

fn jhn() -> UnitId {
    UnitId::book(Book::John)
}

/// A tiny Bible: John with two verses, a paragraph, a Tagged word, a note and a cross-reference.
fn sample(span_order_reversed: bool) -> ResourceBuilder {
    let mut b = ResourceBuilder::new(manifest());
    let mut text = TextAccumulator::new();
    let v16 = text.push(JN_3_16);
    text.push(" ");
    let v17 = text.push("For God sent not his Son into the world to condemn the world.");
    let len = text.pos();
    b.add_unit(
        jhn(),
        UnitKind::BibleBook,
        Some("John".into()),
        text.into_string(),
    );
    b.add_unit(UnitId::note(1), UnitKind::Note, None, "Or, only Son".into());

    let god = 4..7;
    let mut spans = vec![
        (
            0..len,
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        ),
        (0..len, SpanKind::Chapter { chapter: 3 }),
        (
            god.clone(),
            SpanKind::TaggedWord {
                parts: vec![MorphemePart::new("G2316".parse::<StrongsNumber>().unwrap())],
            },
        ),
        (god.clone(), SpanKind::Woj),
        (
            v16.end..v16.end,
            SpanKind::NoteRef {
                note: UnitId::note(1),
                note_kind: NoteKind::Footnote,
            },
        ),
        (
            v16.end..v16.end,
            SpanKind::Xref {
                targets: PassageList::from(vec![Passage::verse(Verse::new(Book::Romans, 5, 8))]),
            },
        ),
    ];
    if span_order_reversed {
        spans.reverse();
    }
    for (range, kind) in spans {
        b.add_span(&jhn(), range, kind);
    }
    b.add_verse(&jhn(), v16, Verse::new(Book::John, 3, 16), 16);
    b.add_verse(&jhn(), v17, Verse::new(Book::John, 3, 17), 17);
    b.add_key(
        Key::Passage(Passage::verse(Verse::new(Book::John, 3, 16))),
        &jhn(),
        Some(0..4),
        false,
    );
    b.count_dropped("\\fig");
    b
}

fn errors(b: ResourceBuilder) -> Vec<ValidationError> {
    match b.build() {
        Ok(_) => panic!("expected validation errors"),
        Err(e) => e.0,
    }
}

#[test]
fn valid_resource_builds_with_canonical_span_order() {
    let r = sample(false).build().unwrap();
    let unit = r.unit(&jhn()).unwrap();
    let kinds: Vec<&str> = unit.spans.iter().map(|s| s.kind.name()).collect();
    assert_eq!(
        kinds,
        [
            "chapter",
            "para",
            "verse",
            "woj",
            "tagged_word",
            "xref",
            "note_ref",
            "verse"
        ]
    );
    assert_eq!(r.verses.len(), 2);
    assert_eq!(r.dropped_markup.get("\\fig"), Some(&1));
    assert!(r.manifest.content_hash.starts_with("sha256:"));
    assert_eq!(r.manifest.content_hash.len(), 7 + 64);
    assert!(r.validate().is_ok());
}

#[test]
fn content_hash_is_stable_and_ignores_insertion_order() {
    let a = sample(false).build().unwrap();
    let b = sample(true).build().unwrap();
    assert_eq!(a.manifest.content_hash, b.manifest.content_hash);
    assert_eq!(a.units, b.units);
    // Golden value: changes only when the model or its serialization changes, which must
    // come with a FORMAT_VERSION bump.
    assert_eq!(
        a.manifest.content_hash,
        "sha256:9ecd79bdd0488f9a7363525d64cf04febbc04f416ea5f740afbef24b95ff657a"
    );
}

#[test]
fn content_hash_ignores_metadata_but_not_text() {
    let base = sample(false).build().unwrap().manifest.content_hash;
    let mut retitled = sample(false);
    retitled.manifest_mut().title = "Another title".into();
    assert_eq!(retitled.build().unwrap().manifest.content_hash, base);

    let mut edited = ResourceBuilder::new(manifest());
    edited.add_unit(
        jhn(),
        UnitKind::BibleBook,
        None,
        JN_3_16.replace("loved", "lovd"),
    );
    let other = edited.build().unwrap().manifest.content_hash;
    assert_ne!(other, base);
}

#[test]
fn span_crossing_its_unit_fails_with_a_clear_error() {
    let mut b = sample(false);
    b.add_span(&UnitId::note(1), 3..400, SpanKind::Italic);
    let errs = errors(b);
    assert_eq!(
        errs,
        [ValidationError::SpanOutsideUnit {
            unit: UnitId::note(1),
            kind: "italic",
            range: 3..400,
            len: 12,
        }]
    );
    assert_eq!(
        errs[0].to_string(),
        "`italic` span 3..400 crosses the end of unit `note.1` (12 bytes)"
    );
}

#[test]
fn span_off_a_utf8_boundary_fails() {
    let mut b = ResourceBuilder::new(manifest());
    let id = UnitId::new("JHN").unwrap();
    b.add_unit(id.clone(), UnitKind::BibleBook, None, "λόγος".into());
    b.add_span(&id, 1..4, SpanKind::Foreign);
    let errs = errors(b);
    assert_eq!(
        errs,
        [ValidationError::SpanNotOnCharBoundary {
            unit: id,
            kind: "foreign",
            offset: 1
        }]
    );
    assert!(errs[0].to_string().contains("inside a UTF-8 character"));
}

#[test]
fn structural_errors_are_all_reported() {
    let mut b = sample(false);
    b.manifest_mut().resource_id = "KJV Test".into();
    b.add_unit(jhn(), UnitKind::BibleBook, None, "dup".into());
    b.add_unit(
        UnitId::new("x").unwrap(),
        UnitKind::Division,
        None,
        "e\u{301}".into(),
    );
    b.add_span(&UnitId::new("nowhere").unwrap(), 0..0, SpanKind::Bold);
    #[allow(clippy::reversed_empty_ranges)]
    b.add_span(&jhn(), 5..2, SpanKind::Bold);
    b.add_span(
        &jhn(),
        1..2,
        SpanKind::NoteRef {
            note: UnitId::note(9),
            note_kind: NoteKind::Footnote,
        },
    );
    b.add_span(
        &jhn(),
        0..1,
        SpanKind::NoteRef {
            note: jhn(),
            note_kind: NoteKind::Footnote,
        },
    );
    b.add_span(&jhn(), 0..3, SpanKind::TaggedWord { parts: vec![] });
    b.add_verse(&jhn(), 0..3, Verse::new(Book::John, 3, 16), 16);
    b.add_verse(&UnitId::note(1), 0..2, Verse::new(Book::John, 3, 20), 19);
    b.add_key(Key::Headword("Love".into()), &jhn(), None, false);
    let errs = errors(b);
    let has = |pred: &dyn Fn(&ValidationError) -> bool| errs.iter().any(pred);
    assert!(has(&|e| matches!(e, ValidationError::InvalidResourceId(_))));
    assert!(has(&|e| matches!(e, ValidationError::DuplicateUnit(_))));
    assert!(has(&|e| matches!(e, ValidationError::TextNotNfc(_))));
    assert!(has(&|e| matches!(
        e,
        ValidationError::UnknownUnit { what: "bold", .. }
    )));
    assert!(has(&|e| matches!(e, ValidationError::SpanReversed { .. })));
    assert!(has(&|e| matches!(
        e,
        ValidationError::NoteRefNotZeroWidth { .. }
    )));
    assert!(has(&|e| matches!(
        e,
        ValidationError::UnknownUnit {
            what: "note_ref",
            ..
        }
    )));
    assert!(has(&|e| matches!(
        e,
        ValidationError::NoteTargetNotANote { .. }
    )));
    assert!(has(&|e| matches!(
        e,
        ValidationError::TaggedWordWithoutParts { .. }
    )));
    assert!(has(&|e| matches!(e, ValidationError::DuplicateVerse(_))));
    assert!(has(&|e| matches!(e, ValidationError::VerseOutOfOrder(_))));
    assert!(has(&|e| matches!(
        e,
        ValidationError::VerseInWrongUnit { .. }
    )));
    assert!(has(&|e| matches!(
        e,
        ValidationError::VerseRangeReversed(_)
    )));
    assert!(has(&|e| matches!(
        e,
        ValidationError::KeySchemeNotDeclared(_)
    )));
    let report = ValidationErrors(errs.clone()).to_string();
    assert!(report.starts_with(&format!("{} validation error(s)", errs.len())));
}

#[test]
fn verses_only_in_bibles_and_spans_must_match_the_list() {
    let mut m = manifest();
    m.resource_type = ResourceType::Commentary;
    let mut b = ResourceBuilder::new(m);
    b.add_unit(jhn(), UnitKind::Section, None, "text".into());
    b.add_verse(&jhn(), 0..4, Verse::new(Book::John, 1, 1), 1);
    assert!(errors(b).contains(&ValidationError::VersesInNonBible));

    let mut b = ResourceBuilder::new(manifest());
    b.add_unit(jhn(), UnitKind::BibleBook, None, "text".into());
    b.add_span(
        &jhn(),
        0..4,
        SpanKind::Verse {
            verse: Verse::new(Book::John, 1, 1),
            last: 1,
        },
    );
    assert_eq!(
        errors(b),
        [ValidationError::VerseSpanMismatch {
            verse: Verse::new(Book::John, 1, 1),
            unit: jhn()
        }]
    );
}

#[test]
fn keys_are_sorted_and_deduplicated() {
    let mut m = manifest();
    m.resource_type = ResourceType::Dictionary;
    m.key_schemes = vec![KeyScheme::Headword];
    let mut b = ResourceBuilder::new(m);
    let id = UnitId::new("Abraham").unwrap();
    b.add_unit(
        id.clone(),
        UnitKind::Entry,
        Some("Abraham".into()),
        "Father of a multitude.".into(),
    );
    b.add_key(Key::Headword("Abram".into()), &id, None, true);
    b.add_key(Key::Headword("Abraham".into()), &id, None, false);
    b.add_key(Key::Headword("Abram".into()), &id, None, true);
    let r = b.build().unwrap();
    let keys: Vec<String> = r.keys.iter().map(|k| k.key.to_string()).collect();
    assert_eq!(keys, ["Abraham", "Abram"]);
    assert!(r.keys[1].alias);
}

#[test]
fn span_kinds_serialize_with_stable_names() {
    let kind = SpanKind::TaggedWord {
        parts: vec![MorphemePart {
            strongs: "H1254A".parse().unwrap(),
            lemma: Some("בָּרָא".into()),
            morph: Some("HVqp3ms".into()),
            gloss: Some("to create".into()),
        }],
    };
    let json = serde_json::to_string(&kind).unwrap();
    assert_eq!(
        json,
        r#"{"kind":"tagged_word","parts":[{"strongs":"H1254A","lemma":"בָּרָא","morph":"HVqp3ms","gloss":"to create"}]}"#
    );
    assert_eq!(serde_json::from_str::<SpanKind>(&json).unwrap(), kind);
    for k in [SpanKind::Woj, SpanKind::DivineName, SpanKind::PsalmTitle] {
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(json, format!(r#"{{"kind":"{}"}}"#, k.name()));
    }
}

#[test]
fn unit_aliases_must_point_at_live_units() {
    let mut b = sample(false);
    b.add_unit_alias(UnitId::new("JOHN").unwrap(), jhn());
    let r = b.build().unwrap();
    assert_eq!(
        r.unit_aliases.get(&UnitId::new("JOHN").unwrap()),
        Some(&jhn())
    );

    let mut b = sample(false);
    b.add_unit_alias(jhn(), UnitId::new("gone").unwrap());
    let errs = errors(b);
    assert!(errs.contains(&ValidationError::AliasShadowsUnit(jhn())));
    assert!(errs.iter().any(|e| matches!(
        e,
        ValidationError::UnknownUnit {
            what: "unit_alias",
            ..
        }
    )));
}
