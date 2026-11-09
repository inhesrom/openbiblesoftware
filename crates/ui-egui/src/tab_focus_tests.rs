//! The Tab the keys act on (#256), with real key and pointer events: after a Tab closes (Ctrl+W
//! or a click on its Panel's tab bar) its Panel's next Tab takes the keys at once; Tab
//! traversal into another Panel's header moves them there; the Command box typed for in a
//! floating OS window brings the main window to the front; » lists the Launchers it holds; and
//! a Bible jumped to before it was ever drawn has its start to go Back to.

use egui::Key;
use egui::accesskit::Role;
use obs_engine::{Launcher, TabId, TabKind};
use obs_scripture::{Book, Verse};

use super::a11y_audit::{focused, key, tab_to};
use super::*;

/// Ctrl as Linux and Windows report it.
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};

fn harness() -> Harness {
    let mut h = Harness::new();
    h.ctx.enable_accesskit();
    h.frames(3);
    h
}

fn focused_tab(h: &Harness) -> Option<TabId> {
    h.shell.st.session.workspace.focused()
}

fn live(h: &Harness, tab: TabId) -> bool {
    h.shell.st.session.workspace.tab(tab).is_some()
}

fn at(h: &Harness, tab: TabId) -> Option<Verse> {
    h.shell.st.session.workspace.tab(tab)?.history.current()
}

fn find_bar_open(h: &Harness, tab: TabId) -> bool {
    h.shell.st.tabs.get(&tab).is_some_and(|t| t.find.is_some())
}

fn home_tab(h: &Harness) -> TabId {
    (h.shell.st.session.workspace.tabs())
        .find(|t| t.kind == TabKind::Home)
        .map(|t| t.id)
        .unwrap()
}

