//! Annotations across Bibles (#63), through the headless harness: open-text fixtures of four
//! Bibles written as real `.obr` files, a real User database, and what each reading pane
//! paints. A Highlight made in one Bible shows in every other Bible that has its Passage:
//! whole Verses when it covered whole Verses, else only a margin mark on the line of each
//! touched Verse's number; Notes show their marker in the same margin.

use std::path::{Path, PathBuf};

use egui::Color32;
use obs_engine::testing::bibles::{bible, paragraphed_bible, v};
use obs_engine::{Anchor, Content, TabId};
use obs_model::{Direction, Resource};
use obs_scripture::{Book, Passage, Verse};

use super::*;
use crate::selection::{DocPos, range_rects};

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-across-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const JOHN_KJV: [&str; 3] = [
    "That whosoever believeth in him should not perish, but have eternal life.",
    "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
    "For God sent not his Son into the world to condemn the world; but that the world through him might be saved.",
];
const JOHN_WEB: [&str; 3] = [
    "that whoever believes in him should not perish, but have eternal life.",
    "For God so loved the world, that he gave his one and only Son, that whoever believes in him should not perish, but have eternal life.",
    "For God didn’t send his Son into the world to judge the world, but that the world should be saved through him.",
];
const JOHN_BSB: [&str; 3] = [
    "that everyone who believes in Him may have eternal life.",
    "For God so loved the world that He gave His one and only Son, that everyone who believes in Him shall not perish but have eternal life.",
    "For God did not send His Son into the world to condemn the world, but to save the world through Him.",
];
/// The Greek of Jn 3:15-17 as printed in public-domain editions (TAGNT-like fixture).
const JOHN_GRC: [&str; 3] = [
    "ἵνα πᾶς ὁ πιστεύων εἰς αὐτὸν μὴ ἀπόληται, ἀλλ’ ἔχῃ ζωὴν αἰώνιον.",
    "Οὕτω γὰρ ἠγάπησεν ὁ Θεὸς τὸν κόσμον, ὥστε τὸν υἱὸν αὐτοῦ τὸν μονογενῆ ἔδωκεν, ἵνα πᾶς ὁ πιστεύων εἰς αὐτὸν μὴ ἀπόληται, ἀλλ’ ἔχῃ ζωὴν αἰώνιον.",
    "οὐ γὰρ ἀπέστειλεν ὁ Θεὸς τὸν υἱὸν αὐτοῦ εἰς τὸν κόσμον ἵνα κρίνῃ τὸν κόσμον, ἀλλ’ ἵνα σωθῇ ὁ κόσμος δι’ αὐτοῦ.",
];
const PSALM_KJV: [&str; 3] = [
    "Have mercy upon me, O God, according to thy lovingkindness: according unto the multitude of thy tender mercies blot out my transgressions.",
    "Wash me throughly from mine iniquity, and cleanse me from my sin.",
    "For I acknowledge my transgressions: and my sin is ever before me.",
];

fn jn(n: u16) -> Verse {
    v(Book::John, 3, n)
}

fn ps(n: u16) -> Verse {
    v(Book::Psalms, 51, n)
}

/// A Bible of Ps 51:1-3 (Standard numbering) and Jn 3:15-17.
fn english(id: &str, abbrev: &str, john: [&str; 3]) -> Resource {
    let mut verses: Vec<(Verse, &str)> = (1..=3)
        .map(|n| (ps(n), PSALM_KJV[n as usize - 1]))
        .collect();
    verses.extend((15..=17).map(|n| (jn(n), john[n as usize - 15])));
    bible(id, abbrev, &verses, None, &[])
}

/// KJV, WEB, BSB and a TAGNT-like Greek New Testament: open texts only.
fn four_bibles() -> Vec<Resource> {
    let mut greek = bible(
        "tagnt",
        "TAGNT",
        &(15..=17)
            .map(|n| (jn(n), JOHN_GRC[n as usize - 15]))
            .collect::<Vec<_>>(),
        None,
        &[],
    );
    greek.manifest.language = "grc".into();
    vec![
        english("kjv", "KJV", JOHN_KJV),
        english("web", "WEB", JOHN_WEB),
        english("bsb", "BSB", JOHN_BSB),
        greek,
    ]
}

