#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use obs_model::{Manifest, ResourceType};
use obs_scripture::{
    Book, ListedVersification, Passage, Standard, Verse, VerseMap, VerseMapEntry, Versification,
};
use serde_json::json;

use crate::compare::{Cell, ComparedBible, Marks, cell_for, narrows};
use crate::*;

const KJV_JN_3_16: &str = "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.";
const WEB_JN_3_16: &str = "For God so loved the world, that he gave his one and only Son, that whoever believes in him should not perish, but have eternal life.";

fn words<'a>(text: &'a str, d: &WordDiff) -> Vec<&'a str> {
    d.changed.iter().map(|r| &text[r.clone()]).collect()
}

/// The word each ‸ stands before (or "" at the end of the row).
fn before<'a>(text: &'a str, d: &WordDiff) -> Vec<&'a str> {
    d.missing
        .iter()
        .map(|&at| text[at..].split(|c: char| !c.is_alphanumeric()).next().unwrap())
        .collect()
}

#[test]
fn diff_marks_known_kjv_web_wording_differences_in_jn_3_16() {
    let d = word_diff(KJV_JN_3_16, WEB_JN_3_16);
    assert_eq!(
        words(WEB_JN_3_16, &d),
        ["one", "and", "whoever", "believes", "eternal"]
    );
    // "begotten" is not in the WEB: marked before "Son".
    assert_eq!(before(WEB_JN_3_16, &d), ["Son"]);

    // With the WEB as the base, the KJV's extra and different words are tinted, and the
    // WEB's "one and" is missing before "only".
    let d = word_diff(WEB_JN_3_16, KJV_JN_3_16);
    assert_eq!(
        words(KJV_JN_3_16, &d),
        ["begotten", "whosoever", "believeth", "everlasting"]
    );
    assert_eq!(before(KJV_JN_3_16, &d), ["only"]);
}

#[test]
fn diff_ignores_case_punctuation_and_diacritics() {
    assert_eq!(
        word_diff("For God so loved the world,", "for god so loved the world"),
        WordDiff::default()
    );
    assert_eq!(word_diff("Ἐν ἀρχῇ ἦν ὁ λόγος", "εν αρχη ην ο λογος"), WordDiff::default());
    // Words missing at the end are marked after the last word.
    let d = word_diff("and the earth was without form", "and the earth");
    assert_eq!(d.changed, []);
    assert_eq!(d.missing, ["and the earth".len()]);
    // Everything differs from an empty base; nothing is missing from an empty row.
    assert_eq!(word_diff("", "In the beginning").changed.len(), 3);
    assert_eq!(word_diff("In the beginning", "").missing, [0]);
}

/// A Bible in memory: its Verses with their text, and maybe a Verse map.
struct TestBible {
    language: &'static str,
    map: Option<VerseMap>,
    verses: Vec<(Verse, u16, String)>,
}

impl ComparedBible for TestBible {
    fn language(&self) -> &str {
        self.language
    }
    fn verse_map(&self) -> Option<&VerseMap> {
        self.map.as_ref()
    }
    fn verse_at(&self, native: Verse) -> Option<(Verse, u16)> {
        self.verses
            .iter()
            .find(|(v, last, _)| {
                v.book == native.book
                    && v.chapter == native.chapter
                    && v.verse <= native.verse
                    && native.verse <= *last
            })
            .map(|(v, last, _)| (*v, *last))
    }
    fn verse_text(&self, native: Verse) -> Option<String> {
        self.verses
            .iter()
            .find(|(v, _, _)| *v == native)
            .map(|(_, _, t)| t.clone())
    }
}

fn ge(c: u16, v: u16) -> Verse {
    Verse::new(Book::Genesis, c, v)
}

/// Text of Standard Genesis `c:v` (the same words in both numberings).
fn gen_text(std: Verse) -> String {
    format!("Text of Genesis {}:{}", std.chapter, std.verse)
}

