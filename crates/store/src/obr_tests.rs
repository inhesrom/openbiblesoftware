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
            unit_aliases: 1
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