/// Ps 51 numbered the Hebrew way: 51:1-2 are the title (Standard 51:0), 51:3 is Standard
/// 51:1, and so on, with the Verse map that says so.
fn hebrew_numbered() -> Resource {
    let text = [
        "To the chief Musician, A Psalm of David,",
        "when Nathan the prophet came unto him, after he had gone in to Bathsheba.",
        PSALM_KJV[0],
        PSALM_KJV[1],
        PSALM_KJV[2],
    ];
    let verses: Vec<(Verse, &str)> = (1..=5).map(|n| (ps(n), text[n as usize - 1])).collect();
    let standard = [
        Some(ps(0)),
        Some(ps(0)),
        Some(ps(1)),
        Some(ps(2)),
        Some(ps(3)),
    ];
    bible("hpsa", "HPSA", &verses, Some(&standard), &[])
}

/// Launch on `dir`'s User database with `resources` written as `.obr` files there, each
/// open in its own Panel (the first in TB's), all at `at`.
fn launch_with(dir: &Path, resources: &[Resource], at: &str) -> Harness {
    obs_engine::testing::bibles::write_library(dir, resources);
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(dir));
    for (i, r) in resources.iter().enumerate() {
        let id = &r.manifest.resource_id;
        h.shell
            .st
            .session
            .library
            .add_file(&dir.join(format!("{id}.obr")))
            .unwrap();
        h.shell
            .run("resource.open", &json!({ "id": id, "split": i > 0 }));
        h.frames(2);
    }
    for r in resources {
        let tab = h.tab_id(&r.manifest.abbreviation);
        h.shell.run("tab.go", &json!({ "tab": tab.0, "text": at }));
    }
    settle_all(&mut h);
    h
}

fn view(h: &Harness, tab: TabId) -> &crate::view::ReadingView {
    h.shell.st.tabs[&tab].view.as_ref().unwrap()
}

/// The Panel showing the Tab titled `title`, below its header strip.
fn pane_of(h: &Harness, title: &str) -> Rect {
    let (r, _) = h
        .panels()
        .into_iter()
        .find(|(_, titles)| titles.iter().any(|t| t == title))
        .unwrap_or_else(|| panic!("no Panel with {title}"));
    Rect::from_min_max(pos2(r.left(), r.top() + 60.0), r.max)
}

/// Where a Bible Verse (own numbering) is drawn in a Tab: its line rects on screen.
fn verse_rects(h: &Harness, tab: TabId, verse: Verse) -> Vec<Rect> {
    let v = view(h, tab);
    let (unit, start) = v.doc.verse_position(verse).unwrap();
    let range = v.doc.verse_range_at(unit, start).unwrap();
    let [ox, oy] = v.text_origin;
    let a = DocPos {
        unit,
        byte: range.start,
    };
    let b = DocPos {
        unit,
        byte: range.end,
    };
    // Only what the pane shows: the view lays out its own height.
    let height = pane_of(h, &h.shell.st.session.workspace.tab(tab).unwrap().title).bottom() - oy;
    range_rects(v, &[(a, b)], height)
        .remove(0)
        .into_iter()
        .map(|[x0, y0, x1, y1]| Rect::from_min_max(pos2(ox + x0, oy + y0), pos2(ox + x1, oy + y1)))
        .collect()
}

/// Rects painted in `color` inside the Panel of `title`.
fn painted_in(h: &Harness, title: &str, color: Color32) -> Vec<Rect> {
    let pane = pane_of(h, title);
    h.rects
        .iter()
        .filter(|(r, c)| *c == color && pane.contains(r.center()))
        .map(|(r, _)| *r)
        .collect()
}

