//! Help → Keyboard shortcuts (#177) through the shell's real-event headless harness: a key
//! recorded for Larger text works at once and after a restart, Reset brings back Ctrl+= (the
//! default Ctrl+Plus, #137), and a key in use offers Swap keys or Cancel before anything is
//! saved.

use egui::Key;
use obs_engine::{HeaderBox, TabId};

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

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

fn pane_rect(h: &Harness, tab: TabId) -> Rect {
    let tiles = &h.shell.tree.tiles;
    tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
}

/// Where `text` is painted in the Tab's pane.
fn in_tab(h: &Harness, tab: TabId, text: &str) -> Option<Pos2> {
    let r = pane_rect(h, tab);
    (h.shapes.iter())
        .find(|(s, t)| t == text && r.contains(s.center()))
        .map(|(s, _)| s.center())
}

fn click_in_tab(h: &mut Harness, tab: TabId, text: &str) {
    let p = in_tab(h, tab, text)
        .unwrap_or_else(|| panic!("{text:?} is not in the Tab: {:?}", h.texts()));
    h.click(p);
}

/// Help → Keyboard shortcuts, filtered to `filter`.
fn open_keys(h: &mut Harness, filter: &str) -> TabId {
    h.open_menu("Help");
    h.click_text("Keyboard shortcuts");
    h.frames(2);
    let tab = h.tab_id("Keyboard shortcuts");
    let ws = &h.shell.st.session.workspace;
    assert_eq!(ws.focused(), Some(tab));
    let t = ws.tab(tab).unwrap();
    assert_eq!(t.kind, TabKind::KeyboardShortcuts);
    assert_eq!(t.header_box(), HeaderBox::Filter);
    if !filter.is_empty() {
        filter_to(h, tab, filter);
    }
    tab
}

fn filter_to(h: &mut Harness, tab: TabId, filter: &str) {
    click_in_tab(h, tab, "Filter");
    type_text(h, filter);
    h.frames(2);
}

fn text_size(h: &Harness) -> f32 {
    h.shell.st.session.text.size
}

/// Whether `k` runs Larger text now (the size goes up a step).
fn is_larger_text(h: &mut Harness, k: Key, modifiers: Modifiers) -> bool {
    let before = text_size(h);
    key(h, k, modifiers);
    text_size(h) > before
}

/// Acceptance: Larger text rebound to Ctrl+Up works at once and after a restart; Reset
/// restores Ctrl+=.
#[test]
fn rebinding_larger_text_to_ctrl_up_works_at_once_survives_a_restart_and_reset_restores_it() {
    let db = temp_db("keys-larger");
    let mut h = Harness::launch(Some(&db));
    let tab = open_keys(&mut h, "larger");
    assert!(in_tab(&h, tab, "Larger text").is_some(), "{:?}", h.texts());
    // Only what the filter matches is listed, under its menu.
    assert!(in_tab(&h, tab, "Smaller text").is_none());
    assert!(in_tab(&h, tab, "VIEW").is_some());
    click_in_tab(&mut h, tab, "Ctrl++");
    assert!(in_tab(&h, tab, "Press a key…").is_some());
    // Recording: the key runs nothing else.
    let size = text_size(&h);
    key(&mut h, Key::ArrowUp, CTRL);
    assert_eq!(text_size(&h), size);
    assert_eq!(
        h.shell.st.session.shortcut("view.text_larger"),
        Some("Ctrl+Up")
    );
    assert!(in_tab(&h, tab, "Ctrl+↑").is_some(), "{:?}", h.texts());
    preview(&h, "keyboard-shortcuts");

    // At once: Ctrl+Up is Larger text, Ctrl+= no longer is.
    assert!(is_larger_text(&mut h, Key::ArrowUp, CTRL));
    assert!(!is_larger_text(&mut h, Key::Equals, CTRL));
    assert!(!is_larger_text(&mut h, Key::Plus, CTRL | Modifiers::SHIFT));
    // The View menu shows the new key.
    h.open_menu("View");
    assert!(h.find("Ctrl+↑").is_some(), "{:?}", h.texts());
    key(&mut h, Key::Escape, Modifiers::NONE);

    // After a restart.
    drop(h);
    let mut h = Harness::launch(Some(&db));
    assert_eq!(
        h.shell.st.session.shortcut("view.text_larger"),
        Some("Ctrl+Up")
    );
    assert!(is_larger_text(&mut h, Key::ArrowUp, CTRL));
    assert!(!is_larger_text(&mut h, Key::Equals, CTRL));

    // Reset: Ctrl+= (and Ctrl++) again, Ctrl+Up nothing.
    let tab = open_keys(&mut h, "larger");
    click_in_tab(&mut h, tab, "Reset");
    assert_eq!(
        h.shell.st.session.shortcut("view.text_larger"),
        Some("Ctrl+Plus")
    );
    assert!(in_tab(&h, tab, "Ctrl++").is_some());
    assert!(is_larger_text(&mut h, Key::Equals, CTRL));
    assert!(is_larger_text(&mut h, Key::Plus, CTRL | Modifiers::SHIFT));
    assert!(!is_larger_text(&mut h, Key::ArrowUp, CTRL));
    drop(h);
    let mut h = Harness::launch(Some(&db));
    assert!(is_larger_text(&mut h, Key::Equals, CTRL));
    drop(h);
    std::fs::remove_dir_all(db.parent().unwrap()).unwrap();
}

