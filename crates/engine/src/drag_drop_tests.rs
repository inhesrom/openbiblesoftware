//! `workspace.drop` (#216): what a Bible reference or a word dropped on the Workspace does.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use obs_model::{KeyScheme, Manifest, ResourceType};
use obs_scripture::{Book, ListedVersification, Passage, Standard, Verse, VerseMap, VerseMapEntry};
use serde_json::json;

use crate::*;

fn entry(id: &str, abbrev: &str, ty: ResourceType, schemes: Vec<KeyScheme>) -> LibraryEntry {
    let mut manifest = Manifest::new(id, abbrev, abbrev, ty, "en");
    manifest.key_schemes = schemes;
    LibraryEntry {
        manifest,
        path: format!("/library/{id}.obr").into(),
    }
}

/// KJV (★) and WEB, a Commentary and a Lexicon; KJV open in one Panel, the Commentary in
/// another.
fn session() -> (Session, TabId, TabId) {
    let mut s = Session::new(JobMode::Inline);
    let passage = || vec![KeyScheme::Passage];
    for e in [
        entry("kjv", "KJV", ResourceType::Bible, passage()),
        entry("web", "WEB", ResourceType::Bible, passage()),
        entry("mhc", "MHC", ResourceType::Commentary, passage()),
        entry(
            "abbott",
            "Abbott",
            ResourceType::Lexicon,
            vec![KeyScheme::StrongsNumber, KeyScheme::Lemma],
        ),
    ] {
        s.library.add(e);
    }
    let kjv = open(&mut s, json!({ "id": "kjv" }));
    let mhc = open(&mut s, json!({ "id": "mhc", "split": true }));
    s.workspace.take_effects();
    (s, kjv, mhc)
}

fn open(s: &mut Session, params: serde_json::Value) -> TabId {
    TabId(
        s.run("resource.open", &params).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    )
}

fn rom_8_28() -> Verse {
    Verse::new(Book::Romans, 8, 28)
}

fn panel_of(s: &Session, tab: TabId) -> PanelId {
    s.workspace.panel_of(tab).unwrap()
}

#[test]
fn a_reference_dropped_on_a_bible_tab_navigates_it() {
    let (mut s, kjv, mhc) = session();
    let tabs = s.workspace.tabs().count();
    let out = s
        .run(
            "workspace.drop",
            &json!({ "passage": "Rom 8:28", "tab": kjv.0 }),
        )
        .unwrap();
    assert_eq!(out["tab"], kjv.0);
    assert_eq!(out["opened"], false);
    let t = s.workspace.tab(kjv).unwrap();
    assert_eq!(t.history.current(), Some(rom_8_28()));
    assert_eq!(s.workspace.focused(), Some(kjv));
    assert_eq!(s.workspace.tabs().count(), tabs, "no new Tab");
    // The Commentary it was dragged from stays where it was.
    assert_eq!(s.workspace.tab(mhc).unwrap().history.current(), None);
}

#[test]
fn a_reference_dropped_on_a_panel_edge_opens_the_star_bible_in_a_new_panel_beside_it() {
    let (mut s, kjv, _) = session();
    let panels = s.workspace.panels().count();
    let beside = panel_of(&s, kjv);
    let out = s
        .run(
            "workspace.drop",
            &json!({ "passage": "Rom 8:28", "beside": beside.0 }),
        )
        .unwrap();
    assert_eq!(out["opened"], true);
    assert_eq!(out["bible"], "kjv");
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(s.workspace.panels().count(), panels + 1);
    assert_ne!(panel_of(&s, tab), beside);
    let effects = s.workspace.take_effects();
    assert!(
        effects.iter().any(|e| matches!(
            e,
            Effect::Opened { tab: t, placement: Placement::NewPanel { beside: Some(b), .. } }
                if *t == tab && *b == beside
        )),
        "{effects:?}"
    );
    assert!(
        effects.contains(&Effect::NavigateStandard {
            tab,
            verse: rom_8_28()
        }),
        "{effects:?}"
    );
}

