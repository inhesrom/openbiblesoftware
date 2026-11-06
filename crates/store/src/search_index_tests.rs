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

/// A three-Verse Greek Bible whose words carry Morphology codes (#145).
fn greek_with_morphology() -> Resource {
    let mut m = Manifest::new("grc", "Greek", "GRC", ResourceType::Bible, "grc");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let john = UnitId::book(Book::John);
    // (word, Strong's, Lemma, code), one Verse per row.
    let verses: &[&[(&str, &str, &str, &str)]] = &[
        &[
            ("ἀγαπᾶτε", "G25", "ἀγαπάω", "V-PAM-2P"),
            ("ἀλλήλους", "G240", "ἀλλήλων", "C-APM"),
        ],
        &[("ἀγαπήσατε", "G25", "ἀγαπάω", "V-AAM-2P")],
        &[
            ("ἔλθετε", "G2064G", "ἔρχομαι", "V-2AAM-2P"),
            ("ἀγαπᾷ", "G25", "ἀγαπάω", "V-PAI-3S"),
        ],
    ];
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut ranges = Vec::new();
    for words in verses {
        let start = text.len();
        for (word, strongs, lemma, code) in *words {
            if text.len() > start {
                text.push(' ');
            }
            let at = text.len();
            text.push_str(word);
            let mut part = MorphemePart::new(strongs.parse().unwrap());
            part.lemma = Some((*lemma).into());
            part.morph = Some((*code).into());
            spans.push((at..text.len(), part));
        }
        ranges.push(start..text.len());
        text.push(' ');
    }
    b.add_unit(john.clone(), UnitKind::BibleBook, None, text);
    for (range, part) in spans {
        b.add_span(&john, range, SpanKind::TaggedWord { parts: vec![part] });
    }
    for (i, range) in ranges.into_iter().enumerate() {
        let verse = 34 + i as u16;
        b.add_verse(&john, range, Verse::new(Book::John, 13, verse), verse);
    }
    b.build().unwrap()
}

/// Morphology criteria hold for one word: a Strong's number, Lemma or other criterion of
/// another word in the Verse does not count (#145).
#[test]
fn morphology_criteria_match_inside_one_word() {
    let dir = TempDir::new("morph");
    let r = greek_with_morphology();
    let index = SqliteSearchIndex::open(&build(&r, 2, &dir.0), None).unwrap();
    let table: &[(&str, &[i64])] = &[
        ("@imperative", &[0, 1, 2]),
        ("@aorist imperative", &[1, 2]),
        ("@verb aorist imperative second plural", &[1, 2]),
        ("@aorist indicative", &[]),
        // The aorist imperative of Verse 2 is ἔρχομαι, not ἀγαπάω.
        ("strong:G25 @aorist imperative", &[1]),
        ("G25 @aorist imperative", &[1]),
        // An explicit AND joins two criteria of the Verse, in either order.
        ("G25 AND @aorist imperative", &[1, 2]),
        ("@aorist imperative AND G25", &[1, 2]),
        ("lemma:ἀγαπάω AND @aorist imperative", &[1, 2]),
        // So does a parenthesis, however deep.
        ("G25 (@aorist imperative)", &[1, 2]),
        ("G25 ((@aorist imperative))", &[1, 2]),
        ("strong:G25 (@aorist imperative)", &[1, 2]),
        ("lemma:ἀγαπάω (@aorist imperative)", &[1, 2]),
        ("(G25) @aorist imperative", &[1, 2]),
        ("(G25 @aorist imperative)", &[1]),
        // A cancelled double negation keeps the parenthesis on either side.
        ("G25 NOT NOT (@aorist imperative)", &[1, 2]),
        ("G25 --(@aorist imperative)", &[1, 2]),
        ("NOT NOT (G25) @aorist imperative", &[1, 2]),
        ("--(lemma:ἀγαπάω) @aorist imperative", &[1, 2]),
        ("--(G25 @aorist imperative)", &[1]),
        ("lemma:ἀγαπάω @aorist imperative", &[1]),
        ("lemma:ἀγαπάω @imperative", &[0, 1]),
        ("G25 @indicative", &[2]),
        // An extended number: its base form or itself.
        ("strong:G2064 @aorist", &[2]),
        ("G2064G @aorist", &[2]),
        ("G2064H @aorist", &[]),
        // Alongside words and NOT.
        ("@imperative -@indicative", &[0, 1]),
        ("ἀλληλουσ @present imperative", &[0]),
        ("@pronoun accusative", &[0]),
        ("G240 @imperative", &[]),
    ];
    for (q, want) in table {
        assert_eq!(docs(&index, q), *want, "{q}");
    }
}

