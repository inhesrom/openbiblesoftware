//! Find in Resource (#175) in the GPUI reading pane (#381): Ctrl+F through the engine's
//! `tab.find`, the Find bar's field, its count and steps, its options, and the matches painted
//! over the right glyphs, in English, Greek and Hebrew. Each test names the egui test it ports
//! (`crates/ui-egui/src/find_tests.rs`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use gpui_kit::{Bounds, Pixels, TestAppContext};
use obs_ui_gpui::find::{FindStatus, status_text};
use support::pane::{self, Open};

fn kjv(cx: &mut TestAppContext, name: &str) -> Open {
    pane::open(
        cx,
        name,
        &[obs_engine::testing::kjv_john()],
        "kjv",
        "John 1:1",
    )
}

/// The text of every match, in order.
fn found_texts(cx: &mut TestAppContext, o: &Open) -> Vec<String> {
    o.pane.read_with(cx, |p, _| {
        let doc = &p.view().doc;
        p.find()
            .map(|f| {
                f.matches()
                    .iter()
                    .map(|m| doc.units[m.unit].text[m.range.clone()].to_string())
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn current(cx: &mut TestAppContext, o: &Open) -> Option<usize> {
    o.pane
        .read_with(cx, |p, _| p.find().and_then(|f| f.current))
}

fn status(cx: &mut TestAppContext, o: &Open) -> Option<FindStatus> {
    o.pane.read_with(cx, |p, _| p.find().map(|f| f.status()))
}

/// The Find matches the last frame painted: the current one's rectangles, and the others'.
fn painted() -> (Vec<Bounds<Pixels>>, Vec<Bounds<Pixels>>) {
    let found: Vec<_> = obs_ui_gpui::reading::observed::overlays()
        .into_iter()
        .flat_map(|o| o.found)
        .collect();
    let current = found.iter().filter(|(_, c)| *c).map(|(r, _)| *r).collect();
    let others = found.iter().filter(|(_, c)| !*c).map(|(r, _)| *r).collect();
    (current, others)
}

/// Type `text` into the Find bar's field, which has the keyboard, and wait for the worker.
fn find(cx: &mut TestAppContext, o: &Open, text: &str) {
    pane::press(cx, o, "ctrl-f");
    pane::type_text(cx, o, text);
    pane::settle(cx, o);
}

/// Ports `find_tests.rs::ctrl_f_love_in_kjv_john_counts_every_hit_enter_steps_through_all_in_order_and_esc_clears`
/// on the engine's KJV John fixture: Ctrl+F opens the bar with the keyboard in its field,
/// every "God" is found in document order, the first from the top is current ("1 of 5"),
/// Enter steps through every match in order and wraps, Shift+Enter and Previous go back,
/// Next forward, and Esc closes the bar and clears every match.
#[gpui_kit::test]
fn ctrl_f_god_in_kjv_john_counts_every_hit_enter_steps_through_all_and_esc_clears(
    cx: &mut TestAppContext,
) {
    let o = kjv(cx, "find-god");
    let expected: Vec<(usize, usize)> = o.pane.read_with(cx, |p, _| {
        let doc = &p.view().doc;
        doc.units
            .iter()
            .enumerate()
            .flat_map(|(u, unit)| {
                unit.text
                    .to_lowercase()
                    .match_indices("god")
                    .map(move |(b, _)| (u, b))
                    .collect::<Vec<_>>()
            })
            .collect()
    });
    let n = expected.len();
    assert_eq!(n, 5, "the fixture's count");
    assert!(!pane::shows(cx, &o, obs_ui_gpui::find::FIND_HINT));
    find(cx, &o, "god");
    assert!(
        pane::shows(cx, &o, obs_ui_gpui::find::FIND_HINT),
        "{:?}",
        pane::labels(cx, &o)
    );
    let matches: Vec<(usize, usize)> = o.pane.read_with(cx, |p, _| {
        p.find()
            .unwrap()
            .matches()
            .iter()
            .map(|m| (m.unit, m.range.start))
            .collect()
    });
    assert_eq!(
        matches, expected,
        "typed into the field, which had the keyboard"
    );
    assert_eq!(current(cx, &o), Some(0));
    assert!(
        pane::shows(cx, &o, &format!("1 of {n}")),
        "{:?}",
        pane::labels(cx, &o)
    );
    let (now, others) = painted();
    assert_eq!(now.len(), 1, "one current match painted");
    assert!(!others.is_empty(), "the other matches are painted");
    for k in 1..=n {
        pane::press(cx, &o, "enter");
        let want = k % n;
        assert_eq!(current(cx, &o), Some(want), "Enter #{k}");
        assert!(
            pane::shows(cx, &o, &format!("{} of {n}", want + 1)),
            "Enter #{k}"
        );
        let (now, _) = painted();
        assert_eq!(
            now.len(),
            1,
            "Enter #{k}: the current match is in view and painted"
        );
    }
    pane::press(cx, &o, "shift-enter");
    assert_eq!(current(cx, &o), Some(n - 1));
    pane::click_label(cx, &o, "Previous match");
    assert_eq!(current(cx, &o), Some(n - 2));
    pane::click_label(cx, &o, "Next match");
    assert_eq!(current(cx, &o), Some(n - 1));
    // The engine's tab.find_next steps it too.
    pane::run(
        cx,
        &o,
        "tab.find_next",
        serde_json::json!({ "tab": o.tab.0 }),
    );
    assert_eq!(current(cx, &o), Some(0));
    // Esc in the bar closes it and clears every match.
    pane::press(cx, &o, "escape");
    assert!(status(cx, &o).is_none());
    assert!(!pane::shows(cx, &o, obs_ui_gpui::find::FIND_HINT));
    let (now, others) = painted();
    assert!(now.is_empty() && others.is_empty());
    // Ctrl+F opens a new, empty one.
    pane::press(cx, &o, "ctrl-f");
    assert_eq!(status(cx, &o), Some(FindStatus::Empty));
    pane::close(cx, o);
}

/// Ports `find_tests.rs::whole_word_and_match_case_narrow_the_hits`.
#[gpui_kit::test]
fn whole_word_and_match_case_narrow_the_hits(cx: &mut TestAppContext) {
    let o = kjv(cx, "find-options");
    find(cx, &o, "so");
    // "so loved", "Son", "whosoever", "Son".
    assert_eq!(found_texts(cx, &o), ["so", "So", "so", "So"]);
    pane::click_label(cx, &o, "Whole word");
    pane::settle(cx, &o);
    assert_eq!(found_texts(cx, &o), ["so"]);
    assert!(pane::shows(cx, &o, "1 of 1"), "{:?}", pane::labels(cx, &o));
    pane::click_label(cx, &o, "Whole word");
    find(cx, &o, "for");
    assert_eq!(found_texts(cx, &o), ["For", "For"]);
    pane::click_label(cx, &o, "Aa");
    pane::settle(cx, &o);
    assert!(found_texts(cx, &o).is_empty());
    assert!(
        pane::shows(cx, &o, "No matches"),
        "{:?}",
        pane::labels(cx, &o)
    );
    assert_eq!(
        status_text(status(cx, &o).unwrap()),
        "No matches".to_string()
    );
    pane::close(cx, o);
}

/// Hebrew without its points and cantillation marks.
fn unpointed(s: &str) -> String {
    s.chars()
        .filter(|c| !('\u{0591}'..='\u{05C7}').contains(c) || *c == '\u{05BE}')
        .collect()
}

/// Ports `find_tests.rs::ignore_diacritics_finds_greek_and_hebrew_typed_without_accents_or_points`:
/// Greek typed without accents finds the accented words, and not with Ignore diacritics off;
/// Hebrew typed without points finds the pointed words, and the current match is painted over
/// the right-to-left word's glyphs.
#[gpui_kit::test]
fn ignore_diacritics_finds_greek_and_hebrew_typed_without_accents_or_points(
    cx: &mut TestAppContext,
) {
    let o = pane::open(
        cx,
        "find-greek",
        &[support::greek_john_1()],
        "tagnt-test",
        "John 1:1",
    );
    find(cx, &o, "λογος");
    assert_eq!(found_texts(cx, &o), ["λόγος", "λόγος", "λόγος"]);
    find(cx, &o, "θεος");
    assert_eq!(found_texts(cx, &o), ["θεὸς"]);
    let (now, _) = painted();
    let spot = pane::word_spot(cx, &o, "θεὸς", 0);
    assert!(now.iter().any(|r| r.contains(&spot)), "over θεὸς: {now:?}");
    pane::click_label(cx, &o, "Ignore diacritics");
    pane::settle(cx, &o);
    assert!(found_texts(cx, &o).is_empty());
    find(cx, &o, "θεὸς");
    assert_eq!(found_texts(cx, &o), ["θεὸς"]);
    pane::close(cx, o);

    let o = pane::open(
        cx,
        "find-hebrew",
        &[support::hebrew_genesis_1()],
        "tahot-test",
        "Gen 1:1",
    );
    find(cx, &o, "אלהים");
    let found = found_texts(cx, &o);
    assert_eq!(found.len(), 6, "{found:?}");
    assert!(found.iter().all(|w| unpointed(w) == "אלהים"), "{found:?}");
    assert_eq!(current(cx, &o), Some(0));
    let (now, _) = painted();
    let first = pane::word_spot(cx, &o, &found[0], 0);
    assert!(
        now.iter().any(|r| r.contains(&first)),
        "over אֱלֹהִים: {now:?}"
    );
    find(cx, &o, "בראשית");
    assert_eq!(found_texts(cx, &o), ["בְּרֵאשִׁ֖ית"]);
    pane::close(cx, o);
}

/// Ports `find_tests.rs::ctrl_f_with_a_selection_finds_the_selected_words`.
#[gpui_kit::test]
fn ctrl_f_with_a_selection_finds_the_selected_words(cx: &mut TestAppContext) {
    let o = kjv(cx, "find-selection");
    let at = pane::word_spot(cx, &o, "beginning", 0);
    pane::multi_click(cx, &o, at, 2);
    pane::press(cx, &o, "ctrl-f");
    pane::settle(cx, &o);
    let text = o.pane.read_with(cx, |p, _| p.find().unwrap().text.clone());
    assert_eq!(text, "beginning");
    assert_eq!(found_texts(cx, &o), ["beginning", "beginning"]);
    // The engine's tab.find with a text, as the Command box runs it.
    pane::run(
        cx,
        &o,
        "tab.find",
        serde_json::json!({ "tab": o.tab.0, "text": "Word" }),
    );
    pane::settle(cx, &o);
    assert_eq!(found_texts(cx, &o), ["Word", "Word", "Word"]);
    pane::close(cx, o);
}
