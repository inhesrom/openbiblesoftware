//! Dataset converters against excerpts of the real sources: OpenBible.info Cross-references
//! and STEPBible TIPNR (both CC BY 4.0).
//!
//! The full sources are checked too when `OBS_TEST_OPENBIBLE_XREFS` points at
//! `cross_references.txt` and `OBS_TEST_TIPNR` at the TIPNR `.txt`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_formats::{
    DROPPED_NOT_STANDARD, DatasetError, DatasetOptions, ImportReport,
    import_openbible_cross_references, import_tipnr, tipnr_description_texts,
};
use obs_model::{EntityKind, Relation, Resource, ResourceType};
use obs_scripture::{Book, Passage, Standard, Verse, Versification};

fn fixture(path: &str) -> String {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/assets")
        .join(path);
    std::fs::read_to_string(p).unwrap()
}

fn options(id: &str) -> DatasetOptions {
    DatasetOptions {
        resource_id: id.into(),
        title: id.into(),
        source_file: format!("{id}.txt"),
        tool_version: "test".into(),
        ..DatasetOptions::default()
    }
}

fn passage(book: Book, chapter: u16, start: u16, end: u16) -> Passage {
    Passage::new(
        Verse::new(book, chapter, start),
        Verse::new(book, chapter, end),
    )
    .unwrap()
}

/// The acceptance check: every Cross-reference source and target is a Standard reference.
fn assert_all_standard(r: &Resource) {
    for x in &r.cross_references {
        for p in [x.from, x.to] {
            assert!(
                Standard.contains(p.start()) && Standard.contains(p.end()),
                "{} → {} is not in the Standard versification",
                x.from,
                x.to
            );
        }
    }
}

fn xrefs(text: &str) -> (Resource, ImportReport) {
    import_openbible_cross_references(text, &options("openbible-xrefs")).unwrap()
}

#[test]
fn openbible_rows_become_cross_references_in_the_standard_versification() {
    let (r, report) = xrefs(&fixture("openbible/cross-references-excerpt.txt"));
    assert_eq!(r.manifest.resource_type, ResourceType::CrossReferenceSet);
    assert_eq!(r.manifest.source_format, "openbible-cross-references");
    assert!(r.units.is_empty() && r.verses.is_empty());
    assert_eq!(r.cross_references.len(), 23, "every row is kept");
    assert_eq!(report.cross_references, 23);
    assert!(report.warnings.is_empty(), "{report}");
    assert_all_standard(&r);

    let gen_1_1 = Passage::verse(Verse::new(Book::Genesis, 1, 1));
    let row = r
        .cross_references
        .iter()
        .find(|x| x.from == gen_1_1 && x.to == passage(Book::Romans, 1, 19, 20))
        .unwrap();
    assert_eq!(row.votes, 59);
    let eph = Passage::verse(Verse::new(Book::Ephesians, 6, 17));
    assert_eq!(
        r.cross_references
            .iter()
            .find(|x| x.from == eph)
            .unwrap()
            .votes,
        -86,
        "negative votes are kept"
    );
    // The ESV's 3 John 1:15 is the Standard's 3 John 1:14.
    let jn3_14 = Verse::new(Book::ThirdJohn, 1, 14);
    assert!(
        r.cross_references
            .iter()
            .any(|x| x.from == Passage::verse(jn3_14) && x.votes == 2)
    );
    assert!(
        r.cross_references
            .iter()
            .any(|x| x.to == Passage::new(Verse::new(Book::SecondJohn, 1, 13), jn3_14).unwrap())
    );
}

#[test]
fn rows_outside_the_standard_versification_are_dropped_and_reported() {
    let mut text = fixture("openbible/cross-references-excerpt.txt");
    text.push_str("Gen.1.1\tGen.1.32\t5\nPs.3.9\tJude.1.26\t1\n");
    let (r, report) = xrefs(&text);
    assert_eq!(r.cross_references.len(), 23);
    assert_eq!(r.dropped_markup.get(DROPPED_NOT_STANDARD), Some(&2));
    assert_eq!(report.warnings.len(), 2, "{report}");
    assert!(report.warnings[0].contains("Gen 1:32"), "{report}");
    assert!(report.to_string().contains("23 cross-reference(s)"));
    assert_all_standard(&r);
}