/// Morphology criteria after a Strong's number stay that word's on either side of
/// `WITHIN n verses` (#180), and both endpoint Verses are listed.
#[test]
fn morphology_criteria_stay_one_word_across_verses() {
    let dir = TempDir::new("morph-within");
    let r = greek_with_morphology();
    let index = SqliteSearchIndex::open(&build(&r, 2, &dir.0), None).unwrap();
    let table: &[(&str, &[i64])] = &[
        // G25 is aorist only in Verse 1, next to G240 in Verse 0.
        ("G25 @aorist WITHIN 2 verses G240", &[0, 1]),
        ("(G25 @aorist) WITHIN 2 verses G240", &[0, 1]),
        ("G240 WITHIN 2 verses G25 @aorist", &[0, 1]),
        ("G240 WITHIN 2 verses (G25 @aorist)", &[0, 1]),
        // Verse 2's aorist is ἔρχομαι, so G25 there does not count.
        ("G25 @aorist WITHIN 1 verse G2064", &[]),
        ("G2064 WITHIN 1 verse G25 @aorist", &[]),
        // An explicit AND keeps them apart: some aorist word near G2064, and G25.
        ("G25 AND @aorist WITHIN 1 verse G2064", &[2]),
    ];
    for (q, want) in table {
        assert_eq!(docs(&index, q), *want, "{q}");
    }
}

