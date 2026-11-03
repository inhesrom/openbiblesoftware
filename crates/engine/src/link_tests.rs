#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use obs_model::{Manifest, ResourceType};
use obs_scripture::{Book, ListedVersification, Passage, Standard, Verse, VerseMap, VerseMapEntry};
use serde_json::json;

use crate::link::{nearest_preceding, to_native};
use crate::*;

fn ge(c: u16, v: u16) -> Verse {
    Verse::new(Book::Genesis, c, v)
}

fn mt(c: u16, v: u16) -> Verse {
    Verse::new(Book::Matthew, c, v)
}

fn entry(native: Verse, std: Option<Verse>) -> VerseMapEntry {
    VerseMapEntry {
        verse: native,
        last: native.verse,
        standard: std.map(Passage::verse),
    }
}

/// Genesis 31-32 numbered the Hebrew way: KJV 31:55 is 32:1 and KJV 32:n is 32:n+1; plus an
/// Unmapped verse Gen 50:27.
fn hebrew_map() -> VerseMap {
    let mut e: Vec<VerseMapEntry> = (1..=54)
        .map(|v| entry(ge(31, v), Some(ge(31, v))))
        .collect();
    e.push(entry(ge(32, 1), Some(ge(31, 55))));
    e.extend((2..=33).map(|v| entry(ge(32, v), Some(ge(32, v - 1)))));
    e.push(entry(ge(50, 27), None));
    VerseMap::new(e, vec![], &Standard)
}

/// Matthew 17 without verse 21.
fn lacking_map() -> VerseMap {
    let e = (1..=27)
        .filter(|v| *v != 21)
        .map(|v| entry(mt(17, v), Some(mt(17, v))))
        .collect();
    VerseMap::new(e, vec![mt(17, 21)], &Standard)
}

fn v11n(map: &VerseMap) -> ListedVersification {
    ListedVersification::new(map.entries().iter().map(|e| (e.verse, e.last)))
}

fn session() -> Session {
    let mut s = Session::new(JobMode::Inline);
    for (id, ab) in [
        ("kjv", "KJV"),
        ("heb", "HEB"),
        ("lacks", "LACKS"),
        ("plain", "PLAIN"),
    ] {
        s.library.add(LibraryEntry {
            manifest: Manifest::new(id, ab, ab, ResourceType::Bible, "en"),
            path: format!("/library/{id}.obr").into(),
        });
    }
    s
}

fn open(s: &mut Session, id: &str) -> TabId {
    TabId(
        s.run("resource.open", &json!({ "id": id })).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    )
}

fn with_map(s: &mut Session, t: TabId, map: VerseMap) {
    let tab = s.workspace.tab_mut(t).unwrap();
    tab.versification = Some(Arc::new(v11n(&map)));
    tab.verse_map = Some(Arc::new(map));
}

fn link(s: &mut Session, t: TabId, set: &str) {
    s.run("tab.link", &json!({ "tab": t.0, "set": set }))
        .unwrap();
}

fn at(s: &Session, t: TabId) -> Option<Verse> {
    s.workspace.tab(t).unwrap().history.current()
}

fn follows(effects: &[Effect]) -> Vec<(TabId, Verse, bool)> {
    effects
        .iter()
        .filter_map(|e| match e {
            Effect::Follow { tab, verse, flash } => Some((*tab, *verse, *flash)),
            _ => None,
        })
        .collect()
}

#[test]
fn scrolling_kjv_to_gen_31_55_moves_a_hebrew_numbered_bible_to_32_1() {
    let mut s = session();
    let kjv = open(&mut s, "kjv");
    let heb = open(&mut s, "heb");
    with_map(&mut s, heb, hebrew_map());
    link(&mut s, kjv, "A");
    link(&mut s, heb, "A");
    s.workspace.take_effects();
    s.workspace.user_scrolled(kjv, ge(31, 55));
    assert_eq!(
        follows(&s.workspace.take_effects()),
        [(heb, ge(32, 1), false)]
    );
    assert_eq!(at(&s, heb), Some(ge(32, 1)));
    // And back the other way: HEB 32:5 is KJV 32:4.
    s.workspace.user_scrolled(heb, ge(32, 5));
    assert_eq!(
        follows(&s.workspace.take_effects()),
        [(kjv, ge(32, 4), false)]
    );
    // Scrolling within the same verse sends nothing.
    s.workspace.user_scrolled(heb, ge(32, 5));
    assert!(follows(&s.workspace.take_effects()).is_empty());
}

