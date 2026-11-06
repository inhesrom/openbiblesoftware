//! The Command box (#139) through the headless harness: Ctrl+L, typing, the dropdown, ↑/↓,
//! Enter, Alt+Enter, Esc, a click on a suggestion, and the recent entries after a restart.

use egui::Key;
use obs_engine::{Guide, TabId};
use obs_scripture::{Book, Verse};

use super::*;
use crate::shell::command_box::{BOX_ID, HINT};

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

fn box_focused(h: &Harness) -> bool {
    h.ctx.memory(|m| m.has_focus(egui::Id::unique(BOX_ID)))
}

fn tb_tabs(h: &Harness) -> Vec<TabId> {
    h.shell
        .st
        .session
        .workspace
        .tabs()
        .filter(|t| t.kind == TabKind::Resource("test-bible".into()))
        .map(|t| t.id)
        .collect()
}

fn location(h: &mut Harness, t: TabId) -> Option<Verse> {
    h.shell
        .st
        .tabs
        .get_mut(&t)
        .and_then(|t| t.view.as_mut())
        .and_then(|v| v.current_location())
}

fn ge(c: u16, v: u16) -> Verse {
    Verse::new(Book::Genesis, c, v)
}

#[test]
fn ctrl_l_focuses_the_box_typing_suggests_and_enter_goes_to_the_reference() {
    let mut h = Harness::new();
    assert!(h.find(HINT).is_some(), "{:?}", h.texts());
    let tb = tb_tabs(&h)[0];
    assert!(!box_focused(&h));
    key(&mut h, Key::L, CTRL);
    assert!(box_focused(&h));

    type_text(&mut h, "gen 32 5");
    // The dropdown: the reference in the focused Bible first, Search last.
    let top = h.at("Genesis 32:5");
    assert!(h.find("Go to · TB").is_some(), "{:?}", h.texts());
    let search = h.at("Search for “gen 32 5”");
    assert!(top.y < search.y);

    key(&mut h, Key::Enter, Modifiers::NONE);
    h.frames(2);
    assert_eq!(location(&mut h, tb), Some(ge(32, 5)));
    assert_eq!(tb_tabs(&h), [tb], "Enter moved the focused Bible");
    // The box is empty again and the keyboard is back in the Workspace.
    assert!(!box_focused(&h));
    assert!(h.shell.st.command_box.text.is_empty());
    assert!(h.find("Search for “gen 32 5”").is_none());
}

#[test]
fn arrows_pick_a_suggestion_and_alt_enter_opens_a_new_tab() {
    let mut h = Harness::new();
    let tb = tb_tabs(&h)[0];
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "gen 31:3");
    assert_eq!(h.shell.st.command_box.selected, 0);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.shell.st.command_box.selected, 1);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.shell.st.command_box.selected, 1, "stops at the last");
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(h.shell.st.command_box.selected, 0);

    // Alt+Enter: the reference in a new Tab of the focused Bible. It joins TB's Link set
    // (A), so TB follows the jump, as with any Tab opened from a linked one.
    key(&mut h, Key::Enter, Modifiers::ALT);
    h.frames(3);
    let tabs = tb_tabs(&h);
    assert_eq!(tabs.len(), 2, "{tabs:?}");
    let new = tabs.into_iter().find(|t| *t != tb).unwrap();
    assert_eq!(location(&mut h, new), Some(ge(31, 3)));
    let ws = &h.shell.st.session.workspace;
    assert_eq!(ws.tab(new).unwrap().link, ws.tab(tb).unwrap().link);
    assert_eq!(location(&mut h, tb), Some(ge(31, 3)));

    // ↓ then Enter runs the second suggestion: here, Search.
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "gen 33:1");
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    let search = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| t.id)
        .expect("a Search Tab");
    assert_eq!(
        h.shell.st.session.search_tab(search).unwrap().query,
        "gen 33:1"
    );
}

#[test]
fn clicking_a_suggestion_runs_it_and_esc_leaves_the_box() {
    let mut h = Harness::new();
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "zzz");
    assert!(h.find("Search for “zzz”").is_some());
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!box_focused(&h));
    assert!(h.find("Search for “zzz”").is_none(), "the dropdown closed");

    key(&mut h, Key::L, CTRL);
    assert!(box_focused(&h));
    // Ctrl+L selected the text left in the box, so typing replaces it.
    type_text(&mut h, "pg gen 31");
    assert_eq!(h.shell.st.command_box.text, "pg gen 31");
    assert!(h.find("Passage Guide").is_some(), "{:?}", h.texts());
    h.click_text("Genesis 31");
    let ws = &h.shell.st.session.workspace;
    let guide = ws
        .tabs()
        .find(|t| t.kind == TabKind::Guide(Guide::PassageGuide))
        .expect("a Passage Guide");
    assert_eq!(guide.history.current(), Some(ge(31, 1)));
    assert_eq!(ws.focused(), Some(guide.id));
}

#[test]
fn an_empty_box_lists_recent_entries_kept_across_a_restart() {
    let db = temp_db("command-box-recent");
    {
        let mut h = Harness::launch(Some(&db));
        for text in ["gen 30:2", "gen 32 5"] {
            key(&mut h, Key::L, CTRL);
            type_text(&mut h, text);
            key(&mut h, Key::Enter, Modifiers::NONE);
        }
        h.shell.save_session(true);
    }
    let mut h = Harness::launch(Some(&db));
    let tb = tb_tabs(&h)[0];
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 33:1" }));
    h.frames(2);
    key(&mut h, Key::L, CTRL);
    // Newest first.
    let newest = h.at("gen 32 5");
    let older = h.at("gen 30:2");
    assert!(newest.y < older.y);
    assert!(h.find("Recent").is_some());
    h.click_text("gen 30:2");
    h.frames(2);
    assert_eq!(location(&mut h, tb), Some(ge(30, 2)));
    assert_eq!(
        h.shell.st.session.command_box_recent(),
        ["gen 30:2", "gen 32 5"]
    );
}
