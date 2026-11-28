//! Selection, copy and Word Info hover in the GPUI reading pane (#381), on GPUI's
//! TestPlatform, driven through the pane's own pointer and keyboard input. Each test names
//! the egui test it ports.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use std::time::Duration;

use gpui_kit::{Bounds, Pixels, Point, TestAppContext, point, px};
use obs_reading::{Granularity, selected_text};
use support::pane::{self, Open};

fn selection_text(cx: &mut TestAppContext, o: &Open) -> Option<String> {
    o.pane.read_with(cx, |p, _| {
        let sel = p.selection()?;
        Some(selected_text(&p.view().doc, &sel))
    })
}

fn lit(rects: &[Bounds<Pixels>], at: Point<Pixels>) -> bool {
    rects.iter().any(|r| r.contains(&at))
}

fn selection_rects() -> Vec<Bounds<Pixels>> {
    obs_ui_gpui::reading::observed::overlays()
        .into_iter()
        .flat_map(|o| o.selection)
        .collect()
}

/// Ports `crates/ui-egui/src/tests.rs::a_selection_made_across_a_scroll_copies_the_exact_source`:
/// a drag from the BSB's heading into Genesis 1:3 selects across blocks in document
/// coordinates, is painted in the Page tone's selection colour, and Ctrl+C copies the exact
/// source with its reference.
#[gpui_kit::test]
fn a_drag_across_blocks_selects_and_ctrl_c_copies_the_exact_source(cx: &mut TestAppContext) {
    let o = pane::open(cx, "drag-select", &[], "bsb", "Gen 1:1");
    let from = pane::word_spot(cx, &o, "Creation", 0);
    let to = pane::word_spot(cx, &o, "light", 0);
    pane::drag(cx, &o, from, to);
    pane::frames(cx, o.handle, 1);
    let (sel, doc) = o.pane.read_with(cx, |p, _| {
        (p.selection().unwrap(), std::sync::Arc::clone(&p.view().doc))
    });
    let (a, b) = sel.range(&doc);
    assert!(a < b);
    assert_ne!(
        doc.block_at(a.unit, a.byte),
        doc.block_at(b.unit, b.byte),
        "the selection runs across blocks"
    );
    let text = selection_text(cx, &o).unwrap();
    assert!(
        text.starts_with("reation") || text.starts_with("Creation"),
        "{text:?}"
    );
    assert!(text.contains("In the beginning God created"), "{text:?}");
    // Painted under the glyphs, over the words it covers.
    let rects = selection_rects();
    assert!(
        lit(&rects, pane::word_spot(cx, &o, "beginning", 0)),
        "{rects:?}"
    );
    assert!(!lit(&rects, pane::word_spot(cx, &o, "formless", 0)) || text.contains("formless"));
    // Ctrl+C: the text with its reference and the Bible's abbreviation.
    pane::press(cx, &o, "ctrl-c");
    let copied = pane::clipboard(cx).expect("copied");
    assert!(copied.starts_with(text.trim()), "{copied}");
    let label = copied.lines().last().unwrap();
    assert!(
        label.starts_with("Genesis 1:") && label.ends_with("(BSB)"),
        "{label}"
    );
    // A click clears it.
    pane::click(cx, &o, pane::word_spot(cx, &o, "earth", 0));
    assert!(o.pane.read_with(cx, |p, _| p.selection()).is_none());
    pane::close(cx, o);
}

/// Ports `crates/ui-egui/src/tests.rs::word_and_verse_granularity`: a double-click selects the
/// word, a triple-click its Verse, and Copy cites the Verse.
#[gpui_kit::test]
fn a_double_click_selects_a_word_and_a_triple_click_its_verse(cx: &mut TestAppContext) {
    let o = pane::open(cx, "granularity", &[], "bsb", "Gen 1:1");
    let at = pane::word_spot(cx, &o, "formless", 0);
    pane::multi_click(cx, &o, at, 2);
    let sel = o.pane.read_with(cx, |p, _| p.selection()).unwrap();
    assert_eq!(sel.granularity, Granularity::Word);
    assert_eq!(selection_text(cx, &o).as_deref(), Some("formless"));
    assert!(lit(&selection_rects(), at));
    pane::multi_click(cx, &o, at, 3);
    let sel = o.pane.read_with(cx, |p, _| p.selection()).unwrap();
    assert_eq!(sel.granularity, Granularity::Verse);
    let verse = selection_text(cx, &o).unwrap();
    assert!(
        verse.contains("Now the earth was formless and void"),
        "{verse:?}"
    );
    assert!(!verse.contains("Let there be light"), "{verse:?}");
    pane::press(cx, &o, "ctrl-c");
    let copied = pane::clipboard(cx).unwrap();
    assert!(copied.ends_with("Genesis 1:2 (BSB)"), "{copied}");
    pane::close(cx, o);
}