/// Genesis 31–32 numbered like the KJV (no Verse map).
fn kjv() -> TestBible {
    let mut verses = Vec::new();
    for (c, n) in [(31, 55), (32, 32)] {
        for v in 1..=n {
            verses.push((ge(c, v), v, gen_text(ge(c, v))));
        }
    }
    TestBible {
        language: "en",
        map: None,
        verses,
    }
}

/// Genesis 31–32 numbered the Hebrew way: KJV 31:55 is 32:1 and KJV 32:n is 32:n+1.
fn hebrew_numbered() -> TestBible {
    let mut verses = Vec::new();
    let mut entries = Vec::new();
    for (c, n) in [(31, 54), (32, 33)] {
        for v in 1..=n {
            let std = match (c, v) {
                (32, 1) => ge(31, 55),
                (32, v) => ge(32, v - 1),
                _ => ge(c, v),
            };
            verses.push((ge(c, v), v, gen_text(std)));
            entries.push(VerseMapEntry {
                verse: ge(c, v),
                last: v,
                standard: Some(Passage::verse(std)),
            });
        }
    }
    TestBible {
        language: "en",
        map: Some(VerseMap::new(entries, vec![], &Standard)),
        verses,
    }
}

fn chapter(book: Book, c: u16) -> Vec<Verse> {
    Passage::chapter(&Standard, book, c)
        .unwrap()
        .verses(&Standard)
        .collect()
}

#[test]
fn gen_31_32_compares_kjv_and_a_hebrew_numbered_bible_on_the_same_rows() {
    let (kjv, heb) = (kjv(), hebrew_numbered());
    let bibles: [&dyn ComparedBible; 2] = [&kjv, &heb];
    for c in [31, 32] {
        let blocks = verse_blocks(&chapter(Book::Genesis, c), &bibles, Some(0), true);
        assert_eq!(blocks.len(), Standard.last_verse(Book::Genesis, c).unwrap() as usize);
        for b in &blocks {
            // Both rows hold the same Standard Verse, whatever each Bible calls it…
            let texts: Vec<&str> = b
                .cells
                .iter()
                .map(|c| match &c.cell {
                    Cell::Text { text, .. } => text.as_str(),
                    other => panic!("{}: {other:?}", b.verse),
                })
                .collect();
            assert_eq!(texts, [gen_text(b.verse), gen_text(b.verse)], "{}", b.verse);
            // …so the diff finds nothing.
            assert_eq!(b.cells[1].marks, Marks::Diff(WordDiff::default()));
            assert_eq!(b.cells[0].marks, Marks::Base);
        }
    }
    // The Hebrew-numbered Bible shows its own number where it differs.
    let label = |c: u16, v: u16| {
        let blocks = verse_blocks(&chapter(Book::Genesis, c), &bibles, Some(0), true);
        let b = blocks.iter().find(|b| b.verse == ge(c, v)).unwrap();
        (
            b.cells[0].cell.native_label(b.verse),
            b.cells[1].cell.native_label(b.verse),
        )
    };
    assert_eq!(label(31, 54), (None, None));
    assert_eq!(label(31, 55), (None, Some("32:1".into())));
    assert_eq!(label(32, 1), (None, Some("32:2".into())));
    assert_eq!(label(32, 32), (None, Some("32:33".into())));
}

