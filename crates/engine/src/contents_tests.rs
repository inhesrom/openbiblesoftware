#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::sync::Arc;

use obs_model::{
    BibleVerse, Key, KeyEntry, KeyScheme, Manifest, ResourceType, TextUnit, UnitId, UnitKind,
};
use obs_scripture::{Book, Passage, Verse};
use serde_json::json;

use crate::contents::{Contents, ContentsTarget};
use crate::resource_navigation::ResourceNavigation;
use crate::*;

fn v(book: Book, chapter: u16, verse: u16) -> Verse {
    Verse::new(book, chapter, verse)
}

fn bible_verse(verse: Verse) -> BibleVerse {
    BibleVerse {
        verse,
        last: verse.verse,
        unit: UnitId::book(verse.book),
        start: 0,
        end: 0,
    }
}

fn section(start: Verse, end: Verse, unit: &str) -> KeyEntry {
    KeyEntry {
        key: Key::Passage(Passage::new(start, end).unwrap()),
        unit: UnitId::new(unit).unwrap(),
        range: None,
        alias: false,
    }
}

fn entry(key: Key, unit: &str, alias: bool) -> KeyEntry {
    KeyEntry {
        key,
        unit: UnitId::new(unit).unwrap(),
        range: None,
        alias,
    }
}

fn unit(id: &str, title: &str) -> TextUnit {
    TextUnit {
        id: UnitId::new(id).unwrap(),
        kind: UnitKind::Entry,
        title: Some(title.into()),
        text: title.into(),
        spans: Vec::new(),
    }
}

/// Each line as "  " per depth, then its label.
fn outline(c: &Contents) -> Vec<String> {
    (0..c.len())
        .map(|n| {
            let node = c.node(n);
            format!("{}{}", "  ".repeat(node.depth), node.label)
        })
        .collect()
}

/// An MHC-like Commentary: book and chapter introductions keyed by the whole book or
/// chapter (StudyImport unit ids), sections, two volumes.
fn mhc() -> (ResourceNavigation, BTreeMap<Book, String>) {
    let ge = |c, vs| v(Book::Genesis, c, vs);
    let ex = |c, vs| v(Book::Exodus, c, vs);
    let nav = ResourceNavigation::new(vec![
        section(ge(1, 1), ge(1, 2), "Gen.1.1-Gen.1.2"),
        section(ge(1, 1), ge(1, 31), "Gen.1.1-Gen.1.31.chapter-intro"),
        section(ge(1, 1), ge(50, 26), "Gen.1.1-Gen.50.26.book-intro"),
        section(ge(1, 3), ge(1, 5), "Gen.1.3-Gen.1.5"),
        section(ge(2, 1), ge(2, 3), "Gen.2.1-Gen.2.3"),
        section(ge(2, 4), ge(2, 4), "Gen.2.4-Gen.2.4"),
        section(ex(1, 1), ex(1, 7), "Exod.1.1-Exod.1.7"),
        section(
            v(Book::Matthew, 1, 1),
            v(Book::Matthew, 1, 17),
            "Matt.1.1-Matt.1.17",
        ),
    ]);
    let volumes = BTreeMap::from([
        (Book::Genesis, "Volume 1".to_string()),
        (Book::Exodus, "Volume 1".to_string()),
        (Book::Matthew, "Volume 5".to_string()),
    ]);
    (nav, volumes)
}

#[test]
fn a_bible_lists_its_books_and_chapters_in_its_own_order_and_numbering() {
    let verses: Vec<BibleVerse> = [
        v(Book::Genesis, 1, 1),
        v(Book::Genesis, 1, 2),
        v(Book::Genesis, 2, 1),
        v(Book::Psalms, 3, 0),
        v(Book::Psalms, 3, 1),
        v(Book::John, 3, 16),
    ]
    .into_iter()
    .map(bible_verse)
    .collect();
    let c = Contents::bible(&verses);
    assert_eq!(
        outline(&c),
        [
            "Genesis",
            "  Chapter 1",
            "  Chapter 2",
            "Psalms",
            "  Chapter 3",
            "John",
            "  Chapter 3"
        ]
    );
    // A chapter opens at its first Verse, a Psalm's title included.
    let psalm = c.at_verse(v(Book::Psalms, 3, 1)).unwrap();
    assert_eq!(
        c.node(psalm).target,
        Some(ContentsTarget::Verse(v(Book::Psalms, 3, 0)))
    );
    assert_eq!(c.ancestors(psalm), vec![3]);
    assert!(!c.is_filterable());
    assert!(c.filter("gen").is_empty());
}