#[test]
fn a_reference_dropped_on_a_tab_bar_opens_the_bible_it_came_from_in_that_panel() {
    let (mut s, _, mhc) = session();
    let panel = panel_of(&s, mhc);
    let out = s
        .run(
            "workspace.drop",
            &json!({ "passage": "Romans 8:28-30", "panel": panel.0, "bible": "web" }),
        )
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(panel_of(&s, tab), panel);
    assert_eq!(
        s.workspace.tab(tab).unwrap().kind,
        TabKind::Resource("web".into())
    );
    assert_eq!(out["passage"], "Romans 8:28-30");
    // Not a Bible: the ★ Bible instead.
    let out = s
        .run(
            "workspace.drop",
            &json!({ "passage": "Rom 8:28", "panel": panel.0, "bible": "mhc" }),
        )
        .unwrap();
    assert_eq!(out["bible"], "kjv");
}

#[test]
fn a_reference_dropped_on_a_tab_that_takes_no_passages_opens_a_bible_in_its_panel() {
    let (mut s, kjv, _) = session();
    let lexicon = open(&mut s, json!({ "id": "abbott", "split": true }));
    let out = s
        .run(
            "workspace.drop",
            &json!({ "passage": "Rom 8:28", "tab": lexicon.0 }),
        )
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_ne!(tab, kjv);
    assert_eq!(panel_of(&s, tab), panel_of(&s, lexicon));
}

#[test]
fn a_reference_dropped_on_a_passage_guide_shows_a_range_whole() {
    let (mut s, _, _) = session();
    let out = s.run("guide.passage_guide", &json!({})).unwrap();
    let guide = TabId(out["tab"].as_u64().unwrap());
    s.run(
        "workspace.drop",
        &json!({ "passage": "Rom 8:28-30", "tab": guide.0 }),
    )
    .unwrap();
    let t = s.workspace.tab(guide).unwrap();
    assert_eq!(
        t.shown_passage(),
        Passage::new(rom_8_28(), Verse::new(Book::Romans, 8, 30)).ok()
    );
}

