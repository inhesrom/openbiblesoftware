use std::path::PathBuf;

use obs_model::{Key, KeyScheme, SpanKind, UnitId};
use obs_scripture::{Book, Passage, Verse};
use rusqlite::Connection;

use super::*;
use crate::sample::sample_resource;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "obs-store-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn round_trips_every_part_of_a_resource() {
    let dir = TempDir::new("roundtrip");
    let path = dir.path("kjv.obr");
    let resource = sample_resource();
    write_obr(&resource, &path).unwrap();
    assert!(!dir.path("kjv.obr.partial").exists());

    let reader = ObrReader::open(&path).unwrap();
    assert_eq!(reader.manifest(), &resource.manifest);
    let loaded = reader.load().unwrap();
    assert_eq!(loaded, resource);
    assert_eq!(loaded.dropped_markup.get("\\fig"), Some(&2));
    assert_eq!(
        reader.summary().unwrap(),
        Summary {
            units: 3,
            spans: 15,
            verses: 4,
            keys: 1,
            unit_aliases: 1,
            cross_references: 0,
            entities: 0,
            mentions: 0,
        }
    );
}

#[test]
fn two_writes_are_byte_identical() {
    let dir = TempDir::new("determinism");
    let (a, b) = (dir.path("a.obr"), dir.path("b.obr"));
    write_obr(&sample_resource(), &a).unwrap();
    write_obr(&sample_resource(), &b).unwrap();
    let (a, b) = (std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
    assert!(a.len() > 4096);
    assert!(a == b, "two writes of the same resource differ");
    // Overwriting an existing file gives the same bytes again.
    let c = dir.path("a.obr");
    write_obr(&sample_resource(), &c).unwrap();
    assert!(std::fs::read(c).unwrap() == a);
}

#[test]
fn queries_find_units_spans_verses_keys_and_aliases() {
    let dir = TempDir::new("queries");
    let path = dir.path("kjv.obr");
    write_obr(&sample_resource(), &path).unwrap();
    let r = ObrReader::open(&path).unwrap();
    let jhn = UnitId::book(Book::John);

    assert_eq!(
        r.unit_ids().unwrap(),
        [UnitId::book(Book::Psalms), jhn.clone(), UnitId::note(1)]
    );
    let unit = r.unit(&jhn).unwrap().unwrap();
    assert!(unit.text.starts_with("For God so loved"));
    assert!(r.unit(&UnitId::new("nope").unwrap()).unwrap().is_none());

    let names = |spans: Vec<obs_model::Span>| -> Vec<&'static str> {
        spans.iter().map(|s| s.kind.name()).collect()
    };
    // "God" at 4..7.
    assert_eq!(
        names(r.spans_in(&jhn, 4..7).unwrap()),
        ["chapter", "para", "verse", "tagged_word"]
    );
    // A point query at the end of v16 finds the zero-width xref there.
    let v16 = r.verse(Verse::new(Book::John, 3, 16)).unwrap().unwrap();
    let at_end = names(
        r.spans_in(&jhn, v16.end as usize..v16.end as usize)
            .unwrap(),
    );
    assert!(at_end.contains(&"xref"), "{at_end:?}");
    assert!(matches!(
        r.spans_in(&UnitId::new("nope").unwrap(), 0..1),
        Err(StoreError::UnknownUnit(_))
    ));

    let title = r.verse(Verse::new(Book::Psalms, 3, 0)).unwrap().unwrap();
    assert_eq!(title.start, 0);
    assert!(r.verse(Verse::new(Book::John, 3, 18)).unwrap().is_none());
    assert_eq!(r.verses().unwrap().len(), 4);

    let key = Key::Passage(Passage::verse(Verse::new(Book::John, 3, 16)));
    let hits = r.lookup_key(&key).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].unit, jhn);
    assert_eq!(hits[0].range, Some((0, 4)));
    assert_eq!(r.keys(KeyScheme::Passage).unwrap().len(), 1);
    assert!(r.keys(KeyScheme::Headword).unwrap().is_empty());

    assert_eq!(
        r.unit_alias(&UnitId::new("JOH").unwrap()).unwrap(),
        Some(jhn)
    );
    assert_eq!(r.unit_alias(&UnitId::new("XYZ").unwrap()).unwrap(), None);

    let ps = r.unit(&UnitId::book(Book::Psalms)).unwrap().unwrap();
    assert!(ps.spans.iter().any(|s| s.kind == SpanKind::DivineName));
}

