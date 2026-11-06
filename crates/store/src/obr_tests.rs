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
    // Only spans that start inside the range: the Tagged words, not the chapter and
    // paragraph that enclose them from byte 0.
    let starting = r.spans_starting_in(&jhn, 1..v16.end as usize).unwrap();
    assert!(starting.iter().all(|s| (1..v16.end).contains(&s.start)));
    let mut starting = names(starting);
    starting.sort_unstable();
    assert_eq!(starting, ["note_ref", "tagged_word", "tagged_word"]);
    assert!(matches!(
        r.spans_starting_in(&UnitId::new("nope").unwrap(), 0..1),
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
        other_strongs: vec!["H0087".parse().unwrap(), "G0011".parse().unwrap()],
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
        other_strongs: Vec::new(),
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
    // Every entity at once (the Dictionary alias cache, #54), like the generic version.
    let all = r.entities().unwrap();
    assert_eq!(all, resource.entities);
    assert_eq!(Defaults(&r).entities().unwrap(), all);
    // One entity's mentions in Bible order, and every entity's count (the Factbook, #149),
    // like the generic versions.
    let abraham_at: Vec<Passage> = r
        .mentions_of(&abraham.id)
        .unwrap()
        .into_iter()
        .map(|m| m.passage)
        .collect();
    assert_eq!(
        abraham_at,
        [verse(Book::Genesis, 11, 26), verse(Book::Genesis, 12, 1)]
    );
    assert_eq!(
        Defaults(&r).mentions_of(&abraham.id).unwrap(),
        r.mentions_of(&abraham.id).unwrap()
    );
    assert!(r.mentions_of("Nobody@Gen.1.1").unwrap().is_empty());
    let stats = r.mention_stats().unwrap();
    let gn = |c, v| Verse::new(Book::Genesis, c, v);
    assert_eq!(
        stats,
        [
            crate::MentionStats {
                entity: abraham.id.clone(),
                count: 2,
                first: gn(11, 26),
                last: gn(12, 1),
            },
            crate::MentionStats {
                entity: "Terah@Gen.11.24-Luk".into(),
                count: 1,
                first: gn(11, 26),
                last: gn(11, 26),
            },
        ]
    );
    assert_eq!(Defaults(&r).mention_stats().unwrap(), stats);
    drop(r);
    // A file written before entities had other Strong's numbers (#149) still opens.
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("DROP TABLE entity_strongs;").unwrap();
    }
    let r = ObrReader::open(&path).unwrap();
    let old = r.entity(&abraham.id).unwrap().unwrap();
    assert!(old.other_strongs.is_empty());
    assert_eq!(old.strongs, abraham.strongs);
}

/// Tagged words read straight from the span table match a walk over every unit (#54).
#[test]
fn every_tagged_word_is_visited_like_the_generic_walk() {
    let dir = TempDir::new("tagged-words");
    let path = dir.path("kjv.obr");
    write_obr(&sample_resource(), &path).unwrap();
    let reader = ObrReader::open(&path).unwrap();
    let collect = |store: &dyn ResourceStore| {
        let mut seen = Vec::new();
        store
            .for_each_tagged_word(&mut |parts| {
                seen.extend(
                    parts
                        .iter()
                        .map(|p| (p.strongs.to_string(), p.lemma.clone())),
                );
            })
            .unwrap();
        seen.sort();
        seen
    };
    let fast = collect(&reader);
    assert_eq!(
        fast,
        [
            ("G2316".to_string(), Some("θεός".to_string())),
            ("G25".to_string(), None)
        ]
    );
    assert_eq!(fast, collect(&Defaults(&reader)));
}