#[test]
fn a_commentary_lists_volumes_books_chapters_and_sections_introductions_first() {
    let (nav, volumes) = mhc();
    let c = Contents::commentary(&nav, &volumes);
    assert_eq!(
        outline(&c),
        [
            "Volume 1 (Genesis–Exodus)",
            "  Genesis",
            "    Introduction",
            "    Chapter 1",
            "      Introduction",
            "      Verses 1–2",
            "      Verses 3–5",
            "    Chapter 2",
            "      Verses 1–3",
            "      Verse 4",
            "  Exodus",
            "    Chapter 1",
            "      Verses 1–7",
            "Volume 5 (Matthew)",
            "  Matthew",
            "    Chapter 1",
            "      Verses 1–17",
        ]
    );
    // Each section line is found by its unit (what the view shows at its top).
    let book_intro = c
        .at_unit(&UnitId::new("Gen.1.1-Gen.50.26.book-intro").unwrap())
        .unwrap();
    assert_eq!(c.node(book_intro).label, "Introduction");
    assert_eq!(c.ancestors(book_intro), vec![1, 0]);
    let chapter_intro = c
        .at_unit(&UnitId::new("Gen.1.1-Gen.1.31.chapter-intro").unwrap())
        .unwrap();
    assert_eq!(c.ancestors(chapter_intro), vec![3, 1, 0]);
    // A group opens at its first section.
    assert_eq!(
        c.node(0).target,
        Some(ContentsTarget::Entry(section(
            v(Book::Genesis, 1, 1),
            v(Book::Genesis, 50, 26),
            "Gen.1.1-Gen.50.26.book-intro"
        )))
    );
    assert!(!c.is_filterable());
}

#[test]
fn a_commentary_without_volumes_starts_at_its_books_and_labels_whole_chapters() {
    let nav = ResourceNavigation::new(vec![
        section(
            v(Book::Psalms, 117, 1),
            v(Book::Psalms, 117, 2),
            "Ps.117.1-Ps.117.2",
        ),
        section(
            v(Book::Psalms, 116, 1),
            v(Book::Psalms, 116, 9),
            "Ps.116.1-Ps.116.9",
        ),
        section(
            v(Book::Psalms, 116, 10),
            v(Book::Psalms, 117, 1),
            "Ps.116.10-Ps.117.1",
        ),
    ]);
    let c = Contents::commentary(&nav, &BTreeMap::new());
    assert_eq!(
        outline(&c),
        [
            "Psalms",
            "  Chapter 116",
            "    Verses 1–9",
            "    116:10–117:1",
            "  Chapter 117",
            "    Whole chapter",
        ]
    );
}