/// Rewrite the header of a written file, as a newer or older build would have.
fn set_user_version(path: &std::path::Path, version: i64) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(&format!("PRAGMA user_version = {version};"))
        .unwrap();
}

#[test]
fn newer_and_older_formats_are_refused_with_typed_errors() {
    let dir = TempDir::new("versions");
    let path = dir.path("kjv.obr");
    write_obr(&sample_resource(), &path).unwrap();

    set_user_version(&path, i64::from(FORMAT_VERSION) + 1);
    let err = ObrReader::open(&path).unwrap_err();
    assert!(
        matches!(err, StoreError::NewerFormat { found, supported }
            if found == FORMAT_VERSION + 1 && supported == FORMAT_VERSION),
        "{err:?}"
    );
    assert!(err.to_string().contains("update the app"));

    set_user_version(&path, 0);
    let err = ObrReader::open(&path).unwrap_err();
    assert!(
        matches!(err, StoreError::OlderFormat { found: 0, .. }),
        "{err:?}"
    );
}

#[test]
fn other_files_are_not_obr() {
    let dir = TempDir::new("other");
    let db = dir.path("other.sqlite");
    Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE t (x);")
        .unwrap();
    assert!(matches!(ObrReader::open(&db), Err(StoreError::NotAnObr(_))));

    let text = dir.path("notes.txt");
    std::fs::write(&text, "hello, not a database at all").unwrap();
    assert!(ObrReader::open(&text).is_err());

    assert!(matches!(
        ObrReader::open(&dir.path("missing.obr")),
        Err(StoreError::Io(_))
    ));
}

#[test]
fn tampered_content_fails_the_hash_check() {
    let dir = TempDir::new("tamper");
    let path = dir.path("kjv.obr");
    write_obr(&sample_resource(), &path).unwrap();
    Connection::open(&path)
        .unwrap()
        .execute_batch("UPDATE units SET text = replace(text, 'loved', 'liked') WHERE id = 'JHN';")
        .unwrap();
    let r = ObrReader::open(&path).unwrap();
    assert!(matches!(r.load(), Err(StoreError::Corrupt(m)) if m.contains("content hash")));
}

#[test]
fn orphan_mentions_in_a_tampered_file_are_rejected() {
    let dir = TempDir::new("orphan-mention");
    let path = dir.path("kjv.obr");
    let resource = sample_resource();
    write_obr(&resource, &path).unwrap();
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "PRAGMA foreign_keys = OFF; INSERT INTO mentions VALUES (999, 1001001, 1001001);",
        )
        .unwrap();
    let reader = ObrReader::open(&path).unwrap();
    assert_eq!(
        reader.manifest().content_hash,
        resource.manifest.content_hash
    );
    assert!(
        matches!(reader.load(), Err(StoreError::Corrupt(m)) if m == "no entity with ordinal 999")
    );
    assert!(matches!(
        reader.mentions_in(Passage::verse(Verse::new(Book::Genesis, 1, 1))),
        Err(StoreError::Corrupt(m)) if m == "no entity with ordinal 999"
    ));
}

#[test]
fn invalid_resources_are_not_written() {
    let dir = TempDir::new("invalid");
    let mut resource = sample_resource();
    resource.units[0].spans[0].end = 10_000;
    let path = dir.path("bad.obr");
    assert!(matches!(
        write_obr(&resource, &path),
        Err(StoreError::Invalid(_))
    ));
    assert!(!path.exists());
}

#[test]
fn verse_map_round_trips() {
    let dir = TempDir::new("versemap");
    let path = dir.path("kjv.obr");
    write_obr(&sample_resource(), &path).unwrap();
    let r = ObrReader::open(&path).unwrap();
    let map = r.verse_map().unwrap().unwrap();
    assert_eq!(map.entries().len(), 4);
    assert!(map.is_absent(Verse::new(Book::John, 3, 18)));
    assert_eq!(
        map.first_native(Verse::new(Book::Psalms, 3, 0)),
        Some(Verse::new(Book::Psalms, 3, 0))
    );
    assert_eq!(Some(map), sample_resource().verse_map);
}