/// Acceptance: a key already in use shows the conflict and offers to swap or cancel; nothing
/// is saved until one is chosen.
#[test]
fn binding_a_key_in_use_shows_the_conflict_and_offers_swap_or_cancel() {
    let db = temp_db("keys-conflict");
    let mut h = Harness::launch(Some(&db));
    let tab = open_keys(&mut h, "larger");
    let conflict = "Ctrl+F is already the key of “Find in Resource”.";

    // Cancel: both keep their keys.
    click_in_tab(&mut h, tab, "Ctrl++");
    key(&mut h, Key::F, CTRL);
    assert!(in_tab(&h, tab, conflict).is_some(), "{:?}", h.texts());
    assert!(in_tab(&h, tab, "Swap keys").is_some());
    let s = &h.shell.st.session;
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+Plus"));
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+F"));
    preview(&h, "keyboard-shortcuts-conflict");
    click_in_tab(&mut h, tab, "Cancel");
    assert!(in_tab(&h, tab, conflict).is_none());
    let s = &h.shell.st.session;
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+Plus"));
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+F"));

    // Swap keys: Larger text gets Ctrl+F, Find in Resource Ctrl+Plus.
    click_in_tab(&mut h, tab, "Ctrl++");
    key(&mut h, Key::F, CTRL);
    click_in_tab(&mut h, tab, "Swap keys");
    assert!(in_tab(&h, tab, conflict).is_none());
    let s = &h.shell.st.session;
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+F"));
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+Plus"));
    assert!(is_larger_text(&mut h, Key::F, CTRL));
    drop(h);
    let h = Harness::launch(Some(&db));
    let s = &h.shell.st.session;
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+F"));
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+Plus"));
    drop(h);
    std::fs::remove_dir_all(db.parent().unwrap()).unwrap();
}

/// Swap keys is offered only when the other binding may take the old key: Unpin Word Info's
/// Esc stays its own, so Ctrl+F for it says why and leaves both keys as they were.
#[test]
fn swap_keys_never_hands_esc_to_another_command() {
    let db = temp_db("keys-reserved-swap");
    let mut h = Harness::launch(Some(&db));
    let tab = open_keys(&mut h, "unpin");
    click_in_tab(&mut h, tab, "Esc");
    key(&mut h, Key::F, CTRL);
    let conflict = "Ctrl+F is already the key of “Find in Resource”.";
    assert!(in_tab(&h, tab, conflict).is_some(), "{:?}", h.texts());
    let why = "“Find in Resource” cannot take Esc: Esc is kept for closing and cancelling; \
               pick another key.";
    assert!(in_tab(&h, tab, why).is_some(), "{:?}", h.texts());
    click_in_tab(&mut h, tab, "Swap keys");
    let s = &h.shell.st.session;
    assert_eq!(s.shortcut("word_info.unpin"), Some("Escape"));
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+F"));
    assert!(in_tab(&h, tab, conflict).is_some());
    click_in_tab(&mut h, tab, "Cancel");
    assert!(in_tab(&h, tab, conflict).is_none());
    drop(h);
    let h = Harness::launch(Some(&db));
    assert_eq!(h.shell.st.session.shortcut("tab.find"), Some("Ctrl+F"));
    drop(h);
    std::fs::remove_dir_all(db.parent().unwrap()).unwrap();
}

/// Esc cancels recording; Enter and keys without Ctrl or Alt are refused with the reason;
/// Clear leaves the command without a key.
#[test]
fn esc_cancels_reserved_keys_are_refused_and_clear_removes_the_key() {
    let mut h = Harness::new();
    let tab = open_keys(&mut h, "");
    // Unfiltered, the menus come first, each with its commands.
    for group in ["FILE", "EDIT", "VIEW"] {
        assert!(in_tab(&h, tab, group).is_some(), "{group}: {:?}", h.texts());
    }
    preview(&h, "keyboard-shortcuts-all");
    filter_to(&mut h, tab, "larger");
    click_in_tab(&mut h, tab, "Ctrl++");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(in_tab(&h, tab, "Press a key…").is_none());
    assert_eq!(
        h.shell.st.session.shortcut("view.text_larger"),
        Some("Ctrl+Plus")
    );

    click_in_tab(&mut h, tab, "Ctrl++");
    key(&mut h, Key::Enter, CTRL);
    assert!(
        in_tab(&h, tab, "Enter is kept for text fields and buttons").is_some(),
        "{:?}",
        h.texts()
    );
    click_in_tab(&mut h, tab, "Ctrl++");
    key(&mut h, Key::Q, Modifiers::NONE);
    assert!(
        in_tab(
            &h,
            tab,
            "Q is kept for typing and moving: add Ctrl or Alt (a function key needs neither)"
        )
        .is_some(),
        "{:?}",
        h.texts()
    );
    assert_eq!(
        h.shell.st.session.shortcut("view.text_larger"),
        Some("Ctrl+Plus")
    );

    click_in_tab(&mut h, tab, "Clear");
    assert_eq!(h.shell.st.session.shortcut("view.text_larger"), None);
    assert!(in_tab(&h, tab, "None").is_some());
    assert!(!is_larger_text(&mut h, Key::Equals, CTRL));
    click_in_tab(&mut h, tab, "Reset all");
    assert!(is_larger_text(&mut h, Key::Equals, CTRL));
}