#[test]
fn dictionary_entries_group_a_to_z_and_the_filter_finds_abraham_first() {
    let nav = ResourceNavigation::new(vec![
        entry(Key::Headword("Aaron".into()), "Aaron", false),
        entry(Key::Headword("Abraham".into()), "Abraham", false),
        entry(Key::Headword("Abram".into()), "Abraham", true),
        entry(
            Key::Headword("Abraham’s bosom".into()),
            "Abraham’s bosom",
            false,
        ),
        entry(Key::Headword("Babel".into()), "Babel", false),
        entry(Key::Headword("Ezra".into()), "Ezra", false),
    ]);
    let units = [
        unit("Aaron", "Aaron"),
        unit("Abraham", "Abraham"),
        unit("Abraham’s bosom", "Abraham’s bosom"),
        unit("Babel", "Babel"),
        unit("Ezra", "Ezra"),
    ];
    let c = Contents::build(
        ResourceType::Dictionary,
        &[],
        &units,
        &nav,
        &BTreeMap::new(),
    );
    assert_eq!(
        outline(&c),
        [
            "A",
            "  Aaron",
            "  Abraham",
            "  Abraham’s bosom",
            "B",
            "  Babel",
            "E",
            "  Ezra"
        ]
    );
    assert!(c.is_filterable());
    let labels = |q: &str| -> Vec<String> {
        c.filter(q)
            .into_iter()
            .map(|n| c.node(n).label.clone())
            .collect()
    };
    assert_eq!(labels("Abraham"), ["Abraham", "Abraham’s bosom"]);
    // Case and accents aside; entries starting with the text before those containing it.
    assert_eq!(labels("  ABRA "), ["Abraham", "Abraham’s bosom"]);
    assert_eq!(labels("ra"), ["Abraham", "Abraham’s bosom", "Ezra"]);
    assert!(labels("").is_empty());
    let abraham = c.at_unit(&UnitId::new("Abraham").unwrap()).unwrap();
    assert_eq!(
        c.node(abraham).target,
        Some(ContentsTarget::Entry(entry(
            Key::Headword("Abraham".into()),
            "Abraham",
            false
        )))
    );
}

#[test]
fn lexicon_entries_show_the_lemma_and_strongs_number_under_the_lemmas_letter() {
    let strong = |s: &str| Key::StrongsNumber(s.parse().unwrap());
    let nav = ResourceNavigation::new(vec![
        entry(strong("G25"), "G25", false),
        entry(strong("G26"), "G26", false),
        entry(Key::Lemma("ἀγάπη".into()), "G26", false),
        entry(strong("G907"), "G907", false),
    ]);
    let units = [
        unit("G25", "ἀγαπάω"),
        unit("G26", "ἀγάπη"),
        unit("G907", "βαπτίζω"),
    ];
    let c = Contents::entries(&units, &nav);
    assert_eq!(
        outline(&c),
        [
            "Α",
            "  ἀγαπάω (G25)",
            "  ἀγάπη (G26)",
            "Β",
            "  βαπτίζω (G907)"
        ]
    );
    let found: Vec<&str> = c
        .filter("g26")
        .into_iter()
        .map(|n| c.node(n).label.as_str())
        .collect();
    assert_eq!(found, ["ἀγάπη (G26)"]);
    assert_eq!(c.filter("αγαπη").len(), 1);
}

