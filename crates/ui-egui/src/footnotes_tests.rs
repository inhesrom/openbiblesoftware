//! Footnote markers and popovers (#208) through the headless harness, on John 1 of the BSB
//! as its official USFM download marks it up: a raised letter where each Footnote belongs,
//! its popover after Word Info's dwell, pinned by a click or Enter, its Bible references
//! previewed on hover, the View toggle per Tab, and copies that leave the notes out.

use std::path::PathBuf;

use egui::{Event, Key, Modifiers, Pos2};
use obs_engine::TabId;
use obs_scripture::{Book, Verse};
use serde_json::json;

use super::*;
use crate::view::FootnoteMarker;

struct Files(PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A library of the BSB excerpt alone (Isaiah 40:1-3, John 1:1-23), its Tab at `at`.
fn bsb(name: &str, at: &str) -> (Harness, Files, TabId) {
    library(name, &[obs_engine::testing::bibles::bsb_john_1()], at)
}

/// A library of `resources`, the first one's Tab at `at`.
fn library(name: &str, resources: &[obs_model::Resource], at: &str) -> (Harness, Files, TabId) {
    let mut h = Harness::new();
    let dir = std::env::temp_dir().join(format!("obs-footnotes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    h.shell.st.session.library = obs_engine::Library::new();
    for r in resources {
        let path = files.0.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
        h.shell.st.session.library.add_file(&path).unwrap();
    }
    h.shell.reset_layout();
    let tab = h.tab_id(&resources[0].manifest.abbreviation);
    h.shell.run("tab.go", &json!({ "tab": tab.0, "text": at }));
    h.frames(6);
    (h, files, tab)
}

/// Open Resource `id` in a pane beside the focused one, at `at`: its Tab.
fn split(h: &mut Harness, beside: TabId, id: &str, at: &str) -> TabId {
    h.shell.st.session.workspace.focus(beside);
    h.shell
        .run("resource.open", &json!({ "id": id, "split": true }));
    h.frames(6);
    let other = h.shell.st.session.workspace.focused().unwrap();
    assert_ne!(other, beside);
    h.shell
        .run("tab.go", &json!({ "tab": other.0, "text": at }));
    h.frames(6);
    other
}

/// Press Tab until the keyboard focus is on `widget` (a Footnote marker).
fn tab_to(h: &mut Harness, widget: egui::Id) {
    for _ in 0..80 {
        if h.ctx.memory(|mem| mem.focused()) == Some(widget) {
            return;
        }
        press(h, Key::Tab);
    }
    panic!("Tab never reached the marker");
}

fn markers(h: &Harness, tab: TabId) -> Vec<FootnoteMarker> {
    h.shell.st.tabs[&tab]
        .view
        .as_ref()
        .unwrap()
        .footnote_markers(900.0)
}

/// The marker of the Footnote whose text starts with `text`, and its centre on screen.
fn marker(h: &Harness, tab: TabId, text: &str) -> (FootnoteMarker, Pos2) {
    let view = h.shell.st.tabs[&tab].view.as_ref().unwrap();
    let [ox, oy] = view.text_origin;
    let m = markers(h, tab)
        .into_iter()
        .find(|m| {
            view.doc
                .footnote(&m.note)
                .is_some_and(|n| n.text.starts_with(text))
        })
        .unwrap_or_else(|| panic!("no marker for {text:?} in view"));
    let [x0, y0, x1, y1] = m.rect;
    let at = pos2(ox + (x0 + x1) / 2.0, oy + (y0 + y1) / 2.0);
    (m, at)
}

/// The Footnote popover drawn, as a test looks at it.
struct Shown {
    tab: TabId,
    hit: crate::pane::FootnoteHit,
    pinned: bool,
    rect: Option<egui::Rect>,
    link_rects: Vec<egui::Rect>,
    body: Option<std::sync::Arc<crate::shell::footnotes::FootnoteBody>>,
}

fn popover(h: &Harness) -> Option<Shown> {
    let (tab, f) = h.shell.st.footnotes.shown()?;
    Some(Shown {
        tab,
        hit: f.state.phase.marker()?.clone(),
        pinned: f.state.phase.pinned(),
        rect: f.rect,
        link_rects: f.link_rects.clone(),
        body: f.body().cloned(),
    })
}

/// The marker whose popover shows or waits for its dwell.
fn pending(h: &Harness) -> Option<crate::pane::FootnoteHit> {
    h.shell.st.footnotes.pending().map(|(_, m)| m.clone())
}

fn shown_text(h: &Harness) -> Option<String> {
    popover(h).and_then(|p| p.body).map(|b| b.unit.text.clone())
}

fn press(h: &mut Harness, key: Key) {
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frame();
}

/// Acceptance: BSB John 1 shows its Footnote markers, lettered from the chapter's first, in
/// the Page tone's marker colour; hovering one shows its text after Word Info's dwell.
#[test]
fn bsb_john_1_shows_footnote_markers_and_hovering_one_shows_its_text_after_the_dwell() {
    let (mut h, _files, tab) = bsb("hover", "Jn 1:1");
    let labels: Vec<String> = markers(&h, tab).into_iter().map(|m| m.label).collect();
    assert_eq!(
        labels[..5],
        ["a", "b", "c", "d", "e"],
        "1:5, 1:14 (two), 1:18 (two)"
    );
    assert!(
        h.meshes.contains(&h.shell.st.palette.note_marker),
        "the letters are drawn in the marker colour"
    );
    let (m, at) = marker(&h, tab, "Or comprehended");
    assert_eq!(m.label, "a");
    h.events.push(Event::PointerMoved(at));
    h.frames(2);
    assert!(
        shown_text(&h).is_none(),
        "two frames is less than the dwell"
    );
    h.frames(12);
    assert_eq!(shown_text(&h).as_deref(), Some("Or comprehended"));
    assert!(h.find("Footnote a · John 1:5").is_some(), "{:?}", h.texts());
    // The note is Resource text: the text pipeline draws it, not an egui label.
    assert!(!h.texts().iter().any(|t| t.contains("comprehended")));
    // Off the marker (and the popover), it goes.
    h.events.push(Event::PointerMoved(pos2(at.x, at.y + 300.0)));
    h.frames(3);
    assert!(shown_text(&h).is_none());
}

/// Acceptance: a Bible reference in a Footnote is a link; hovering it previews the Passage,
/// clicking it goes there.
#[test]
fn a_reference_in_a_footnote_previews_its_passage() {
    let (mut h, _files, tab) = bsb("reference", "Jn 1:19");
    let (_, at) = marker(&h, tab, "Isaiah 40:3");
    h.events.push(Event::PointerMoved(at));
    h.frames(14);
    assert_eq!(
        shown_text(&h).as_deref(),
        Some("Isaiah 40:3 (see also LXX)")
    );
    let body = popover(&h).unwrap().body.clone().unwrap();
    assert_eq!(body.links.len(), 1);
    assert!(
        h.find("Isa 40:3").is_none(),
        "no preview before hovering the link"
    );
    // Into the popover and onto its link: the popover stays and the Passage previews.
    let link = popover(&h).unwrap().link_rects[0];
    let path = [at, pos2(at.x, link.center().y), link.center()];
    for p in path {
        h.events.push(Event::PointerMoved(p));
        h.frames(2);
    }
    h.frames(6);
    assert!(popover(&h).is_some(), "the pointer may rest on the popover");
    assert!(h.find("Isa 40:3").is_some(), "{:?}", h.texts());
    h.click(link.center());
    h.frames(6);
    assert!(popover(&h).is_none(), "following the link closes it");
    let isaiah = Verse::new(Book::Isaiah, 40, 3);
    let went = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .any(|t| t.history.current() == Some(isaiah))
        || h.shell
            .st
            .references
            .pending_navigation
            .values()
            .any(|v| *v == isaiah);
    assert!(went, "a Tab goes to Isaiah 40:3");
}

/// Clicking a marker pins its popover: it stays when the pointer leaves, until Esc, its ×,
/// a click elsewhere or another click on the marker.
#[test]
fn clicking_a_marker_pins_its_popover_until_esc_or_another_click() {
    let (mut h, _files, tab) = bsb("pin", "Jn 1:1");
    let (_, at) = marker(&h, tab, "Or comprehended");
    h.click(at);
    assert!(
        popover(&h).is_some_and(|p| p.pinned),
        "shown at once, pinned"
    );
    h.events
        .push(Event::PointerMoved(pos2(at.x + 200.0, at.y + 300.0)));
    h.frames(4);
    assert_eq!(shown_text(&h).as_deref(), Some("Or comprehended"));
    // It follows its marker as the text scrolls.
    let before = popover(&h).unwrap().hit.rect;
    let view = h
        .shell
        .st
        .tabs
        .get_mut(&tab)
        .unwrap()
        .view
        .as_mut()
        .unwrap();
    view.scroll_by(20.0);
    h.frames(2);
    let after = popover(&h).unwrap().hit.rect;
    assert!(
        (after.top() - (before.top() - 20.0)).abs() < 0.5,
        "{before:?} {after:?}"
    );
    let (_, at) = marker(&h, tab, "Or comprehended");
    press(&mut h, Key::Escape);
    h.frames(30);
    assert!(popover(&h).is_none(), "Esc closes it, past the dwell");
    h.click(at);
    assert!(popover(&h).is_some());
    h.click(at);
    assert!(
        popover(&h).is_none(),
        "a second click on the marker closes it"
    );
    h.frames(30);
    assert!(
        pending(&h).is_none(),
        "it stays closed past the dwell with the pointer still on the marker"
    );
    h.click(at);
    assert!(
        popover(&h).is_some_and(|p| p.pinned),
        "a further click pins it again"
    );
    h.click(at);
    h.click(at);
    let area = popover(&h).unwrap().rect.unwrap();
    let close = (h.shapes.iter())
        .find(|(r, t)| *t == crate::icons::Icon::Close.marker() && area.contains(r.center()))
        .map(|(r, _)| r.center())
        .expect("a Close in the pinned popover");
    h.click(close);
    assert!(popover(&h).is_none(), "Close closes it");
}

/// The markers are reachable from the keyboard: Tab focuses one (its popover follows after
/// the dwell), Enter pins it and moves focus to its first link, Esc closes it and gives the
/// marker its focus back. Each marker and link has an AccessKit name.
#[test]
fn the_keyboard_reaches_markers_and_the_popovers_links() {
    let (mut h, _files, tab) = bsb("keys", "Jn 1:19");
    h.ctx.enable_accesskit();
    let (m, at) = marker(&h, tab, "Isaiah 40:3");
    // Click the text just before the marker so the pane has focus, then Tab to it.
    h.click(pos2(at.x - 40.0, at.y + 4.0));
    let widget = |h: &Harness| {
        h.shell.st.tabs[&tab].view.as_ref().map(|_| ())?;
        h.ctx.memory(|mem| mem.focused())
    };
    for _ in 0..20 {
        if pending(&h).is_some_and(|p| p.note == m.note) {
            break;
        }
        press(&mut h, Key::Tab);
    }
    h.frames(14);
    let focused = widget(&h).expect("a marker has focus");
    assert_eq!(
        shown_text(&h).as_deref(),
        Some("Isaiah 40:3 (see also LXX)")
    );
    press(&mut h, Key::Enter);
    h.frames(3);
    assert!(popover(&h).is_some_and(|p| p.pinned));
    assert_ne!(widget(&h), Some(focused), "focus moved into the popover");
    assert!(h.find("Isa 40:3").is_some(), "the focused link previews");
    let tree = h.tree.clone().expect("an AccessKit tree");
    let names: Vec<String> = tree
        .nodes
        .iter()
        .filter_map(|(_, n)| n.label().map(str::to_string))
        .collect();
    assert!(names.iter().any(|n| n == "Footnote f"), "{names:?}");
    assert!(names.iter().any(|n| n == "Isaiah 40:3"), "{names:?}");
    assert!(names.iter().any(|n| n == "Close footnote"), "{names:?}");
    press(&mut h, Key::Escape);
    h.frames(2);
    assert!(popover(&h).is_none());
    assert_eq!(widget(&h), Some(focused), "focus is back on the marker");
    // The focused marker does not bring it back after the dwell.
    h.frames(30);
    assert!(popover(&h).is_none(), "Esc keeps it closed past the dwell");
    assert_eq!(widget(&h), Some(focused), "the marker keeps its focus");
    // Enter on the marker opens it again.
    press(&mut h, Key::Enter);
    h.frames(3);
    assert!(popover(&h).is_some_and(|p| p.pinned));
}

/// Esc also closes a popover shown on hover, and it stays closed while the pointer rests on
/// the marker; it shows again once the pointer has left and come back.
#[test]
fn esc_closes_a_hover_popover_until_the_pointer_leaves_its_marker() {
    let (mut h, _files, tab) = bsb("esc-hover", "Jn 1:1");
    let (_, at) = marker(&h, tab, "Or comprehended");
    h.events.push(Event::PointerMoved(at));
    h.frames(14);
    assert!(popover(&h).is_some_and(|p| !p.pinned), "shown on hover");
    press(&mut h, Key::Escape);
    h.frames(30);
    assert!(popover(&h).is_none(), "Esc keeps it closed past the dwell");
    h.events
        .push(Event::PointerMoved(pos2(at.x + 200.0, at.y + 300.0)));
    h.frames(2);
    h.events.push(Event::PointerMoved(at));
    h.frames(14);
    assert_eq!(shown_text(&h).as_deref(), Some("Or comprehended"));
}

/// Click the text just before the Footnote starting with `text`, then Tab until its marker
/// has the keyboard focus: the marker and its widget.
fn focus_marker(h: &mut Harness, tab: TabId, text: &str) -> (FootnoteMarker, egui::Id) {
    let (m, at) = marker(h, tab, text);
    h.click(pos2(at.x - 40.0, at.y + 4.0));
    for _ in 0..20 {
        if pending(h).is_some_and(|p| p.note == m.note) {
            break;
        }
        press(h, Key::Tab);
    }
    let widget = h
        .ctx
        .memory(|mem| mem.focused())
        .expect("a marker has focus");
    (m, widget)
}

/// Run frames until the keyboard focus has left `marker` for its pinned popover (a link, or
/// the ×): the note is laid out off-thread, slower on some CI runners.
fn wait_for_focus_inside(h: &mut Harness, marker: egui::Id) {
    for _ in 0..300 {
        let focused = h.ctx.memory(|mem| mem.focused());
        if focused.is_some() && focused != Some(marker) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.frame();
    }
}

/// A shown Footnote has first refusal on Esc: with Word Info pinned too, Esc closes the
/// Footnote and gives its marker the focus back, leaving Word Info pinned; the next Esc
/// unpins Word Info.
#[test]
fn esc_closes_a_pinned_footnote_before_it_unpins_word_info() {
    let (mut h, _files, tab) = bsb("esc-word-info", "Jn 1:19");
    let (_, widget) = focus_marker(&mut h, tab, "Isaiah 40:3");
    let part = obs_model::MorphemePart::new("G3056".parse().unwrap());
    let word =
        obs_engine::ShownWord::new("λόγος", false, "John 1:1".into(), &[part], |_| None).unwrap();
    h.shell.st.session.word_info.pin(word);
    press(&mut h, Key::Enter);
    h.frames(3);
    assert!(popover(&h).is_some_and(|p| p.pinned));
    press(&mut h, Key::Escape);
    h.frames(2);
    assert!(popover(&h).is_none(), "Esc closes the Footnote first");
    assert_eq!(
        h.ctx.memory(|mem| mem.focused()),
        Some(widget),
        "focus is back on the marker"
    );
    assert!(
        h.shell.st.session.word_info.pinned,
        "Word Info stays pinned"
    );
    press(&mut h, Key::Escape);
    assert!(
        !h.shell.st.session.word_info.pinned,
        "the next Esc unpins Word Info"
    );
}

/// Space on a focused marker pins its popover, even with the pointer resting on the text:
/// the text does not page first and carry the marker away.
#[test]
fn space_on_a_focused_marker_pins_it_without_paging_the_text() {
    let (mut h, _files, tab) = bsb("keys-space", "Jn 1:19");
    let (m, _) = focus_marker(&mut h, tab, "Isaiah 40:3");
    let (_, at) = marker(&h, tab, "Isaiah 40:3");
    h.events
        .push(Event::PointerMoved(pos2(at.x - 120.0, at.y - 40.0)));
    h.frames(2);
    let position = |h: &mut Harness| {
        (h.shell
            .st
            .tabs
            .get_mut(&tab)
            .unwrap()
            .view
            .as_mut()
            .unwrap())
        .position()
        .0
    };
    let before = position(&mut h);
    press(&mut h, Key::Space);
    h.frames(3);
    let p = popover(&h).expect("Space pins it");
    assert!(p.pinned && p.hit.note == m.note);
    assert!(
        (position(&mut h) - before).abs() < 0.01,
        "the text did not page: {before} → {}",
        position(&mut h)
    );
    assert_eq!(
        shown_text(&h).as_deref(),
        Some("Isaiah 40:3 (see also LXX)")
    );
}

/// The scroll position of `tab`'s text.
fn scrolled(h: &mut Harness, tab: TabId) -> f64 {
    let view = h
        .shell
        .st
        .tabs
        .get_mut(&tab)
        .unwrap()
        .view
        .as_mut()
        .unwrap();
    view.position().0
}

/// Review repro: Space on a focused marker with the pointer resting on another pane's text
/// pins the marker's Footnote, and neither pane pages.
#[test]
fn space_on_a_focused_marker_pages_no_pane_under_the_pointer() {
    let (mut h, _files, tab) = bsb("split-space", "Jn 1:1");
    let other = split(&mut h, tab, "bsb", "Jn 1:1");
    let (m, _) = focus_marker(&mut h, tab, "Or comprehended");
    assert_eq!(
        h.shell.st.footnotes.pending().map(|(t, _)| t),
        Some(tab),
        "the marker focused is in the first pane"
    );
    // The pointer on the other pane's text, away from its markers.
    let [left, top] = h.shell.st.tabs[&other].view.as_ref().unwrap().text_origin;
    h.events
        .push(Event::PointerMoved(pos2(left + 20.0, top + 60.0)));
    h.frames(2);
    let before = (scrolled(&mut h, tab), scrolled(&mut h, other));
    press(&mut h, Key::Space);
    h.frames(3);
    let p = popover(&h).expect("Space pins it");
    assert!(p.pinned && p.tab == tab && p.hit.note == m.note);
    let after = (scrolled(&mut h, tab), scrolled(&mut h, other));
    assert!(
        (after.0 - before.0).abs() < 0.01 && (after.1 - before.1).abs() < 0.01,
        "no pane paged: {before:?} → {after:?}"
    );
    // With no marker focused, Space pages the pane under the pointer.
    press(&mut h, Key::Escape);
    h.frames(2);
    if let Some(focused) = h.ctx.memory(|mem| mem.focused()) {
        h.ctx.memory_mut(|mem| mem.surrender_focus(focused));
    }
    h.frames(2);
    let before = scrolled(&mut h, other);
    press(&mut h, Key::Space);
    h.frames(2);
    assert!(scrolled(&mut h, other) > before + 1.0, "the pane pages");
}

/// Whether a Tab goes (or is opening to go) to `verse`.
fn went_to(h: &Harness, verse: obs_scripture::Verse) -> bool {
    (h.shell.st.session.workspace.tabs()).any(|t| t.history.current() == Some(verse))
        || (h.shell.st.references.pending_navigation.values()).any(|v| *v == verse)
}

/// Review repro: once Enter has moved the focus into a pinned popover, Space on its focused
/// link follows the link, with the pointer resting on the text of the marker's pane or of
/// another pane: neither pane pages first (which would carry the marker, and the popover
/// with it, away before the link could take the key). The two Tabs are unlinked, so the
/// link opens a new Tab and leaves both where they were.
#[test]
fn space_on_a_focused_popover_link_follows_it_and_pages_no_pane() {
    for pointer_on_other in [false, true] {
        let name = format!("space-link-{pointer_on_other}");
        let (mut h, _files, tab) = bsb(&name, "Jn 1:19");
        let other = split(&mut h, tab, "bsb", "Jn 1:19");
        for t in [tab, other] {
            h.shell.st.session.workspace.set_link(t, None);
        }
        h.frames(2);
        let (m, marker_widget) = focus_marker(&mut h, tab, "Isaiah 40:3");
        press(&mut h, Key::Enter);
        h.frames(3);
        wait_for_focus_inside(&mut h, marker_widget);
        let p = popover(&h).expect("Enter pins it");
        assert!(p.pinned && p.tab == tab && p.hit.note == m.note);
        let focused = h.ctx.memory(|mem| mem.focused());
        assert!(
            focused.is_some() && focused != Some(marker_widget),
            "focus moved onto the popover's link"
        );
        // The pointer onto reading text, away from the markers and the popover, no click.
        let at = if pointer_on_other {
            let [left, top] = h.shell.st.tabs[&other].view.as_ref().unwrap().text_origin;
            pos2(left + 20.0, top + 60.0)
        } else {
            let (_, at) = marker(&h, tab, "Isaiah 40:3");
            pos2(at.x - 120.0, at.y - 40.0)
        };
        h.events.push(Event::PointerMoved(at));
        h.frames(2);
        assert!(popover(&h).is_some_and(|p| p.pinned), "still pinned");
        let before = (scrolled(&mut h, tab), scrolled(&mut h, other));
        press(&mut h, Key::Space);
        h.frames(6);
        assert!(popover(&h).is_none(), "following the link closes it");
        assert!(
            went_to(&h, Verse::new(Book::Isaiah, 40, 3)),
            "Space on the link goes to Isaiah 40:3 (pointer on other pane: {pointer_on_other})"
        );
        let after = (scrolled(&mut h, tab), scrolled(&mut h, other));
        assert!(
            (after.0 - before.0).abs() < 0.01 && (after.1 - before.1).abs() < 0.01,
            "no pane paged: {before:?} → {after:?}"
        );
    }
}

/// Review repro: Space on a pinned popover's focused × (a Footnote with no link to focus)
/// closes it, with the pointer resting on another pane's text, and no pane pages.
#[test]
fn space_on_a_focused_popover_close_closes_it_and_pages_no_pane() {
    let (mut h, _files, tab) = bsb("space-close", "Jn 1:1");
    let other = split(&mut h, tab, "bsb", "Jn 1:1");
    let (_, marker_widget) = focus_marker(&mut h, tab, "Or comprehended");
    press(&mut h, Key::Enter);
    h.frames(3);
    wait_for_focus_inside(&mut h, marker_widget);
    assert!(popover(&h).is_some_and(|p| p.pinned && p.tab == tab));
    let focused = h.ctx.memory(|mem| mem.focused());
    assert!(
        focused.is_some() && focused != Some(marker_widget),
        "focus moved onto the popover's ×"
    );
    let [left, top] = h.shell.st.tabs[&other].view.as_ref().unwrap().text_origin;
    h.events
        .push(Event::PointerMoved(pos2(left + 20.0, top + 60.0)));
    h.frames(2);
    let before = (scrolled(&mut h, tab), scrolled(&mut h, other));
    press(&mut h, Key::Space);
    h.frames(3);
    assert!(popover(&h).is_none(), "Space on the × closes it");
    let after = (scrolled(&mut h, tab), scrolled(&mut h, other));
    assert!(
        (after.0 - before.0).abs() < 0.01 && (after.1 - before.1).abs() < 0.01,
        "no pane paged: {before:?} → {after:?}"
    );
}

/// Review repro: Esc on a keyboard-pinned popover gives the focus back to its marker, and
/// Space on the very next frame (no idle frame between) is the marker's: it pins the
/// Footnote again, and no pane under the pointer pages.
#[test]
fn space_right_after_esc_returns_focus_to_the_marker_pages_no_pane() {
    for pointer_on_other in [false, true] {
        let name = format!("esc-space-{pointer_on_other}");
        let (mut h, _files, tab) = bsb(&name, "Jn 1:19");
        let other = split(&mut h, tab, "bsb", "Jn 1:19");
        let (m, marker_widget) = focus_marker(&mut h, tab, "Isaiah 40:3");
        press(&mut h, Key::Enter);
        h.frames(3);
        wait_for_focus_inside(&mut h, marker_widget);
        assert!(popover(&h).is_some_and(|p| p.pinned && p.tab == tab));
        assert_ne!(
            h.ctx.memory(|mem| mem.focused()),
            Some(marker_widget),
            "focus moved into the popover"
        );
        let at = if pointer_on_other {
            let [left, top] = h.shell.st.tabs[&other].view.as_ref().unwrap().text_origin;
            pos2(left + 20.0, top + 60.0)
        } else {
            let (_, at) = marker(&h, tab, "Isaiah 40:3");
            pos2(at.x - 120.0, at.y - 40.0)
        };
        h.events.push(Event::PointerMoved(at));
        h.frames(2);
        assert!(popover(&h).is_some_and(|p| p.pinned), "still pinned");
        let before = (scrolled(&mut h, tab), scrolled(&mut h, other));
        press(&mut h, Key::Escape);
        assert!(popover(&h).is_none(), "Esc closes it");
        press(&mut h, Key::Space);
        h.frames(3);
        let p = popover(&h).expect("Space on the marker pins it again");
        assert!(p.pinned && p.tab == tab && p.hit.note == m.note);
        let after = (scrolled(&mut h, tab), scrolled(&mut h, other));
        assert!(
            (after.0 - before.0).abs() < 0.01 && (after.1 - before.1).abs() < 0.01,
            "no pane paged (pointer on other pane: {pointer_on_other}): {before:?} → {after:?}"
        );
    }
}

/// Esc on a popover shown on hover leaves the keyboard focus off every Footnote widget, so
/// Space on the next frame pages the pane under the pointer.
#[test]
fn space_right_after_esc_on_a_hover_popover_pages_the_pane() {
    let (mut h, _files, tab) = bsb("esc-hover-space", "Jn 1:1");
    let (_, at) = marker(&h, tab, "Or comprehended");
    h.events.push(Event::PointerMoved(at));
    h.frames(14);
    assert!(popover(&h).is_some_and(|p| !p.pinned), "shown on hover");
    h.events
        .push(Event::PointerMoved(pos2(at.x - 120.0, at.y + 40.0)));
    h.frame();
    let before = scrolled(&mut h, tab);
    press(&mut h, Key::Escape);
    press(&mut h, Key::Space);
    h.frames(2);
    assert!(popover(&h).is_none());
    assert!(scrolled(&mut h, tab) > before + 1.0, "the pane pages");
}

/// With a keyboard-pinned popover, a click on another pane's text closes it and takes the
/// focus off its widgets, so Space on the next frame pages that pane, not the marker's.
#[test]
fn space_after_clicking_another_panes_text_pages_that_pane() {
    let (mut h, _files, tab) = bsb("click-other-space", "Jn 1:19");
    let other = split(&mut h, tab, "bsb", "Jn 1:19");
    for t in [tab, other] {
        h.shell.st.session.workspace.set_link(t, None);
    }
    h.frames(2);
    let (_, marker_widget) = focus_marker(&mut h, tab, "Isaiah 40:3");
    press(&mut h, Key::Enter);
    h.frames(3);
    wait_for_focus_inside(&mut h, marker_widget);
    assert!(popover(&h).is_some_and(|p| p.pinned && p.tab == tab));
    let [left, top] = h.shell.st.tabs[&other].view.as_ref().unwrap().text_origin;
    h.click(pos2(left + 20.0, top + 60.0));
    assert!(popover(&h).is_none(), "the click closes it");
    let before = (scrolled(&mut h, tab), scrolled(&mut h, other));
    press(&mut h, Key::Space);
    h.frames(2);
    let after = (scrolled(&mut h, tab), scrolled(&mut h, other));
    assert!(
        (after.0 - before.0).abs() < 0.01,
        "the marker's pane stays: {before:?} → {after:?}"
    );
    assert!(
        after.1 > before.1 + 1.0,
        "the clicked pane pages: {before:?} → {after:?}"
    );
}

/// Review repro: a marker whose popover was dismissed while it keeps the keyboard focus
/// stays dismissed after the pointer has shown another marker's popover (in its pane, or
/// another pane) and left it for the text, however long the focus rests on it.
#[test]
fn a_focused_marker_stays_dismissed_after_hovering_another() {
    focused_marker_dismissed_after_hovering_b("focus-dismissed", false);
    focused_marker_dismissed_after_hovering_b("focus-dismissed-split", true);
}

/// Dismiss focused marker a, then hover marker b of the same pane or, with `split_pane`,
/// the marker of a second pane's Tab (the same Footnote, another marker).
fn focused_marker_dismissed_after_hovering_b(name: &str, split_pane: bool) {
    let (mut h, _files, tab) = bsb(name, "Jn 1:1");
    let hovered = if split_pane {
        split(&mut h, tab, "bsb", "Jn 1:1")
    } else {
        tab
    };
    let (_, widget) = focus_marker(&mut h, tab, "Or comprehended");
    press(&mut h, Key::Enter);
    h.frames(3);
    assert_eq!(shown_text(&h).as_deref(), Some("Or comprehended"));
    press(&mut h, Key::Escape);
    h.frames(2);
    assert!(popover(&h).is_none(), "Esc closes it");
    assert_eq!(
        h.ctx.memory(|mem| mem.focused()),
        Some(widget),
        "focus is back on a"
    );
    // The pointer onto marker b: its popover shows; a keeps the focus.
    let view = h.shell.st.tabs[&hovered].view.as_ref().unwrap();
    let [ox, oy] = view.text_origin;
    // In a second pane, the same Footnote's marker there is another marker.
    let label = if split_pane { "a" } else { "b" };
    let b = (markers(&h, hovered).into_iter())
        .find(|m| m.label == label)
        .expect("marker b");
    let b_note = view.doc.footnote(&b.note).unwrap().text.clone();
    let [x0, y0, x1, y1] = b.rect;
    let at = pos2(ox + (x0 + x1) / 2.0, oy + (y0 + y1) / 2.0);
    h.events.push(Event::PointerMoved(at));
    h.frames(14);
    assert_eq!(shown_text(&h), Some(b_note), "b's popover shows on hover");
    assert_eq!(popover(&h).unwrap().tab, hovered);
    assert_eq!(h.ctx.memory(|mem| mem.focused()), Some(widget));
    // Off b, onto the text: b's goes, and a's does not come back with the dwell.
    h.events.push(Event::PointerMoved(pos2(at.x, at.y + 300.0)));
    h.frames(30);
    assert!(popover(&h).is_none(), "the focused marker stays dismissed");
    assert!(pending(&h).is_none(), "no dwell either");
    assert_eq!(h.ctx.memory(|mem| mem.focused()), Some(widget));
    // Enter on it opens it again.
    press(&mut h, Key::Enter);
    h.frames(3);
    assert!(popover(&h).is_some_and(|p| p.pinned));
}

/// A Footnote taller than the window scrolls inside its popover, which stays on screen: its
/// last words and the reference after them can be scrolled to and followed.
#[test]
fn a_long_footnote_scrolls_to_its_last_reference() {
    let verses = [
        (
            Verse::new(Book::Psalms, 3, 1),
            "LORD, how are they increased that trouble me!",
        ),
        (
            Verse::new(Book::Psalms, 3, 2),
            "Many there be which say of my soul,",
        ),
        (
            Verse::new(Book::Psalms, 3, 3),
            "But thou, O LORD, art a shield for me;",
        ),
    ];
    let mut bible = obs_engine::testing::bibles::bible("long-note", "LNB", &verses, None, &[]);
    let note = obs_model::UnitId::note(1);
    let text = format!(
        "{}See Ps 3:3.",
        "Many words of a long note, more than any window can show at once. ".repeat(60)
    );
    bible.units.push(obs_model::TextUnit {
        id: note.clone(),
        kind: obs_model::UnitKind::Note,
        title: None,
        text: text.clone(),
        spans: Vec::new(),
    });
    let psalms = bible
        .units
        .iter_mut()
        .find(|u| u.kind == obs_model::UnitKind::BibleBook)
        .unwrap();
    let end = u32::try_from(psalms.text.len()).unwrap();
    psalms.spans.push(obs_model::Span {
        start: end,
        end,
        kind: obs_model::SpanKind::NoteRef {
            note,
            note_kind: obs_model::NoteKind::Footnote,
        },
    });
    let (mut h, _files, tab) = library("long-note", &[bible], "Ps 3:1");
    // A short window: the popover's cap is the window's, not its own.
    h.screen = egui::vec2(1000.0, 420.0);
    h.frames(4);
    let (_, at) = marker(&h, tab, "Many words");
    h.click(at);
    // The long note is laid out off-thread: wait until its link is drawn.
    for _ in 0..300 {
        if popover(&h).is_some_and(|p| !p.link_rects.is_empty()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.frame();
    }
    h.frames(2);
    let p = popover(&h).expect("pinned");
    let area = p.rect.unwrap();
    assert!(
        area.top() >= 0.0 && area.bottom() <= h.screen.y,
        "the popover stays in the window: {area:?}"
    );
    assert_eq!(p.link_rects.len(), 1);
    assert!(
        !area.contains(p.link_rects[0].center()),
        "the reference is below what shows at first"
    );
    h.events.push(Event::PointerMoved(area.center()));
    // To the end of the note: the reference is its last line.
    for _ in 0..80 {
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -60.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frames(10);
    let p = popover(&h).expect("still pinned");
    assert_eq!(p.rect, Some(area), "the popover itself did not move");
    let link = p.link_rects[0];
    assert!(
        area.contains(link.center()),
        "scrolled to the reference: {link:?} in {area:?}"
    );
    h.events.push(Event::PointerMoved(link.center()));
    h.frames(8);
    assert!(h.find("Ps 3:3").is_some(), "{:?}", h.texts());
    h.click(link.center());
    h.frames(6);
    assert!(popover(&h).is_none(), "following the link closes it");
    let ps = Verse::new(Book::Psalms, 3, 3);
    let went = (h.shell.st.session.workspace.tabs()).any(|t| t.history.current() == Some(ps))
        || (h.shell.st.references.pending_navigation.values()).any(|v| *v == ps);
    assert!(went, "a Tab goes to Ps 3:3");
}

/// Enter on a marker whose popover has not been laid out yet (pressed before the dwell, the
/// note's text still with the layout worker) still moves focus to its first link once the
/// text is drawn.
#[test]
fn enter_before_the_dwell_moves_focus_to_the_first_link_once_it_is_drawn() {
    let (mut h, _files, tab) = bsb("keys-cold", "Jn 1:19");
    let (m, at) = marker(&h, tab, "Isaiah 40:3");
    h.click(pos2(at.x - 40.0, at.y + 4.0));
    for _ in 0..20 {
        if pending(&h).is_some_and(|p| p.note == m.note) {
            break;
        }
        press(&mut h, Key::Tab);
    }
    let marker_focus = h
        .ctx
        .memory(|mem| mem.focused())
        .expect("a marker has focus");
    assert!(
        popover(&h).is_none(),
        "not shown yet: Enter comes before the dwell"
    );
    press(&mut h, Key::Enter);
    assert!(popover(&h).is_some_and(|p| p.pinned));
    h.frames(8);
    let link = popover(&h).unwrap().link_rects[0];
    assert!(link.is_positive(), "the link is drawn");
    let focused = h.ctx.memory(|mem| mem.focused());
    assert!(
        focused.is_some() && focused != Some(marker_focus),
        "focus moved into the popover"
    );
    assert!(h.find("Isa 40:3").is_some(), "the focused link previews");
}

/// A pinned popover stays when the pointer rests on a marker in another pane; clicking that
/// marker replaces it.
#[test]
fn a_pinned_popover_stays_when_hovering_a_marker_in_another_pane() {
    let (mut h, _files, tab) = bsb("split-pin", "Jn 1:1");
    let other = split(&mut h, tab, "bsb", "Jn 1:1");
    let (_, at) = marker(&h, tab, "Or comprehended");
    h.click(at);
    assert!(popover(&h).is_some_and(|p| p.pinned && p.tab == tab));
    // The same Footnote's marker in the other pane: only its Tab tells them apart.
    let (_, there) = marker(&h, other, "Or comprehended");
    h.events.push(Event::PointerMoved(there));
    h.frames(14);
    let p = popover(&h).expect("still shown");
    assert!(
        p.pinned && p.tab == tab && p.hit.rect.contains(at),
        "hovering another pane's marker leaves the pinned popover"
    );
    h.click(there);
    let p = popover(&h).expect("replaced");
    assert!(p.pinned && p.tab == other && p.hit.rect.contains(there));
}

/// Closing the Tab of a pinned popover takes the popover with it: hovering a marker in the
/// pane left shows that marker's popover. Rebuilding the Workspace (a Layout loaded) does too.
#[test]
fn a_pinned_popover_goes_with_its_closed_tab() {
    let (mut h, _files, tab) = bsb("close-pin", "Jn 1:1");
    let other = split(&mut h, tab, "bsb", "Jn 1:1");
    let (_, at) = marker(&h, tab, "Or comprehended");
    h.click(at);
    assert!(popover(&h).is_some_and(|p| p.pinned && p.tab == tab));
    h.events
        .push(Event::PointerMoved(pos2(at.x + 200.0, at.y + 300.0)));
    h.frames(2);
    assert!(popover(&h).is_some_and(|p| p.pinned && p.tab == tab));
    h.shell.run("tab.close", &json!({ "tab": tab.0 }));
    h.frames(3);
    assert!(pending(&h).is_none(), "gone with its Tab");
    let (_, there) = marker(&h, other, "Or comprehended");
    h.events.push(Event::PointerMoved(there));
    h.frames(14);
    let p = popover(&h).expect("the hovered marker's popover");
    assert!(!p.pinned && p.tab == other);
    h.click(there);
    assert!(popover(&h).is_some_and(|p| p.pinned));
    h.shell.rebuild();
    assert!(pending(&h).is_none(), "gone when the Tabs are rebuilt");
}

/// Two Resources whose text is the same can have different Footnotes under the same marker:
/// pinning one, then the other's, shows the other's note, not the first one's again.
#[test]
fn a_marker_in_another_resource_shows_that_resources_note() {
    let first = obs_engine::testing::bibles::bsb_john_1();
    let mut second = first.clone();
    second.manifest.resource_id = "bsb-copy".into();
    second.manifest.title = "BSB copy (test)".into();
    second.manifest.abbreviation = "BSBC".into();
    let note = second
        .units
        .iter_mut()
        .find(|u| u.kind == obs_model::UnitKind::Note && u.text == "Or comprehended")
        .unwrap();
    note.text = "Or grasped".into();
    note.spans.clear();
    let (mut h, _files, tab) = library("two-resources", &[first, second], "Jn 1:1");
    let other = split(&mut h, tab, "bsb-copy", "Jn 1:1");
    let (m, at) = marker(&h, tab, "Or comprehended");
    let (n, there) = marker(&h, other, "Or grasped");
    assert_eq!((m.unit, m.anchor, &m.note), (n.unit, n.anchor, &n.note));
    h.click(at);
    assert_eq!(shown_text(&h).as_deref(), Some("Or comprehended"));
    h.click(there);
    let p = popover(&h).expect("replaced");
    assert!(p.pinned && p.tab == other);
    let second_marker = p.hit.widget;
    assert_eq!(shown_text(&h).as_deref(), Some("Or grasped"));
    // The first pinned again, then the second's marker from the keyboard.
    h.click(at);
    assert_eq!(shown_text(&h).as_deref(), Some("Or comprehended"));
    tab_to(&mut h, second_marker);
    press(&mut h, Key::Enter);
    h.frames(4);
    let p = popover(&h).expect("replaced from the keyboard");
    assert!(p.pinned && p.tab == other);
    assert_eq!(shown_text(&h).as_deref(), Some("Or grasped"));
}

/// Enter on a focused marker moves focus into its popover even with the pointer resting on
/// that marker: where the activation came from decides, not where the pointer is.
#[test]
fn enter_on_a_marker_under_the_pointer_moves_focus_into_its_popover() {
    let (mut h, _files, tab) = bsb("keys-hovered", "Jn 1:19");
    let (_, at) = marker(&h, tab, "Isaiah 40:3");
    h.click(pos2(at.x - 40.0, at.y + 4.0));
    h.events.push(Event::PointerMoved(at));
    h.frames(14);
    let widget = popover(&h).expect("shown on hover").hit.widget;
    tab_to(&mut h, widget);
    press(&mut h, Key::Enter);
    h.frames(6);
    assert!(popover(&h).is_some_and(|p| p.pinned));
    let focused = h.ctx.memory(|mem| mem.focused());
    assert!(
        focused.is_some() && focused != Some(widget),
        "focus moved into the popover"
    );
    assert!(h.find("Isa 40:3").is_some(), "the focused link previews");
}

/// A Bible numbered its own way (Hebrew Psalms: the superscription is verse 1) reads the
/// references written in its Footnotes in that numbering: "Ps 3:9", a verse the Standard
/// lacks, is a link to the Standard's Ps 3:8, previews it and goes there. A book the Bible
/// does not have is read in the Standard, item by item: a list naming both ("Ps 3:8; Rom
/// 3:4; Ps 3:9") keeps every item, each in its own numbering.
#[test]
fn a_footnote_reference_is_read_in_its_bibles_own_numbering() {
    let texts = [
        "A Psalm of David, when he fled from Absalom his son.",
        "LORD, how are they increased that trouble me!",
        "Many there be which say of my soul,",
        "But thou, O LORD, art a shield for me;",
        "I cried unto the LORD with my voice,",
        "I laid me down and slept;",
        "I will not be afraid of ten thousands of people,",
        "Arise, O LORD; save me, O my God:",
        "Salvation belongeth unto the LORD: thy blessing is upon thy people. Selah.",
    ];
    let verses: Vec<(Verse, &str)> = (1..)
        .zip(texts)
        .map(|(n, t)| (Verse::new(Book::Psalms, 3, n), t))
        .collect();
    let standard: Vec<Option<Verse>> = (0..)
        .take(texts.len())
        .map(|n| Some(Verse::new(Book::Psalms, 3, n)))
        .collect();
    let mut bible =
        obs_engine::testing::bibles::bible("heb-ps", "HPS", &verses, Some(&standard), &[]);
    let note = obs_model::UnitId::note(1);
    let text = "Cited in Ps 3:9; see Rom 3:4. Also Ps 3:8; Rom 3:4; Ps 3:9.";
    bible.units.push(obs_model::TextUnit {
        id: note.clone(),
        kind: obs_model::UnitKind::Note,
        title: None,
        text: text.into(),
        spans: Vec::new(),
    });
    let psalms = bible
        .units
        .iter_mut()
        .find(|u| u.kind == obs_model::UnitKind::BibleBook)
        .unwrap();
    let end = u32::try_from(psalms.text.len()).unwrap();
    psalms.spans.push(obs_model::Span {
        start: end,
        end,
        kind: obs_model::SpanKind::NoteRef {
            note,
            note_kind: obs_model::NoteKind::Footnote,
        },
    });
    let (mut h, _files, tab) = library("native", &[bible], "Ps 3:1");
    let (_, at) = marker(&h, tab, "Cited in");
    h.click(at);
    h.frames(4);
    let body = popover(&h).unwrap().body.clone().unwrap();
    let shown: Vec<(&str, Vec<String>)> = body
        .links
        .iter()
        .map(|(r, targets)| {
            (
                &text[r.clone()],
                targets
                    .iter()
                    .map(|l| {
                        obs_scripture::format_passage_list(
                            l,
                            &obs_scripture::Standard,
                            obs_scripture::RefStyle::Short,
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    assert_eq!(
        shown,
        [
            ("Ps 3:9", vec!["Ps 3:8".to_string()]),
            ("Rom 3:4", vec!["Rom 3:4".to_string()]),
            (
                "Ps 3:8; Rom 3:4; Ps 3:9",
                vec![
                    "Ps 3:7".to_string(),
                    "Rom 3:4".to_string(),
                    "Ps 3:8".to_string()
                ]
            ),
        ]
    );
    let native = Verse::new(Book::Psalms, 3, 9);
    assert_ne!(
        h.shell
            .st
            .session
            .workspace
            .tab(tab)
            .unwrap()
            .history
            .current(),
        Some(native)
    );
    let link = popover(&h).unwrap().link_rects[0];
    h.events.push(Event::PointerMoved(link.center()));
    h.frames(8);
    assert!(h.find("Ps 3:8").is_some(), "{:?}", h.texts());
    h.click(link.center());
    h.frames(6);
    assert!(popover(&h).is_none(), "following the link closes it");
    let standard = Verse::new(Book::Psalms, 3, 8);
    let went = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .any(|t| t.history.current() == Some(native))
        || h.shell
            .st
            .references
            .pending_navigation
            .values()
            .any(|v| *v == standard);
    let histories: Vec<_> = (h.shell.st.session.workspace.tabs())
        .map(|t| t.history.current())
        .collect();
    assert!(
        went,
        "a Tab goes to Standard Ps 3:8, the Bible's Ps 3:9: {histories:?} {:?}",
        h.shell.st.references.pending_navigation
    );
}

/// Acceptance: View → Footnote markers hides and shows them in the focused Tab only, and a
/// Layout keeps the choice.
#[test]
fn view_footnote_markers_toggles_them_per_tab() {
    let (mut h, _files, tab) = bsb("toggle", "Jn 1:1");
    assert!(!markers(&h, tab).is_empty());
    h.shell.st.session.workspace.focus(tab);
    h.open_menu("View");
    let item = h.find("Footnote markers").expect("in the View menu");
    let check = crate::icons::Icon::Check.marker();
    let checked = |h: &Harness| {
        (h.shapes.iter()).any(|(r, t)| *t == check && (r.center().y - item.center().y).abs() < 6.0)
    };
    assert!(checked(&h), "checked: {:?}", h.texts());
    h.click_text("Footnote markers");
    h.frames(4);
    assert!(markers(&h, tab).is_empty(), "hidden in this Tab");
    h.open_menu("View");
    assert!(!checked(&h), "unchecked once hidden");
    press(&mut h, Key::Escape);
    assert!(
        !h.shell
            .st
            .session
            .workspace
            .tab(tab)
            .unwrap()
            .footnote_markers
    );
    // Another Tab of the same Bible still shows them.
    h.shell
        .run("resource.open", &json!({ "id": "bsb", "split": true }));
    h.frames(6);
    let other = h.shell.st.session.workspace.focused().unwrap();
    assert_ne!(other, tab);
    h.shell
        .run("tab.go", &json!({ "tab": other.0, "text": "Jn 1:1" }));
    h.frames(6);
    assert!(!markers(&h, other).is_empty());
    h.shell.run(
        "view.footnote_markers",
        &json!({ "tab": tab.0, "on": true }),
    );
    h.frames(4);
    assert!(!markers(&h, tab).is_empty(), "shown again");
}

/// Acceptance: selecting across a marker and copying it (Copy with reference, #34) copies the
/// clean text; Copy verses (#154) leaves the notes out too.
#[test]
fn copying_across_a_marker_copies_clean_text() {
    let (mut h, _files, tab) = bsb("copy", "Jn 1:1");
    let from = word_info::word_spot(&h, tab, "overcome");
    let to = word_info::word_spot(&h, tab, "it.\n");
    h.drag(pos2(from.x - 30.0, from.y), pos2(to.x + 30.0, to.y));
    let selected = {
        let view = h.shell.st.tabs[&tab].view.as_ref().unwrap();
        crate::selection::selected_text(&view.doc, &view.selection.unwrap())
    };
    assert!(selected.ends_with("overcome it."), "{selected:?}");
    h.click_text("Copy");
    let copied = h.copied.last().cloned().unwrap_or_default();
    assert!(copied.starts_with(&selected), "{copied:?}");
    assert!(copied.ends_with("John 1:5 (BSB)"), "{copied:?}");
    assert!(!copied.contains("comprehended") && !copied.contains("overcomea"));

    h.shell.run(
        "tool.copy_verses",
        &json!({ "passages": "Jn 1:14", "bibles": "bsb" }),
    );
    h.frames(3);
    h.click_text("Copy plain text");
    let copied = h.copied.last().cloned().unwrap_or_default();
    assert!(copied.contains("full of grace and truth."), "{copied:?}");
    for note in ["tabernacled", "Only Begotten", "Unique One"] {
        assert!(!copied.contains(note), "{copied:?}");
    }
}

/// Visual evidence for `docs/previews/footnotes-*.png` when `UPDATE_PREVIEWS` is set.
#[test]
fn footnote_previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
    std::fs::create_dir_all(&dir).unwrap();
    let (mut h, _files, tab) = bsb("previews", "Jn 1:1");
    h.screenshot(&dir.join("footnotes-markers.ppm"));
    let (_, at) = marker(&h, tab, "Or the Only Begotten");
    h.events.push(Event::PointerMoved(at));
    h.frames(14);
    h.screenshot(&dir.join("footnotes-hover.ppm"));
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": "Jn 1:19" }));
    h.frames(6);
    let (_, at) = marker(&h, tab, "Isaiah 40:3");
    h.click(at);
    let link = popover(&h).unwrap().link_rects[0];
    h.events.push(Event::PointerMoved(link.center()));
    // The preview's verse is laid out off-thread: give it time to arrive.
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(10));
        h.frame();
    }
    h.screenshot(&dir.join("footnotes-pinned-reference.ppm"));
    h.events.push(Event::PointerMoved(pos2(10.0, 10.0)));
    press(&mut h, Key::Escape);
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.frames(4);
    let (_, at) = marker(&h, tab, "Isaiah 40:3");
    h.events.push(Event::PointerMoved(at));
    h.frames(14);
    h.screenshot(&dir.join("footnotes-dark.ppm"));
    h.shell.run(
        "view.footnote_markers",
        &json!({ "tab": tab.0, "on": false }),
    );
    h.frames(4);
    h.screenshot(&dir.join("footnotes-hidden.ppm"));
}

/// Review repro: a floating window's native viewport draws within the main window's frame,
/// its own pass count far behind the main window's. The Footnote widgets it draws leave the
/// main window's alone, so Space on a marker still focused there is still the marker's.
#[test]
fn a_native_floating_window_leaves_the_main_windows_footnote_widgets_alone() {
    use crate::pane::{footnote_widget_focused, register_footnote_widget};
    let ctx = egui::Context::default();
    let floating = egui::ViewportId::from_hash_of("floating-window");
    let pass_in = |viewport: egui::ViewportId, f: &mut dyn FnMut(&egui::Context)| {
        let input = egui::RawInput {
            viewport_id: viewport,
            ..Default::default()
        };
        let _ = ctx.run_ui(input, |ui| f(ui.ctx()));
    };
    let main_marker = egui::Id::new("main-window-marker");
    let floating_marker = egui::Id::new("floating-window-marker");
    for _ in 0..100 {
        pass_in(egui::ViewportId::ROOT, &mut |ctx| {
            register_footnote_widget(ctx, main_marker);
            ctx.memory_mut(|m| m.request_focus(main_marker));
        });
    }
    for _ in 0..3 {
        pass_in(floating, &mut |ctx| register_footnote_widget(ctx, floating_marker));
    }
    assert_ne!(
        ctx.cumulative_pass_nr_for(egui::ViewportId::ROOT),
        ctx.cumulative_pass_nr_for(floating),
        "the two viewports count their passes apart"
    );
    let mut focused = false;
    pass_in(egui::ViewportId::ROOT, &mut |ctx| {
        assert_eq!(ctx.memory(|m| m.focused()), Some(main_marker));
        focused = footnote_widget_focused(ctx);
    });
    assert!(focused, "the main window's marker still holds Space");
}