/// The Highlight rects of a Tab are exactly its Verse's lines: one per line, the same
/// columns, within the line.
fn assert_verse_highlighted(h: &Harness, title: &str, verse: Verse, color: Color32) {
    let tab = h.tab_id(title);
    let lines = verse_rects(h, tab, verse);
    let painted = painted_in(h, title, color);
    assert!(!lines.is_empty(), "{title} {verse:?} is not in view");
    assert_eq!(
        painted.len(),
        lines.len(),
        "{title}: painted {painted:?} for the lines {lines:?}"
    );
    for line in &lines {
        assert!(
            painted.iter().any(|p| (p.left() - line.left()).abs() < 0.6
                && (p.right() - line.right()).abs() < 0.6
                && p.top() >= line.top() - 0.1
                && p.bottom() <= line.bottom() + 0.1),
            "{title}: line {line:?} of {verse:?} not covered by {painted:?}"
        );
    }
}

/// A Tab's text column on screen: its left and right edges.
fn column(h: &Harness, tab: TabId) -> (f32, f32) {
    let v = view(h, tab);
    (v.text_origin[0], v.text_origin[0] + v.width())
}

/// Marks beside a verse number sit in the column's margin (left of it in left-to-right
/// text, right of it in right-to-left text) on the line where the Verse starts, inside the
/// Panel and clear of its scrollbar: never over the previous Verse's words, which come just
/// before the number when a Verse starts mid-line.
fn assert_in_margin(h: &Harness, title: &str, verse: Verse, m: Rect) {
    let tab = h.tab_id(title);
    let first = verse_rects(h, tab, verse)[0];
    let (left, right) = column(h, tab);
    let panel = pane_of(h, title);
    if view(h, tab).doc.direction() == Direction::Rtl {
        assert!(
            m.left() >= right && m.right() <= panel.right() - 10.0,
            "{title}: {m:?} is not in the right margin of {left}..{right} in {panel:?}"
        );
    } else {
        assert!(
            m.right() <= left && m.left() >= panel.left(),
            "{title}: {m:?} is not in the left margin of {left}..{right} in {panel:?}"
        );
    }
    assert!(
        m.top() >= first.top() - 0.1 && m.bottom() <= first.bottom() + 0.1,
        "{title}: {m:?} is not on the line of {verse:?} at {first:?}"
    );
}

/// One margin mark for the Verse: thin, just outside the text column, on the line of its
/// verse number.
fn assert_margin_mark(h: &Harness, title: &str, verse: Verse, color: Color32) {
    let marks = painted_in(h, title, color);
    assert_eq!(marks.len(), 1, "{title}: margin marks {marks:?}");
    let m = marks[0];
    assert_in_margin(h, title, verse, m);
    let (left, right) = column(h, h.tab_id(title));
    let outside = if view(h, h.tab_id(title)).doc.direction() == Direction::Rtl {
        m.left() - right
    } else {
        left - m.right()
    };
    assert!(
        (0.0..6.0).contains(&outside),
        "{title}: {m:?} is not just outside {left}..{right}"
    );
    assert!(m.width() < 6.0 && m.height() > 10.0, "{m:?}");
}

/// The KJV Verse's bytes, as `annotation.highlight` params over `from`..`to` inside it
/// (the whole Verse when both are `None`).
fn kjv_selection(h: &Harness, verse: Verse, words: Option<&str>) -> serde_json::Value {
    let v = view(h, h.tab_id("KJV"));
    let (unit, start) = v.doc.verse_position(verse).unwrap();
    let range = v.doc.verse_range_at(unit, start).unwrap();
    let text = &v.doc.units[unit].text;
    let (s, e) = match words {
        None => (range.start, range.end),
        Some(w) => {
            let at = range.start + text[range.clone()].find(w).unwrap();
            (at, at + w.len())
        }
    };
    json!({
        "resource": "kjv",
        "start_unit": v.doc.units[unit].id.as_str(),
        "start": s,
        "end": e,
    })
}

