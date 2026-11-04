//! STEPBible TAHOT/TAGNT Import against excerpts of the real files, compared with golden JSON
//! snapshots.
//!
//! Regenerate the snapshots with `UPDATE_GOLDEN=1 cargo nextest run -p obs-formats` and review
//! the diff. Full-text checks run when `OBS_TEST_STEPBIBLE_DIR` points at a directory holding
//! the four TAHOT and two TAGNT files from STEPBible-Data (`Translators Amalgamated OT+NT/`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_formats::{ImportReport, StepbibleImport, StepbibleOptions, StepbibleText};
use obs_model::{Direction, MorphemePart, Resource, SpanKind, TextUnit, UnitId};
use obs_scripture::{Book, Passage, Verse};
use serde_json::{Value, json};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/assets/stepbible")
        .join(name)
}

fn import(text: StepbibleText, files: &[PathBuf]) -> (Resource, ImportReport) {
    let id = match text {
        StepbibleText::Tahot => "tahot",
        StepbibleText::Tagnt => "tagnt",
    };
    let mut import = StepbibleImport::new(StepbibleOptions {
        text,
        resource_id: id.into(),
        title: id.to_uppercase(),
        source_file: id.into(),
        tool_version: "test".into(),
        ..StepbibleOptions::default()
    });
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        import.add_file(&name, std::fs::read_to_string(f).unwrap());
    }
    import.finish().unwrap_or_else(|e| panic!("{e}"))
}

/// The excerpts, deliberately not in canonical order: the Import sorts books.
fn tahot() -> (Resource, ImportReport) {
    import(
        StepbibleText::Tahot,
        &[
            fixture("tahot-isa-mal.txt"),
            fixture("tahot-gen-deu.txt"),
            fixture("tahot-job-sng.txt"),
        ],
    )
}

fn tagnt() -> (Resource, ImportReport) {
    import(
        StepbibleText::Tagnt,
        &[fixture("tagnt-act-rev.txt"), fixture("tagnt-mat-jhn.txt")],
    )
}

fn v(book: Book, chapter: u16, verse: u16) -> Verse {
    Verse::new(book, chapter, verse)
}

fn verse_text(r: &Resource, verse: Verse) -> &str {
    let bv = r
        .verses
        .iter()
        .find(|bv| bv.verse == verse)
        .unwrap_or_else(|| panic!("{verse} missing"));
    &r.unit(&bv.unit).unwrap().text[bv.range()]
}