/// Tagged words with their unit and start byte (#56) match the generic walk, in document
/// order.
#[test]
fn tagged_words_with_positions_match_the_generic_walk() {
    let dir = TempDir::new("tagged-words-at");
    let path = dir.path("kjv.obr");
    write_obr(&sample_resource(), &path).unwrap();
    let reader = ObrReader::open(&path).unwrap();
    let collect = |store: &dyn ResourceStore| {
        let mut seen = Vec::new();
        store
            .for_each_tagged_word_at(&mut |unit, start, parts| {
                seen.push((unit.to_string(), start, parts[0].strongs.to_string()));
            })
            .unwrap();
        seen
    };
    let fast = collect(&reader);
    assert_eq!(fast.len(), 2, "{fast:?}");
    assert!(fast.windows(2).all(|w| w[0].1 < w[1].1 || w[0].0 != w[1].0));
    assert_eq!(fast, collect(&Defaults(&reader)));
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
fn keys_of_unit_lists_a_lexicon_entrys_strongs_numbers_and_lemma() {
    let dir = TempDir::new("unit-keys");
    let path = dir.path("lex.obr");
    write_obr(&lexicon(), &path).unwrap();
    let r = ObrReader::open(&path).unwrap();
    let unit = UnitId::new("H1254A").unwrap();
    let keys: Vec<(String, bool)> = r
        .keys_of_unit(&unit)
        .unwrap()
        .into_iter()
        .map(|k| (k.key.to_string(), k.alias))
        .collect();
    assert_eq!(
        keys,
        [
            ("H1254".to_string(), true),
            ("H1254A".to_string(), false),
            ("בָּרָא".to_string(), false),
        ]
    );
    // The trait's default (every scheme's Keys, filtered) agrees.
    assert_eq!(
        Defaults(&r).keys_of_unit(&unit).unwrap(),
        r.keys_of_unit(&unit).unwrap()
    );
    assert!(
        r.keys_of_unit(&UnitId::new("nowhere").unwrap())
            .unwrap()
            .is_empty()
    );
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

#[test]
fn compact_spans_preserve_every_range_query_and_use_typed_columns() {
    let dir = TempDir::new("compact-ranges");
    let path = dir.path("sample.obr");
    let resource = sample_resource();
    write_obr(&resource, &path).unwrap();
    let reader = ObrReader::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    let wrong_types: i64 = conn
        .query_row(
            "SELECT count(*) FROM spans WHERE typeof(kind) != 'integer' OR typeof(attrs) != 'blob'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(wrong_types, 0);
    for unit in &resource.units {
        for start in 0..=unit.text.len() + 1 {
            for end in [
                start.saturating_sub(1),
                start,
                start + 1,
                unit.text.len() + 1,
            ] {
                let expected: Vec<_> = unit
                    .spans
                    .iter()
                    .filter(|span| {
                        let s = span.start as usize;
                        let e = span.end as usize;
                        if start >= end {
                            s <= start && e >= start
                        } else {
                            (s < end && e > start) || (s == e && s >= start && s <= end)
                        }
                    })
                    .cloned()
                    .collect();
                assert_eq!(reader.spans_in(&unit.id, start..end).unwrap(), expected);
            }
        }
        assert!(
            reader
                .spans_in(&unit.id, usize::MAX..usize::MAX)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn corrupt_compact_span_columns_are_rejected_by_the_reader() {
    let dir = TempDir::new("compact-corrupt");
    let path = dir.path("sample.obr");
    for change in [
        "kind = 99",
        "attrs = X'FF'",
        "kind = 4, attrs = X'FFFFFFFF0F'",
    ] {
        write_obr(&sample_resource(), &path).unwrap();
        Connection::open(&path)
            .unwrap()
            .execute_batch(&format!(
                "UPDATE spans SET {change} WHERE unit = 0 AND seq = 0"
            ))
            .unwrap();
        let reader = ObrReader::open(&path).unwrap();
        assert!(matches!(
            reader.unit(&UnitId::book(Book::Psalms)),
            Err(StoreError::Corrupt(_))
        ));
    }
}

/// Full pinned builds are local/release evidence, not a network requirement for ordinary CI.
/// Set OBS_COMPACT_BEFORE/AFTER to format-2/format-3 Starter output directories.
#[test]
#[ignore = "requires both full Starter builds; see docs/dev/obr-span-storage.md"]
fn starter_format_two_and_three_have_identical_logical_content() {
    let before = PathBuf::from(std::env::var_os("OBS_COMPACT_BEFORE").unwrap());
    let after = PathBuf::from(std::env::var_os("OBS_COMPACT_AFTER").unwrap());
    let mut count = 0;
    for entry in std::fs::read_dir(&before).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "obr") {
            continue;
        }
        let old = Connection::open(&path).unwrap();
        let reader = ObrReader::open(&after.join(path.file_name().unwrap())).unwrap();
        let mut resource = reader.load().unwrap(); // validates all content against its logical hash
        let old_hash: String = old
            .query_row("SELECT content_hash FROM manifest", [], |r| r.get(0))
            .unwrap();
        // Format version participates in the hash. Rehash the identical logical content
        // with the old version, without changing the production hash contract.
        resource.manifest.format_version = 2;
        assert_eq!(resource.content_hash(), old_hash);
        for (ord, unit) in resource.units.iter().enumerate() {
            let (id, text): (String, String) = old
                .query_row(
                    "SELECT id, text FROM units WHERE ord = ?1",
                    [ord as i64],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(unit.id.as_str(), id);
            assert_eq!(unit.text, text);
            let mut stmt = old
                .prepare("SELECT start, end, kind, attrs FROM spans WHERE unit = ?1 ORDER BY seq")
                .unwrap();
            let old_spans = stmt
                .query_map([ord as i64], |r| {
                    Ok((
                        r.get::<_, u32>(0)?,
                        r.get::<_, u32>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                })
                .unwrap();
            let old_spans: Vec<Span> = old_spans
                .map(|r| {
                    let (start, end, kind, attrs) = r.unwrap();
                    let mut attrs: serde_json::Map<String, serde_json::Value> = attrs
                        .map(|a| serde_json::from_str(&a).unwrap())
                        .unwrap_or_default();
                    attrs.insert("kind".into(), kind.into());
                    Span {
                        start,
                        end,
                        kind: serde_json::from_value(attrs.into()).unwrap(),
                    }
                })
                .collect();
            assert_eq!(
                unit.spans, old_spans,
                "{} {}",
                resource.manifest.resource_id, unit.id
            );
        }
        eprintln!(
            "{}: identical logical hash and every text/span",
            resource.manifest.resource_id
        );
        count += 1;
    }
    assert_eq!(count, 16);
}

/// Delegates only the required methods, so the trait's default methods run.
struct Defaults<'a>(&'a ObrReader);

impl ResourceStore for Defaults<'_> {
    fn manifest(&self) -> &obs_model::Manifest {
        self.0.manifest()
    }
    fn summary(&self) -> Result<Summary, StoreError> {
        self.0.summary()
    }
    fn unit_ids(&self) -> Result<Vec<UnitId>, StoreError> {
        self.0.unit_ids()
    }
    fn unit(&self, id: &UnitId) -> Result<Option<obs_model::TextUnit>, StoreError> {
        self.0.unit(id)
    }
    fn spans_in(&self, unit: &UnitId, range: Range<usize>) -> Result<Vec<Span>, StoreError> {
        self.0.spans_in(unit, range)
    }
    fn verse(&self, verse: Verse) -> Result<Option<obs_model::BibleVerse>, StoreError> {
        self.0.verse(verse)
    }
    fn verses(&self) -> Result<Vec<obs_model::BibleVerse>, StoreError> {
        self.0.verses()
    }
    fn lookup_key(&self, key: &Key) -> Result<Vec<obs_model::KeyEntry>, StoreError> {
        self.0.lookup_key(key)
    }
    fn lemmas_for_transliteration(&self, t: &str) -> Result<Vec<String>, StoreError> {
        self.0.lemmas_for_transliteration(t)
    }
    fn keys(&self, scheme: KeyScheme) -> Result<Vec<obs_model::KeyEntry>, StoreError> {
        self.0.keys(scheme)
    }
    fn unit_alias(&self, old: &UnitId) -> Result<Option<UnitId>, StoreError> {
        self.0.unit_alias(old)
    }
    fn verse_map(&self) -> Result<Option<VerseMap>, StoreError> {
        self.0.verse_map()
    }
    fn cross_references_from(
        &self,
        passage: Passage,
    ) -> Result<Vec<obs_model::CrossReference>, StoreError> {
        self.0.cross_references_from(passage)
    }
    fn entity(&self, id: &str) -> Result<Option<obs_model::Entity>, StoreError> {
        self.0.entity(id)
    }
    fn mentions_in(&self, passage: Passage) -> Result<Vec<obs_model::Mention>, StoreError> {
        self.0.mentions_in(passage)
    }
    fn load(&self) -> Result<Resource, StoreError> {
        self.0.load()
    }
}

/// The SQL versions of `unit_text` and `verses_overlapping` (used to anchor Annotations,
/// #61) answer like the trait's generic versions.
#[test]
fn unit_text_and_verses_overlapping_match_the_generic_versions() {
    let dir = TempDir::new("overlap");
    let path = dir.path("kjv.obr");
    write_obr(&sample_resource(), &path).unwrap();
    let reader = ObrReader::open(&path).unwrap();
    let generic = Defaults(&reader);
    let jhn = UnitId::book(Book::John);
    let (kind, text) = reader.unit_text(&jhn).unwrap().unwrap();
    assert_eq!(
        (kind, text.clone()),
        generic.unit_text(&jhn).unwrap().unwrap()
    );
    assert!(
        reader
            .unit_text(&UnitId::new("NOPE").unwrap())
            .unwrap()
            .is_none()
    );
    for unit in [jhn.clone(), UnitId::book(Book::Psalms), UnitId::note(1)] {
        for range in [0..1, 0..10_000, 140..150, 141..142, 5..5] {
            assert_eq!(
                reader.verses_overlapping(&unit, range.clone()).unwrap(),
                generic.verses_overlapping(&unit, range.clone()).unwrap(),
                "{unit} {range:?}"
            );
        }
    }
    let both = reader.verses_overlapping(&jhn, 0..text.len()).unwrap();
    assert_eq!(both.len(), 2);
    assert_eq!(
        both[1].1,
        Some(Some(Passage::verse(Verse::new(Book::John, 3, 17))))
    );
}

#[test]
fn rendering_index_round_trips_and_older_files_answer_none() {
    let dir = TempDir::new("renderings");
    let path = dir.path("kjv.obr");
    let resource = sample_resource();
    assert!(!resource.renderings.renderings.is_empty());
    assert!(!resource.renderings.totals.is_empty());
    write_obr(&resource, &path).unwrap();
    {
        let r = ObrReader::open(&path).unwrap();
        assert_eq!(
            r.rendering_index().unwrap().as_ref(),
            Some(&resource.renderings)
        );
        assert_eq!(r.load().unwrap(), resource);
    }
    // A file written before the index existed: no tables, `None`, and `load` does not
    // derive it (a Search index build never uses it; Word Study derives it lazily).
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("DROP TABLE renderings; DROP TABLE strongs_totals;")
            .unwrap();
    }
    let r = ObrReader::open(&path).unwrap();
    assert_eq!(r.rendering_index().unwrap(), None);
    let loaded = r.load().unwrap();
    assert!(loaded.renderings.is_empty());
    assert_eq!(
        Resource {
            renderings: resource.renderings.clone(),
            ..loaded
        },
        resource
    );
}

#[test]
fn part_of_speech_is_stored_as_a_stable_id_and_unknown_ids_read_as_none() {
    let dir = TempDir::new("renderings-pos");
    let path = dir.path("tahot.obr");
    let mut resource = sample_resource();
    // Every number gets a part of speech, including the one whose display name differs
    // from its id (Foreign: "transliterated word").
    let first = resource.renderings.totals[0].strongs;
    for t in &mut resource.renderings.totals {
        t.part_of_speech = Some(if t.strongs == first {
            obs_model::PartOfSpeech::Foreign
        } else {
            obs_model::PartOfSpeech::Verb
        });
    }
    write_obr(&resource, &path).unwrap();
    {
        let conn = Connection::open(&path).unwrap();
        let stored: Vec<String> = conn
            .prepare("SELECT DISTINCT part_of_speech FROM strongs_totals ORDER BY 1")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut want = vec!["foreign".to_string()];
        if resource.renderings.totals.len() > 1 {
            want.push("verb".to_string());
        }
        assert_eq!(stored, want, "ids, not display names");
    }
    assert_eq!(
        ObrReader::open(&path).unwrap().rendering_index().unwrap(),
        Some(resource.renderings.clone())
    );
    // A value this build does not know (a renamed label, a newer writer): no part of
    // speech, and the file still loads (Search index builds load every Bible).
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute(
            "UPDATE strongs_totals SET part_of_speech = 'transliterated word' WHERE strongs = ?1",
            [first.to_string()],
        )
        .unwrap();
    }
    let r = ObrReader::open(&path).unwrap();
    let index = r.rendering_index().unwrap().unwrap();
    let total = index.totals.iter().find(|t| t.strongs == first).unwrap();
    assert_eq!(total.part_of_speech, None);
    assert_eq!(total.count, resource.renderings.totals[0].count);
    let loaded = r.load().unwrap();
    assert_eq!(loaded.renderings, index);
    assert_eq!(loaded.units, resource.units);
}

#[test]
fn only_bibles_get_a_rendering_index() {
    let lex = lexicon();
    assert!(lex.renderings.is_empty());
    let dir = TempDir::new("renderings-lex");
    let path = dir.path("lex.obr");
    write_obr(&lex, &path).unwrap();
    let r = ObrReader::open(&path).unwrap();
    assert_eq!(
        r.rendering_index().unwrap(),
        Some(RenderingIndex::default())
    );
}
