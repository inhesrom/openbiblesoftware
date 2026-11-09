//! Notes in the margin (#210), through the headless harness: KJV and WEB Romans 8 written as
//! real `.obr` files, a real User database, Notes made in the KJV, and what each reading
//! pane draws in Margin mode (View → Note display): cards beside their markers, aligned
//! while the text scrolls, stacked with leader lines where they would overlap, expanded and
//! edited in place, and Icons again in a Tab too narrow for the margin.

use std::path::{Path, PathBuf};

use obs_engine::testing::bibles::{bible, paragraphed_bible, v};
use obs_engine::{NoteDisplay, TabId};
use obs_model::{Direction, Resource};
use obs_scripture::{Book, Verse};

use super::*;
use crate::note_margin::{CARD_GAP, GUTTER};
use crate::shell::note_margin::DrawnCard;
use crate::tokens::note_margin::LIFT;

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-margin-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn rom(n: u16) -> Verse {
    v(Book::Romans, 8, n)
}

/// KJV Romans 8:1-11 and 8:26-30 (public domain), each verse its own paragraph.
const ROMANS_KJV: [(u16, &str); 16] = [
    (
        1,
        "There is therefore now no condemnation to them which are in Christ Jesus, who walk not after the flesh, but after the Spirit.",
    ),
    (
        2,
        "For the law of the Spirit of life in Christ Jesus hath made me free from the law of sin and death.",
    ),
    (
        3,
        "For what the law could not do, in that it was weak through the flesh, God sending his own Son in the likeness of sinful flesh, and for sin, condemned sin in the flesh:",
    ),
    (
        4,
        "That the righteousness of the law might be fulfilled in us, who walk not after the flesh, but after the Spirit.",
    ),
    (
        5,
        "For they that are after the flesh do mind the things of the flesh; but they that are after the Spirit the things of the Spirit.",
    ),
    (
        6,
        "For to be carnally minded is death; but to be spiritually minded is life and peace.",
    ),
    (
        7,
        "Because the carnal mind is enmity against God: for it is not subject to the law of God, neither indeed can be.",
    ),
    (8, "So then they that are in the flesh cannot please God."),
    (
        9,
        "But ye are not in the flesh, but in the Spirit, if so be that the Spirit of God dwell in you. Now if any man have not the Spirit of Christ, he is none of his.",
    ),
    (
        10,
        "And if Christ be in you, the body is dead because of sin; but the Spirit is life because of righteousness.",
    ),
    (
        11,
        "But if the Spirit of him that raised up Jesus from the dead dwell in you, he that raised up Christ from the dead shall also quicken your mortal bodies by his Spirit that dwelleth in you.",
    ),
    (
        26,
        "Likewise the Spirit also helpeth our infirmities: for we know not what we should pray for as we ought: but the Spirit itself maketh intercession for us with groanings which cannot be uttered.",
    ),
    (
        27,
        "And he that searcheth the hearts knoweth what is the mind of the Spirit, because he maketh intercession for the saints according to the will of God.",
    ),
    (
        28,
        "And we know that all things work together for good to them that love God, to them who are the called according to his purpose.",
    ),
    (
        29,
        "For whom he did foreknow, he also did predestinate to be conformed to the image of his Son, that he might be the firstborn among many brethren.",
    ),
    (
        30,
        "Moreover whom he did predestinate, them he also called: and whom he called, them he also justified: and whom he justified, them he also glorified.",
    ),
];

/// WEB Romans 8:1-6 and 8:28 (public domain), printed as one paragraph.
const ROMANS_WEB: [(u16, &str); 7] = [
    (
        1,
        "There is therefore now no condemnation to those who are in Christ Jesus, who don’t walk according to the flesh, but according to the Spirit.",
    ),
    (
        2,
        "For the law of the Spirit of life in Christ Jesus made me free from the law of sin and of death.",
    ),
    (
        3,
        "For what the law couldn’t do, in that it was weak through the flesh, God did, sending his own Son in the likeness of sinful flesh and for sin, he condemned sin in the flesh;",
    ),
    (
        4,
        "that the ordinance of the law might be fulfilled in us who walk not after the flesh, but after the Spirit.",
    ),
    (
        5,
        "For those who live according to the flesh set their minds on the things of the flesh, but those who live according to the Spirit, the things of the Spirit.",
    ),
    (
        6,
        "For the mind of the flesh is death, but the mind of the Spirit is life and peace;",
    ),
    (
        28,
        "We know that all things work together for good for those who love God, for those who are called according to his purpose.",
    ),
];