/// Ports `crates/ui-egui/src/tests.rs::selecting_mixed_hebrew_and_ltr_text_highlights_the_right_glyphs`
/// through the pane: in right-to-left Hebrew a double-click selects the word under the pointer,
/// its band is painted over that word's glyphs and not its neighbours', and a triple-click
/// selects its Verse; Greek words select whole.
#[gpui_kit::test]
fn hebrew_and_greek_words_and_verses_select_under_the_pointer(cx: &mut TestAppContext) {
    let o = pane::open(
        cx,
        "hebrew-select",
        &[support::hebrew_genesis_1()],
        "tahot-test",
        "Gen 1:1",
    );
    let bara = "בָּרָ֣א";
    let at = pane::word_spot(cx, &o, bara, 0);
    pane::multi_click(cx, &o, at, 2);
    assert_eq!(selection_text(cx, &o).as_deref(), Some(bara));
    let rects = selection_rects();
    assert!(lit(&rects, at), "{rects:?}");
    let neighbour = pane::word_spot(cx, &o, "אֱלֹהִ֑ים", 0);
    assert!(!lit(&rects, neighbour), "only בָּרָא is painted");
    pane::multi_click(cx, &o, at, 3);
    let verse = selection_text(cx, &o).unwrap();
    assert!(
        verse.starts_with("בְּרֵאשִׁ֖ית") && verse.contains("הָאָֽרֶץ׃"),
        "{verse:?}"
    );
    pane::press(cx, &o, "ctrl-c");
    assert!(
        pane::clipboard(cx)
            .unwrap()
            .ends_with("Genesis 1:1 (TAHOT)")
    );
    pane::close(cx, o);

    let o = pane::open(
        cx,
        "greek-select",
        &[support::greek_john_1()],
        "tagnt-test",
        "John 1:1",
    );
    let at = pane::word_spot(cx, &o, "λόγος", 1);
    pane::multi_click(cx, &o, at, 2);
    assert_eq!(selection_text(cx, &o).as_deref(), Some("λόγος"));
    assert!(lit(&selection_rects(), at));
    pane::close(cx, o);
}

fn shown_strongs(cx: &mut TestAppContext, o: &Open) -> Option<String> {
    o.shell.read_with(cx, |s, _| {
        s.session()
            .word_info
            .shown
            .as_ref()
            .map(|w| w.main.strongs.to_string())
    })
}

fn rest(cx: &mut TestAppContext, o: &Open, for_: Duration) {
    cx.executor().advance_clock(for_);
    cx.run_until_parked();
    pane::frames(cx, o.handle, 2);
}

fn tahot(cx: &mut TestAppContext, name: &str) -> Open {
    pane::open(
        cx,
        name,
        &[obs_engine::testing::bibles::tahot()],
        "tahot",
        "Gen 1:1",
    )
}

/// Ports `crates/ui-egui/src/word_info_tests.rs::word_info_follows_hover_only_after_the_dwell_and_is_sticky`:
/// moving onto a Tagged word underlines it at once, but Word Info follows only after the
/// pointer has rested 150 ms; moving off keeps what it last showed.
#[gpui_kit::test]
fn word_info_follows_hover_only_after_the_dwell_and_is_sticky(cx: &mut TestAppContext) {
    let o = tahot(cx, "dwell");
    let spot = pane::tagged_spot(cx, &o, "H1254A");
    pane::pointer_move(cx, &o, spot);
    pane::frames(cx, o.handle, 1);
    assert!(o.pane.read_with(cx, |p, _| p.hovered_word().is_some()));
    let underlined: Vec<_> = obs_ui_gpui::reading::observed::overlays()
        .into_iter()
        .flat_map(|o| o.underline)
        .collect();
    assert!(
        underlined
            .iter()
            .any(|r| r.origin.x <= spot.x && spot.x <= r.origin.x + r.size.width),
        "the hovered word is underlined at once: {underlined:?}"
    );
    rest(cx, &o, Duration::from_millis(50));
    assert_eq!(shown_strongs(cx, &o), None, "50 ms is less than the dwell");
    rest(cx, &o, Duration::from_millis(120));
    assert_eq!(shown_strongs(cx, &o).as_deref(), Some("H1254A"));
    // Off every word: Word Info keeps the word (sticky).
    let off = point(spot.x, spot.y + px(300.0));
    pane::pointer_move(cx, &o, off);
    rest(cx, &o, Duration::from_millis(300));
    assert_eq!(shown_strongs(cx, &o).as_deref(), Some("H1254A"));
    pane::close(cx, o);
}