#[test]
fn absent_merged_and_shared_rows() {
    let mt = |c, v| Verse::new(Book::Matthew, c, v);
    // Matthew 17 without verse 21 (no Verse map).
    let lacking = TestBible {
        language: "en",
        map: None,
        verses: (1..=27)
            .filter(|v| *v != 21)
            .map(|v| (mt(17, v), v, format!("v{v}")))
            .collect(),
    };
    assert_eq!(cell_for(&lacking, mt(17, 21), mt(17, 1)), Cell::Absent);
    // A Bible without the book at all is absent too.
    assert_eq!(cell_for(&lacking, ge(1, 1), ge(1, 1)), Cell::Absent);

    // `\v 1-2`: one Bible Verse spanning two rows is shown merged in the first.
    let ranged = TestBible {
        language: "en",
        map: None,
        verses: vec![(mt(17, 1), 2, "one and two".into()), (mt(17, 3), 3, "three".into())],
    };
    let first = cell_for(&ranged, mt(17, 1), mt(17, 1));
    assert_eq!(
        first,
        Cell::Text {
            natives: vec![(mt(17, 1), 2)],
            text: "one and two".into()
        }
    );
    assert_eq!(first.native_label(mt(17, 1)), Some("17:1–2".into()));
    assert_eq!(
        cell_for(&ranged, mt(17, 2), mt(17, 1)),
        Cell::Merged {
            into: mt(17, 1),
            native: mt(17, 1)
        }
    );
    // A range starting at verse 2 still shows the merged text, in its top row.
    assert!(matches!(cell_for(&ranged, mt(17, 2), mt(17, 2)), Cell::Text { .. }));

    // Hebrew Ps 51:1-2 are the Standard title: both share the title row.
    let ps = |v| Verse::new(Book::Psalms, 51, v);
    let entries = vec![
        VerseMapEntry {
            verse: ps(1),
            last: 1,
            standard: Some(Passage::verse(ps(0))),
        },
        VerseMapEntry {
            verse: ps(2),
            last: 2,
            standard: Some(Passage::verse(ps(0))),
        },
        VerseMapEntry {
            verse: ps(3),
            last: 3,
            standard: Some(Passage::verse(ps(1))),
        },
    ];
    let hebrew = TestBible {
        language: "hbo",
        map: Some(VerseMap::new(entries, vec![ps(2)], &Standard)),
        verses: vec![
            (ps(1), 1, "For the leader".into()),
            (ps(2), 2, "when Nathan came".into()),
            (ps(3), 3, "Have mercy".into()),
        ],
    };
    let title = cell_for(&hebrew, ps(0), ps(0));
    assert_eq!(
        title,
        Cell::Text {
            natives: vec![(ps(1), 1), (ps(2), 2)],
            text: "For the leader when Nathan came".into()
        }
    );
    assert_eq!(title.native_label(ps(0)), Some("51:1–2".into()));
    assert_eq!(
        cell_for(&hebrew, ps(1), ps(0)).native_label(ps(1)),
        Some("51:3".into())
    );
    assert_eq!(cell_for(&hebrew, ps(2), ps(0)), Cell::Absent);
}

#[test]
fn rows_in_another_language_than_the_base_are_not_compared() {
    let english = kjv();
    let mut hebrew = kjv();
    hebrew.language = "hbo";
    let mut us = kjv();
    us.language = "en-US";
    let bibles: [&dyn ComparedBible; 3] = [&english, &hebrew, &us];
    let extent = [ge(31, 1)];
    let marks = |base, on| -> Vec<Marks> {
        verse_blocks(&extent, &bibles, base, on)[0]
            .cells
            .iter()
            .map(|c| c.marks.clone())
            .collect()
    };
    assert_eq!(
        marks(Some(0), true),
        [Marks::Base, Marks::NotCompared, Marks::Diff(WordDiff::default())]
    );
    assert_eq!(
        marks(Some(1), true),
        [Marks::NotCompared, Marks::Base, Marks::NotCompared]
    );
    assert_eq!(marks(Some(0), false), [Marks::None, Marks::None, Marks::None]);
}

// ------------------------------------------------------------------ Tool state and commands

fn session(bibles: &[&str]) -> Session {
    let mut s = Session::new(JobMode::Inline);
    s.library.add(LibraryEntry {
        manifest: Manifest::new("mhc", "Commentary", "MHC", ResourceType::Commentary, "en"),
        path: "/library/mhc.obr".into(),
    });
    for id in bibles {
        s.library.add(LibraryEntry {
            manifest: Manifest::new(*id, *id, id.to_uppercase(), ResourceType::Bible, "en"),
            path: format!("/library/{id}.obr").into(),
        });
    }
    s
}