fn kjv() -> Resource {
    let verses: Vec<(Verse, &str)> = ROMANS_KJV.iter().map(|(n, t)| (rom(*n), *t)).collect();
    bible("kjv", "KJV", &verses, None, &[])
}

fn web() -> Resource {
    let verses: Vec<(Verse, &str)> = ROMANS_WEB.iter().map(|(n, t)| (rom(*n), *t)).collect();
    paragraphed_bible("web", "WEB", &verses, Direction::Ltr)
}

/// Launch on `dir`'s User database with `resources` written as `.obr` files there, each
/// open in its own Panel, all at Rom 8:1, in a window wide enough for two Panels with Note
/// margins.
fn launch_with(dir: &Path, resources: &[Resource]) -> Harness {
    obs_engine::testing::bibles::write_library(dir, resources);
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(dir));
    h.screen = WIDE;
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
    // Only the Bibles' Panels: the Library's goes.
    let library = h.tab_id("Library");
    h.shell.run("panel.close", &json!({ "tab": library.0 }));
    h.frames(2);
    for r in resources {
        let tab = h.tab_id(&r.manifest.abbreviation);
        h.shell
            .run("tab.go", &json!({ "tab": tab.0, "text": "Rom 8:1" }));
    }
    settle_all(&mut h);
    past_flash(&mut h);
    h
}

/// Past the flash that marks where a jump landed.
fn past_flash(h: &mut Harness) {
    h.time += 2.0;
    h.frames(2);
}

const WIDE: egui::Vec2 = egui::vec2(1900.0, 1000.0);

fn view(h: &mut Harness, tab: TabId) -> &mut crate::view::ReadingView {
    h.shell
        .st
        .tabs
        .get_mut(&tab)
        .and_then(|t| t.view.as_mut())
        .unwrap()
}

/// A Note on `words` of KJV Rom 8:`verse`.
fn note(h: &mut Harness, verse: u16, words: &str, body: &str) -> String {
    let tab = h.tab_id("KJV");
    let v = view(h, tab);
    let (unit, start) = v.doc.verse_position(rom(verse)).unwrap();
    let range = v.doc.verse_range_at(unit, start).unwrap();
    let text = &v.doc.units[unit].text;
    let at = range.start + text[range.clone()].find(words).unwrap();
    let params = json!({
        "resource": "kjv",
        "start_unit": v.doc.units[unit].id.as_str(),
        "start": at,
        "end": at + words.len(),
        "body": body,
    });
    let out = h.shell.run("annotation.note", &params).unwrap();
    h.frames(2);
    out["id"].as_str().unwrap().to_string()
}

fn margin(h: &Harness, title: &str) -> Vec<DrawnCard> {
    h.shell.st.tabs[&h.tab_id(title)].margin.drawn.clone()
}

fn set_display(h: &mut Harness, title: &str, value: &str) {
    let tab = h.tab_id(title);
    h.shell
        .run(
            "view.note_display",
            &json!({ "tab": tab.0, "value": value }),
        )
        .unwrap();
    settle_all(h);
    h.frames(3);
}

/// The KJV Tab's text column: its left and right edges on screen.
fn column(h: &mut Harness, title: &str) -> (f32, f32) {
    let tab = h.tab_id(title);
    let v = view(h, tab);
    (v.text_origin[0], v.text_origin[0] + v.width())
}