fn open(
    s: &mut Session,
    id: &str,
    ty: ResourceType,
    schemes: Vec<KeyScheme>,
    nav: Option<ResourceNavigation>,
) -> TabId {
    let mut manifest = Manifest::new(id, id, id, ty, "en");
    manifest.key_schemes = schemes;
    s.library.add(LibraryEntry {
        manifest,
        path: format!("/{id}.obr").into(),
    });
    let t = TabId(
        s.run("resource.open", &json!({"id": id})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    s.workspace.tab_mut(t).unwrap().navigation = nav.map(Arc::new);
    s.workspace.set_link(t, Some('A'));
    t
}

#[test]
fn clicking_a_line_jumps_the_tab_and_its_link_set_like_any_jump() {
    let mut s = Session::new(JobMode::Inline);
    let bible = open(
        &mut s,
        "bible",
        ResourceType::Bible,
        vec![KeyScheme::Passage],
        None,
    );
    let (nav, volumes) = mhc();
    let commentary = open(
        &mut s,
        "mhc",
        ResourceType::Commentary,
        vec![KeyScheme::Passage],
        Some(nav.clone()),
    );
    let c = Contents::commentary(&nav, &volumes);
    s.workspace.take_effects();

    // A Bible chapter: the Bible jumps; the Commentary follows to its section.
    let ge2 = v(Book::Genesis, 2, 1);
    contents::go(&mut s.workspace, bible, &ContentsTarget::Verse(ge2));
    assert_eq!(s.workspace.tab(bible).unwrap().history.current(), Some(ge2));
    assert_eq!(
        s.workspace.tab(commentary).unwrap().history.current(),
        Some(ge2)
    );
    let effects = s.workspace.take_effects();
    assert!(effects.contains(&Effect::Navigate {
        tab: bible,
        verse: ge2
    }));

    // A Commentary section its Passage reaches: an ordinary jump; the Bible follows.
    let line = c.at_unit(&UnitId::new("Gen.1.3-Gen.1.5").unwrap()).unwrap();
    let target = c.node(line).target.clone().unwrap();
    contents::go(&mut s.workspace, commentary, &target);
    let ge13 = v(Book::Genesis, 1, 3);
    assert_eq!(
        s.workspace.tab(commentary).unwrap().history.current(),
        Some(ge13)
    );
    assert_eq!(
        s.workspace.tab(bible).unwrap().history.current(),
        Some(ge13)
    );
    assert!(s.workspace.take_effects().contains(&Effect::Navigate {
        tab: commentary,
        verse: ge13
    }));

    // The book introduction starts where Gen 1:1-2 does: it is reached by its unit.
    let intro = c
        .at_unit(&UnitId::new("Gen.1.1-Gen.50.26.book-intro").unwrap())
        .unwrap();
    let target = c.node(intro).target.clone().unwrap();
    contents::go(&mut s.workspace, commentary, &target);
    let ge11 = v(Book::Genesis, 1, 1);
    assert_eq!(
        s.workspace.tab(commentary).unwrap().history.current(),
        Some(ge11)
    );
    assert_eq!(
        s.workspace.tab(bible).unwrap().history.current(),
        Some(ge11)
    );
    let effects = s.workspace.take_effects();
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::NavigateText { tab, unit, .. }
            if *tab == commentary && unit.as_str() == "Gen.1.1-Gen.50.26.book-intro"
    )));
    // Each is a step in the Tab's history.
    s.run("tab.back", &json!({"tab": commentary.0})).unwrap();
    assert_eq!(
        s.workspace.tab(commentary).unwrap().history.current(),
        Some(ge13)
    );
    assert_eq!(s.workspace.tab(commentary).unwrap().history.unit(), None);
    assert!(s.workspace.take_effects().contains(&Effect::Navigate {
        tab: commentary,
        verse: ge13
    }));
}

/// The unit a Commentary's book introduction was reached by, as Back and Forward go to it.
fn returned_to_unit(s: &mut Session, tab: TabId, unit: &str) -> bool {
    let effects = s.workspace.take_effects();
    let stop = s.workspace.tab(tab).unwrap().history.unit().cloned();
    stop.is_some_and(|stop| stop.unit.as_str() == unit)
        && effects.iter().any(|e| {
            matches!(e, Effect::NavigateText { tab: t, unit: u, key, .. }
                if *t == tab
                    && u.as_str() == unit
                    && *key == Some(Key::Passage(Passage::verse(v(Book::Genesis, 1, 1)))))
        })
}