#[test]
fn malformed_openbible_rows_fail_with_their_line() {
    let text = "From Verse\tTo Verse\tVotes\nGen.1.1\tRev.14.7\t82\nGen.1.1\tNowhere.1.1\t3\n";
    match import_openbible_cross_references(text, &options("x")) {
        Err(DatasetError::Syntax { line: 3, message }) => {
            assert!(message.contains("Nowhere.1.1"), "{message}");
        }
        other => panic!("expected a syntax error, got {other:?}"),
    }
    assert!(matches!(
        import_openbible_cross_references("From Verse\tTo Verse\tVotes\n", &options("x")),
        Err(DatasetError::NoRecords)
    ));
}

fn tipnr(text: &str) -> (Resource, ImportReport) {
    import_tipnr(text, &options("tipnr")).unwrap()
}

#[test]
fn tipnr_people_and_places_keep_only_structured_fields() {
    let (r, report) = tipnr(&fixture("tipnr/tipnr-excerpt.txt"));
    assert_eq!(r.manifest.resource_type, ResourceType::PeopleAndPlaces);
    assert!(r.units.is_empty());
    let names: Vec<&str> = r.entities.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Abana",
            "Abda",
            "Abel",
            "Abraham",
            "Ashkenaz",
            "Beth-gader",
            "Eran",
            "Jahdai",
            "Joshua",
            "Sarah",
            "Terah",
            "Unnamed#1"
        ],
        "OTHER and EXCLUDED OTHER records are not people or places"
    );
    assert_eq!(report.entities, 12);
    let get = |id: &str| r.entities.iter().find(|e| e.id == id).unwrap();

    let abraham = get("Abraham@Gen.11.26-1Pe");
    assert_eq!(abraham.kind, EntityKind::Male);
    assert_eq!(abraham.strongs, Some("H85".parse().unwrap()));
    assert_eq!(abraham.other_names, ["Abram"]);
    assert_eq!(abraham.region.as_deref(), Some("Early Patriarch"));
    let rel = |e: &obs_model::Entity, relation| -> Vec<String> {
        e.relations
            .iter()
            .filter(|r| r.relation == relation)
            .map(|r| r.entity.clone())
            .collect()
    };
    assert_eq!(rel(abraham, Relation::Father), ["Terah@Gen.11.24-Luk"]);
    assert_eq!(rel(abraham, Relation::Partner), ["Sarah@Gen.11.29-1Pe"]);
    assert!(
        rel(abraham, Relation::Offspring).is_empty(),
        "relations to entities outside the excerpt are dropped"
    );
    assert_eq!(get("Sarah@Gen.11.29-1Pe").kind, EntityKind::Female);
    assert_eq!(
        rel(get("Sarah@Gen.11.29-1Pe"), Relation::Partner),
        ["Abraham@Gen.11.26-1Pe"]
    );
    let abana = get("Abana@2Ki.5.12");
    assert_eq!(abana.kind, EntityKind::Place);
    assert_eq!(abana.other_names, ["Amana"]);
    assert_eq!(
        get("Unnamed#1@1Ki.2.27").strongs,
        None,
        "H0000 is no number"
    );
    assert_eq!(
        get("Joshua@Exo.17.9-Heb").region.as_deref(),
        Some("Tribe of Ephraim")
    );

    // Mentions: one per Verse, in the Standard versification.
    let mentions = |id: &str| -> Vec<Passage> {
        r.mentions
            .iter()
            .filter(|m| m.entity == id)
            .map(|m| m.passage)
            .collect()
    };
    let v = |b, c, n| Passage::verse(Verse::new(b, c, n));
    assert!(mentions("Abraham@Gen.11.26-1Pe").contains(&v(Book::Genesis, 12, 1)));
    let joshua = mentions("Joshua@Exo.17.9-Heb");
    assert!(
        joshua.contains(&v(Book::Numbers, 13, 8)) && joshua.contains(&v(Book::Numbers, 13, 16))
    );
    assert_eq!(
        mentions("Eran@Num.26.36"),
        [v(Book::Numbers, 26, 36)],
        "LXX-only references are dropped"
    );
    for m in &r.mentions {
        assert!(Standard.contains(m.passage.start()), "{}", m.passage);
    }
    assert_eq!(report.mentions, r.mentions.len());
    for (what, n) in [
        ("record: OTHER (not a person or place)", 2),
        ("mention: only in the Septuagint", 1),
        ("relation: free text, not a unique name", 1),
        ("description: @Article", 12),
        ("description: Description column", 9),
    ] {
        assert_eq!(r.dropped_markup.get(what), Some(&n), "{what}: {report}");
    }
}