/// Every card is in the margin right of the text column, beside the line of its Note's
/// marker (a card is never above its marker's line), and no two cards overlap.
fn assert_cards_in_margin(h: &mut Harness, title: &str, cards: &[DrawnCard]) {
    let (_, right) = column(h, title);
    for c in cards {
        assert!(
            c.rect.left() >= right + GUTTER - 1.0,
            "{title}: {c:?} is not right of the text column ending at {right}"
        );
        assert!(
            c.marker.right() <= right + 1.0,
            "{c:?}: marker outside the text"
        );
        assert!(
            c.rect.top() >= c.marker.top() - LIFT - 1.0,
            "{title}: {c:?} starts above its marker's line"
        );
        // The leader leaves the text's edge on the marker's line and reaches the card.
        assert!((c.leader[0].y - c.marker.center().y).abs() < 0.5, "{c:?}");
        assert!((c.leader[0].x - right).abs() < 5.0, "{c:?}");
        assert!((c.leader[2].x - c.rect.left()).abs() < 0.5, "{c:?}");
        assert!(c.rect.y_range().contains(c.leader[2].y), "{c:?}");
    }
    for w in cards.windows(2) {
        assert!(
            w[1].rect.top() >= w[0].rect.bottom() + CARD_GAP - 0.5,
            "{title}: cards overlap: {:?} / {:?}",
            w[0],
            w[1]
        );
    }
}