#[test]
fn back_and_forward_return_to_an_introduction_reached_by_its_unit() {
    let mut s = Session::new(JobMode::Inline);
    let (nav, volumes) = mhc();
    let commentary = open(
        &mut s,
        "mhc",
        ResourceType::Commentary,
        vec![KeyScheme::Passage],
        Some(nav.clone()),
    );
    let c = Contents::commentary(&nav, &volumes);
    let go_to = |s: &mut Session, unit: &str| {
        let line = c.at_unit(&UnitId::new(unit).unwrap()).unwrap();
        let target = c.node(line).target.clone().unwrap();
        contents::go(&mut s.workspace, commentary, &target);
    };
    let history = |s: &Session| s.workspace.tab(commentary).unwrap().history.clone();
    const BOOK_INTRO: &str = "Gen.1.1-Gen.50.26.book-intro";
    const CHAPTER_INTRO: &str = "Gen.1.1-Gen.1.31.chapter-intro";
    let ge11 = v(Book::Genesis, 1, 1);

    go_to(&mut s, "Gen.2.1-Gen.2.3");
    go_to(&mut s, BOOK_INTRO);
    // The section sharing Gen 1:1 and the chapter introduction: each a step of its own,
    // though all three start at Gen 1:1.
    go_to(&mut s, "Gen.1.1-Gen.1.2");
    assert_eq!(history(&s).current(), Some(ge11));
    assert_eq!(history(&s).unit(), None);
    go_to(&mut s, CHAPTER_INTRO);
    go_to(&mut s, BOOK_INTRO);
    s.workspace.take_effects();

    // Back: the chapter introduction, the section, the book introduction, Genesis 2.
    s.run("tab.back", &json!({"tab": commentary.0})).unwrap();
    assert!(returned_to_unit(&mut s, commentary, CHAPTER_INTRO));
    s.run("tab.back", &json!({"tab": commentary.0})).unwrap();
    assert_eq!(history(&s).current(), Some(ge11));
    assert_eq!(history(&s).unit(), None);
    assert!(s.workspace.take_effects().contains(&Effect::Navigate {
        tab: commentary,
        verse: ge11
    }));
    s.run("tab.back", &json!({"tab": commentary.0})).unwrap();
    assert!(returned_to_unit(&mut s, commentary, BOOK_INTRO));
    s.run("tab.back", &json!({"tab": commentary.0})).unwrap();
    assert_eq!(history(&s).current(), Some(v(Book::Genesis, 2, 1)));
    assert!(!history(&s).can_go_back());

    // Forward retraces them.
    s.workspace.take_effects();
    s.run("tab.forward", &json!({"tab": commentary.0})).unwrap();
    assert!(returned_to_unit(&mut s, commentary, BOOK_INTRO));
    s.run("tab.forward", &json!({"tab": commentary.0})).unwrap();
    assert_eq!(history(&s).unit(), None);
    s.run("tab.forward", &json!({"tab": commentary.0})).unwrap();
    assert!(returned_to_unit(&mut s, commentary, CHAPTER_INTRO));
    s.run("tab.forward", &json!({"tab": commentary.0})).unwrap();
    assert!(returned_to_unit(&mut s, commentary, BOOK_INTRO));
    assert!(!history(&s).can_go_forward());

    // Scrolling on to another Verse leaves the introduction: Back from a later jump goes to
    // that Verse, not the introduction.
    s.workspace
        .user_scrolled(commentary, v(Book::Genesis, 1, 3));
    assert_eq!(history(&s).unit(), None);
    go_to(&mut s, "Gen.2.4-Gen.2.4");
    s.workspace.take_effects();
    s.run("tab.back", &json!({"tab": commentary.0})).unwrap();
    assert_eq!(history(&s).current(), Some(v(Book::Genesis, 1, 3)));
    assert_eq!(history(&s).unit(), None);

    // Scrolling from the introduction into the section sharing its first Verse leaves the
    // introduction though the Verse is unchanged: Back from a later jump goes to the section.
    let section = UnitId::new("Gen.1.1-Gen.1.2").unwrap();
    go_to(&mut s, BOOK_INTRO);
    s.workspace
        .user_scrolled_into(commentary, &UnitId::new(BOOK_INTRO).unwrap());
    assert!(history(&s).unit().is_some(), "still in the introduction");
    s.workspace.user_scrolled_into(commentary, &section);
    assert_eq!(history(&s).current(), Some(ge11));
    assert_eq!(history(&s).unit(), None);
    go_to(&mut s, "Gen.2.4-Gen.2.4");
    s.workspace.take_effects();
    s.run("tab.back", &json!({"tab": commentary.0})).unwrap();
    assert_eq!(history(&s).current(), Some(ge11));
    assert_eq!(history(&s).unit(), None);
    assert!(s.workspace.take_effects().contains(&Effect::Navigate {
        tab: commentary,
        verse: ge11
    }));
}