/// A cache from before the `morph` field (still searched while it is rebuilt) finds nothing
/// for Morphology criteria, and everything else as before.
#[test]
fn a_cache_without_morphology_finds_no_morphology() {
    let dir = TempDir::new("no-morph");
    let path = build(&greek_with_morphology(), 2, &dir.0);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE morph_codes").unwrap();
    drop(conn);
    let index = SqliteSearchIndex::open(&path, None).unwrap();
    assert!(docs(&index, "@imperative").is_empty());
    assert_eq!(docs(&index, "G25"), [0, 1, 2]);
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

#[test]
fn removing_matches_whole_names_only() {
    let dir = TempDir::new("exact");
    for name in [
        "kjv.0123.v1.obsidx",
        "kjv.b.0123.v1.obsidx",
        "kjv.0123.v1.obsidx.1-0.partial",
        "kjv.0123.vx.obsidx",
        "kjv..v1.obsidx",
    ] {
        std::fs::write(dir.0.join(name), b"x").unwrap();
    }
    let removed: Vec<String> = remove_search_indexes(&dir.0, "kjv", None)
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(removed, ["kjv.0123.v1.obsidx"]);
    assert_eq!(search_index_files(&dir.0, "kjv").len(), 0);
}

#[test]
fn unchecked_resource_ids_stay_inside_the_cache_folder() {
    let dir = TempDir::new("ids");
    let key = |id: &str| IndexKey {
        resource_id: id.into(),
        content_hash: "sha256:0123456789abcdef0123".into(),
        version: 1,
    };
    assert_eq!(key("kjv-1").file_name(), "kjv-1.0123456789abcdef.v1.obsidx");
    for id in ["a.b", "../x", "a/b", "C:\\x", "KJV", "", &"a".repeat(65)] {
        let name = key(id).file_name();
        assert!(name.starts_with('_'), "{id}: {name}");
        assert_eq!(name.split('.').count(), 4, "{id}: {name}");
        assert!(!name.contains(['/', '\\', ':']), "{id}: {name}");
        assert!(is_cache_of(&name, id));
        assert!(!is_cache_of(&name, "a"), "{id}: {name}");
    }
    assert_ne!(key("a.b").file_name(), key("a.c").file_name());
    // A Resource whose id has a dot is built (inside the folder) and pruned on its own.
    let mut r = sample_resource();
    r.manifest.resource_id = "a.b".into();
    let path = build(&r, 1, &dir.0);
    assert_eq!(path.parent(), Some(dir.0.as_path()));
    std::fs::write(dir.0.join("a.0123.v1.obsidx"), b"x").unwrap();
    assert!(remove_search_indexes(&dir.0, "a", None).len() == 1);
    assert!(path.is_file(), "`a`'s caches do not include `a.b`'s");
    assert_eq!(search_index_files(&dir.0, "a.b"), [path]);
}

#[test]
fn temp_files_are_unique_per_build() {
    let dir = TempDir::new("tmp");
    let r = sample_resource();
    let mut seen = Vec::new();
    for _ in 0..2 {
        build_search_index(&r, 1, &dir.0, &mut |_, _| {
            for e in std::fs::read_dir(&dir.0).unwrap() {
                let name = e.unwrap().file_name().to_string_lossy().into_owned();
                if name.ends_with(".partial") && !seen.contains(&name) {
                    seen.push(name);
                }
            }
            true
        })
        .unwrap();
    }
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(
        seen.iter()
            .all(|n| n.contains(&std::process::id().to_string()))
    );
    let left: Vec<_> = std::fs::read_dir(&dir.0).unwrap().collect();
    assert_eq!(left.len(), 1, "only the cache remains");
}

/// KJV Hebrews 13:21, James 1:3-4 and James 2:17-22, one Verse each (ordinals 0-8).
fn james() -> Resource {
    let mut m = Manifest::new("kjv-james", "KJV James", "KJV", ResourceType::Bible, "en");
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "LicenseRef-PD".into();
    let mut b = ResourceBuilder::new(m);
    let verses: &[(Book, u16, u16, &str)] = &[
        (
            Book::Hebrews,
            13,
            21,
            "Make you perfect in every good work to do his will, working in you that which is \
             wellpleasing in his sight, through Jesus Christ; to whom be glory for ever and \
             ever. Amen.",
        ),
        (
            Book::James,
            1,
            3,
            "Knowing this, that the trying of your faith worketh patience.",
        ),
        (
            Book::James,
            1,
            4,
            "But let patience have her perfect work, that ye may be perfect and entire, \
             wanting nothing.",
        ),
        (
            Book::James,
            2,
            17,
            "Even so faith, if it hath not works, is dead, being alone.",
        ),
        (
            Book::James,
            2,
            18,
            "Yea, a man may say, Thou hast faith, and I have works: shew me thy faith without \
             thy works, and I will shew thee my faith by my works.",
        ),
        (
            Book::James,
            2,
            19,
            "Thou believest that there is one God; thou doest well: the devils also believe, \
             and tremble.",
        ),
        (
            Book::James,
            2,
            20,
            "But wilt thou know, O vain man, that faith without works is dead?",
        ),
        (
            Book::James,
            2,
            21,
            "Was not Abraham our father justified by works, when he had offered Isaac his son \
             upon the altar?",
        ),
        (
            Book::James,
            2,
            22,
            "Seest thou how faith wrought with his works, and by works was faith made perfect?",
        ),
    ];
    for book in [Book::Hebrews, Book::James] {
        let unit = UnitId::book(book);
        let mut text = String::new();
        let mut ranges = Vec::new();
        for (_, chapter, verse, words) in verses.iter().filter(|v| v.0 == book) {
            let start = text.len();
            text.push_str(words);
            ranges.push((start..text.len(), Verse::new(book, *chapter, *verse)));
            text.push(' ');
        }
        b.add_unit(unit.clone(), UnitKind::BibleBook, None, text);
        for (range, verse) in ranges {
            b.add_verse(&unit, range, verse, verse.verse);
        }
    }
    b.build().unwrap()
}

fn james_index(dir: &TempDir) -> SqliteSearchIndex {
    let r = james();
    let key = IndexKey::new(&r.manifest, 1);
    SqliteSearchIndex::open(&build(&r, 1, &dir.0), Some(&key)).unwrap()
}

/// Documents with Word forms on.
fn docs_with_word_forms(index: &SqliteSearchIndex, q: &str) -> Vec<i64> {
    let parsed = obs_search::parse(q).unwrap().with_word_forms();
    let compiled = obs_search::compile(&parsed, &NoLexicon).unwrap();
    index
        .search(&compiled, &SearchOptions::default())
        .unwrap()
        .into_iter()
        .map(|h| h.document)
        .collect()
}

/// `WITHIN n verses` (#180): a second pass over neighbouring Verses of the same book.
#[test]
fn within_verses_finds_both_sides_in_neighbouring_verses() {
    let dir = TempDir::new("within");
    let index = james_index(&dir);
    // In one Verse: James 2:17, 18, 20 and 22.
    assert_eq!(docs(&index, "faith works"), [3, 4, 6, 8]);
    assert_eq!(docs(&index, "faith WITHIN 1 verse works"), [3, 4, 6, 8]);
    // Two Verses: James 2:21 ("works") with 2:20 or 2:22 ("faith"); not 2:19.
    assert_eq!(docs(&index, "faith WITHIN 2 verses works"), [3, 4, 6, 7, 8]);
    let found = hits(
        &index,
        "faith WITHIN 2 verses works",
        &SearchOptions::default(),
    );
    assert_eq!(
        found[0].key,
        Some(Key::Passage(Passage::verse(Verse::new(Book::James, 2, 17))))
    );
    assert_eq!(
        found[1].key,
        Some(Key::Passage(Passage::verse(Verse::new(Book::James, 2, 18))))
    );
    // Neighbours stay in one book: Hebrews 13:21 ("work") is next to James 1:3 ("faith")
    // only in document order. James 1:4 ("work") is next to James 1:3 and 2:17.
    assert_eq!(docs(&index, "faith WITHIN 2 verses work"), [1, 2, 3]);
    assert_eq!(docs(&index, "faith WITHIN 3 verses work"), [1, 2, 3, 4]);
    // Only inside the Search scope.
    let scoped = hits(
        &index,
        "faith WITHIN 2 verses works",
        &SearchOptions {
            documents: std::iter::once(6..=7).collect(),
            ..Default::default()
        },
    );
    let scoped: Vec<i64> = scoped.iter().map(|h| h.document).collect();
    assert_eq!(scoped, [6, 7]);
    // Combined with other terms, and best first.
    assert_eq!(
        docs(&index, "(faith WITHIN 2 verses works) -dead"),
        [4, 7, 8]
    );
    assert_eq!(docs(&index, "abraham (faith WITHIN 2 verses works)"), [7]);
    let ranked = hits(
        &index,
        "faith WITHIN 2 verses works",
        &SearchOptions {
            order: HitOrder::Relevance,
            limit: Some(2),
            ..Default::default()
        },
    );
    assert_eq!(ranked.len(), 2);
    assert!(ranked[0].score <= ranked[1].score && ranked[1].score < 0.0);
}

/// `WITHIN n paragraphs` in other Resources: neighbouring paragraphs of one text unit.
#[test]
fn within_paragraphs_stays_inside_one_unit() {
    let dir = TempDir::new("within-para");
    let index = SqliteSearchIndex::open(&build(&dictionary(), 1, &dir.0), None).unwrap();
    // Grace: heading 0, "Favour freely shown." 1, "The grace … salvation." 2; Faith: 3.
    assert_eq!(docs(&index, "favour salvation"), Vec::<i64>::new());
    assert_eq!(docs(&index, "favour WITHIN 2 paragraphs salvation"), [1, 2]);
    assert_eq!(
        docs(&index, "salvation WITHIN 2 paragraphs substance"),
        Vec::<i64>::new()
    );
}

/// `NEAR/n` either way round, `BEFORE/n` in order, on word positions in the Verse.
#[test]
fn before_keeps_near_candidates_in_order() {
    let dir = TempDir::new("before");
    let index = james_index(&dir);
    // "faith without works" (2:20); "faith wrought with his works" is 3 apart (2:22).
    assert_eq!(docs(&index, "faith NEAR/1 works"), [6, 8]);
    assert_eq!(docs(&index, "faith BEFORE/1 works"), [6]);
    assert_eq!(docs(&index, "works BEFORE/1 faith"), [8]);
    // "thy faith without thy works" (2:18).
    assert_eq!(docs(&index, "faith BEFORE/2 works"), [4, 6]);
    assert_eq!(docs(&index, "faith WITHIN 2 words works"), [4, 6, 8]);
    assert_eq!(docs(&index, r#""thy faith" BEFORE/1 "thy works""#), [4]);
    assert_eq!(
        docs(&index, r#""thy works" BEFORE/1 "thy faith""#),
        Vec::<i64>::new()
    );
    assert_eq!(docs(&index, "faith BEFORE/0 wr*"), [8]);
    assert_eq!(
        docs(&index, "(faith OR patience) BEFORE/0 (worketh OR wrought)"),
        [1, 8]
    );
    assert_eq!(
        docs(&index, "faith BEFORE/1 works -dead"),
        Vec::<i64>::new()
    );
    assert_eq!(docs(&index, "faith BEFORE/3 works dead"), [6]);
}

/// Word forms (#180): "work" also finds "works", "worketh" and "working".
#[test]
fn word_forms_find_the_other_words_of_a_stem() {
    let dir = TempDir::new("stems");
    let index = james_index(&dir);
    assert_eq!(docs(&index, "work"), [0, 2]);
    assert_eq!(
        docs_with_word_forms(&index, "work"),
        [0, 1, 2, 3, 4, 6, 7, 8]
    );
    assert_eq!(docs(&index, "believe"), [5]);
    assert_eq!(docs_with_word_forms(&index, "believeth"), [5]);
    assert_eq!(docs_with_word_forms(&index, "faith BEFORE/0 work"), [1]);
    assert_eq!(
        docs_with_word_forms(&index, "worketh WITHIN 2 verses perfect"),
        [0, 1, 2, 3, 7, 8]
    );
    // No forms in this index: nothing.
    assert_eq!(docs_with_word_forms(&index, "loveth"), Vec::<i64>::new());
    // Exact as quoted, with or without `text:`.
    assert_eq!(docs_with_word_forms(&index, r#""work""#), [0, 2]);
    assert_eq!(docs_with_word_forms(&index, r#"text:"work""#), [0, 2]);
    assert_eq!(
        docs_with_word_forms(&index, "text:work"),
        [0, 1, 2, 3, 4, 6, 7, 8]
    );
    // Short words too: "do" (Hebrews 13:21) finds "doest" (James 2:19).
    assert_eq!(docs(&index, "do"), [0]);
    assert_eq!(docs_with_word_forms(&index, "do"), [0, 5]);
    assert_eq!(docs_with_word_forms(&index, "doth"), [0, 5]);
}

/// A cache whose words cannot be read fails a Word forms search instead of finding nothing.
#[test]
fn unreadable_words_are_a_search_error() {
    let dir = TempDir::new("stems-damaged");
    let path = build(&james(), 1, &dir.0);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("DROP TABLE search;").unwrap();
    drop(conn);
    let index = SqliteSearchIndex::open(&path, None).unwrap();
    let parsed = obs_search::parse("work").unwrap().with_word_forms();
    let compiled = obs_search::compile(&parsed, &NoLexicon).unwrap();
    assert!(
        index.search(&compiled, &SearchOptions::default()).is_err(),
        "a damaged cache must not look like no hits"
    );
}