#[test]
fn jumping_to_mt_17_21_moves_a_bible_lacking_it_to_17_20() {
    let mut s = session();
    let kjv = open(&mut s, "kjv");
    let lacks = open(&mut s, "lacks");
    let plain = open(&mut s, "plain");
    with_map(&mut s, lacks, lacking_map());
    // A Bible with no Verse map whose own numbering ends Matthew 17 at verse 20. (Without a
    // Verse map only chapter ends are known; imports always build a map, so gaps inside a
    // chapter are Absent verses there.)
    let short = ListedVersification::new((1..=20).map(|v| (mt(17, v), v)));
    s.workspace.tab_mut(plain).unwrap().versification = Some(Arc::new(short));
    for t in [kjv, lacks, plain] {
        link(&mut s, t, "B");
    }
    s.workspace.take_effects();
    s.run("tab.go", &json!({ "tab": kjv.0, "text": "Mt 17:21" }))
        .unwrap();
    let mut f = follows(&s.workspace.take_effects());
    f.sort();
    // Explicit jumps flash the target in every member.
    assert_eq!(f, [(lacks, mt(17, 20), true), (plain, mt(17, 20), true)]);
    let short = ListedVersification::new((1..=20).map(|v| (mt(17, v), v)));
    assert_eq!(nearest_preceding(&short, mt(17, 21)), Some(mt(17, 20)));
    assert_eq!(nearest_preceding(&short, mt(18, 3)), Some(mt(17, 20)));
    assert_eq!(nearest_preceding(&short, mt(17, 4)), Some(mt(17, 4)));
}

#[test]
fn navigating_to_an_unmapped_verse_leaves_other_members_still() {
    let mut s = session();
    let kjv = open(&mut s, "kjv");
    let heb = open(&mut s, "heb");
    with_map(&mut s, heb, hebrew_map());
    link(&mut s, kjv, "A");
    link(&mut s, heb, "A");
    s.run("tab.go", &json!({ "tab": kjv.0, "text": "Gen 31:10" }))
        .unwrap();
    s.workspace.take_effects();
    s.workspace.navigate(heb, ge(50, 27));
    let e = s.workspace.take_effects();
    assert!(follows(&e).is_empty(), "{e:?}");
    assert_eq!(at(&s, kjv), Some(ge(31, 10)));
    assert_eq!(at(&s, heb), Some(ge(50, 27)));
}

#[test]
fn only_members_with_the_key_scheme_follow() {
    let mut s = session();
    let kjv = open(&mut s, "kjv");
    let plain = open(&mut s, "plain");
    link(&mut s, kjv, "A");
    // PLAIN stays unlinked; Word Info cannot follow passages.
    let wi = TabId(
        s.run("tool.word_info", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    assert!(s.workspace.tab(wi).unwrap().link.is_none());
    s.workspace.user_focused(kjv);
    // A Tab opened from a linked Tab joins its set and starts at the set's location.
    s.run("tab.go", &json!({ "tab": kjv.0, "text": "Gen 3:15" }))
        .unwrap();
    let xr = TabId(
        s.run("tool.cross_references", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    assert_eq!(s.workspace.tab(xr).unwrap().link, Some('A'));
    assert_eq!(s.workspace.set_location(xr), Some(ge(3, 15)));
    s.workspace.take_effects();
    s.workspace.user_scrolled(kjv, ge(3, 20));
    assert_eq!(
        follows(&s.workspace.take_effects()),
        [(xr, ge(3, 20), false)]
    );
    assert_eq!(at(&s, plain), None);
    // Joining a set moves the Tab to the set's location.
    link(&mut s, plain, "A");
    assert_eq!(
        follows(&s.workspace.take_effects()),
        [(plain, ge(3, 20), false)]
    );
    // Linking a Library is refused; bad letters too.
    let lib = TabId(
        s.run("library.open", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    assert!(
        s.run("tab.link", &json!({ "tab": lib.0, "set": "A" }))
            .is_err()
    );
    assert!(
        s.run("tab.link", &json!({ "tab": kjv.0, "set": "Q" }))
            .is_err()
    );
    assert_eq!(s.workspace.members('A').len(), 3);
}

#[test]
fn absent_verses_follow_to_the_nearest_preceding_verse_through_the_map() {
    let map = lacking_map();
    assert_eq!(to_native(Some(&map), None, mt(17, 21)), Some(mt(17, 20)));
    assert_eq!(to_native(Some(&map), None, mt(17, 22)), Some(mt(17, 22)));
    assert_eq!(
        to_native(Some(&hebrew_map()), None, ge(31, 55)),
        Some(ge(32, 1))
    );
    // No map and no own numbering: the Standard Verse as it is.
    assert_eq!(to_native(None, None, mt(17, 21)), Some(mt(17, 21)));
}