/// The Tagged words of one Verse: (text, parts).
fn tagged_words(r: &Resource, verse: Verse) -> Vec<(String, Vec<MorphemePart>)> {
    let bv = r.verses.iter().find(|bv| bv.verse == verse).unwrap();
    let unit = r.unit(&bv.unit).unwrap();
    unit.spans
        .iter()
        .filter(|s| s.start >= bv.start && s.end <= bv.end)
        .filter_map(|s| match &s.kind {
            SpanKind::TaggedWord { parts } => {
                Some((unit.text[s.range()].to_string(), parts.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Note bodies referenced inside one Verse.
fn notes_in(r: &Resource, verse: Verse) -> Vec<String> {
    let bv = r.verses.iter().find(|bv| bv.verse == verse).unwrap();
    let unit = r.unit(&bv.unit).unwrap();
    unit.spans
        .iter()
        .filter(|s| s.start >= bv.start && s.start <= bv.end)
        .filter_map(|s| match &s.kind {
            SpanKind::NoteRef { note, .. } => Some(r.unit(note).unwrap().text.clone()),
            _ => None,
        })
        .collect()
}

fn part(strongs: &str, lemma: &str, morph: &str, gloss: &str) -> MorphemePart {
    MorphemePart {
        strongs: strongs.parse().unwrap(),
        lemma: Some(nfc(lemma)),
        morph: Some(morph.to_string()),
        gloss: Some(gloss.to_string()),
    }
}

fn nfc(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfc().collect()
}

#[test]
fn tahot_gen_1_1_yields_the_expected_morpheme_parts() {
    let (r, _) = tahot();
    let words = tagged_words(&r, v(Book::Genesis, 1, 1));
    let expected: Vec<(&str, Vec<MorphemePart>)> = vec![
        (
            "בְּרֵאשִׁ֖ית",
            vec![
                part("H9003", "ב", "HR", "in"),
                part("H7225G", "רֵאשִׁית", "HNcfsa", "beginning"),
            ],
        ),
        ("בָּרָ֣א", vec![part("H1254A", "בָּרָא", "HVqp3ms", "he created")]),
        ("אֱלֹהִ֑ים", vec![part("H430G", "אֱלֹהִים", "HNcmpa", "God")]),
        ("אֵ֥ת", vec![part("H853", "אֵת", "HTo", "<obj.>")]),
        (
            "הַשָּׁמַ֖יִם",
            vec![
                part("H9009", "ה", "HTd", "the"),
                part("H8064", "שָׁמַיִם", "HNcmpa", "heavens"),
            ],
        ),
        (
            "וְאֵ֥ת",
            vec![
                part("H9002", "ו", "HC", "and"),
                part("H853", "אֵת", "HTo", "<obj.>"),
            ],
        ),
        (
            "הָאָֽרֶץ",
            vec![
                part("H9009", "ה", "HTd", "the"),
                part("H776G", "אֶ֫רֶץ", "HNcfsa", "earth"),
            ],
        ),
    ];
    assert_eq!(words.len(), expected.len());
    for ((text, parts), (want_text, want_parts)) in words.iter().zip(&expected) {
        assert_eq!(*text, nfc(want_text));
        assert_eq!(parts, want_parts, "{text}");
    }
    // Strong's numbers are normalized: no zero padding, extended letters kept.
    assert_eq!(words[2].1[0].strongs.to_string(), "H430G");
    // The sof pasuq follows the last word, outside its Tagged word.
    assert!(verse_text(&r, v(Book::Genesis, 1, 1)).ends_with("הָאָֽרֶץ׃"));
}

#[test]
fn prefixes_such_as_vav_and_he_are_their_own_parts() {
    let (r, _) = tahot();
    // Gen 1:2 וְ/הָ/אָ֗רֶץ "and the earth": conjunction, article, noun.
    let words = tagged_words(&r, v(Book::Genesis, 1, 2));
    let (text, parts) = &words[0];
    assert_eq!(*text, nfc("וְהָאָ֗רֶץ"));
    let summary: Vec<(String, Option<String>, Option<&str>)> = parts
        .iter()
        .map(|p| (p.strongs.to_string(), p.lemma.clone(), p.morph.as_deref()))
        .collect();
    assert_eq!(
        summary,
        [
            ("H9002".to_string(), Some(nfc("ו")), Some("HC")),
            ("H9009".to_string(), Some(nfc("ה")), Some("HTd")),
            ("H776G".to_string(), Some(nfc("אֶ֫רֶץ")), Some("HNcfsa")),
        ]
    );
    // A pronominal suffix is a part too: Gen 9:21 אָהֳלֽ/וֹ "his tent".
    let words = tagged_words(&r, v(Book::Genesis, 9, 21));
    let suffixed = words
        .iter()
        .find(|(_, p)| p.iter().any(|p| p.strongs.to_string() == "H9023"))
        .unwrap();
    assert_eq!(suffixed.1.len(), 2);
    assert_eq!(suffixed.1[1].morph.as_deref(), Some("HSp3ms"));
    assert_eq!(suffixed.1[1].lemma, None, "a suffix has no Lemma");
    // A maqaf of its own between two Qere words joins them; it is not a part.
    assert!(verse_text(&r, v(Book::Exodus, 4, 2)).contains(&nfc("מַה־זֶּ֣ה")));
    let words = tagged_words(&r, v(Book::Exodus, 4, 2));
    assert!(words.iter().any(|(t, p)| *t == nfc("מַה") && p.len() == 1));
    // Aramaic parts carry the Aramaic language letter: Dan 2:4 מַלְכָּ/א is `ANcbsd` + `ATa`.
    let words = tagged_words(&r, v(Book::Daniel, 2, 4));
    assert!(
        words
            .iter()
            .flat_map(|(_, p)| p)
            .any(|p| p.morph.as_deref().is_some_and(|m| m.starts_with('A')))
    );
    // Every morphology code of every part expands.
    for unit in &r.units {
        for span in &unit.spans {
            if let SpanKind::TaggedWord { parts } = &span.kind {
                for p in parts {
                    let code = p.morph.as_deref().unwrap();
                    obs_model::morph::parse(code).unwrap_or_else(|e| panic!("{code}: {e}"));
                }
            }
        }
    }
}

#[test]
fn every_tahot_and_tagnt_verse_is_mapped_or_explicitly_unmapped() {
    for (r, report) in [tahot(), tagnt()] {
        let map = r.verse_map.as_ref().expect("a Verse map");
        assert_eq!(map.entries().len(), r.verses.len());
        for (entry, bv) in map.entries().iter().zip(&r.verses) {
            assert_eq!(entry.verse, bv.verse);
        }
        let counts = report.verse_map.unwrap();
        assert_eq!(counts.unmapped, map.unmapped().count());
        let unmapped: Vec<String> = map.unmapped().map(|e| e.verse.to_string()).collect();
        assert!(
            unmapped.is_empty(),
            "{}: {unmapped:?}",
            r.manifest.resource_id
        );
    }

    // TAHOT keeps the Hebrew numbering; the Verse map takes it to the Standard.
    let (r, _) = tahot();
    let map = r.verse_map.as_ref().unwrap();
    let to = |verse: Verse| map.to_standard(verse).map(Passage::start);
    assert_eq!(to(v(Book::Genesis, 32, 1)), Some(v(Book::Genesis, 31, 55)));
    assert_eq!(to(v(Book::Genesis, 32, 2)), Some(v(Book::Genesis, 32, 1)));
    assert_eq!(to(v(Book::Malachi, 3, 19)), Some(v(Book::Malachi, 4, 1)));
    assert_eq!(to(v(Book::Psalms, 3, 1)), Some(v(Book::Psalms, 3, 0)));
    assert_eq!(to(v(Book::Psalms, 3, 2)), Some(v(Book::Psalms, 3, 1)));
    assert_eq!(to(v(Book::Psalms, 51, 2)), Some(v(Book::Psalms, 51, 0)));
    // Hebrew Ps 23:1 holds the title and the first English verse: it covers both, so the
    // title slot is not Absent.
    assert_eq!(
        map.to_standard(v(Book::Psalms, 23, 1)),
        Passage::new(v(Book::Psalms, 23, 0), v(Book::Psalms, 23, 1)).ok()
    );
    assert!(!map.is_absent(v(Book::Psalms, 23, 0)));
    assert_eq!(to(v(Book::Genesis, 1, 1)), Some(v(Book::Genesis, 1, 1)));

    // TAGNT keeps the NA numbering: NRSV Acts 13:39's first words are NA 13:38.
    let (r, _) = tagnt();
    let map = r.verse_map.as_ref().unwrap();
    assert!(map.to_standard(v(Book::Acts, 13, 38)).is_some());
    assert_eq!(
        map.to_standard(v(Book::Revelation, 12, 18))
            .map(Passage::start),
        Some(v(Book::Revelation, 13, 1))
    );
}

#[test]
fn tahot_manifest_text_paragraphs_and_notes() {
    let (r, report) = tahot();
    let m = &r.manifest;
    assert_eq!(m.direction, Direction::Rtl);
    assert_eq!(m.language, "hbo");
    assert_eq!(m.source_format, "stepbible-tahot");
    let codes: Vec<&str> = report.books.iter().map(|b| b.code.as_str()).collect();
    assert_eq!(
        codes,
        ["GEN", "EXO", "PSA", "ISA", "DAN", "MAL"],
        "canonical order"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    // Maqaf joins words; petuha closes the paragraph after Gen 1:5.
    let genesis = r.unit(&UnitId::book(Book::Genesis)).unwrap();
    assert!(verse_text(&r, v(Book::Genesis, 1, 2)).contains(&nfc("עַל־פְּנֵ֣י")));
    assert!(verse_text(&r, v(Book::Genesis, 1, 5)).ends_with("׃ פ"));
    let paras: Vec<&str> = paragraphs(genesis);
    assert!(paras[0].ends_with("׃ פ"), "{}", paras[0]);

    // A Qere of two words for one Ketiv gives two Tagged words; the Ketiv is a note.
    let words = tagged_words(&r, v(Book::Genesis, 30, 11));
    assert!(words.iter().any(|(t, _)| *t == nfc("בָּ֣א")));
    assert!(words.iter().any(|(t, _)| *t == nfc("גָ֑ד")));
    assert!(
        notes_in(&r, v(Book::Genesis, 30, 11))
            .iter()
            .any(|n| n.starts_with("Ketiv (written text): ")),
    );
    // Lower-case ketiv from the spelling column, words from the Septuagint, a Ketiv without
    // a Qere.
    assert!(
        notes_in(&r, v(Book::Genesis, 8, 17))
            .iter()
            .any(|n| *n == nfc("Ketiv (written text): הוֹצֵא"))
    );
    let lxx = notes_in(&r, v(Book::Genesis, 4, 8));
    assert_eq!(
        lxx.iter().filter(|n| n.contains("Septuagint")).count(),
        1,
        "one note for the run: {lxx:?}"
    );
    assert!(
        notes_in(&r, v(Book::Isaiah, 44, 24))
            .iter()
            .any(|n| n.starts_with("Ketiv (written text): "))
    );

    // The Psalm 3 title (English verse 0) is a psalm_title span, Hebrew verse 1.
    let psa = r.unit(&UnitId::book(Book::Psalms)).unwrap();
    let title = psa
        .spans
        .iter()
        .find(|s| s.kind == SpanKind::PsalmTitle)
        .unwrap();
    assert_eq!(
        &psa.text[title.range()],
        verse_text(&r, v(Book::Psalms, 3, 1))
    );
}

#[test]
fn tagnt_follows_na27_and_notes_other_editions() {
    let (r, report) = tagnt();
    let m = &r.manifest;
    assert_eq!(m.direction, Direction::Ltr);
    assert_eq!(m.language, "grc");
    assert_eq!(m.source_format, "stepbible-tagnt");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let codes: Vec<&str> = report.books.iter().map(|b| b.code.as_str()).collect();
    assert_eq!(codes, ["MAT", "MRK", "JHN", "ACT", "REV"]);

    // Mat 1:1: one part per word, Gloss from the English rendering, punctuation outside.
    let words = tagged_words(&r, v(Book::Matthew, 1, 1));
    assert_eq!(words.len(), 8);
    assert_eq!(words[0].0, "Βίβλος");
    assert_eq!(words[0].1, [part("G976", "βίβλος", "N-NSF", "[The] book")]);
    assert_eq!(words[7].0, "Ἀβραάμ");
    assert_eq!(
        verse_text(&r, v(Book::Matthew, 1, 1)),
        "Βίβλος γενέσεως Ἰησοῦ Χριστοῦ υἱοῦ Δαυὶδ υἱοῦ Ἀβραάμ."
    );

    // The doxology of Mat 6:13 is not in NA27: one note, not text.
    assert!(!verse_text(&r, v(Book::Matthew, 6, 13)).contains("δόξα"));
    let notes = notes_in(&r, v(Book::Matthew, 6, 13));
    assert!(
        notes
            .iter()
            .any(|n| n.starts_with("Not in NA27. TR, Byz add: ὅτι σοῦ")),
        "{notes:?}"
    );
    // Mat 17:21 is an Absent verse here, with a note where it would be.
    assert!(
        !r.verses
            .iter()
            .any(|bv| bv.verse == v(Book::Matthew, 17, 21))
    );
    let mat = r.unit(&UnitId::book(Book::Matthew)).unwrap();
    assert!(mat.spans.iter().any(|s| matches!(&s.kind,
        SpanKind::NoteRef { note, .. } if r.unit(note).unwrap().text.starts_with("Verse 17:21 is not in NA27."))));
    assert!(
        r.verse_map
            .as_ref()
            .unwrap()
            .is_absent(v(Book::Matthew, 17, 21))
    );

    // NA27's double-bracketed passages are text.
    assert!(verse_text(&r, v(Book::Mark, 16, 9)).starts_with("[[Ἀναστὰς"));
    assert!(verse_text(&r, v(Book::Mark, 1, 13)).contains("τεσσεράκοντα ἡμέρας"));
    // Where TAGNT gives NA27's word only as a variant of another edition's, the NA27 word is
    // the text and the other is the note.
    assert_eq!(
        verse_text(&r, v(Book::John, 7, 53)),
        "[[Καὶ ἐπορεύθησαν ἕκαστος εἰς τὸν οἶκον αὐτοῦ,"
    );
    assert!(
        notes_in(&r, v(Book::John, 7, 53))
            .iter()
            .any(|n| n == "Variant: Tyn, TR, Byz: ἐπορεύθη “he went”")
    );
    // Meaning variants are notes.
    assert!(
        notes_in(&r, v(Book::Acts, 1, 6))
            .iter()
            .any(|n| n.starts_with("Variant: "))
    );
}

fn paragraphs(unit: &TextUnit) -> Vec<&str> {
    unit.spans
        .iter()
        .filter(|s| matches!(s.kind, SpanKind::Para { .. }))
        .map(|s| &unit.text[s.range()])
        .collect()
}

fn snapshot(resource: &Resource, report: &ImportReport) -> Value {
    let units: Vec<Value> = resource
        .units
        .iter()
        .map(|u| {
            let spans: Vec<Value> = u
                .spans
                .iter()
                .map(|s| {
                    let mut v = serde_json::to_value(&s.kind).unwrap();
                    v["at"] = json!(format!("{}..{}", s.start, s.end));
                    v["text"] = json!(&u.text[s.range()]);
                    v
                })
                .collect();
            json!({ "id": u.id, "kind": u.kind, "text": u.text, "spans": spans })
        })
        .collect();
    let map = resource.verse_map.as_ref().unwrap();
    let verses: Vec<Value> = resource
        .verses
        .iter()
        .zip(map.entries())
        .map(|(bv, e)| {
            json!({
                "verse": bv.verse.to_string(),
                "standard": e.standard.map(|p| p.to_string()),
            })
        })
        .collect();
    json!({
        "report": {
            "books": report.books.iter().map(|b| json!([b.code, b.chapters, b.verses])).collect::<Vec<_>>(),
            "verses": report.verses,
            "notes": report.notes,
            "tagged_words": report.tagged_words,
            "warnings": report.warnings,
            "verse_map": report.verse_map.map(|c| json!({ "unmapped": c.unmapped, "absent": c.absent })),
            "traditions": report.traditions,
        },
        "content_hash": resource.manifest.content_hash,
        "verses": verses,
        "units": units,
    })
}

fn check_golden(name: &str, actual: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("stepbible-{name}.json"));
    let text = serde_json::to_string_pretty(actual).unwrap() + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        expected == text,
        "golden snapshot {} differs; rerun with UPDATE_GOLDEN=1 and review the diff",
        path.display()
    );
}

#[test]
fn tahot_excerpt_golden() {
    let (r, report) = tahot();
    check_golden("tahot", &snapshot(&r, &report));
}

#[test]
fn tagnt_excerpt_golden() {
    let (r, report) = tagnt();
    check_golden("tagnt", &snapshot(&r, &report));
}

/// The whole of TAHOT and TAGNT, when `OBS_TEST_STEPBIBLE_DIR` is set.
#[test]
fn full_tahot_and_tagnt() {
    let Some(dir) = std::env::var_os("OBS_TEST_STEPBIBLE_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let files = |prefix: &str| -> Vec<PathBuf> {
        let mut f: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix))
            .collect();
        f.sort();
        f
    };
    for (text, prefix, books) in [
        (StepbibleText::Tagnt, "TAGNT", 27),
        (StepbibleText::Tahot, "TAHOT", 39),
    ] {
        let (r, report) = import(text, &files(prefix));
        eprintln!("{report}");
        assert_eq!(report.books.len(), books);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let map = r.verse_map.as_ref().unwrap();
        assert_eq!(map.entries().len(), r.verses.len());
        let absent: Vec<String> = map.absent().iter().map(Verse::to_string).collect();
        eprintln!("absent {absent:?}");
        // Every Hebrew Psalm title reaches its Standard title slot; Ps 119 has none in Hebrew.
        let titles: Vec<&String> = absent.iter().filter(|v| v.ends_with(":title")).collect();
        assert!(
            titles.iter().all(|v| *v == "Ps 119:title"),
            "{prefix}: {titles:?}"
        );
        assert_eq!(map.unmapped().count(), 0, "{prefix}");
        let mut bad = std::collections::BTreeSet::new();
        for unit in &r.units {
            for span in &unit.spans {
                if let SpanKind::TaggedWord { parts } = &span.kind {
                    for p in parts {
                        let code = p.morph.as_deref().unwrap_or("");
                        if obs_model::morph::parse(code).is_err() {
                            bad.insert(code.to_string());
                        }
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "{prefix}: codes that do not expand: {bad:?}"
        );
    }
}
