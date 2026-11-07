//! The toolbar Launchers (#140) through the headless harness: each pane from the keyboard and
//! the mouse, opening a Resource in two interactions, docking one by dragging it, Layout
//! cards with thumbnails, the launchers' accessible names, and the light and dark previews.

use egui::Key;
use obs_engine::{Guide, Launcher, TabId};
use obs_model::Key as ResourceKey;
use obs_scripture::{Book, Verse};

use super::*;

/// Ctrl as Linux and Windows report it.
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};

fn key(h: &mut Harness, key: Key, modifiers: Modifiers) {
    h.modifiers = modifiers;
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frame();
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers,
    });
    h.modifiers = Modifiers::NONE;
    h.frames(2);
}

fn type_text(h: &mut Harness, text: &str) {
    h.events.push(Event::Text(text.into()));
    h.frames(2);
}

/// `text`, then Enter, in one frame: as a fast typist, or a paste then Enter, delivers them.
fn type_and_enter(h: &mut Harness, text: &str) {
    h.events.push(Event::Text(text.into()));
    let enter = |pressed| Event::Key {
        key: Key::Enter,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    h.events.push(enter(true));
    h.frame();
    h.events.push(enter(false));
    h.frames(2);
}

fn open(h: &Harness) -> Option<Launcher> {
    h.shell.st.toolbar.open
}

fn tabs_of(h: &Harness, id: &str) -> Vec<TabId> {
    h.shell
        .st
        .session
        .workspace
        .tabs()
        .filter(|t| t.kind == TabKind::Resource(id.into()))
        .map(|t| t.id)
        .collect()
}

fn focused_kind(h: &Harness) -> Option<TabKind> {
    let ws = &h.shell.st.session.workspace;
    ws.focused().and_then(|t| ws.tab(t)).map(|t| t.kind.clone())
}

/// Texts painted inside the open pane.
fn pane_texts(h: &Harness) -> Vec<&str> {
    let pane = h.shell.st.toolbar.pane.expect("a pane is open");
    h.shapes
        .iter()
        .filter(|(r, _)| pane.contains(r.center()))
        .map(|(_, t)| t.as_str())
        .collect()
}

/// Where `text` is painted inside the open pane.
fn in_pane(h: &Harness, text: &str) -> Pos2 {
    let pane = h.shell.st.toolbar.pane.expect("a pane is open");
    h.shapes
        .iter()
        .rev()
        .find(|(r, t)| t == text && pane.contains(r.center()))
        .map(|(r, _)| r.center())
        .unwrap_or_else(|| panic!("{text:?} not in the pane: {:?}", pane_texts(h)))
}

fn ge(c: u16, v: u16) -> Verse {
    Verse::new(Book::Genesis, c, v)
}

fn button(h: &Harness, which: Launcher) -> Pos2 {
    h.shell.st.toolbar.buttons[&which].center()
}

#[test]
fn opening_any_resource_takes_two_interactions_library_then_type_and_enter() {
    let mut h = Harness::new();
    let tb = h.tab_id("TB");
    let tb_panel = h.shell.st.session.workspace.panel_of(tb);
    assert!(tabs_of(&h, "heb-num").is_empty());
    // 1: the Library launcher.
    h.click(button(&h, Launcher::Library));
    assert_eq!(open(&h), Some(Launcher::Library));
    // 2: type, then Enter. The keyboard is already in the search field.
    type_text(&mut h, "hnum");
    key(&mut h, Key::Enter, Modifiers::NONE);
    let hnum = tabs_of(&h, "heb-num");
    assert_eq!(hnum.len(), 1);
    let ws = &h.shell.st.session.workspace;
    assert_eq!(ws.focused(), Some(hnum[0]));
    assert_eq!(ws.panel_of(hnum[0]), tb_panel, "in the focused Panel");
    assert_eq!(open(&h), None, "the pane closed");

    // The same from the keyboard alone: Alt+L, then type and Enter. A title, a type or a
    // language finds it too.
    key(&mut h, Key::L, Modifiers::ALT);
    assert_eq!(open(&h), Some(Launcher::Library));
    type_text(&mut h, "hebrew test");
    let texts = pane_texts(&h);
    assert!(
        texts.contains(&"BEST MATCHES") && texts.contains(&"Bible · Hebrew"),
        "{texts:?}"
    );
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(focused_kind(&h), Some(TabKind::Resource("heb-test".into())));
}

#[test]
fn the_library_pane_lists_recent_and_preferred_rows_and_its_links_work_from_the_keyboard() {
    let mut h = Harness::new();
    key(&mut h, Key::L, Modifiers::ALT);
    let texts = pane_texts(&h);
    for t in [
        "RECENT",
        "BIBLES",
        "Test Bible",
        "Hebrew test",
        "Hebrew-numbered Bible",
        "Open full Library",
        "Import…",
        "Get Resources…",
        "Get Bibles…",
        "Starter library",
    ] {
        assert!(texts.contains(&t), "{t} missing: {texts:?}");
    }
    // Recent comes first.
    assert!(h.at("RECENT").y < h.at("BIBLES").y);
    // Esc closes it.
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(open(&h), None);

    // ↓ walks the rows (Recent TB, then the three Bibles) into the footer links; Enter on
    // Open full Library opens the Library Tab.
    let library = h.tab_id("Library");
    h.shell.run("tab.close", &json!({ "tab": library.0 }));
    h.frames(2);
    key(&mut h, Key::L, Modifiers::ALT);
    for _ in 0..4 {
        key(&mut h, Key::ArrowDown, Modifiers::NONE);
    }
    assert_eq!(h.shell.st.toolbar.selected, 4);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(focused_kind(&h), Some(TabKind::Library));

    // Ctrl/Cmd-click: always a new Tab, split off as a new Panel.
    let panels = h.panels().len();
    h.click(button(&h, Launcher::Library));
    let row = in_pane(&h, "Test Bible");
    h.modifiers = CTRL;
    h.click(row);
    h.modifiers = Modifiers::NONE;
    h.frames(2);
    assert_eq!(tabs_of(&h, "test-bible").len(), 2);
    assert_eq!(h.panels().len(), panels + 1);

    // A click outside closes the pane without running anything.
    let before = h.shell.st.session.command_log().len();
    h.click(button(&h, Launcher::Library));
    assert_eq!(open(&h), Some(Launcher::Library));
    h.click(pos2(700.0, 800.0));
    assert_eq!(open(&h), None);
    assert!(!h.shell.st.session.command_log()[before..].contains(&"toolbar.open_resource"));
}

#[test]
fn a_resource_dragged_out_of_the_library_pane_docks_where_it_is_dropped() {
    let mut h = Harness::new();
    let panels = h.panels();
    assert_eq!(panels.len(), 2, "{panels:?}");
    let right = panels[1].0;
    // Onto the right-hand Panel: a Tab there.
    key(&mut h, Key::L, Modifiers::ALT);
    let from = in_pane(&h, "Hebrew-numbered Bible");
    h.drag(from, pos2(right.center().x - 40.0, right.bottom() - 60.0));
    let hnum = tabs_of(&h, "heb-num");
    assert_eq!(hnum.len(), 1);
    assert!(
        h.panels()[1].1.contains(&"HNUM".to_string()),
        "{:?}",
        h.panels()
    );
    assert_eq!(open(&h), None);
    assert!(h.shell.st.toolbar.dragging.is_none());

    // Onto the left Panel's right edge: a new Panel beside it.
    let left = h.panels()[0].0;
    key(&mut h, Key::L, Modifiers::ALT);
    let from = in_pane(&h, "Hebrew test");
    h.drag(from, pos2(left.right() - 20.0, left.bottom() - 60.0));
    let panels = h.panels();
    assert_eq!(panels.len(), 3, "{panels:?}");
    assert_eq!(panels[1].1, ["HEB"], "between the two");
}

#[test]
fn the_guides_pane_opens_each_guide_at_what_is_typed() {
    let mut h = Harness::new();
    key(&mut h, Key::G, Modifiers::ALT);
    assert_eq!(open(&h), Some(Launcher::Guides));
    for t in [
        "Passage Guide",
        "Exegetical Guide",
        "Word Study",
        "Factbook",
    ] {
        assert!(pane_texts(&h).contains(&t), "{t}: {:?}", pane_texts(&h));
    }
    // The keyboard starts in the Passage Guide's box.
    type_text(&mut h, "Gen 31:3");
    key(&mut h, Key::Enter, Modifiers::NONE);
    let ws = &h.shell.st.session.workspace;
    let guide = ws
        .tabs()
        .find(|t| t.kind == TabKind::Guide(Guide::PassageGuide))
        .expect("a Passage Guide");
    assert_eq!(guide.history.current(), Some(ge(31, 3)));
    assert_eq!(open(&h), None);

    // ↓ to the Exegetical Guide's box (#144): a chapter stays whole.
    key(&mut h, Key::G, Modifiers::ALT);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    type_text(&mut h, "Gen 31");
    key(&mut h, Key::Enter, Modifiers::NONE);
    let ws = &h.shell.st.session.workspace;
    let exegetical = ws
        .tabs()
        .find(|t| t.kind == TabKind::Guide(Guide::ExegeticalGuide))
        .expect("an Exegetical Guide");
    assert_eq!(exegetical.history.current(), Some(ge(31, 1)));
    assert_eq!(
        exegetical.shown_passage().map(|p| p.end()),
        Some(ge(31, 55))
    );

    // ↓↓ to Word Study's box: a Strong's number.
    key(&mut h, Key::G, Modifiers::ALT);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    type_text(&mut h, "G25");
    key(&mut h, Key::Enter, Modifiers::NONE);
    let ws = &h.shell.st.session.workspace;
    let study = ws
        .tabs()
        .find(|t| t.kind == TabKind::Guide(Guide::WordStudy))
        .expect("a Word Study");
    assert_eq!(
        study.entry_history.current(),
        Some(ResourceKey::StrongsNumber("G25".parse().unwrap()))
    );

    // A click on a Guide's name opens it as its menu item did.
    h.click(button(&h, Launcher::Guides));
    h.click(in_pane(&h, "Factbook"));
    assert_eq!(focused_kind(&h), Some(TabKind::Guide(Guide::Factbook)));
}

#[test]
fn the_tools_pane_filters_as_you_type_and_runs_from_the_keyboard() {
    let mut h = Harness::new();
    key(&mut h, Key::T, Modifiers::ALT);
    assert_eq!(open(&h), Some(Launcher::Tools));
    for t in [
        "Search",
        "Text Comparison",
        "Notes",
        "Word Info",
        "Cross-references",
        "Ctrl+Shift+F",
    ] {
        assert!(pane_texts(&h).contains(&t), "{t}: {:?}", pane_texts(&h));
    }
    type_text(&mut h, "no");
    let texts = pane_texts(&h);
    assert!(
        texts.contains(&"Notes") && !texts.contains(&"Search"),
        "{texts:?}"
    );
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(focused_kind(&h), Some(TabKind::Tool(Tool::Notes)));

    // ↓ Enter: the second tool in menu order.
    key(&mut h, Key::T, Modifiers::ALT);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(focused_kind(&h), Some(TabKind::Tool(Tool::TextComparison)));
    // Alt+T twice: open, then closed.
    key(&mut h, Key::T, Modifiers::ALT);
    key(&mut h, Key::T, Modifiers::ALT);
    assert_eq!(open(&h), None);
}

/// The Tools pane shows the key the user has for each tool (#177), not its default.
#[test]
fn the_tools_pane_shows_a_rebound_or_cleared_key() {
    let mut h = Harness::new();
    let search = |h: &Harness| {
        let pane = h.shell.st.toolbar.pane.expect("a pane is open");
        let row = in_pane(h, "Search");
        (h.shapes.iter())
            .filter(|(r, t)| {
                pane.contains(r.center())
                    && (r.center().y - row.y).abs() < 4.0
                    && t != "Search"
                    && t != "[search]"
            })
            .map(|(_, t)| t.clone())
            .collect::<Vec<_>>()
    };
    h.shell.run(
        "keys.set",
        &json!({ "command": "tool.search", "key": "Ctrl+Up" }),
    );
    key(&mut h, Key::T, Modifiers::ALT);
    assert_eq!(search(&h), ["Ctrl+↑"], "{:?}", pane_texts(&h));
    assert!(!pane_texts(&h).contains(&"Ctrl+Shift+F"));
    key(&mut h, Key::T, Modifiers::ALT);
    h.shell
        .run("keys.clear", &json!({ "command": "tool.search" }));
    key(&mut h, Key::T, Modifiers::ALT);
    assert!(search(&h).is_empty(), "{:?}", pane_texts(&h));
    assert!(!pane_texts(&h).contains(&"Ctrl+Shift+F"));
}

fn layouts_harness(name: &str) -> Harness {
    let db = temp_db(name);
    let mut h = Harness::launch(Some(&db));
    h.shell
        .run("resource.open", &json!({ "id": "heb-num", "split": true }));
    h.frames(3);
    h.shell.run("layout.save", &json!({ "name": "Two Bibles" }));
    h.frames(2);
    h
}

#[test]
fn the_layouts_pane_shows_thumbnails_and_loads_a_layout_in_one_click() {
    let mut h = layouts_harness("toolbar-layouts");
    let tb = h.tab_id("TB");
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:20" }));
    settle_all(&mut h);
    key(&mut h, Key::Y, Modifiers::ALT);
    assert_eq!(open(&h), Some(Launcher::Layouts));
    let texts = pane_texts(&h);
    for t in [
        "SAVED LAYOUTS",
        "BUILT-IN",
        "Two Bibles",
        "Bible + Commentary",
        "Word study desk",
        "Parallel Bibles",
        "Reading only",
        "Needs a Commentary in the library",
        "Save Layout as…",
        "Reset Layout",
    ] {
        assert!(texts.contains(&t), "{t}: {texts:?}");
    }
    // Thumbnails: each Panel of the tiling, titled with its Tab, drawn above the card name.
    let name = in_pane(&h, "Two Bibles");
    let titled = |t: &str| {
        h.shapes
            .iter()
            .any(|(r, s)| s == t && (r.center().x - name.x).abs() < 80.0 && r.center().y < name.y)
    };
    assert!(titled("TB") && titled("HNUM"), "{texts:?}");
    assert!(texts.contains(&"Word Study") && texts.contains(&"Word Info"));

    // One click on a Built-in Layout loads it, at the Passage shown now.
    h.click(in_pane(&h, "Parallel Bibles"));
    settle_all(&mut h);
    let titles: Vec<Vec<String>> = h.panels().into_iter().map(|(_, t)| t).collect();
    assert_eq!(titles, [vec!["TB".to_string()], vec!["HEB".to_string()]]);
    assert_eq!(loc_of(&mut h, "TB"), Some(ge(31, 20)));
    assert_eq!(open(&h), None);

    // A Built-in Layout this library cannot make says why and changes nothing.
    h.click(button(&h, Launcher::Layouts));
    h.click(in_pane(&h, "Bible + Commentary"));
    assert!(
        h.find("Bible + Commentary: Needs a Commentary in the library")
            .is_some(),
        "{:?}",
        h.texts()
    );
    assert_eq!(h.panels().len(), 2);
}

#[test]
fn layout_cards_load_rename_and_delete_from_the_keyboard() {
    let mut h = layouts_harness("toolbar-layout-keys");
    // → to Reading only (the saved one, then four Built-ins), Enter loads it.
    key(&mut h, Key::Y, Modifiers::ALT);
    for _ in 0..4 {
        key(&mut h, Key::ArrowRight, Modifiers::NONE);
    }
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle_all(&mut h);
    let titles: Vec<Vec<String>> = h.panels().into_iter().map(|(_, t)| t).collect();
    assert_eq!(titles, [vec!["TB".to_string()]]);

    // F2 renames the picked saved Layout in place; Esc stops renaming, Enter keeps it.
    key(&mut h, Key::Y, Modifiers::ALT);
    key(&mut h, Key::F2, Modifiers::NONE);
    assert!(h.shell.st.toolbar.renaming.is_some());
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.shell.st.toolbar.renaming.is_none());
    assert_eq!(
        open(&h),
        Some(Launcher::Layouts),
        "Esc only stopped renaming"
    );
    key(&mut h, Key::F2, Modifiers::NONE);
    for _ in 0.."Two Bibles".len() {
        key(&mut h, Key::Backspace, Modifiers::NONE);
    }
    // The new name and Enter in one frame: the name as typed.
    type_and_enter(&mut h, "Desk");
    let user = h.shell.st.session.user.as_ref().unwrap();
    assert_eq!(user.layouts().unwrap(), ["Desk"]);
    assert!(pane_texts(&h).contains(&"Desk"));
    // Delete deletes it; the card is gone, the pane stays.
    key(&mut h, Key::Delete, Modifiers::NONE);
    let user = h.shell.st.session.user.as_ref().unwrap();
    assert!(user.layouts().unwrap().is_empty());
    assert!(!pane_texts(&h).contains(&"Desk"));
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(open(&h), None);
}

