#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use obs_model::{Key, KeyEntry, KeyScheme, Manifest, ResourceType, UnitId};
use obs_scripture::{Book, Passage, Verse};
use serde_json::json;

use crate::resource_navigation::ResourceNavigation;
use crate::*;

fn ge(chapter: u16, verse: u16) -> Verse {
    Verse::new(Book::Genesis, chapter, verse)
}

fn entry(key: Key, unit: &str, alias: bool) -> KeyEntry {
    KeyEntry {
        key,
        unit: UnitId::new(unit).unwrap(),
        range: None,
        alias,
    }
}

fn section(start: Verse, end: Verse, unit: &str) -> KeyEntry {
    entry(Key::Passage(Passage::new(start, end).unwrap()), unit, false)
}

fn strong(text: &str) -> Key {
    Key::StrongsNumber(text.parse().unwrap())
}

fn lexicon() -> ResourceNavigation {
    ResourceNavigation::new(vec![
        entry(strong("G25"), "G25", false),
        entry(strong("G26"), "G26", false),
        entry(Key::Lemma("ἀγάπη".into()), "G26", false),
        entry(strong("G27"), "G27", false),
    ])
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
fn commentary_nearest_earlier_section_orders_standard_passages_not_unit_ids() {
    let nav = ResourceNavigation::new(vec![
        section(ge(32, 5), ge(32, 9), "a"),
        section(ge(31, 50), ge(31, 54), "z"),
        section(ge(31, 50), ge(31, 55), "introduction"),
    ]);
    for target in [ge(31, 50), ge(31, 53), ge(31, 55), ge(32, 4)] {
        assert_eq!(nav.section(target).unwrap().unit.as_str(), "z");
    }
    assert_eq!(nav.section(ge(32, 5)).unwrap().unit.as_str(), "a");
    assert_eq!(
        nav.section(Verse::new(Book::Exodus, 1, 1))
            .unwrap()
            .unit
            .as_str(),
        "a"
    );
    assert!(nav.section(ge(31, 49)).is_none());
    assert!(ResourceNavigation::default().section(ge(1, 1)).is_none());
}

#[test]
fn commentary_follow_keeps_bible_on_requested_verse_and_uses_standard_reference_box() {
    let mut s = Session::new(JobMode::Inline);
    let bible = open(
        &mut s,
        "bible",
        ResourceType::Bible,
        vec![KeyScheme::Passage],
        None,
    );
    let commentary = open(
        &mut s,
        "commentary",
        ResourceType::Commentary,
        vec![KeyScheme::Passage],
        Some(ResourceNavigation::new(vec![
            section(ge(31, 50), ge(31, 54), "section"),
            section(ge(32, 5), ge(32, 9), "next"),
        ])),
    );
    s.workspace.take_effects();
    s.workspace.navigate(bible, ge(31, 55));
    assert_eq!(
        s.workspace.tab(bible).unwrap().history.current(),
        Some(ge(31, 55))
    );
    assert_eq!(
        s.workspace.tab(commentary).unwrap().history.current(),
        Some(ge(31, 50))
    );
    assert!(s.workspace.take_effects().contains(&Effect::Follow {
        tab: commentary,
        verse: ge(31, 50),
        flash: true
    }));
    s.run("tab.go", &json!({"tab": commentary.0, "text": "Gen 32:4"}))
        .unwrap();
    assert_eq!(
        s.workspace.tab(commentary).unwrap().history.current(),
        Some(ge(31, 50))
    );
    assert_eq!(
        s.workspace.tab(bible).unwrap().history.current(),
        Some(ge(32, 4))
    );
    s.workspace.user_scrolled(bible, ge(32, 6));
    assert_eq!(
        s.workspace.tab(commentary).unwrap().history.current(),
        Some(ge(32, 5))
    );
    assert!(
        s.run("tab.go", &json!({"tab": commentary.0, "text": "Gen 1:1"}))
            .is_err()
    );
    assert_eq!(
        s.workspace.tab(commentary).unwrap().history.current(),
        Some(ge(32, 5))
    );
}

#[test]
fn entries_resolve_strongs_fallback_lemma_transliteration_and_headword_alias() {
    let nav = lexicon();
    for text in ["g26", "G0026", "G26A", "ἀγάπη", "Agape", "AGAPE!"] {
        assert_eq!(
            nav.parse_entry(text).unwrap().unit.as_str(),
            "G26",
            "{text}"
        );
    }
    assert!(nav.parse_entry("G99999").is_none());
    assert_eq!(nav.entries().count(), 3);
    assert_eq!(
        nav.adjacent(&strong("G26"), false).unwrap().key,
        strong("G25")
    );
    assert_eq!(
        nav.adjacent(&strong("G26"), true).unwrap().key,
        strong("G27")
    );
    assert!(nav.adjacent(&strong("G25"), false).is_none());
    assert!(nav.adjacent(&strong("G27"), true).is_none());
    let dictionary = ResourceNavigation::new(vec![
        entry(Key::Headword("Abraham".into()), "Abraham", false),
        entry(Key::Headword("Abram".into()), "Abraham", true),
        entry(Key::Headword("Adam".into()), "Adam", false),
    ]);
    assert_eq!(
        dictionary.parse_entry("  ABRAM ").unwrap().unit.as_str(),
        "Abraham"
    );
    assert_eq!(dictionary.entries().count(), 2);
    assert_eq!(
        dictionary
            .adjacent(&Key::Headword("Abram".into()), true)
            .unwrap()
            .unit
            .as_str(),
        "Adam"
    );
}

#[test]
fn entry_commands_history_links_and_saved_location_share_one_key_contract() {
    let mut s = Session::new(JobMode::Inline);
    let schemes = vec![KeyScheme::StrongsNumber, KeyScheme::Lemma];
    let a = open(
        &mut s,
        "lexicon-a",
        ResourceType::Lexicon,
        schemes.clone(),
        Some(lexicon()),
    );
    let b = open(
        &mut s,
        "lexicon-b",
        ResourceType::Lexicon,
        schemes,
        Some(lexicon()),
    );
    let bible = open(
        &mut s,
        "bible",
        ResourceType::Bible,
        vec![KeyScheme::Passage],
        None,
    );
    assert_eq!(s.workspace.tab(a).unwrap().header_box(), HeaderBox::Lexicon);
    for text in ["G25", "G26"] {
        s.run("tab.go", &json!({"tab": a.0, "text": text})).unwrap();
    }
    assert_eq!(
        s.workspace.tab(b).unwrap().entry_history.current(),
        Some(strong("G26"))
    );
    assert_eq!(s.workspace.tab(bible).unwrap().history.current(), None);
    assert!(!s.workspace.tab(b).unwrap().entry_history.can_go_back());
    s.run("tab.next_entry", &json!({"tab": a.0})).unwrap();
    assert_eq!(
        s.workspace.tab(a).unwrap().entry_history.current(),
        Some(strong("G27"))
    );
    s.run("tab.back", &json!({"tab": a.0})).unwrap();
    assert_eq!(
        s.workspace.tab(b).unwrap().entry_history.current(),
        Some(strong("G26"))
    );
    s.run("tab.forward", &json!({"tab": a.0})).unwrap();
    assert_eq!(
        s.workspace.tab(b).unwrap().entry_history.current(),
        Some(strong("G27"))
    );
    s.workspace.take_effects();
    s.workspace.user_scrolled_entry(a, strong("G26"));
    let effects = s.workspace.take_effects();
    assert_eq!(
        effects,
        vec![Effect::NavigateEntry {
            tab: b,
            key: strong("G26")
        }]
    );
    s.workspace.set_link(b, None);
    s.workspace.navigate_entry(a, strong("G25"));
    s.workspace.set_link(b, Some('A'));
    assert_eq!(
        s.workspace.tab(b).unwrap().entry_history.current(),
        Some(strong("G25"))
    );
    let snap = s.workspace.snapshot();
    assert_eq!(
        snap.tabs
            .iter()
            .find(|t| t.id == a.0)
            .unwrap()
            .entry_location,
        Some(strong("G25"))
    );
    assert_eq!(WorkspaceSnapshot::from_json(&snap.to_json()).unwrap(), snap);
    assert!(
        s.run("tab.go", &json!({"tab": a.0, "text": "unknown"}))
            .is_err()
    );
    assert_eq!(
        s.workspace.tab(a).unwrap().entry_history.current(),
        Some(strong("G25"))
    );
}

#[test]
fn exact_extended_strongs_wins_over_base_and_primary_key_wins_over_alias() {
    let nav = ResourceNavigation::new(vec![
        entry(strong("H1254"), "base", false),
        entry(strong("H1254A"), "extended", false),
        entry(strong("H1254"), "extended", true),
    ]);
    assert_eq!(nav.parse_entry("H1254A").unwrap().unit.as_str(), "extended");
    assert_eq!(nav.parse_entry("H1254B").unwrap().unit.as_str(), "base");
    assert_eq!(nav.parse_entry("H1254").unwrap().unit.as_str(), "base");
}

#[test]
fn scrolling_within_same_entry_does_not_snap_view_or_add_history() {
    let mut s = Session::new(JobMode::Inline);
    let tab = open(
        &mut s,
        "lexicon",
        ResourceType::Lexicon,
        vec![KeyScheme::StrongsNumber],
        Some(lexicon()),
    );
    s.workspace.navigate_entry(tab, strong("G26"));
    s.workspace.take_effects();
    s.workspace.user_scrolled_entry(tab, strong("G26"));
    assert!(s.workspace.take_effects().is_empty());
    assert!(!s.workspace.tab(tab).unwrap().entry_history.can_go_back());
}