fn middle_click(h: &mut Harness, p: Pos2) {
    h.events.push(Event::PointerMoved(p));
    h.frame();
    for pressed in [true, false] {
        h.events.push(Event::PointerButton {
            pos: p,
            button: PointerButton::Middle,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frames(2);
}

fn ge(chapter: u16, verse: u16) -> Verse {
    Verse::new(Book::Genesis, chapter, verse)
}

/// WORKSPACE-MAC-1: Home closed with Ctrl+W leaves TB shown and focused, and Ctrl+F, Ctrl+Page
/// Down, Alt+← and Ctrl+W act on TB at once, without a click; Window → Close Panel stays on.
#[test]
fn closing_home_with_ctrl_w_hands_the_keys_to_the_bible_it_covered() {
    let mut h = harness();
    let tb = h.tab_id("TB");
    h.shell.run("home.open", &json!({}));
    h.frames(3);
    let home = home_tab(&h);
    assert_eq!(focused_tab(&h), Some(home));
    key(&mut h, Key::W, CTRL);
    assert!(!live(&h, home));
    assert_eq!(focused_tab(&h), Some(tb), "TB, now shown, takes the keys");
    assert!(h.shell.st.session.is_enabled("panel.close"));
    key(&mut h, Key::F, CTRL);
    assert!(find_bar_open(&h, tb), "Ctrl+F opened TB's Find bar");
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::PageDown, CTRL);
    assert_eq!(at(&h, tb), Some(ge(31, 1)));
    key(&mut h, Key::ArrowLeft, Modifiers::ALT);
    assert_eq!(at(&h, tb), Some(ge(30, 1)));
    key(&mut h, Key::W, CTRL);
    assert!(!live(&h, tb), "a second Ctrl+W closes TB");
}

/// WORKSPACE-MAC-1, the × way: Home closed from its tab bar (a middle click, which egui_tiles
/// closes like the ×) hands the keys to TB too.
#[test]
fn closing_home_from_its_tab_bar_hands_the_keys_to_the_bible_it_covered() {
    let mut h = harness();
    let tb = h.tab_id("TB");
    h.shell.run("home.open", &json!({}));
    h.frames(3);
    let home = home_tab(&h);
    let p = h.tab_at("Home");
    middle_click(&mut h, p);
    assert!(!live(&h, home));
    assert_eq!(focused_tab(&h), Some(tb));
    key(&mut h, Key::PageDown, CTRL);
    assert_eq!(at(&h, tb), Some(ge(31, 1)), "Ctrl+Page Down moved TB");
}

/// AUD-UI-SHELL-2: Ctrl+W twice closes two Tabs.
#[test]
fn successive_ctrl_w_close_successive_tabs() {
    let mut h = harness();
    h.shell.run("tool.history", &json!({}));
    h.frames(3);
    let count = |h: &Harness| h.shell.st.session.workspace.tabs().count();
    let before = count(&h);
    key(&mut h, Key::W, CTRL);
    assert_eq!(count(&h), before - 1);
    assert!(focused_tab(&h).is_some(), "the keys still have a Tab");
    key(&mut h, Key::W, CTRL);
    assert_eq!(count(&h), before - 2);
}

/// AUD-UI-SHELL-1: Tab from the Library to TB's "Go to passage" field moves the keys to TB:
/// Ctrl+W closes TB, where the keyboard is, and leaves the Library open.
#[test]
fn tab_traversal_into_another_panel_moves_the_keys_there() {
    let mut h = harness();
    let (tb, library) = (h.tab_id("TB"), h.tab_id("Library"));
    let p = h.tab_at("Library");
    h.click(p);
    assert_eq!(focused_tab(&h), Some(library));
    tab_to(&mut h, "Go to passage");
    let (_, role, rect) = focused(&h).unwrap();
    assert_eq!(role, Role::TextInput);
    let tb_panel = h.shell.st.session.workspace.panel_of(tb).unwrap();
    let in_tb = (h.panels().into_iter())
        .any(|(r, tabs)| r.contains(rect.center()) && tabs.iter().any(|t| t == "TB"));
    assert!(in_tb, "the field is TB's ({tb_panel:?})");
    assert_eq!(focused_tab(&h), Some(tb), "the keys follow the keyboard");
    key(&mut h, Key::W, CTRL);
    assert!(!live(&h, tb), "Ctrl+W closed the Tab the keyboard is in");
    assert!(live(&h, library));
}

/// A popup over a Panel is not in it: the Command box's list, drawn over TB, leaves the keys
/// on the Library.
#[test]
fn the_keyboard_in_the_command_box_leaves_the_keys_where_they_were() {
    let mut h = harness();
    let library = h.tab_id("Library");
    let p = h.tab_at("Library");
    h.click(p);
    key(&mut h, Key::L, CTRL);
    h.events.push(Event::Text("gen".into()));
    h.frames(2);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(focused_tab(&h), Some(library));
}

/// WORKSPACE-MAC-3: a Bible jumped to before it is ever drawn (Home on top of it, TB just
/// opened) has its first Verse to go Back to.
#[test]
fn a_jump_into_a_bible_never_drawn_can_go_back() {
    let mut shell = Shell::new(Session::new(JobMode::Inline));
    add_doc(&mut shell, bible());
    let tb = shell.run("resource.open", &json!({ "id": "test-bible" }));
    let tb = TabId(tb.unwrap()["tab"].as_u64().unwrap());
    shell.run("home.open", &json!({}));
    let mut h = Harness::from_shell(shell);
    h.ctx.enable_accesskit();
    assert_ne!(focused_tab(&h), Some(tb), "Home on top of TB");
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 32:5" }));
    h.frames(3);
    let t = h.shell.st.session.workspace.tab(tb).unwrap();
    assert!(t.history.can_go_back(), "a Back entry");
    let p = h.tab_at("TB");
    h.click(p);
    key(&mut h, Key::ArrowLeft, Modifiers::ALT);
    assert_eq!(at(&h, tb), Some(ge(30, 1)), "Back to where TB started");
}

/// AUD-UI-SHELL-3: Ctrl+L typed in a floating OS window brings the main window, where the
/// Command box is, to the front; what is typed next goes into the Command box.
#[test]
fn ctrl_l_in_a_floating_os_window_brings_the_main_window_to_the_front() {
    let mut h = harness();
    h.native_windows();
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    let heb = h.tab_id("HEB");
    h.shell.run("tab.float", &json!({ "tab": heb.0 }));
    h.frames(3);
    let w = h.shell.floating[0].id;
    h.root_info.focused = Some(true);
    // Ctrl+3: HEB's Panel, in the floating window, which takes the keyboard.
    key(&mut h, Key::Num3, CTRL);
    assert_eq!(focused_tab(&h), Some(heb));
    assert_eq!(
        h.root_info.focused,
        Some(false),
        "the floating window has the keyboard"
    );
    h.key_in(w, Key::L, CTRL);
    assert_eq!(
        h.root_info.focused,
        Some(true),
        "the main window came to the front"
    );
    h.events.push(Event::Text("Gen 31:55".into()));
    h.frames(2);
    assert_eq!(h.shell.st.command_box.text, "Gen 31:55");
}

/// WORKSPACE-MAC-2: at a Larger Interface size, » opens its own list of the Launchers it
/// holds, not a menu of the menu bar.
#[test]
fn more_launchers_lists_the_launchers_it_holds_at_every_interface_size() {
    let mut h = harness();
    h.screen = vec2(1100.0, 700.0);
    for size in ["default", "larger"] {
        h.shell
            .run("view.interface_size", &json!({ "value": size }));
        h.frames(4);
        let tb = &h.shell.st.toolbar;
        let Some(more) = tb.overflow else {
            continue;
        };
        let rest: Vec<Launcher> = (Launcher::ALL.into_iter())
            .filter(|w| tb.buttons.get(w) == Some(&more))
            .collect();
        assert!(!rest.is_empty());
        h.click(more.center());
        let texts = h.texts();
        for w in &rest {
            assert!(
                texts.contains(&w.title()),
                "{size}: {} listed: {texts:?}",
                w.title()
            );
        }
        assert!(
            !texts.contains(&"Reopen closed Tab"),
            "{size}: the Window menu opened: {texts:?}"
        );
        key(&mut h, Key::Escape, Modifiers::NONE);
    }
}