/// Acceptance 1: three Notes in Rom 8 show as margin cards aligned to their markers, and stay
/// aligned while the text scrolls, in the Bible they were made in and in another Bible.
#[test]
fn three_notes_in_romans_8_show_as_aligned_margin_cards_that_stay_aligned_while_scrolling() {
    let dir = folder("aligned");
    let mut h = launch_with(&dir, &[kjv(), web()]);
    let bodies = [
        "No condemnation: the answer to 7:24.",
        "Free: a new law, not no law.",
        "The law fulfilled in us, not by us.",
    ];
    let ids = [
        note(&mut h, 1, "no condemnation", bodies[0]),
        note(&mut h, 2, "made me free", bodies[1]),
        note(&mut h, 4, "fulfilled in us", bodies[2]),
    ];
    // Icons by default: markers, no cards.
    assert!(margin(&h, "KJV").is_empty());
    let marker = h.shell.st.palette.note_marker;
    preview(&h, "note-margin-before");
    assert_eq!(
        h.rects.iter().filter(|(_, c)| *c == marker).count(),
        6,
        "three markers in each Bible"
    );

    set_display(&mut h, "KJV", "margin");
    set_display(&mut h, "WEB", "margin");
    for t in ["KJV", "WEB"] {
        let cards = margin(&h, t);
        assert_eq!(
            cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            ids.iter().map(String::as_str).collect::<Vec<_>>(),
            "{t}: one card per Note, in text order"
        );
        assert_cards_in_margin(&mut h, t, &cards);
    }
    // In the KJV each card starts on its marker's line: none had to move.
    let before = margin(&h, "KJV");
    for c in &before {
        assert!(
            (c.rect.top() - (c.marker.top() - LIFT)).abs() < 0.5,
            "{c:?} is not beside its marker"
        );
    }
    for b in bodies {
        assert!(h.find(b).is_some(), "{b:?} not drawn: {:?}", h.texts());
    }
    preview(&h, "note-margin-cards");

    // Scroll the KJV by 60 points, a step at a time: the cards move with the text.
    let kjv = h.tab_id("KJV");
    for _ in 0..3 {
        view(&mut h, kjv).scroll_by(20.0);
        h.frames(2);
        let now = margin(&h, "KJV");
        assert_cards_in_margin(&mut h, "KJV", &now);
        for c in &now {
            assert!(
                (c.rect.top() - (c.marker.top() - LIFT)).abs() < 0.5,
                "{c:?} left its marker while scrolling"
            );
        }
    }
    settle_all(&mut h);
    let after = margin(&h, "KJV");
    for (a, b) in after.iter().zip(&before) {
        assert_eq!(a.id, b.id);
        assert!(
            (b.rect.top() - a.rect.top() - 60.0).abs() < 1.0,
            "{} moved {} points, not 60",
            a.id,
            b.rect.top() - a.rect.top()
        );
    }
    // The wheel over a card scrolls the text too, and the cards with it.
    let (y, _) = view(&mut h, kjv).position();
    let over = after[2].rect.center();
    h.events.push(Event::PointerMoved(over));
    h.frame();
    for _ in 0..4 {
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -10.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    settle_all(&mut h);
    let (moved, _) = view(&mut h, kjv).position();
    assert!(
        moved > y + 10.0,
        "the wheel over a card did not scroll: {y} → {moved}"
    );
    let wheeled = margin(&h, "KJV");
    assert_cards_in_margin(&mut h, "KJV", &wheeled);
    for c in &wheeled {
        assert!(
            (c.rect.top() - (c.marker.top() - LIFT)).abs() < 0.5,
            "{c:?}"
        );
    }
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Acceptance 2: Notes whose cards would overlap stack legibly, in text order with leader
/// lines; expanding one pushes the next down; a Tab too narrow for the margin shows Icons.
#[test]
fn overlapping_cards_stack_legibly_and_a_narrow_tab_falls_back_to_icons() {
    let dir = folder("stack");
    let mut h = launch_with(&dir, &[kjv(), web()]);
    let ids = [
        note(&mut h, 28, "we know", "We know: not we feel."),
        note(
            &mut h,
            28,
            "all things",
            "All things work together: one purpose, many means. The verb is present tense; God is doing it now, not only at the end.\n\n- Gen 50:20\n- Jas 1:2-4\n\nCompare Gen 50:20.",
        ),
        note(&mut h, 28, "work together", "Work together: one purpose."),
    ];
    let kjv = h.tab_id("KJV");
    h.shell
        .run("tab.go", &json!({ "tab": kjv.0, "text": "Rom 8:27" }));
    settle_all(&mut h);
    past_flash(&mut h);
    set_display(&mut h, "KJV", "margin");
    let cards = margin(&h, "KJV");
    assert_eq!(
        cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ids.iter().map(String::as_str).collect::<Vec<_>>()
    );
    // All three markers are on Rom 8:28's first line, so the cards stack under the first.
    assert!((cards[0].marker.top() - cards[2].marker.top()).abs() < 1.0);
    assert_cards_in_margin(&mut h, "KJV", &cards);
    assert!(
        cards[1].rect.top() > cards[1].marker.bottom(),
        "{:?}",
        cards[1]
    );
    // A stacked card's leader slopes down from its marker's line to the card.
    assert!(cards[2].leader[2].y > cards[2].leader[0].y + 20.0);
    // Collapsed: the long Note shows its first lines only, cut with "…".
    assert!(!cards[1].expanded);
    assert!(h.find("Compare Gen 50:20.").is_none(), "{:?}", h.texts());

    // In the WEB the three Notes are beside Rom 8:28's verse number (#63): their cards
    // stack there too, oldest first.
    let web = h.tab_id("WEB");
    h.shell
        .run("tab.go", &json!({ "tab": web.0, "text": "Rom 8:28" }));
    settle_all(&mut h);
    past_flash(&mut h);
    set_display(&mut h, "WEB", "margin");
    let across = margin(&h, "WEB");
    assert_eq!(
        across.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ids.iter().map(String::as_str).collect::<Vec<_>>()
    );
    assert_cards_in_margin(&mut h, "WEB", &across);
    preview(&h, "note-margin-stacked");

    // A click on the long card expands it; the card below moves down to make room. (The
    // KJV followed the WEB in their Link set: its cards moved with its text.)
    let cards = margin(&h, "KJV");
    assert_cards_in_margin(&mut h, "KJV", &cards);
    let collapsed = cards[1].rect;
    h.click(pos2(collapsed.center().x, collapsed.bottom() - 10.0));
    settle_all(&mut h);
    let open = margin(&h, "KJV");
    assert!(open[1].expanded, "{:?}", open[1]);
    assert!(open[1].rect.height() > collapsed.height() + 10.0);
    assert!(h.find("Compare Gen 50:20.").is_some(), "{:?}", h.texts());
    assert_cards_in_margin(&mut h, "KJV", &open);
    assert!(open[2].rect.top() > cards[2].rect.top() + 10.0);
    preview(&h, "note-margin-expanded");

    // A click on its text now edits it in place with the Note editor; Esc saves it.
    let body = open[1].rect;
    h.click(pos2(body.center().x, body.bottom() - 12.0));
    h.frames(2);
    let ed = h.shell.st.notes_ui.editing.as_ref().expect("editing");
    assert_eq!(ed.id, ids[1]);
    assert_eq!(ed.place, crate::shell::notes_tab::EditPlace::Margin(kjv));
    assert!(margin(&h, "KJV")[1].editing);
    h.events.push(Event::Key {
        key: egui::Key::End,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.frame();
    h.events.push(Event::Text(" And Joseph.".into()));
    h.frame();
    preview(&h, "note-margin-editing");
    h.events.push(Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(3);
    assert!(h.shell.st.notes_ui.editing.is_none(), "editing ended");
    let saved = h.shell.st.session.annotations(None).unwrap();
    let saved = saved.iter().find(|a| a.id == ids[1]).unwrap();
    assert!(
        saved.body().ends_with("Gen 50:20. And Joseph."),
        "{:?}",
        saved.body()
    );

    // A narrow window: the KJV's Panel has no room for the margin, so it shows Icons.
    h.screen = vec2(760.0, 900.0);
    settle_all(&mut h);
    h.frames(2);
    assert_eq!(
        h.shell.st.session.workspace.tab(kjv).unwrap().note_display,
        NoteDisplay::Margin,
        "the choice is kept"
    );
    assert!(margin(&h, "KJV").is_empty(), "{:?}", margin(&h, "KJV"));
    let (left, right) = column(&mut h, "KJV");
    let marker = h.shell.st.palette.note_marker;
    let markers: Vec<Rect> = h
        .rects
        .iter()
        .filter(|(r, c)| *c == marker && r.left() >= left && r.right() <= right + 12.0)
        .map(|(r, _)| *r)
        .collect();
    assert_eq!(
        markers.len(),
        3,
        "the Notes' markers are in the text: {markers:?}"
    );
    assert!(h.find("We know: not we feel.").is_none());
    preview(&h, "note-margin-narrow");
    // Wide again: the cards come back.
    h.screen = WIDE;
    settle_all(&mut h);
    h.frames(2);
    assert_eq!(margin(&h, "KJV").len(), 3);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Hidden draws no Notes in that Tab (Highlights still show); View → Note display offers the
/// three choices for the focused Resource Tab.
#[test]
fn hidden_draws_no_notes_and_the_view_menu_sets_the_focused_tab() {
    let dir = folder("hidden");
    let mut h = launch_with(&dir, &[kjv(), web()]);
    note(&mut h, 1, "no condemnation", "No condemnation.");
    let marker = h.shell.st.palette.note_marker;
    let count = |h: &Harness| h.rects.iter().filter(|(_, c)| *c == marker).count();
    assert_eq!(count(&h), 2);
    // Focus the KJV, then View → Note display → Hidden.
    let kjv = h.tab_id("KJV");
    h.shell.st.session.workspace.user_focused(kjv);
    h.frames(2);
    h.open_menu("View");
    h.click_text("Note display");
    h.click_text("Hidden");
    h.frames(2);
    assert_eq!(
        h.shell.st.session.workspace.tab(kjv).unwrap().note_display,
        NoteDisplay::Hidden
    );
    assert_eq!(count(&h), 1, "only the WEB's marker");
    assert!(margin(&h, "KJV").is_empty());
    h.open_menu("View");
    h.click_text("Note display");
    h.click_text("Margin");
    settle_all(&mut h);
    assert_eq!(margin(&h, "KJV").len(), 1);
    assert!(h.find("No condemnation.").is_some());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}
