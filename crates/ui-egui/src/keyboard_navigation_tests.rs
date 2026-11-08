//! Keyboard navigation across Panels and Tabs (#215) with real key events: the Tab switcher
//! while Ctrl is held (Ctrl+Tab, Ctrl+Shift+Tab, and the Control key on macOS), Ctrl+1…9 to
//! the Panels in screen order, Alt+←/→, Ctrl+Page Up/Down, and F6 / Shift+F6 round the
//! window's areas, each landing with a visible focus ring, and every key listed in Help →
//! Keyboard shortcuts.

use egui::Key;
use egui::accesskit::Role;
use obs_engine::{TabId, Tool};
use obs_scripture::{Book, Verse};

use super::a11y_audit::{assert_focus_ring, focused, key};
use super::*;

/// Ctrl as Linux and Windows report it.
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};

/// The Control key as macOS reports it (Cmd is `command` there).
const MAC_CONTROL: Modifiers = Modifiers {
    ctrl: true,
    ..Modifiers::NONE
};

fn harness() -> Harness {
    let mut h = Harness::new();
    h.ctx.enable_accesskit();
    h.frames(3);
    h
}

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// Press `key` with `modifiers` already held (and left held).
fn press_held(h: &mut Harness, key: Key, modifiers: Modifiers) {
    h.modifiers = modifiers;
    for pressed in [true, false] {
        h.events.push(Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
    }
    h.frames(2);
}

/// Let go of every modifier.
fn let_go(h: &mut Harness) {
    h.modifiers = Modifiers::NONE;
    h.frames(3);
}

fn focused_tab(h: &Harness) -> Option<TabId> {
    h.shell.st.session.workspace.focused()
}

fn title(h: &Harness, tab: TabId) -> String {
    h.shell.st.session.workspace.tab(tab).unwrap().title.clone()
}

/// The keyboard is on `tab`'s button in its tab bar, ringed.
fn assert_keyboard_on(h: &Harness, tab: TabId) {
    let (name, role, _) = focused(h).expect("something has the keyboard");
    assert_eq!(
        (name.as_str(), role),
        (title(h, tab).as_str(), Role::Button)
    );
    assert_focus_ring(h);
}

/// The default layout's Library Panel with Search and History opened beside the Library:
/// Library, then Word Info, Search, History (most recent last).
fn four_tabs_in_one_panel(h: &mut Harness) -> Vec<TabId> {
    let library = h.tab_id("Library");
    let panel = h.shell.st.session.workspace.panel_of(library).unwrap();
    for id in ["tool.search", "tool.history"] {
        h.shell.run(id, &json!({ "panel": panel.0 }));
    }
    h.frames(3);
    let tabs: Vec<TabId> = h
        .shell
        .st
        .session
        .workspace
        .panels()
        .find(|(p, _)| *p == panel)
        .unwrap()
        .1
        .to_vec();
    assert_eq!(tabs.len(), 4, "{tabs:?}");
    tabs
}

#[test]
fn ctrl_tab_shows_the_tab_switcher_while_ctrl_is_held_and_letting_go_shows_the_pick() {
    let mut h = harness();
    four_tabs_in_one_panel(&mut h);
    let search = h
        .shell
        .st
        .session
        .workspace
        .most_recent(|t| t.kind == TabKind::Tool(Tool::Search))
        .unwrap();
    let history = focused_tab(&h).unwrap();
    assert_eq!(title(&h, history), "History");
    let library = h.tab_id("Library");
    // Library was used before Search: Library, then Search.
    h.shell.st.session.workspace.user_focused(library);
    h.shell.st.session.workspace.user_focused(search);
    h.shell.st.session.workspace.user_focused(history);
    let order = h.shell.st.session.workspace.tab_switcher_order(history);
    assert_eq!(&order[..3], &[history, search, library]);

    // As the app was before #215: Ctrl+Tab had no binding, so this stayed as it is.
    preview(&h, "keyboard-navigation-before-ctrl-tab");
    // Ctrl down, Tab: the switcher lists the Panel's Tabs, last used first, the one before
    // picked; nothing changes yet.
    press_held(&mut h, Key::Tab, CTRL);
    assert!(
        h.find("Tabs in this Panel, last used first").is_some(),
        "{:?}",
        h.texts()
    );
    assert_eq!(h.shell.switcher.as_ref().map(|s| s.at), Some(1));
    assert_eq!(focused_tab(&h), Some(history));
    preview(&h, "keyboard-navigation-tab-switcher");
    // Tab again, Ctrl still held: one further down.
    press_held(&mut h, Key::Tab, CTRL);
    assert_eq!(h.shell.switcher.as_ref().map(|s| s.at), Some(2));
    // Let go: the picked Tab shows, with the keyboard on its tab, ringed.
    let_go(&mut h);
    assert!(h.shell.switcher.is_none());
    assert!(h.find("Tabs in this Panel, last used first").is_none());
    assert_eq!(focused_tab(&h), Some(library));
    assert_keyboard_on(&h, library);

    // A quick Ctrl+Tab goes back to the Tab used before.
    key(&mut h, Key::Tab, CTRL);
    assert_eq!(focused_tab(&h), Some(history));
    key(&mut h, Key::Tab, CTRL);
    assert_eq!(focused_tab(&h), Some(library));

    // Ctrl+Shift+Tab goes up the list, from the least recently used.
    let order = h.shell.st.session.workspace.tab_switcher_order(library);
    key(&mut h, Key::Tab, CTRL | Modifiers::SHIFT);
    assert_eq!(focused_tab(&h), order.last().copied());

    // Esc, Ctrl still held, closes the switcher and leaves things as they were.
    let before = focused_tab(&h);
    press_held(&mut h, Key::Tab, CTRL);
    assert!(h.shell.switcher.is_some());
    press_held(&mut h, Key::Escape, CTRL);
    let_go(&mut h);
    assert!(h.shell.switcher.is_none());
    assert_eq!(focused_tab(&h), before);
}

#[test]
fn on_macos_the_control_key_runs_ctrl_tab_since_cmd_tab_switches_apps() {
    let mut h = harness();
    four_tabs_in_one_panel(&mut h);
    let history = focused_tab(&h).unwrap();
    let before = h.shell.st.session.workspace.tab_switcher_order(history)[1];
    press_held(&mut h, Key::Tab, MAC_CONTROL);
    assert!(h.shell.switcher.is_some());
    let_go(&mut h);
    assert_eq!(focused_tab(&h), Some(before));
}

#[test]
fn ctrl_1_to_9_put_the_keyboard_on_the_nth_panel_in_screen_order() {
    let mut h = harness();
    // A third Panel, split off to the right.
    h.shell.run("tool.search", &json!({ "split": true }));
    h.frames(3);
    let order = h.shell.st.session.workspace.panels_in_order();
    assert_eq!(order.len(), 3);
    // The order is the screen's, left to right.
    let lefts: Vec<f32> = (order.iter())
        .map(|p| {
            let tab = h.shell.st.session.workspace.active_in(*p).unwrap();
            let tiles = &h.shell.tree.tiles;
            tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap().left()
        })
        .collect();
    assert!(lefts.windows(2).all(|w| w[0] < w[1]), "{lefts:?}");
    let active = |h: &Harness, n: usize| h.shell.st.session.workspace.active_in(order[n]).unwrap();

    key(&mut h, Key::Num1, CTRL);
    assert_eq!(focused_tab(&h), Some(active(&h, 0)));
    assert_keyboard_on(&h, active(&h, 0));
    preview(&h, "keyboard-navigation-ctrl-1");
    key(&mut h, Key::Num3, CTRL);
    assert_eq!(focused_tab(&h), Some(active(&h, 2)));
    assert_keyboard_on(&h, active(&h, 2));
    // Digits need Shift on AZERTY: Ctrl+2 arrives as Num2 with Shift held.
    key(&mut h, Key::Num2, CTRL | Modifiers::SHIFT);
    assert_eq!(focused_tab(&h), Some(active(&h, 1)));
    // Ctrl+Alt+1 is AltGr+1 on Windows, which types: not Ctrl+1.
    key(&mut h, Key::Num1, CTRL | Modifiers::ALT);
    assert_eq!(focused_tab(&h), Some(active(&h, 1)));
    // No fourth Panel: nothing happens.
    key(&mut h, Key::Num4, CTRL);
    assert_eq!(focused_tab(&h), Some(active(&h, 1)));
    // While a text field has the keyboard, Ctrl+1 still goes to Panel 1.
    key(&mut h, Key::L, CTRL);
    assert!(h.ctx.text_edit_focused());
    key(&mut h, Key::Num1, CTRL);
    assert!(!h.ctx.text_edit_focused());
    assert_keyboard_on(&h, active(&h, 0));
}

/// Ctrl+1…9 to a Panel in a floating OS window brings that window to the front, so the keys
/// typed next reach it, and back again to the main window.
#[test]
fn ctrl_digit_to_a_panel_in_another_os_window_brings_that_window_to_the_front() {
    let mut h = harness();
    h.native_windows();
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    let (tb, heb) = (h.tab_id("TB"), h.tab_id("HEB"));
    h.shell.run("tab.float", &json!({ "tab": heb.0 }));
    h.frames(3);
    let w = h.shell.floating[0].id;
    assert!(h.shell.floating[0].native, "an OS window of its own");
    let order = h.shell.st.session.workspace.panels_in_order();
    assert_eq!(order.len(), 3);
    assert_eq!(h.shell.st.session.workspace.panel_of(heb), Some(order[2]));
    let focus = egui::ViewportCommand::Focus;
    let root_asked = |h: &Harness| {
        let n = h.native.as_ref().unwrap().borrow();
        n.root_commands.iter().filter(|c| **c == focus).count()
    };
    let window_asked = |h: &Harness| {
        let win = h.window(w);
        win.commands.iter().filter(|c| **c == focus).count()
    };
    // In the main window, on TB.
    h.root_info.focused = Some(true);
    key(&mut h, Key::Num1, CTRL);
    assert_eq!(focused_tab(&h), Some(tb));
    assert_eq!(window_asked(&h), 0, "the floating window stays behind");

    // Ctrl+3: HEB's Panel, in the floating window, which comes to the front.
    key(&mut h, Key::Num3, CTRL);
    assert_eq!(focused_tab(&h), Some(heb));
    assert_eq!(window_asked(&h), 1, "HEB's window brought to the front");
    assert_eq!(
        h.root_info.focused,
        Some(false),
        "the main window lost the keyboard"
    );
    // The keys typed next are typed there, on HEB.
    assert_eq!(h.shell.focus_in(Some(w)), Some(heb));

    // Ctrl+1 typed in the floating window: back to TB, and the main window to the front.
    let before = root_asked(&h);
    h.key_in(w, Key::Num1, CTRL);
    assert_eq!(focused_tab(&h), Some(tb));
    assert_eq!(
        root_asked(&h),
        before + 1,
        "the main window brought to the front"
    );
    assert_eq!(h.root_info.focused, Some(true));
    assert_eq!(window_asked(&h), 1, "not asked again");
    assert_eq!(h.shell.focus_in(None), Some(tb));
}

fn at(h: &Harness, tab: TabId) -> Verse {
    h.shell
        .st
        .session
        .workspace
        .tab(tab)
        .unwrap()
        .history
        .current()
        .unwrap()
}

#[test]
fn ctrl_page_keys_move_a_bible_by_chapter_and_alt_arrows_go_back_and_forward() {
    let mut h = harness();
    let tb = h.tab_id("TB");
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 30:12" }));
    h.frames(3);
    key(&mut h, Key::Num1, CTRL);
    assert_eq!(focused_tab(&h), Some(tb));
    key(&mut h, Key::PageDown, CTRL);
    assert_eq!(at(&h, tb), Verse::new(Book::Genesis, 31, 1));
    key(&mut h, Key::PageDown, CTRL);
    assert_eq!(at(&h, tb), Verse::new(Book::Genesis, 32, 1));
    key(&mut h, Key::PageUp, CTRL);
    assert_eq!(at(&h, tb), Verse::new(Book::Genesis, 31, 1));
    // Alt+← and Alt+→ are the Tab's Back and Forward.
    key(&mut h, Key::ArrowLeft, Modifiers::ALT);
    assert_eq!(at(&h, tb), Verse::new(Book::Genesis, 32, 1));
    key(&mut h, Key::ArrowLeft, Modifiers::ALT);
    assert_eq!(at(&h, tb), Verse::new(Book::Genesis, 31, 1));
    key(&mut h, Key::ArrowRight, Modifiers::ALT);
    assert_eq!(at(&h, tb), Verse::new(Book::Genesis, 32, 1));
}

/// F6's stops, read from where the keyboard is after each press.
fn f6_round(h: &mut Harness, modifiers: Modifiers, presses: usize) -> Vec<String> {
    (0..presses)
        .map(|_| {
            key(h, Key::F6, modifiers);
            assert_focus_ring(h);
            focused(h).unwrap().0
        })
        .collect()
}

#[test]
fn f6_and_shift_f6_move_the_keyboard_round_the_menu_bar_the_toolbar_and_the_panels() {
    let mut h = harness();
    let order = h.shell.st.session.workspace.panels_in_order();
    let panels: Vec<String> = (order.iter())
        .map(|p| title(&h, h.shell.st.session.workspace.active_in(*p).unwrap()))
        .collect();
    assert_eq!(panels.len(), 2);
    // From the second Panel.
    key(&mut h, Key::Num2, CTRL);
    let tb = h.tab_id("TB");
    assert_eq!(h.shell.st.session.workspace.active_in(order[0]), Some(tb));
    let round = f6_round(&mut h, Modifiers::NONE, 4);
    assert_eq!(
        round,
        vec![
            "View".to_string(),
            "Library".to_string(),
            panels[0].clone(),
            panels[1].clone()
        ]
    );
    assert_eq!(
        focused_tab(&h),
        Some(h.shell.st.session.workspace.active_in(order[1]).unwrap())
    );
    preview(&h, "keyboard-navigation-f6");
    // A Panel reached by F6 is the focused one: its Tab's keys act there.
    key(&mut h, Key::F6, Modifiers::SHIFT);
    assert_eq!(focused_tab(&h), Some(tb));
    let back = f6_round(&mut h, Modifiers::SHIFT, 3);
    assert_eq!(
        back,
        vec!["Library".to_string(), "View".to_string(), panels[1].clone()]
    );
}

#[test]
fn f6_stops_on_more_launchers_when_every_launcher_overflows() {
    let mut h = harness();
    h.screen = egui::vec2(560.0, 600.0);
    h.frames(4);
    let tb = &h.shell.st.toolbar;
    let more = tb.overflow.expect("a » button");
    assert!(
        obs_engine::Launcher::ALL
            .iter()
            .all(|w| tb.buttons.get(w).is_none_or(|r| *r == more)),
        "every Launcher overflows: {:?}",
        tb.buttons
    );
    let order = h.shell.st.session.workspace.panels_in_order();
    let first_panel = title(
        &h,
        h.shell.st.session.workspace.active_in(order[0]).unwrap(),
    );
    // From the last Panel: the menu bar, then the Toolbar's » button, then the first Panel.
    key(&mut h, Key::Num2, CTRL);
    let round = f6_round(&mut h, Modifiers::NONE, 3);
    assert_eq!(
        round,
        vec![
            "View".to_string(),
            "More launchers".to_string(),
            first_panel
        ]
    );
}

#[test]
fn every_navigation_key_shows_in_help_keyboard_shortcuts() {
    let mut h = harness();
    h.shell.run("help.keyboard_shortcuts", &json!({}));
    h.frames(4);
    let names: Vec<String> = h
        .tree
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .filter_map(|(_, n)| n.label().map(str::to_string))
        .collect();
    for want in [
        "Next Tab in Panel shortcut: Ctrl+Tab",
        "Previous Tab in Panel shortcut: Ctrl+Shift+Tab",
        "Go to Panel 1 shortcut: Ctrl+1",
        "Go to Panel 9 shortcut: Ctrl+9",
        "Back shortcut: Alt+←",
        "Forward shortcut: Alt+→",
        "Previous chapter shortcut: Ctrl+Page Up",
        "Next chapter shortcut: Ctrl+Page Down",
        "Next area of the window shortcut: F6",
        "Previous area of the window shortcut: Shift+F6",
        "Highlight with the palette's style 1 shortcut: Alt+1",
    ] {
        assert!(names.iter().any(|n| n == want), "{want} not in {names:?}");
    }
    assert!(h.shell.st.session.key_conflicts().is_empty());
}