/// Switching away from the Tab while recording stops it: keys work again.
#[test]
fn recording_stops_when_the_tab_is_not_shown() {
    let mut h = Harness::new();
    let tab = open_keys(&mut h, "larger");
    click_in_tab(&mut h, tab, "Ctrl++");
    assert!(h.shell.st.keys_ui.is_recording());
    h.shell.run("tab.close", &json!({ "tab": tab.0 }));
    h.frames(3);
    assert!(!h.shell.st.keys_ui.is_recording());
    assert!(is_larger_text(&mut h, Key::Equals, CTRL));
}

/// Every key name the engine accepts is one egui reads, written as egui writes it, so a key
/// recorded from egui is saved as itself.
#[test]
fn every_key_name_the_engine_accepts_is_an_egui_key() {
    for name in obs_engine::key_bindings::key_names() {
        let key = egui::Key::from_name(&name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(key.name(), name);
    }
    for (given, sc) in [("Ctrl+Up", "Ctrl+Up"), ("Ctrl+Shift+Plus", "Ctrl+Plus")] {
        let canonical = obs_engine::key_bindings::canonical_key(given).unwrap();
        assert_eq!(canonical, sc);
        assert!(crate::parse_shortcut(&canonical).is_some(), "{canonical}");
    }
}

/// Every row's buttons are inside the Tab, in a wide Panel (name beside them) and in a
/// narrow one (name above them).
#[test]
fn every_row_fits_in_wide_and_narrow_panels() {
    for width in [1400.0, 900.0] {
        let mut h = Harness::new();
        h.screen = vec2(width, 900.0);
        h.frames(3);
        let tab = open_keys(&mut h, "");
        let pane = pane_rect(&h, tab);
        let buttons: Vec<Rect> = (h.shapes.iter())
            .filter(|(r, t)| (t == "Clear" || t == "Reset") && pane.contains(r.left_top()))
            .map(|(r, _)| *r)
            .collect();
        assert!(buttons.len() >= 10, "{width}: {buttons:?}");
        for r in buttons {
            assert!(pane.contains_rect(r), "{width}: {r:?} outside {pane:?}");
        }
        let library = in_tab(&h, tab, "Library").unwrap();
        let key = in_tab(&h, tab, "Ctrl+Shift+L").unwrap();
        let beside = (library.y - key.y).abs() < 4.0;
        assert_eq!(
            beside,
            width > 1000.0,
            "{width}: name {library:?}, key {key:?}"
        );
        preview(&h, &format!("keyboard-shortcuts-{width}"));
    }
}

/// Recording in the main window stops once the user goes on in a floating OS window (#152):
/// a click there, or a key pressed there, and that window's keys run as usual.
#[test]
fn recording_stops_when_another_os_window_is_used() {
    let mut h = Harness::new();
    h.native_windows();
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    let heb = h.tab_id("HEB");
    h.shell.run("tab.float", &json!({ "tab": heb.0 }));
    h.frames(3);
    let w = h.shell.floating[0].id;
    assert!(h.shell.floating[0].native, "an OS window of its own");
    let tab = open_keys(&mut h, "larger");

    // A click in the floating window, then a shortcut typed there.
    click_in_tab(&mut h, tab, "Ctrl++");
    assert!(h.shell.st.keys_ui.is_recording());
    h.click_in(w, pos2(200.0, 200.0), 1);
    assert!(!h.shell.st.keys_ui.is_recording());
    let size = text_size(&h);
    h.key_in(w, Key::Equals, CTRL);
    assert!(
        text_size(&h) > size,
        "Larger text ran in the floating window"
    );
    assert!(in_tab(&h, tab, "Press a key…").is_none());

    // A shortcut typed there at once (the window took focus without a click).
    click_in_tab(&mut h, tab, "Ctrl++");
    assert!(h.shell.st.keys_ui.is_recording());
    let size = text_size(&h);
    h.window(w).events.push(Event::WindowFocused(true));
    h.key_in(w, Key::Equals, CTRL);
    assert!(
        text_size(&h) > size,
        "Larger text ran in the floating window"
    );
    assert!(!h.shell.st.keys_ui.is_recording());
    assert_eq!(
        h.shell.st.session.shortcut("view.text_larger"),
        Some("Ctrl+Plus")
    );

    // The main window losing focus stops recording too.
    click_in_tab(&mut h, tab, "Ctrl++");
    assert!(h.shell.st.keys_ui.is_recording());
    h.events.push(Event::WindowFocused(false));
    h.frames(2);
    assert!(!h.shell.st.keys_ui.is_recording());
}