fn verse(book: Book, chapter: u16, verse: u16) -> Passage {
    Passage::verse(Verse::new(book, chapter, verse))
}

fn dataset_manifest(id: &str, resource_type: obs_model::ResourceType) -> obs_model::Manifest {
    let mut m = obs_model::Manifest::new(id, "Dataset (test)", "DS", resource_type, "en");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "CC-BY-4.0".into();
    m
}

#[test]
fn cross_reference_sets_round_trip_and_answer_by_source_passage() {
    use obs_model::{CrossReference, ResourceBuilder, ResourceType};
    let dir = TempDir::new("xrefs");
    let path = dir.path("xrefs.obr");
    let mut b = ResourceBuilder::new(dataset_manifest("xrefs", ResourceType::CrossReferenceSet));
    let x = |from: Passage, to: Passage, votes| CrossReference { from, to, votes };
    let gen_1_1 = verse(Book::Genesis, 1, 1);
    let jn_1_1_3 =
        Passage::new(Verse::new(Book::John, 1, 1), Verse::new(Book::John, 1, 3)).unwrap();
    // A source range that starts before the queried verse must still be found.
    let gen_1_1_2 = Passage::new(gen_1_1.start(), Verse::new(Book::Genesis, 1, 2)).unwrap();
    let rows = [
        x(gen_1_1, verse(Book::Psalms, 33, 6), 40),
        x(gen_1_1, jn_1_1_3, 82),
        x(gen_1_1, verse(Book::Hebrews, 11, 3), -3),
        x(gen_1_1_2, verse(Book::Psalms, 3, 0), 5),
        x(
            verse(Book::Revelation, 22, 21),
            verse(Book::Genesis, 1, 1),
            1,
        ),
    ];
    for r in rows {
        b.add_cross_reference(r);
    }
    let resource = b.build().unwrap();
    write_obr(&resource, &path).unwrap();
    let r = ObrReader::open(&path).unwrap();
    assert_eq!(r.load().unwrap(), resource);
    assert_eq!(r.summary().unwrap().cross_references, 5);

    let found = r.cross_references_from(verse(Book::Genesis, 1, 2)).unwrap();
    assert_eq!(found, [x(gen_1_1_2, verse(Book::Psalms, 3, 0), 5)]);
    let found = r.cross_references_from(gen_1_1).unwrap();
    let votes: Vec<i32> = found.iter().map(|x| x.votes).collect();
    assert_eq!(
        votes,
        [82, 40, -3, 5],
        "by source, then votes, highest first"
    );
    assert_eq!(found[0].to, jn_1_1_3);
    assert!(
        r.cross_references_from(verse(Book::Exodus, 1, 1))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        r.cross_references_from(Passage::book(&obs_scripture::Standard, Book::Revelation).unwrap())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn people_and_places_round_trip_and_answer_by_passage() {
    use obs_model::{Entity, EntityKind, EntityRelation, Relation, ResourceBuilder, ResourceType};
    let dir = TempDir::new("people");
    let path = dir.path("people.obr");
    let mut b = ResourceBuilder::new(dataset_manifest("people", ResourceType::PeopleAndPlaces));
    let abraham = Entity {
        id: "Abraham@Gen.11.26-1Pe".into(),
        name: "Abraham".into(),
        kind: EntityKind::Male,
        strongs: Some("H0085".parse().unwrap()),
        other_names: vec!["Abram".into()],
        region: None,
        relations: vec![EntityRelation {
            relation: Relation::Father,
            entity: "Terah@Gen.11.24-Luk".into(),
            uncertain: false,
        }],
    };
    let terah = Entity {
        id: "Terah@Gen.11.24-Luk".into(),
        name: "Terah".into(),
        kind: EntityKind::Male,
        strongs: Some("H8646G".parse().unwrap()),
        other_names: Vec::new(),
        region: Some("Mesopotamia".into()),
        relations: Vec::new(),
    };
    b.add_entity(abraham.clone());
    b.add_entity(terah);
    b.add_mention(&abraham.id, verse(Book::Genesis, 12, 1));
    b.add_mention(&abraham.id, verse(Book::Genesis, 11, 26));
    b.add_mention("Terah@Gen.11.24-Luk", verse(Book::Genesis, 11, 26));
    let resource = b.build().unwrap();
    write_obr(&resource, &path).unwrap();
    let r = ObrReader::open(&path).unwrap();
    assert_eq!(r.load().unwrap(), resource);
    let s = r.summary().unwrap();
    assert_eq!((s.entities, s.mentions, s.units), (2, 3, 0));

    assert_eq!(r.entity(&abraham.id).unwrap(), Some(abraham.clone()));
    assert_eq!(r.entity("Nobody@Gen.1.1").unwrap(), None);
    let in_gen_12 = r
        .mentions_in(Passage::chapter(&obs_scripture::Standard, Book::Genesis, 12).unwrap())
        .unwrap();
    assert_eq!(in_gen_12.len(), 1);
    assert_eq!(in_gen_12[0].entity, abraham.id);
    let at_11_26: Vec<String> = r
        .mentions_in(verse(Book::Genesis, 11, 26))
        .unwrap()
        .into_iter()
        .map(|m| m.entity)
        .collect();
    assert_eq!(at_11_26, [abraham.id.as_str(), "Terah@Gen.11.24-Luk"]);
}

/// A two-entry Lexicon: H1254A (extended, with its base number as a secondary Key) and G5485.
fn lexicon() -> Resource {
    use obs_model::{Manifest, ResourceBuilder, ResourceType};
    let mut m = Manifest::new("lex", "Test Lexicon", "LEX", ResourceType::Lexicon, "en");
    m.key_schemes = vec![KeyScheme::StrongsNumber, KeyScheme::Lemma];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    for (id, lemma, base) in [("H1254A", "בָּרָא", Some("H1254")), ("G5485", "χάρις", None)]
    {
        let unit = UnitId::new(id).unwrap();
        b.add_unit(
            unit.clone(),
            UnitKind::Entry,
            None,
            format!("{lemma} entry"),
        );
        b.add_key(Key::StrongsNumber(id.parse().unwrap()), &unit, None, false);
        if let Some(base) = base {
            b.add_key(Key::StrongsNumber(base.parse().unwrap()), &unit, None, true);
        }
        b.add_key(Key::Lemma(lemma.into()), &unit, None, false);
    }
    b.build().unwrap()
}

#[test]
fn transliteration_index_round_trips_and_answers_lookups() {
    let dir = TempDir::new("translit");
    let path = dir.path("lex.obr");
    let resource = lexicon();
    write_obr(&resource, &path).unwrap();
    let r = ObrReader::open(&path).unwrap();
    assert_eq!(r.load().unwrap(), resource);
    assert_eq!(r.load().unwrap().transliterations.len(), 2);
    assert_eq!(r.lemmas_for_transliteration("bara").unwrap(), ["בָּרָא"]);
    assert_eq!(r.lemmas_for_transliteration(" Charis ").unwrap(), ["χάρις"]);
    assert!(r.lemmas_for_transliteration("agape").unwrap().is_empty());
}

#[test]
fn strongs_lookup_falls_back_from_an_extended_to_a_base_number() {
    let dir = TempDir::new("strongs");
    let path = dir.path("lex.obr");
    write_obr(&lexicon(), &path).unwrap();
    let r = ObrReader::open(&path).unwrap();
    let units = |n: &str| -> Vec<String> {
        r.lookup_strongs(n.parse().unwrap())
            .unwrap()
            .into_iter()
            .map(|e| e.unit.to_string())
            .collect()
    };
    // Exact extended number.
    assert_eq!(units("H1254A"), ["H1254A"]);
    // The base number finds the extended entry through its secondary Key.
    assert_eq!(units("H1254"), ["H1254A"]);
    // An extended number with no entry falls back to its base number.
    assert_eq!(units("G5485B"), ["G5485"]);
    // H1254B has no entry: its base number H1254 finds the entries that share it.
    assert_eq!(units("H1254B"), ["H1254A"]);
    // A base number with no entry finds nothing.
    assert!(units("G26").is_empty());
}
