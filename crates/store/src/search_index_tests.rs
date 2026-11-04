use std::path::PathBuf;

use obs_model::{
    Key, KeyScheme, Manifest, MorphemePart, ParaStyle, Resource, ResourceBuilder, ResourceType,
    SpanKind, UnitId, UnitKind,
};
use obs_scripture::{Book, Passage, Verse};
use obs_search::{HitOrder, NoLexicon, SearchIndex, SearchOptions, to_fts5};

use super::*;
use crate::sample::sample_resource;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "obs-store-search-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn build(r: &Resource, version: u32, dir: &Path) -> PathBuf {
    build_search_index(r, version, dir, &mut |_, _| true).unwrap()
}

fn hits(index: &SqliteSearchIndex, q: &str, options: &SearchOptions) -> Vec<SearchHit> {
    let compiled = to_fts5(q, &NoLexicon).unwrap();
    index.search(&compiled, options).unwrap()
}

fn docs(index: &SqliteSearchIndex, q: &str) -> Vec<i64> {
    hits(index, q, &SearchOptions::default())
        .into_iter()
        .map(|h| h.document)
        .collect()
}

/// A two-entry Dictionary: paragraphs, a heading outside any paragraph, Headword Keys.
fn dictionary() -> Resource {
    let mut m = Manifest::new(
        "test-dict",
        "Test Dictionary",
        "TD",
        ResourceType::Dictionary,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Headword];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let grace = UnitId::new("Grace").unwrap();
    let text = "GRACE\nFavour freely shown.\nThe grace of God that bringeth salvation.";
    b.add_unit(
        grace.clone(),
        UnitKind::Entry,
        Some("Grace".into()),
        text.into(),
    );
    b.add_span(&grace, 0..5, SpanKind::Heading { level: 1 });
    let p1 = text.find("Favour").unwrap()..text.find('.').unwrap() + 1;
    let p2 = text.find("The").unwrap()..text.len();
    for p in [p1, p2] {
        b.add_span(
            &grace,
            p,
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
    }
    b.add_key(Key::Headword("Grace".into()), &grace, None, false);
    b.add_key(Key::Headword("Favour".into()), &grace, None, true);
    let faith = UnitId::new("Faith").unwrap();
    b.add_unit(
        faith.clone(),
        UnitKind::Entry,
        Some("Faith".into()),
        "Faith is the substance of things hoped for; by grace through faith.".into(),
    );
    b.add_key(Key::Headword("Faith".into()), &faith, None, false);
    b.build().unwrap()
}

#[test]
fn bible_documents_are_verses_keyed_by_ordinal() {
    let dir = TempDir::new("bible");
    let r = sample_resource();
    let path = build(&r, 1, &dir.0);
    assert_eq!(
        path.file_name().unwrap().to_string_lossy(),
        IndexKey::new(&r.manifest, 1).file_name()
    );
    let index = SqliteSearchIndex::open(&path, Some(&IndexKey::new(&r.manifest, 1))).unwrap();
    assert_eq!(index.document_kind(), DocumentKind::Verse);
    assert_eq!(index.document_count(), r.verses.len() as u64);
    // Verse ordinals: Ps 3:0, Ps 3:1, Jn 3:16, Jn 3:17.
    assert_eq!(docs(&index, "absalom"), [0]);
    assert_eq!(docs(&index, "lord"), [1]);
    assert_eq!(docs(&index, "world"), [2, 3]);
    assert_eq!(docs(&index, "\"only begotten son\""), [2]);
    assert_eq!(docs(&index, "\"begotten only\""), Vec::<i64>::new());
    assert_eq!(docs(&index, "strong:G2316"), [2]);
    assert_eq!(docs(&index, "G25 world"), [2]);
    assert_eq!(docs(&index, "lemma:θεος"), [2]);
    assert_eq!(docs(&index, "belie*"), [2]);
    assert_eq!(docs(&index, "world -perish"), [3]);

    let hit = &hits(&index, "condemn", &SearchOptions::default())[0];
    let v17 = &r.verses[3];
    assert_eq!(hit.unit, UnitId::book(Book::John));
    assert_eq!(hit.range, v17.start..v17.end);
    assert_eq!(
        hit.key,
        Some(Key::Passage(Passage::verse(Verse::new(Book::John, 3, 17))))
    );
    assert_eq!(hit.title.as_deref(), Some("John"));

    // A Bible's Search scope is verse-ordinal ranges (Jn 3:17, and past the end).
    let scoped = SearchOptions {
        documents: vec![3..=3, 10..=20],
        ..Default::default()
    };
    let in_scope: Vec<i64> = hits(&index, "world", &scoped)
        .iter()
        .map(|h| h.document)
        .collect();
    assert_eq!(in_scope, [3]);
}

#[test]
fn tagged_words_index_extended_and_base_strongs() {
    let dir = TempDir::new("strongs");
    let mut m = Manifest::new("heb", "Hebrew", "HEB", ResourceType::Bible, "hbo");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let genesis = UnitId::book(Book::Genesis);
    let text = "בְּרֵאשִׁ֖ית בָּרָ֣א";
    b.add_unit(genesis.clone(), UnitKind::BibleBook, None, text.into());
    let split = text.find(' ').unwrap();
    let part = |s: &str, lemma: &str| MorphemePart {
        strongs: s.parse().unwrap(),
        lemma: Some(lemma.into()),
        morph: None,
        gloss: None,
    };
    b.add_span(
        &genesis,
        0..split,
        SpanKind::TaggedWord {
            parts: vec![part("H9003", "בְּ"), part("H7225G", "רֵאשִׁית")],
        },
    );
    b.add_span(
        &genesis,
        split + 1..text.len(),
        SpanKind::TaggedWord {
            parts: vec![part("H1254A", "בָּרָא")],
        },
    );
    b.add_verse(&genesis, 0..text.len(), Verse::new(Book::Genesis, 1, 1), 1);
    let r = b.build().unwrap();
    let index = SqliteSearchIndex::open(&build(&r, 1, &dir.0), None).unwrap();
    for q in [
        "H1254A",
        "strong:H1254",
        "H7225",
        "H7225G",
        "lemma:ברא",
        "בראשית",
    ] {
        assert_eq!(docs(&index, q), [0], "{q}");
    }
    assert!(docs(&index, "H1254B").is_empty());
}

#[test]
fn other_resources_index_paragraphs_with_the_unit_key_and_title() {
    let dir = TempDir::new("dict");
    let r = dictionary();
    let index = SqliteSearchIndex::open(&build(&r, 1, &dir.0), None).unwrap();
    assert_eq!(index.document_kind(), DocumentKind::Paragraph);
    // Heading, two paragraphs of Grace, the unparagraphed Faith entry.
    assert_eq!(index.document_count(), 4);
    let all = hits(&index, "grace", &SearchOptions::default());
    let found: Vec<(&str, &str)> = all
        .iter()
        .map(|h| {
            let unit = &r.units.iter().find(|u| u.id == h.unit).unwrap().text;
            (
                h.unit.as_str(),
                &unit[h.range.start as usize..h.range.end as usize],
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("Grace", "GRACE"),
            ("Grace", "The grace of God that bringeth salvation."),
            (
                "Faith",
                "Faith is the substance of things hoped for; by grace through faith."
            ),
        ]
    );
    assert_eq!(all[1].key, Some(Key::Headword("Grace".into())));
    assert_eq!(all[1].title.as_deref(), Some("Grace"));
    assert_eq!(all[2].key, Some(Key::Headword("Faith".into())));

    // Relevance: the entry that says "faith" twice ranks first; scores are bm25 (lower wins).
    let ranked = hits(
        &index,
        "faith OR grace",
        &SearchOptions {
            order: HitOrder::Relevance,
            limit: Some(2),
            ..Default::default()
        },
    );
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].unit.as_str(), "Faith");
    assert!(ranked[0].score <= ranked[1].score);
}