/// Ports `crates/ui-egui/src/word_info_tests.rs::pinning_holds_the_panel_while_hovering_elsewhere`:
/// a click pins the word at once; hovering another word past the dwell leaves Word Info on
/// it; Esc unpins, and Word Info follows hover again.
#[gpui_kit::test]
fn pinning_holds_word_info_while_hovering_elsewhere(cx: &mut TestAppContext) {
    let o = tahot(cx, "pin");
    let bara = pane::tagged_spot(cx, &o, "H1254A");
    pane::click(cx, &o, bara);
    let pinned = o.shell.read_with(cx, |s, _| s.session().word_info.pinned);
    assert!(pinned, "a click pins at once");
    assert_eq!(shown_strongs(cx, &o).as_deref(), Some("H1254A"));
    let (unit, range) = pane::tagged(cx, &o, "H0430G");
    let elohim = pane::spot_of(cx, &o, unit, range);
    pane::pointer_move(cx, &o, elohim);
    rest(cx, &o, Duration::from_millis(300));
    assert!(o.pane.read_with(cx, |p, _| p.hovered_word().is_some()));
    assert_eq!(
        shown_strongs(cx, &o).as_deref(),
        Some("H1254A"),
        "pinned: Word Info holds while hovering elsewhere"
    );
    pane::press(cx, &o, "escape");
    let pinned = o.shell.read_with(cx, |s, _| s.session().word_info.pinned);
    assert!(!pinned, "Esc unpins");
    pane::pointer_move(cx, &o, bara);
    rest(cx, &o, Duration::from_millis(300));
    pane::pointer_move(cx, &o, elohim);
    rest(cx, &o, Duration::from_millis(300));
    let shown = shown_strongs(cx, &o).unwrap();
    assert_ne!(shown, "H1254A", "unpinned: Word Info follows hover again");
    pane::close(cx, o);
}

/// A Commentary section that cites Genesis 1:1 with a Scripture reference.
fn commentary_citing_genesis_1_1() -> obs_model::Resource {
    use obs_model::{
        KeyScheme, Manifest, ParaStyle, ResourceBuilder, ResourceType, SpanKind, UnitId, UnitKind,
    };
    use obs_scripture::{Book, Passage, Verse};
    let section = Passage::verse(Verse::new(Book::John, 1, 1));
    let mut m = Manifest::new(
        "refs-test",
        "References (test)",
        "REF",
        ResourceType::Commentary,
        "en",
    );
    m.key_schemes = vec![KeyScheme::Passage];
    m.license = "CC0-1.0".into();
    let mut b = ResourceBuilder::new(m);
    let unit = UnitId::new("John.1.1-John.1.1").unwrap();
    let text = "In the beginning: compare Genesis 1:1 with this verse.".to_string();
    let start = text.find("Genesis 1:1").unwrap();
    let end = start + "Genesis 1:1".len();
    b.add_unit(
        unit.clone(),
        UnitKind::Section,
        Some(section.to_string()),
        text.clone(),
    );
    b.add_span(
        &unit,
        0..text.len(),
        SpanKind::Para {
            style: ParaStyle::Prose,
        },
    );
    b.add_span(
        &unit,
        start..end,
        SpanKind::ScriptureRef {
            targets: vec![Passage::verse(Verse::new(Book::Genesis, 1, 1))].into(),
        },
    );
    b.add_key(obs_model::Key::Passage(section), &unit, None, false);
    b.build().unwrap()
}

/// Ports the egui Shell's reference popup (`references.rs::reference_popup`, covered there by
/// `crates/ui-egui/src/reference_tests.rs`): resting the pointer on a Scripture reference shows
/// the ★ Bible's text of it, laid out by our pipeline; moving off hides it.
#[gpui_kit::test]
fn hovering_a_scripture_reference_previews_its_text(cx: &mut TestAppContext) {
    let o = pane::open(
        cx,
        "reference-preview",
        &[commentary_citing_genesis_1_1()],
        "refs-test",
        "John 1:1",
    );
    let at = pane::word_spot(cx, &o, "Genesis", 0);
    pane::pointer_move(cx, &o, at);
    pane::frames(cx, o.handle, 1);
    let preview = o
        .shell
        .read_with(cx, |s, _| s.reference_preview().cloned())
        .expect("a preview");
    assert_eq!(preview.parts.len(), 1);
    assert_eq!(preview.parts[0].label, "Gen 1:1");
    let text = &preview.parts[0].preview.as_ref().unwrap().text;
    assert!(text.starts_with("In the beginning God created"), "{text}");
    assert!(pane::shows(cx, &o, "Reference preview"));
    let off = pane::word_spot(cx, &o, "verse", 0);
    pane::pointer_move(cx, &o, off);
    pane::frames(cx, o.handle, 1);
    assert!(
        o.shell
            .read_with(cx, |s, _| s.reference_preview().is_none())
    );
    pane::close(cx, o);
}