/// `event`, then Delete, in one frame: the filter takes the text before Delete can mean
/// "delete the picked Layout".
fn event_then_delete(h: &mut Harness, event: Event) {
    h.events.push(event);
    let delete = |pressed| Event::Key {
        key: Key::Delete,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    h.events.push(delete(true));
    h.frame();
    h.events.push(delete(false));
    h.frames(2);
}

#[test]
fn typing_or_pasting_then_delete_in_one_frame_keeps_the_picked_saved_layout() {
    let mut h = layouts_harness("toolbar-paste-delete");
    let saved = |h: &Harness| {
        let user = h.shell.st.session.user.as_ref().unwrap();
        user.layouts().unwrap()
    };
    for event in [Event::Paste("Two".into()), Event::Text("Two".into())] {
        // The saved Layout is picked and the filter is empty.
        key(&mut h, Key::Y, Modifiers::ALT);
        assert_eq!(open(&h), Some(Launcher::Layouts));
        assert_eq!(h.shell.st.toolbar.selected, 0);
        assert!(h.shell.st.toolbar.query.is_empty());
        event_then_delete(&mut h, event.clone());
        assert_eq!(saved(&h), ["Two Bibles"], "{event:?} then Delete");
        assert_eq!(
            h.shell.st.toolbar.query, "Two",
            "{event:?} went to the filter"
        );
        key(&mut h, Key::Escape, Modifiers::NONE);
        assert_eq!(open(&h), None);
    }
}

#[test]
fn typing_and_enter_in_one_frame_run_the_row_for_what_was_typed() {
    let mut h = layouts_harness("toolbar-one-frame");
    // Library: the first match for "hnum", not the first row before it was typed.
    key(&mut h, Key::L, Modifiers::ALT);
    type_and_enter(&mut h, "hnum");
    assert_eq!(focused_kind(&h), Some(TabKind::Resource("heb-num".into())));
    assert_eq!(open(&h), None);
    // Tools: the only tool matching "no".
    key(&mut h, Key::T, Modifiers::ALT);
    type_and_enter(&mut h, "no");
    assert_eq!(focused_kind(&h), Some(TabKind::Tool(Tool::Notes)));
    // Guides: the Passage Guide at the passage typed.
    key(&mut h, Key::G, Modifiers::ALT);
    type_and_enter(&mut h, "Gen 31:3");
    let ws = &h.shell.st.session.workspace;
    let guide = ws
        .tabs()
        .find(|t| t.kind == TabKind::Guide(Guide::PassageGuide))
        .expect("a Passage Guide");
    assert_eq!(guide.history.current(), Some(ge(31, 3)));
    // Layouts: the only Layout matching "reading".
    key(&mut h, Key::Y, Modifiers::ALT);
    type_and_enter(&mut h, "reading");
    settle_all(&mut h);
    let titles: Vec<Vec<String>> = h.panels().into_iter().map(|(_, t)| t).collect();
    assert_eq!(titles, [vec!["TB".to_string()]], "Reading only");
}

#[test]
fn arrows_reach_every_layout_card_and_footer_link_filtered_or_not() {
    let mut h = layouts_harness("toolbar-layout-arrows");
    let selected = |h: &Harness| h.shell.st.toolbar.selected;
    // Filtered: Two Bibles, Parallel Bibles, Save Layout as…, Reset Layout; ↓/↑ one at a time.
    key(&mut h, Key::Y, Modifiers::ALT);
    type_text(&mut h, "bibles");
    let mut walked = vec![selected(&h)];
    for _ in 0..3 {
        key(&mut h, Key::ArrowDown, Modifiers::NONE);
        walked.push(selected(&h));
    }
    assert_eq!(walked, [0, 1, 2, 3]);
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(selected(&h), 1);
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle_all(&mut h);
    let titles: Vec<Vec<String>> = h.panels().into_iter().map(|(_, t)| t).collect();
    assert_eq!(titles, [vec!["TB".to_string()], vec!["HEB".to_string()]]);

    // Unfiltered, ↓/↑ keep the column across the rows as drawn: the saved card, the first
    // Built-in under it, Save Layout as… under that; → along a row.
    key(&mut h, Key::Y, Modifiers::ALT);
    let mut walked = vec![selected(&h)];
    for _ in 0..2 {
        key(&mut h, Key::ArrowDown, Modifiers::NONE);
        walked.push(selected(&h));
    }
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    walked.push(selected(&h));
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    walked.push(selected(&h));
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    walked.push(selected(&h));
    // 0 Two Bibles; 1..=4 the Built-ins; 5 Save Layout as…, 6 Reset Layout.
    assert_eq!(walked, [0, 1, 5, 6, 2, 0]);
}

#[test]
fn many_saved_layouts_scroll_and_the_footer_stays_in_the_window() {
    let mut h = layouts_harness("toolbar-many-layouts");
    for n in 1..=24 {
        h.shell
            .run("layout.save", &json!({ "name": format!("Desk {n:02}") }));
    }
    h.frames(2);
    key(&mut h, Key::Y, Modifiers::ALT);
    let pane = h.shell.st.toolbar.pane.unwrap();
    let window = 900.0;
    assert!(pane.bottom() <= window, "{pane:?}");
    let reset = in_pane(&h, "Reset Layout");
    assert!(reset.y < window);
    // The Built-ins are below the fold, out of the pane.
    let reading = h.find("Reading only").unwrap().center();
    assert!(!pane.contains(reading), "{reading:?} in {pane:?}");
    // ↓ through the seven rows of saved cards to the Built-ins, → to Reading only: scrolled
    // into view, and Enter loads it.
    for _ in 0..7 {
        key(&mut h, Key::ArrowDown, Modifiers::NONE);
    }
    for _ in 0..3 {
        key(&mut h, Key::ArrowRight, Modifiers::NONE);
    }
    let pane = h.shell.st.toolbar.pane.unwrap();
    let reading = in_pane(&h, "Reading only");
    assert!(reading.y < pane.bottom() && pane.bottom() <= window);
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle_all(&mut h);
    let titles: Vec<Vec<String>> = h.panels().into_iter().map(|(_, t)| t).collect();
    assert_eq!(titles, [vec!["TB".to_string()]]);
}

#[test]
fn each_launcher_is_a_named_button_with_a_shortcut() {
    let mut h = Harness::new();
    h.ctx.enable_accesskit();
    h.frames(2);
    let tree = h.tree.as_ref().expect("an AccessKit tree");
    for which in Launcher::ALL {
        let named = tree.nodes.iter().any(|(_, n)| {
            n.role() == egui::accesskit::Role::Button && n.label() == Some(which.title())
        });
        assert!(named, "no button named {}", which.title());
        let spec = h.shell.st.session.commands().get(which.command()).unwrap();
        assert!(spec.shortcut.is_some_and(|s| s.starts_with("Alt+")));
    }
    // The text menus left are View and Help.
    assert!(h.find("View").is_some() && h.find("Help").is_some());
}

#[test]
fn previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let mut h = layouts_harness("toolbar-previews");
    let commentary = obs_engine::testing::commentary(
        "tc",
        "Test Commentary on Genesis",
        "TC",
        &[(
            obs_scripture::Passage::verse(ge(31, 1)),
            None,
            "On the chapter.",
        )],
    );
    add_doc(
        &mut h.shell,
        Arc::new(ReadingDoc::from_resource(commentary)),
    );
    let tb = h.tab_id("TB");
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:20" }));
    settle_all(&mut h);
    h.shell.st.toast = None;
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/toolbar");
    for theme in ["light", "dark"] {
        h.shell.run("view.theme", &json!({ "value": theme }));
        h.frames(3);
        for (which, k, extra) in [
            (Launcher::Library, Key::L, ""),
            (Launcher::Library, Key::L, "gen"),
            (Launcher::Guides, Key::G, ""),
            (Launcher::Tools, Key::T, ""),
            (Launcher::Layouts, Key::Y, ""),
        ] {
            key(&mut h, k, Modifiers::ALT);
            if !extra.is_empty() {
                type_text(&mut h, extra);
            }
            h.events.push(Event::PointerMoved(pos2(1300.0, 880.0)));
            h.frames(4);
            let name = format!(
                "toolbar-{}{}-{theme}.ppm",
                which.title().to_lowercase(),
                if extra.is_empty() { "" } else { "-search" }
            );
            h.screenshot(&dir.join(name));
            key(&mut h, Key::Escape, Modifiers::NONE);
        }
    }
}