#[test]
fn builds_are_deterministic() {
    let a = TempDir::new("det-a");
    let b = TempDir::new("det-b");
    for r in [sample_resource(), dictionary()] {
        let pa = build(&r, 1, &a.0);
        let pb = build(&r, 1, &b.0);
        assert_eq!(std::fs::read(pa).unwrap(), std::fs::read(pb).unwrap());
    }
}

#[test]
fn a_cancelled_build_leaves_nothing_behind() {
    let dir = TempDir::new("cancel");
    let r = sample_resource();
    let mut calls = 0;
    let err = build_search_index(&r, 1, &dir.0, &mut |_, _| {
        calls += 1;
        false
    })
    .unwrap_err();
    assert!(matches!(err, StoreError::Cancelled), "{err}");
    assert_eq!(calls, 1);
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
}

#[test]
fn progress_reaches_the_total() {
    let dir = TempDir::new("progress");
    let r = sample_resource();
    let mut seen = Vec::new();
    build_search_index(&r, 1, &dir.0, &mut |d, t| {
        seen.push((d, t));
        true
    })
    .unwrap();
    assert_eq!(seen.first(), Some(&(0, 4)));
    assert_eq!(seen.last(), Some(&(4, 4)));
}

#[test]
fn a_new_version_replaces_the_old_cache_and_the_key_is_checked() {
    let dir = TempDir::new("version");
    let r = sample_resource();
    let dict = dictionary();
    let old = build(&r, 1, &dir.0);
    let other = build(&dict, 1, &dir.0);
    let new = build(&r, 2, &dir.0);
    assert_ne!(old, new);
    assert!(!old.exists(), "the stale cache is removed");
    assert!(other.exists(), "other Resources' caches are kept");
    assert!(SqliteSearchIndex::open(&new, Some(&IndexKey::new(&r.manifest, 2))).is_ok());
    let wrong = SqliteSearchIndex::open(&new, Some(&IndexKey::new(&r.manifest, 1)));
    assert!(matches!(wrong, Err(StoreError::Corrupt(_))));
    // An .obr is not a Search index.
    let obr = dir.0.join("x.obr");
    crate::write_obr(&r, &obr).unwrap();
    assert!(SqliteSearchIndex::open(&obr, None).is_err());
}

#[test]
fn a_deleted_cache_is_reported_not_fatal() {
    let dir = TempDir::new("deleted");
    let r = sample_resource();
    let path = build(&r, 1, &dir.0);
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        SqliteSearchIndex::open(&path, None),
        Err(StoreError::Io(_))
    ));
    // Rebuilding puts it back.
    let again = build(&r, 1, &dir.0);
    assert_eq!(again, path);
    assert_eq!(
        docs(&SqliteSearchIndex::open(&path, None).unwrap(), "world"),
        [2, 3]
    );
}

#[test]
fn removing_one_resources_caches_keeps_similar_names() {
    let dir = TempDir::new("prune");
    for name in [
        "kjv.0123.v1.obsidx",
        "kjv.4567.v2.obsidx",
        "kjv-strongs.0123.v1.obsidx",
        "kjv.obr",
    ] {
        std::fs::write(dir.0.join(name), b"x").unwrap();
    }
    let mut removed: Vec<String> = remove_search_indexes(&dir.0, "kjv", Some("kjv.4567.v2.obsidx"))
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    removed.sort();
    assert_eq!(removed, ["kjv.0123.v1.obsidx"]);
}