fn open_comparison(s: &mut Session) -> TabId {
    TabId(
        s.run("tool.text_comparison", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    )
}

fn comparison(s: &Session, t: TabId) -> &Comparison {
    &s.workspace.tab(t).unwrap().comparison
}

#[test]
fn bibles_default_to_the_first_five_in_preferred_order_and_the_base_is_selectable() {
    let mut s = session(&["kjv", "web", "asv", "ylt", "tahot", "bsb"]);
    let t = open_comparison(&mut s);
    let c = comparison(&s, t).clone();
    let bibles = c.bibles(&s.library);
    assert_eq!(bibles, ["kjv", "web", "asv", "ylt", "tahot"]);
    assert_eq!(c.base_index(&bibles), Some(0), "★ is the first row by default");

    let out = s
        .run("compare.base", &json!({ "tab": t.0, "id": "web" }))
        .unwrap();
    assert_eq!(out["base"], "web");
    let c = comparison(&s, t).clone();
    assert_eq!(c.base_index(&c.bibles(&s.library)), Some(1));

    // Remove a Bible and add another (it goes last).
    s.run("compare.bible", &json!({ "tab": t.0, "id": "asv" }))
        .unwrap();
    let out = s
        .run("compare.bible", &json!({ "tab": t.0, "id": "bsb" }))
        .unwrap();
    assert_eq!(out["bibles"], json!(["kjv", "web", "ylt", "tahot", "bsb"]));
    // Removing the base makes the first row the base again.
    let out = s
        .run("compare.bible", &json!({ "tab": t.0, "id": "web" }))
        .unwrap();
    assert_eq!(out["base"], "kjv");

    assert_eq!(
        s.run("compare.differences", &json!({ "tab": t.0 })).unwrap()["differences"],
        false
    );
    // Only Bibles being compared can be the base; only Bibles can be compared.
    assert!(
        s.run("compare.base", &json!({ "tab": t.0, "id": "asv" }))
            .is_err()
    );
    assert!(
        s.run("compare.bible", &json!({ "tab": t.0, "id": "mhc" }))
            .is_err()
    );
    // The commands only work on a Text Comparison Tab.
    let bible = TabId(
        s.run("resource.open", &json!({ "id": "kjv" })).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    assert!(
        s.run("compare.differences", &json!({ "tab": bible.0 }))
            .is_err()
    );
}

#[test]
fn a_typed_range_narrows_the_chapter_until_cleared_or_a_jump_leaves_it() {
    let mut s = session(&["kjv"]);
    let t = open_comparison(&mut s);
    let rom = |c, v| Verse::new(Book::Romans, c, v);

    // A whole chapter or one Verse is the whole chapter: no chip.
    s.run("tab.go", &json!({ "tab": t.0, "text": "Ro 8" })).unwrap();
    assert_eq!(comparison(&s, t).range, None);
    assert_eq!(comparison(&s, t).extent(rom(8, 1)).len(), 39);
    s.run("tab.go", &json!({ "tab": t.0, "text": "Ro 8:28" }))
        .unwrap();
    assert_eq!(comparison(&s, t).range, None);

    s.run("tab.go", &json!({ "tab": t.0, "text": "Ro 8:28-39" }))
        .unwrap();
    let range = Passage::new(rom(8, 28), rom(8, 39)).unwrap();
    assert_eq!(comparison(&s, t).range, Some(range));
    assert_eq!(
        comparison(&s, t).extent(rom(8, 28)),
        range.verses(&Standard).collect::<Vec<_>>()
    );
    // Jumps inside the chapter keep it…
    s.run("tab.go", &json!({ "tab": t.0, "text": "Ro 8:30" }))
        .unwrap();
    s.run("tab.go", &json!({ "tab": t.0, "text": "Ro 8:28-39" }))
        .unwrap();
    s.workspace.navigate(t, rom(8, 1));
    assert_eq!(comparison(&s, t).range, Some(range));
    // …a jump to another chapter clears it…
    s.workspace.navigate(t, rom(9, 1));
    assert_eq!(comparison(&s, t).range, None);
    // …and so does Back.
    s.run("tab.go", &json!({ "tab": t.0, "text": "Ro 8:28-39" }))
        .unwrap();
    s.workspace.navigate(t, rom(12, 1));
    s.workspace.back(t);
    assert_eq!(comparison(&s, t).range, None);

    // The chip's ×.
    s.run("tab.go", &json!({ "tab": t.0, "text": "Ro 8:28-39" }))
        .unwrap();
    let out = s
        .run("compare.clear_range", &json!({ "tab": t.0 }))
        .unwrap();
    assert_eq!(out["range"], json!(null));
    assert_eq!(comparison(&s, t).range, None);

    assert!(!narrows(Passage::verse(rom(8, 1))));
    assert!(narrows(Passage::new(rom(8, 39), rom(9, 2)).unwrap()));
}

#[test]
fn a_linked_bible_moving_to_another_chapter_clears_the_range() {
    let mut s = session(&["kjv"]);
    let bible = TabId(
        s.run("resource.open", &json!({ "id": "kjv" })).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    s.run("tab.link", &json!({ "tab": bible.0, "set": "A" }))
        .unwrap();
    let t = open_comparison(&mut s);
    s.run("tab.link", &json!({ "tab": t.0, "set": "A" }))
        .unwrap();
    s.run("tab.go", &json!({ "tab": t.0, "text": "Gen 31:50-32:2" }))
        .unwrap();
    assert!(comparison(&s, t).range.is_some());
    s.workspace.user_scrolled(bible, ge(32, 1));
    assert!(comparison(&s, t).range.is_some(), "still inside the range's chapters");
    s.workspace.user_scrolled(bible, ge(33, 1));
    assert_eq!(comparison(&s, t).range, None);
}

#[test]
fn scrolling_the_comparison_drives_linked_bibles() {
    let mut s = session(&["kjv", "heb"]);
    let open = |s: &mut Session, id: &str| {
        TabId(
            s.run("resource.open", &json!({ "id": id })).unwrap()["tab"]
                .as_u64()
                .unwrap(),
        )
    };
    let kjv = open(&mut s, "kjv");
    let heb = open(&mut s, "heb");
    let map = hebrew_numbered().map.unwrap();
    {
        let tab = s.workspace.tab_mut(heb).unwrap();
        tab.versification = Some(Arc::new(ListedVersification::new(
            map.entries().iter().map(|e| (e.verse, e.last)),
        )));
        tab.verse_map = Some(Arc::new(map));
    }
    let t = open_comparison(&mut s);
    for tab in [kjv, heb, t] {
        s.run("tab.link", &json!({ "tab": tab.0, "set": "A" }))
            .unwrap();
    }
    s.workspace.take_effects();
    // The comparison speaks Standard: its row Gen 31:55 is HEB 32:1.
    s.workspace.user_scrolled(t, ge(31, 55));
    let follows: Vec<(TabId, Verse)> = s
        .workspace
        .take_effects()
        .into_iter()
        .filter_map(|e| match e {
            Effect::Follow { tab, verse, .. } => Some((tab, verse)),
            _ => None,
        })
        .collect();
    assert_eq!(follows, [(kjv, ge(31, 55)), (heb, ge(32, 1))]);
    // And a Hebrew-numbered Bible scrolled to 32:5 moves the comparison to row 32:4.
    s.workspace.user_scrolled(heb, ge(32, 5));
    assert_eq!(s.workspace.tab(t).unwrap().history.current(), Some(ge(32, 4)));
}