#[test]
fn clicking_an_entry_jumps_the_dictionary_tab_and_its_link_set() {
    let mut s = Session::new(JobMode::Inline);
    let nav = ResourceNavigation::new(vec![
        entry(Key::Headword("Aaron".into()), "Aaron", false),
        entry(Key::Headword("Abraham".into()), "Abraham", false),
    ]);
    let a = open(
        &mut s,
        "easton",
        ResourceType::Dictionary,
        vec![KeyScheme::Headword],
        Some(nav.clone()),
    );
    let b = open(
        &mut s,
        "smith",
        ResourceType::Dictionary,
        vec![KeyScheme::Headword],
        Some(nav.clone()),
    );
    let c = Contents::entries(&[unit("Aaron", "Aaron"), unit("Abraham", "Abraham")], &nav);
    s.workspace.take_effects();
    let abraham = c.filter("Abraham")[0];
    contents::go(
        &mut s.workspace,
        a,
        &c.node(abraham).target.clone().unwrap(),
    );
    let key = Key::Headword("Abraham".into());
    assert_eq!(
        s.workspace.tab(a).unwrap().entry_history.current(),
        Some(key.clone())
    );
    assert_eq!(
        s.workspace.tab(b).unwrap().entry_history.current(),
        Some(key.clone())
    );
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::NavigateEntry { tab: a, key })
    );
}