#[test]
fn dropped_text_that_is_a_reference_navigates_and_a_word_is_looked_up_or_searched() {
    let (mut s, kjv, mhc) = session();
    // A selected "Rom 8:28" (not marked up as a reference) still navigates.
    s.run(
        "workspace.drop",
        &json!({ "text": " Rom 8:28 ", "tab": kjv.0 }),
    )
    .unwrap();
    assert_eq!(
        s.workspace.tab(kjv).unwrap().history.current(),
        Some(rom_8_28())
    );
    // A word is no place in a Bible.
    let err = s
        .run("workspace.drop", &json!({ "text": "love", "tab": kjv.0 }))
        .unwrap_err();
    assert!(
        err.to_string().contains("“love” is not a Bible reference"),
        "{err}"
    );
    // A Lexicon looks it up as typed into its box.
    let lexicon = open(&mut s, json!({ "id": "abbott", "split": true }));
    let err = s
        .run(
            "workspace.drop",
            &json!({ "text": "love", "tab": lexicon.0 }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("No entry for love"), "{err}");
    // On a Panel's tab bar: a Search for it in that Panel.
    let panel = panel_of(&s, mhc);
    let out = s
        .run(
            "workspace.drop",
            &json!({ "text": "love", "panel": panel.0 }),
        )
        .unwrap();
    let search = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(
        s.workspace.tab(search).unwrap().kind,
        TabKind::Tool(Tool::Search)
    );
    assert_eq!(panel_of(&s, search), panel);
    assert_eq!(s.search_tab(search).unwrap().query, "love");
    // On that Search Tab: it searches again.
    s.run(
        "workspace.drop",
        &json!({ "text": "grace", "tab": search.0 }),
    )
    .unwrap();
    assert_eq!(s.search_tab(search).unwrap().query, "grace");
    // A bare book name is a word, not the whole book.
    let out = s
        .run(
            "workspace.drop",
            &json!({ "text": "Job", "panel": panel.0 }),
        )
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(
        s.workspace.tab(tab).unwrap().kind,
        TabKind::Tool(Tool::Search)
    );
}

fn ge(chapter: u16, verse: u16) -> Verse {
    Verse::new(Book::Genesis, chapter, verse)
}

/// HEB, a Bible numbering Genesis 31-32 the Hebrew way (its 32:1 is the Standard 31:55, its
/// 32:n the Standard 32:n-1; its 50:27 is Unmapped), open in its own Panel.
fn hebrew_numbered_bible(s: &mut Session) -> TabId {
    s.library.add(entry(
        "heb",
        "HEB",
        ResourceType::Bible,
        vec![KeyScheme::Passage],
    ));
    let heb = open(s, json!({ "id": "heb", "split": true }));
    let at = |native: Verse, standard: Option<Verse>| VerseMapEntry {
        verse: native,
        last: native.verse,
        standard: standard.map(Passage::verse),
    };
    let mut entries: Vec<_> = (1..=54).map(|v| at(ge(31, v), Some(ge(31, v)))).collect();
    entries.push(at(ge(32, 1), Some(ge(31, 55))));
    entries.extend((2..=33).map(|v| at(ge(32, v), Some(ge(32, v - 1)))));
    entries.push(at(ge(50, 27), None));
    let map = VerseMap::new(entries, vec![], &Standard);
    let t = s.workspace.tab_mut(heb).unwrap();
    t.versification = Some(Arc::new(ListedVersification::new(
        map.entries().iter().map(|e| (e.verse, e.last)),
    )));
    t.verse_map = Some(Arc::new(map));
    s.workspace.take_effects();
    heb
}

#[test]
fn a_reference_selected_in_a_bible_is_read_in_that_bibles_numbering() {
    let (mut s, kjv, mhc) = session();
    let heb = hebrew_numbered_bible(&mut s);
    // "Genesis 32:1" selected in HEB is the Standard Genesis 31:55: KJV goes there.
    let out = s
        .run(
            "workspace.drop",
            &json!({ "text": "Genesis 32:1", "from": heb.0, "tab": kjv.0 }),
        )
        .unwrap();
    assert_eq!(out["verse"], "Gen.31.55");
    assert_eq!(at(&s, kjv), Some(ge(31, 55)));
    // A range keeps both ends, each through the map: HEB 32:2-3 is the Standard 32:1-2.
    let out = s.run("guide.passage_guide", &json!({})).unwrap();
    let guide = TabId(out["tab"].as_u64().unwrap());
    s.run(
        "workspace.drop",
        &json!({ "text": "Gen 32:2-3", "from": heb.0, "tab": guide.0 }),
    )
    .unwrap();
    assert_eq!(
        s.workspace.tab(guide).unwrap().shown_passage(),
        Passage::new(ge(32, 1), ge(32, 2)).ok()
    );
    // On a tab bar, the new Tab of HEB opens at its own 32:1 (the Standard 31:55).
    s.workspace.take_effects();
    let out = s
        .run(
            "workspace.drop",
            &json!({
                "text": "Genesis 32:1",
                "from": heb.0,
                "bible": "heb",
                "panel": panel_of(&s, mhc).0,
            }),
        )
        .unwrap();
    assert_eq!(out["bible"], "heb");
    assert_eq!(out["verse"], "Gen.31.55");
    // The same words in a Commentary (no `from` Bible) are Standard numbering.
    s.run(
        "workspace.drop",
        &json!({ "text": "Genesis 32:1", "from": mhc.0, "tab": kjv.0 }),
    )
    .unwrap();
    assert_eq!(at(&s, kjv), Some(ge(32, 1)));
    // An Unmapped verse has nowhere to go.
    let err = s
        .run(
            "workspace.drop",
            &json!({ "text": "Gen 50:27", "from": heb.0, "tab": kjv.0 }),
        )
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("“Gen 50:27” has no Standard equivalent"),
        "{err}"
    );
    assert_eq!(at(&s, kjv), Some(ge(32, 1)));
}

fn at(s: &Session, tab: TabId) -> Option<Verse> {
    s.workspace.tab(tab).unwrap().history.current()
}

#[test]
fn a_drop_needs_something_dropped_and_a_live_place() {
    let (mut s, _, _) = session();
    for params in [
        json!({ "tab": 999, "passage": "Rom 8:28" }),
        json!({ "panel": 999, "passage": "Rom 8:28" }),
        json!({ "passage": "Nowhere 1:1" }),
        json!({ "text": "  " }),
    ] {
        assert!(s.run("workspace.drop", &params).is_err(), "{params}");
    }
    // An empty Workspace: a new Tab of the ★ Bible.
    s.workspace.clear();
    let out = s
        .run("workspace.drop", &json!({ "passage": "Rom 8:28" }))
        .unwrap();
    assert_eq!(out["bible"], "kjv");
    // No Bible to open.
    let mut empty = Session::new(JobMode::Inline);
    let err = empty
        .run("workspace.drop", &json!({ "passage": "Rom 8:28" }))
        .unwrap_err();
    assert!(err.to_string().contains("Add a Bible"), "{err}");
}