#[test]
fn a_kjv_highlight_of_john_3_16_shows_in_web_bsb_and_tagnt() {
    let dir = folder("jn316");
    let mut h = launch_with(&dir, &four_bibles(), "Jn 3:15");
    let yellow = h.shell.st.palette.built_in_color("yellow");
    for t in ["KJV", "WEB", "BSB", "TAGNT"] {
        assert!(painted_in(&h, t, yellow).is_empty(), "{t}");
    }
    // Triple-click selects the whole Verse; Ctrl+1 highlights it in yellow.
    let tab = h.tab_id("KJV");
    let line = verse_rects(&h, tab, jn(16))[0];
    let p = line.center();
    h.time += 1.0;
    h.events.push(Event::PointerMoved(p));
    h.frame();
    for _ in 0..3 {
        h.button(p, true);
        h.frame();
        h.button(p, false);
        h.frame();
    }
    h.events.push(Event::Key {
        key: egui::Key::Num1,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.frames(3);
    let stored = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(stored.len(), 1, "{stored:?}");
    let Some(Anchor::Passage { passage, origin }) = &stored[0].anchor else {
        panic!("{:?}", stored[0].anchor);
    };
    assert_eq!(*passage, Passage::verse(jn(16)));
    assert!(origin.as_ref().unwrap().whole_verses);

    // The whole of Jn 3:16, and nothing else, in all four Bibles.
    for t in ["KJV", "WEB", "BSB", "TAGNT"] {
        assert_verse_highlighted(&h, t, jn(16), yellow);
        assert!(painted_in(&h, t, h.shell.st.palette.built_in_mark("yellow")).is_empty());
    }

    // View → Show Highlights hides them everywhere, and shows them again.
    h.open_menu("View");
    h.click_text("Show Highlights");
    for t in ["KJV", "WEB", "BSB", "TAGNT"] {
        assert!(
            painted_in(&h, t, yellow).is_empty(),
            "{t} still highlighted"
        );
    }
    h.open_menu("View");
    h.click_text("Show Highlights");
    assert!(h.shell.st.session.appearance.show_highlights);
    for t in ["KJV", "WEB", "BSB", "TAGNT"] {
        assert_verse_highlighted(&h, t, jn(16), yellow);
    }

    // The Notebook's Show in text, from View → Notebooks in text, hides them too.
    h.open_menu("View");
    h.click_text("Notebooks in text");
    h.click_text("My Notebook");
    assert!(!h.shell.st.session.default_notebook().unwrap().show_in_text);
    for t in ["KJV", "WEB", "BSB", "TAGNT"] {
        assert!(
            painted_in(&h, t, yellow).is_empty(),
            "{t} still highlighted"
        );
    }
    // Undo is a Notebook change like any other.
    h.shell.run("edit.undo", &json!({}));
    h.frames(2);
    for t in ["KJV", "WEB", "BSB", "TAGNT"] {
        assert_verse_highlighted(&h, t, jn(16), yellow);
    }
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_partial_verse_highlight_shows_only_a_margin_mark_in_other_bibles() {
    let dir = folder("partial");
    let mut h = launch_with(&dir, &four_bibles(), "Jn 3:15");
    let palette = h.shell.st.palette;
    let params = kjv_selection(&h, jn(16), Some("so loved the world"));
    let mut with_style = params.clone();
    with_style["style"] = json!("green");
    h.shell.run("annotation.highlight", &with_style);
    h.frames(2);
    let stored = h.shell.st.session.annotations(None).unwrap();
    assert!(
        !stored[0]
            .anchor
            .as_ref()
            .unwrap()
            .text_range()
            .unwrap()
            .whole_verses
    );
    let green = palette.built_in_color("green");
    // The KJV: exactly the words, no margin mark.
    let kjv = painted_in(&h, "KJV", green);
    let words = kjv.iter().copied().reduce(|a, b| a.union(b)).unwrap();
    let first_line = verse_rects(&h, h.tab_id("KJV"), jn(16))[0];
    // "16 For God" comes before the words, on the same line.
    assert!(
        words.left() > first_line.left() + 10.0
            && words.top() >= first_line.top() - 0.1
            && words.bottom() <= first_line.bottom() + 0.1,
        "{kjv:?} vs {first_line:?}"
    );
    assert!(painted_in(&h, "KJV", palette.built_in_mark("green")).is_empty());
    // The others: no green background, one green margin mark beside the 16.
    for t in ["WEB", "BSB", "TAGNT"] {
        assert!(painted_in(&h, t, green).is_empty(), "{t} has a background");
        assert_margin_mark(&h, t, jn(16), palette.built_in_mark("green"));
    }

    // A Note made in the KJV: its marker at the end of its words there, at the verse number
    // elsewhere (outside the margin mark), previewed on hover.
    let mut note = params;
    note["body"] = json!("Agape, God's love");
    h.shell.run("annotation.note", &note);
    h.frames(2);
    for t in ["WEB", "BSB", "TAGNT"] {
        let markers = painted_in(&h, t, palette.note_marker);
        assert_eq!(markers.len(), 1, "{t}: {markers:?}");
        let mark = painted_in(&h, t, palette.built_in_mark("green"))[0];
        assert!(
            markers[0].right() <= mark.left(),
            "{t}: {markers:?} vs {mark:?}"
        );
        let first = verse_rects(&h, h.tab_id(t), jn(16))[0];
        assert!(markers[0].top() >= first.top() - 0.1 && markers[0].bottom() <= first.bottom());
    }
    let m = painted_in(&h, "WEB", palette.note_marker)[0];
    h.events.push(Event::PointerMoved(m.center()));
    h.frames(3);
    assert!(h.find("Agape, God's love").is_some(), "{:?}", h.texts());

    // In the dark tone the marks take that tone's colours (#21).
    h.shell.run("view.page_tone", &json!({ "value": "dark" }));
    h.frames(2);
    let dark = crate::Palette::DARK;
    for t in ["WEB", "BSB", "TAGNT"] {
        assert_margin_mark(&h, t, jn(16), dark.built_in_mark("green"));
    }
    assert_eq!(
        painted_in(&h, "KJV", dark.built_in_color("green")).len(),
        kjv.len()
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_annotation_on_hebrew_numbered_ps_51_3_maps_to_standard_ps_51_1() {
    let dir = folder("ps51");
    let resources = vec![hebrew_numbered(), english("kjv", "KJV", JOHN_KJV)];
    let mut h = launch_with(&dir, &resources, "Ps 51:1");
    let yellow = h.shell.st.palette.built_in_color("yellow");
    // Highlight Ps 51:3 as the Hebrew-numbered Bible numbers it, whole.
    let hp = h.tab_id("HPSA");
    let (unit, start) = view(&h, hp).doc.verse_position(ps(3)).unwrap();
    let range = view(&h, hp).doc.verse_range_at(unit, start).unwrap();
    let unit_id = view(&h, hp).doc.units[unit].id.as_str().to_string();
    h.shell.run(
        "annotation.highlight",
        &json!({ "resource": "hpsa", "start_unit": unit_id, "start": range.start, "end": range.end }),
    );
    h.frames(2);
    let stored = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(
        stored[0].anchor.as_ref().unwrap().passage(),
        Some(Passage::verse(ps(1))),
        "Hebrew Ps 51:3 is Standard Ps 51:1"
    );
    assert_verse_highlighted(&h, "HPSA", ps(3), yellow);
    assert_verse_highlighted(&h, "KJV", ps(1), yellow);

    // The other way: a few words of KJV Ps 51:2 are a margin mark at Hebrew 51:4.
    h.shell.run("edit.undo", &json!({}));
    let params = kjv_selection(&h, ps(2), Some("cleanse me"));
    h.shell.run("annotation.highlight", &params);
    h.frames(2);
    assert_eq!(
        h.shell.st.session.annotations(None).unwrap()[0]
            .anchor
            .as_ref()
            .unwrap()
            .passage(),
        Some(Passage::verse(ps(2)))
    );
    assert!(painted_in(&h, "HPSA", yellow).is_empty());
    assert_margin_mark(
        &h,
        "HPSA",
        ps(4),
        h.shell.st.palette.built_in_mark("yellow"),
    );
    // And a Note on the KJV's 51:1 shows at the Hebrew 51:3.
    h.shell.run("edit.undo", &json!({}));
    h.shell.run(
        "annotation.note",
        &json!({ "body": "Miserere", "passage": "Ps 51:1" }),
    );
    h.frames(2);
    let markers = painted_in(&h, "HPSA", h.shell.st.palette.note_marker);
    assert_eq!(markers.len(), 1, "{markers:?}");
    let first = verse_rects(&h, hp, ps(3))[0];
    assert!(markers[0].right() <= first.left() && markers[0].top() >= first.top() - 0.1);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Real paragraphed Bibles (the WEB, the BSB) start most verses mid-line, right after the
/// previous verse's last word. Marks for a verse number go in the margin on the number's
/// line, not over that word; a partial Highlight over several Verses marks each of them.
#[test]
fn marks_in_a_paragraphed_bible_sit_on_the_verse_numbers_line() {
    let dir = folder("paragraphed");
    let john: Vec<(Verse, &str)> = (15..=17)
        .map(|n| (jn(n), JOHN_BSB[n as usize - 15]))
        .collect();
    let resources = vec![
        english("kjv", "KJV", JOHN_KJV),
        paragraphed_bible("bsb", "BSB", &john, Direction::Ltr),
    ];
    let mut h = launch_with(&dir, &resources, "Jn 3:15");
    let bsb = h.tab_id("BSB");
    let (left, _) = column(&h, bsb);
    // The 16 comes mid-line, after the end of 15 (the 17 starts a line in this width).
    let first = verse_rects(&h, bsb, jn(16))[0];
    assert!(first.left() > left + 20.0, "16 starts its line: {first:?}");
    // Part of the KJV's 3:16, through part of 3:17.
    let mut params = kjv_selection(&h, jn(16), Some("so loved the world"));
    params["end"] = kjv_selection(&h, jn(17), Some("condemn the world"))["end"].clone();
    params["style"] = json!("green");
    h.shell.run("annotation.highlight", &params);
    h.frames(2);
    let stored = h.shell.st.session.annotations(None).unwrap();
    let anchor = stored[0].anchor.as_ref().unwrap();
    assert_eq!(
        anchor.passage(),
        Some(Passage::new(jn(16), jn(17)).unwrap())
    );
    assert!(!anchor.text_range().unwrap().whole_verses);
    let palette = h.shell.st.palette;
    let green = palette.built_in_mark("green");
    assert!(painted_in(&h, "BSB", palette.built_in_color("green")).is_empty());
    let marks = painted_in(&h, "BSB", green);
    assert_eq!(marks.len(), 2, "{marks:?}");
    let mut marks = marks;
    marks.sort_by(|a, b| a.top().total_cmp(&b.top()));
    assert_in_margin(&h, "BSB", jn(16), marks[0]);
    assert_in_margin(&h, "BSB", jn(17), marks[1]);
    // Never over the words before the verse number.
    let before = verse_rects(&h, bsb, jn(15));
    for m in &marks {
        assert!(
            !before.iter().any(|r| r.intersects(*m)),
            "{m:?} over {before:?}"
        );
    }

    // A Note on 3:16 from elsewhere: its marker on the 16's line too, outside the mark.
    h.shell.run(
        "annotation.note",
        &json!({ "body": "Agape", "passage": "Jn 3:16" }),
    );
    h.frames(2);
    let markers = painted_in(&h, "BSB", palette.note_marker);
    assert_eq!(markers.len(), 1, "{markers:?}");
    assert_in_margin(&h, "BSB", jn(16), markers[0]);
    assert!(
        markers[0].right() <= marks[0].left(),
        "{markers:?} vs {marks:?}"
    );
    assert!(!before.iter().any(|r| r.intersects(markers[0])));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// In a right-to-left Bible (the TAHOT) the marks go in the margin right of the column, on
/// the verse number's line, and stay inside the Panel, clear of its scrollbar.
#[test]
fn marks_in_a_right_to_left_bible_stay_inside_the_pane() {
    let dir = folder("rtl");
    let gn = |n| v(Book::Genesis, 1, n);
    let kjv = bible(
        "kjv",
        "KJV",
        &[
            (
                gn(1),
                "In the beginning God created the heaven and the earth.",
            ),
            (
                gn(2),
                "And the earth was without form, and void; and darkness was upon the face of the deep. And the Spirit of God moved upon the face of the waters.",
            ),
            (
                gn(3),
                "And God said, Let there be light: and there was light.",
            ),
        ],
        None,
        &[],
    );
    let mut hebrew = paragraphed_bible(
        "heb",
        "HEB",
        &[
            (gn(1), "בְּרֵאשִׁית בָּרָא אֱלֹהִים אֵת הַשָּׁמַיִם וְאֵת הָאָרֶץ׃"),
            (
                gn(2),
                "וְהָאָרֶץ הָיְתָה תֹהוּ וָבֹהוּ וְחֹשֶׁךְ עַל־פְּנֵי תְהוֹם וְרוּחַ אֱלֹהִים מְרַחֶפֶת עַל־פְּנֵי הַמָּיִם׃",
            ),
            (gn(3), "וַיֹּאמֶר אֱלֹהִים יְהִי אוֹר וַיְהִי־אוֹר׃"),
        ],
        Direction::Rtl,
    );
    hebrew.manifest.language = "hbo".into();
    let mut h = launch_with(&dir, &[kjv, hebrew], "Gen 1:1");
    let heb = h.tab_id("HEB");
    let (_, right) = column(&h, heb);
    let first = verse_rects(&h, heb, gn(2))[0];
    assert!(
        first.right() < right - 20.0,
        "1:2 starts its line: {first:?}"
    );

    let v = view(&h, h.tab_id("KJV"));
    let (unit, start) = v.doc.verse_position(gn(2)).unwrap();
    let range = v.doc.verse_range_at(unit, start).unwrap();
    let at = range.start + v.doc.units[unit].text[range].find("without form").unwrap();
    let unit_id = v.doc.units[unit].id.as_str().to_string();
    h.shell.run(
        "annotation.highlight",
        &json!({ "resource": "kjv", "start_unit": unit_id, "start": at, "end": at + 12 }),
    );
    h.shell.run(
        "annotation.note",
        &json!({ "body": "Tohu wa-bohu", "passage": "Gen 1:2" }),
    );
    h.frames(2);
    let palette = h.shell.st.palette;
    assert_margin_mark(&h, "HEB", gn(2), palette.built_in_mark("yellow"));
    let mark = painted_in(&h, "HEB", palette.built_in_mark("yellow"))[0];
    let markers = painted_in(&h, "HEB", palette.note_marker);
    assert_eq!(markers.len(), 1, "{markers:?}");
    assert_in_margin(&h, "HEB", gn(2), markers[0]);
    // Laid outward from the column: the Note's marker right of the margin mark.
    assert!(markers[0].left() >= mark.right(), "{markers:?} vs {mark:?}");
    // Hovering it previews the Note.
    h.events.push(Event::PointerMoved(markers[0].center()));
    h.frames(3);
    assert!(h.find("Tohu wa-bohu").is_some(), "{:?}", h.texts());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Text-range anchors show only in their own Resource: an Unmapped verse's Highlight, or one
/// in a Commentary, never reaches another Bible. (The marks pipeline itself, without the
/// shell.)
#[test]
fn text_range_anchors_show_only_in_their_resource() {
    let kjv = crate::ReadingDoc::from_resource(english("kjv", "KJV", JOHN_KJV));
    let web = crate::ReadingDoc::from_resource(english("web", "WEB", JOHN_WEB));
    let unit = kjv.verses[0].unit.clone();
    let text = &kjv.units.iter().find(|u| u.id == unit).unwrap().text;
    let range = obs_engine::TextRange {
        resource_id: "kjv".into(),
        unit: unit.clone(),
        start: 0,
        end: 4,
        quote: text[..4].into(),
        prefix: String::new(),
        suffix: text[4..20].into(),
        content_hash: kjv.manifest.content_hash.clone(),
        native_ref: Some("Ps.51.1".into()),
        whole_verses: false,
        orphaned: false,
    };
    let a = obs_engine::Annotation {
        id: "a".into(),
        notebook_id: "n".into(),
        content: Content::Highlight {
            style: "blue".into(),
        },
        anchor: Some(Anchor::TextRange { range }),
        created_at: 1,
        modified_at: 1,
    };
    assert_eq!(
        crate::marks_for(
            &kjv,
            std::slice::from_ref(&a),
            &crate::HighlightLooks::built_in()
        )
        .len(),
        1
    );
    assert!(crate::marks_for(&web, &[a], &crate::HighlightLooks::built_in()).is_empty());
}

/// The Starter library: a KJV Highlight of Jn 3:16 shows over the whole Verse in the WEB,
/// BSB and TAGNT, and one of Ps 51:1 at the TAHOT's Hebrew 51:3.
/// `cargo test -p obs-ui-egui --release starter_library_annotations -- --ignored --nocapture`
#[test]
#[ignore = "needs the Starter library in the platform data folder"]
fn starter_library_annotations_show_across_bibles() {
    let Some(lib) = obs_engine::default_library_dir().filter(|d| {
        ["kjv-strongs", "web", "bsb", "tagnt", "tahot"]
            .iter()
            .all(|id| d.join(format!("{id}.obr")).is_file())
    }) else {
        eprintln!("no Starter library; skipped");
        return;
    };
    let root = folder("starter");
    let mut s = obs_engine::Session::new(JobMode::Inline);
    s.library.scan(&lib);
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    let open = |id: &str| crate::ReadingDoc::open(&lib.join(format!("{id}.obr"))).unwrap();
    let kjv = open("kjv-strongs");
    let whole = |s: &mut obs_engine::Session, verse: Verse| {
        let (unit, start) = kjv.verse_position(verse).unwrap();
        let range = kjv.verse_range_at(unit, start).unwrap();
        let out = s
            .run(
                "annotation.highlight",
                &json!({
                    "resource": "kjv-strongs",
                    "start_unit": kjv.units[unit].id.as_str(),
                    "start": range.start,
                    "end": range.end,
                }),
            )
            .unwrap();
        eprintln!("{verse:?}: {}", out["annotations"][0]["anchor"]);
        let a = s.annotations(None).unwrap();
        assert!(
            a.last()
                .unwrap()
                .anchor
                .as_ref()
                .unwrap()
                .text_range()
                .unwrap()
                .whole_verses
        );
    };
    whole(&mut s, jn(16));
    whole(&mut s, ps(1));
    let annotations = s.annotations(None).unwrap();
    let check = |id: &str, verse: Verse| {
        let doc = open(id);
        let t = std::time::Instant::now();
        let marks = crate::marks_for(&doc, &annotations, &crate::HighlightLooks::built_in());
        let (unit, start) = doc.verse_position(verse).unwrap();
        let range = doc.verse_range_at(unit, start).unwrap();
        let hit = marks.iter().find(|m| {
            m.start
                == DocPos {
                    unit,
                    byte: range.start,
                }
                && m.end
                    == DocPos {
                        unit,
                        byte: range.end,
                    }
        });
        assert!(
            matches!(hit.map(|m| m.kind), Some(crate::MarkKind::Highlight(_))),
            "{id} {verse:?}: {marks:?}"
        );
        eprintln!("{id}: {} marks in {:?}", marks.len(), t.elapsed());
    };
    for id in ["web", "bsb", "tagnt"] {
        check(id, jn(16));
    }
    check("tahot", ps(3));
    check("web", ps(1));
    drop(s);
    let _ = std::fs::remove_dir_all(&root);
}

/// Writes `target/previews/annotations-across-*.ppm` when `UPDATE_PREVIEWS` is set: a whole-
/// Verse Highlight and a partial one with a Note, made in the KJV, as the four Bibles show
/// them, in a light and a dark tone. Open texts only.
#[test]
fn previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
    let dir = folder("previews");
    let mut h = launch_with(&dir, &four_bibles(), "Jn 3:15");
    let mut whole = kjv_selection(&h, jn(15), None);
    whole["style"] = json!("blue");
    h.shell.run("annotation.highlight", &whole);
    let part = kjv_selection(&h, jn(16), Some("so loved the world"));
    h.shell.run("annotation.highlight", &part);
    let mut note = part;
    note["body"] = json!("Agape");
    h.shell.run("annotation.note", &note);
    let mut red = kjv_selection(&h, jn(17), Some("condemn"));
    red["style"] = json!("red_underline");
    h.shell.run("annotation.highlight", &red);
    h.frames(3);
    h.screenshot(&out.join("annotations-across-light.ppm"));
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.frames(3);
    h.screenshot(&out.join("annotations-across-dark.ppm"));
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}