#[test]
fn malformed_tipnr_strongs_numbers_warn_without_panicking() {
    let text = fixture("tipnr/tipnr-excerpt.txt");
    for value in ["א", "א000", "X0000", "H", "G", ""] {
        let changed = text.replace("Abda@1Ki.4.6=H5653G", &format!("Abda@1Ki.4.6={value}"));
        assert_ne!(changed, text);
        let (resource, report) = tipnr(&changed);
        let abda = resource
            .entities
            .iter()
            .find(|e| e.id == "Abda@1Ki.4.6")
            .unwrap();
        assert_eq!(abda.strongs, None);
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.contains(&format!(
                    "`Abda@1Ki.4.6` has an unreadable Strong's number `{value}`"
                ))),
            "{report}"
        );
    }
    for value in ["H0000", "G0000"] {
        let changed = text.replace("Abda@1Ki.4.6=H5653G", &format!("Abda@1Ki.4.6={value}"));
        let (resource, report) = tipnr(&changed);
        assert_eq!(
            resource
                .entities
                .iter()
                .find(|e| e.id == "Abda@1Ki.4.6")
                .unwrap()
                .strongs,
            None
        );
        assert!(
            !report
                .warnings
                .iter()
                .any(|warning| warning.contains("unreadable Strong's")),
            "{report}"
        );
    }
}

#[test]
fn no_tipnr_description_text_is_kept() {
    let text = fixture("tipnr/tipnr-excerpt.txt");
    let (r, _) = tipnr(&text);
    // Every string the Resource keeps, joined.
    let mut kept: Vec<&str> = vec![&r.manifest.title, &r.manifest.attribution];
    for e in &r.entities {
        kept.extend([e.id.as_str(), e.name.as_str()]);
        kept.extend(e.other_names.iter().map(String::as_str));
        kept.extend(e.region.as_deref());
        kept.extend(e.relations.iter().map(|r| r.entity.as_str()));
    }
    kept.extend(r.dropped_markup.keys().map(String::as_str));
    let kept = kept.join("\n");
    let descriptions = tipnr_description_texts(&text).unwrap();
    for column in [
        "Description",
        "#Summary description",
        "@Brief",
        "@Short",
        "@Article",
    ] {
        let texts = &descriptions[column];
        assert!(!texts.is_empty(), "{column}");
        for t in texts {
            assert!(!kept.contains(t.as_str()), "{column} text kept: {t}");
        }
    }
    assert!(
        descriptions["@Article"]
            .iter()
            .any(|t| t.starts_with("Abraham, initially named Abram"))
    );
}

#[test]
fn crlf_sources_convert_the_same() {
    let text = fixture("tipnr/tipnr-excerpt.txt");
    let (lf, _) = tipnr(&text);
    let (crlf, _) = tipnr(&text.replace('\n', "\r\n"));
    assert_eq!(lf, crlf);
    let text = fixture("openbible/cross-references-excerpt.txt");
    assert_eq!(xrefs(&text).0, xrefs(&text.replace('\n', "\r\n")).0);
}

/// Full-source runs, when the sources are available locally.
#[test]
fn full_sources_convert_when_available() {
    if let Ok(path) = std::env::var("OBS_TEST_OPENBIBLE_XREFS") {
        let (r, report) = xrefs(&std::fs::read_to_string(path).unwrap());
        assert!(r.cross_references.len() > 340_000, "{report}");
        assert_all_standard(&r);
        eprintln!("{report}");
    }
    if let Ok(path) = std::env::var("OBS_TEST_TIPNR") {
        let text = std::fs::read_to_string(path).unwrap();
        // The converter itself fails if a record's description prose reaches its entity.
        let (r, report) = tipnr(&text);
        assert!(r.entities.len() > 4_000, "{report}");
        eprintln!("{report}");
    }
}
