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
    let mut h = Harness::new();
    let dir = std::env::temp_dir().join(format!("obs-footnotes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    let path = files.0.join("bsb.obr");
    obs_store::write_obr(&obs_engine::testing::bibles::bsb_john_1(), &path).unwrap();
    h.shell.st.session.library = obs_engine::Library::new();
    h.shell.st.session.library.add_file(&path).unwrap();
    h.shell.reset_layout();
    let tab = h.tab_id("BSB");
    h.shell.run("tab.go", &json!({ "tab": tab.0, "text": at }));
    h.frames(6);
    (h, files, tab)
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

fn popover(h: &Harness) -> Option<&crate::shell::footnotes::FootnotePopover> {
    h.shell.st.footnotes.popover.as_ref().filter(|p| p.shown)
}

fn shown_text(h: &Harness) -> Option<String> {
    popover(h)
        .and_then(|p| p.body.as_ref())
        .map(|b| b.unit.text.clone())
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
    h.frames(2);
    assert!(popover(&h).is_none(), "Esc closes it");
    h.click(at);
    assert!(popover(&h).is_some());
    h.click(at);
    assert!(
        popover(&h).is_none(),
        "a second click on the marker closes it"
    );
    h.click(at);
    let area = popover(&h).unwrap().rect.unwrap();
    let close = (h.shapes.iter())
        .find(|(r, t)| t == "×" && area.contains(r.center()))
        .map(|(r, _)| r.center())
        .expect("a × in the pinned popover");
    h.click(close);
    assert!(popover(&h).is_none(), "× closes it");
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
        if h.shell
            .st
            .footnotes
            .popover
            .as_ref()
            .is_some_and(|p| p.hit.note == m.note)
        {
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
        if h.shell
            .st
            .footnotes
            .popover
            .as_ref()
            .is_some_and(|p| p.hit.note == m.note)
        {
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
    h.shell.st.session.workspace.focus(tab);
    h.shell
        .run("resource.open", &json!({ "id": "bsb", "split": true }));
    h.frames(6);
    let other = h.shell.st.session.workspace.focused().unwrap();
    assert_ne!(other, tab);
    h.shell
        .run("tab.go", &json!({ "tab": other.0, "text": "Jn 1:1" }));
    h.frames(6);
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

/// Acceptance: View → Footnote markers hides and shows them in the focused Tab only, and a
/// Layout keeps the choice.
#[test]
fn view_footnote_markers_toggles_them_per_tab() {
    let (mut h, _files, tab) = bsb("toggle", "Jn 1:1");
    assert!(!markers(&h, tab).is_empty());
    h.shell.st.session.workspace.focus(tab);
    h.open_menu("View");
    assert!(h.find("✓ Footnote markers").is_some(), "{:?}", h.texts());
    h.click_text("✓ Footnote markers");
    h.frames(4);
    assert!(markers(&h, tab).is_empty(), "hidden in this Tab");
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
    h.frames(8);
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