#[test]
fn tab_contents_toggles_on_resource_tabs_only_and_is_kept_in_layouts() {
    let mut s = Session::new(JobMode::Inline);
    let bible = open(
        &mut s,
        "bible",
        ResourceType::Bible,
        vec![KeyScheme::Passage],
        None,
    );
    assert!(!s.workspace.tab(bible).unwrap().contents);
    let out = s.run("tab.contents", &json!({"tab": bible.0})).unwrap();
    assert_eq!(out["on"], true);
    assert!(s.workspace.tab(bible).unwrap().contents);
    let snap = s.workspace.snapshot();
    let back = crate::persist::WorkspaceSnapshot::from_json(&snap.to_json()).unwrap();
    assert_eq!(back, snap);
    s.workspace.restore(back);
    let restored = s
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("bible".into()))
        .unwrap();
    assert!(restored.contents);
    let id = restored.id;
    s.run("tab.contents", &json!({"tab": id.0, "on": false}))
        .unwrap();
    assert!(!s.workspace.tab(id).unwrap().contents);
    // A Tab without the pane saves nothing extra.
    assert!(!s.workspace.snapshot().to_json().contains("contents"));
    let library = TabId(
        s.run("library.open", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    assert!(s.run("tab.contents", &json!({"tab": library.0})).is_err());
}

/// The Starter library's KJV, Matthew Henry and Easton as `cargo xtask starter` builds them.
/// Not run by default (they are downloads):
/// `OBS_REAL_STARTER=dist/starter cargo test -p obs-engine -- --ignored real_starter_contents`
#[test]
#[ignore = "needs the built Starter library"]
fn real_starter_contents_of_kjv_mhc_and_easton() {
    let dir =
        std::path::PathBuf::from(std::env::var("OBS_REAL_STARTER").expect("set OBS_REAL_STARTER"));
    use obs_store::ResourceStore;
    let contents = |id: &str| {
        let reader = obs_store::ObrReader::open(&dir.join(format!("{id}.obr"))).unwrap();
        let r = reader.load().unwrap();
        let units: Vec<TextUnit> = r
            .units
            .into_iter()
            .filter(|u| u.kind != UnitKind::Note)
            .collect();
        let started = std::time::Instant::now();
        let c = Contents::build(
            r.manifest.resource_type,
            &r.verses,
            &units,
            &ResourceNavigation::new(r.keys),
            &r.volumes,
        );
        eprintln!("{id}: {} lines built in {:?}", c.len(), started.elapsed());
        c
    };
    let kjv = contents("kjv-strongs");
    let roots: Vec<&str> = kjv
        .roots()
        .iter()
        .map(|&n| kjv.node(n).label.as_str())
        .collect();
    assert_eq!(roots.len(), 66);
    assert_eq!(roots[0], "Genesis");
    assert_eq!(roots[65], "Revelation");
    assert_eq!(kjv.len(), 66 + 1189);
    let ps = kjv.at_verse(v(Book::Psalms, 119, 1)).unwrap();
    assert_eq!(kjv.node(ps).label, "Chapter 119");

    let mhc = contents("mhc");
    let roots: Vec<&str> = mhc
        .roots()
        .iter()
        .map(|&n| mhc.node(n).label.as_str())
        .collect();
    eprintln!("MHC: {roots:?}");
    assert_eq!(
        roots,
        [
            "Volume 1 (Genesis–Deuteronomy)",
            "Volume 2 (Joshua–Esther)",
            "Volume 3 (Job–Song of Songs)",
            "Volume 4 (Isaiah–Malachi)",
            "Volume 5 (Matthew–John)",
            "Volume 6 (Acts–Revelation)",
        ]
    );
    let genesis = mhc.node(mhc.node(mhc.roots()[0]).children[0]);
    assert_eq!(genesis.label, "Genesis");
    let first: Vec<&str> = genesis.children[..3]
        .iter()
        .map(|&n| mhc.node(n).label.as_str())
        .collect();
    assert_eq!(first, ["Introduction", "Chapter 1", "Chapter 2"]);

    let easton = contents("easton");
    let letters: Vec<&str> = easton
        .roots()
        .iter()
        .map(|&n| easton.node(n).label.as_str())
        .collect();
    eprintln!("Easton: {letters:?}");
    assert_eq!(letters.first(), Some(&"A"));
    let started = std::time::Instant::now();
    let found = easton.filter("Abraham");
    let took = started.elapsed();
    eprintln!(
        "Easton filter \"Abraham\": {} found in {took:?}",
        found.len()
    );
    assert_eq!(easton.node(found[0]).label, "Abraham");
    assert!(took < std::time::Duration::from_millis(16), "{took:?}");
}

#[test]
fn back_returns_to_a_second_entry_of_the_same_headword_by_its_unit() {
    let mut s = Session::new(JobMode::Inline);
    let abel = Key::Headword("Abel".into());
    let nav = ResourceNavigation::new(vec![
        entry(abel.clone(), "Abel-1", false),
        entry(abel.clone(), "Abel-2", false),
        entry(Key::Headword("Aaron".into()), "Aaron", false),
    ]);
    let tab = open(
        &mut s,
        "smith",
        ResourceType::Dictionary,
        vec![KeyScheme::Headword],
        Some(nav.clone()),
    );
    let c = Contents::entries(
        &[
            unit("Aaron", "Aaron"),
            unit("Abel-1", "Abel"),
            unit("Abel-2", "Abel"),
        ],
        &nav,
    );
    let go_to = |s: &mut Session, unit: &str| {
        let line = c.at_unit(&UnitId::new(unit).unwrap()).unwrap();
        contents::go(&mut s.workspace, tab, &c.node(line).target.clone().unwrap());
    };
    go_to(&mut s, "Aaron");
    go_to(&mut s, "Abel-2");
    go_to(&mut s, "Abel-1");
    s.workspace.take_effects();
    let history = |s: &Session| s.workspace.tab(tab).unwrap().entry_history.clone();
    assert_eq!(history(&s).current(), Some(abel.clone()));
    assert_eq!(history(&s).unit(), None);

    // The same headword, another unit: a step of its own, and Back goes to that unit.
    s.run("tab.back", &json!({"tab": tab.0})).unwrap();
    assert_eq!(
        history(&s)
            .unit()
            .map(|stop| stop.unit.as_str().to_string()),
        Some("Abel-2".to_string())
    );
    assert!(s.workspace.take_effects().iter().any(|e| matches!(
        e,
        Effect::NavigateText { tab: t, unit, key, .. }
            if *t == tab && unit.as_str() == "Abel-2" && *key == Some(abel.clone())
    )));
    s.run("tab.back", &json!({"tab": tab.0})).unwrap();
    assert_eq!(history(&s).current(), Some(Key::Headword("Aaron".into())));
    assert!(s.workspace.take_effects().contains(&Effect::NavigateEntry {
        tab,
        key: Key::Headword("Aaron".into())
    }));
}
