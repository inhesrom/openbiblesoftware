//! Reopen closed Tab (#214) through the shell's real-event headless harness: three Bible Tabs
//! closed with Ctrl+W, a middle click on the Tab and its ⋯ menu's Close Panel, then Ctrl+Shift+T
//! pressed three times brings them back newest first, each in its old Panel at its old place
//! in the dock, at its old location (to the point it was scrolled) and in its old Link set.
//! Window → Recently closed lists them and reopens the one clicked.

use egui::Key;
use obs_engine::TabId;
use obs_scripture::{Book, Verse};

use super::*;

const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};
const CTRL_SHIFT: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    shift: true,
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
    settle_all(h);
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
    settle_all(h);
}

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

fn ge(c: u16, v: u16) -> Verse {
    Verse::new(Book::Genesis, c, v)
}

fn go(h: &mut Harness, t: TabId, text: &str) {
    h.shell
        .run("tab.go", &json!({ "tab": t.0, "text": text }));
    settle_all(h);
}

fn link(h: &mut Harness, t: TabId, set: Option<char>) {
    h.shell.st.session.workspace.set_link(t, set);
    h.shell.apply_effects();
    settle_all(h);
}

/// What the user sees of a Tab: its title, the Verse its view shows, the Link set its chip
/// names, and the Tabs of the Panel it is in.
fn seen(h: &mut Harness, t: TabId) -> (String, Option<Verse>, Option<char>, Vec<String>) {
    let at = location(h, t);
    let tiles = &h.shell.tree.tiles;
    let pane = tiles.find_pane(&t).unwrap();
    let container = tiles.rect(tiles.parent_of(pane).unwrap()).unwrap();
    let panel = (h.panels().into_iter())
        .find(|(r, _)| *r == container)
        .map(|(_, titles)| titles)
        .unwrap();
    let tab = h.shell.st.session.workspace.tab(t).unwrap();
    (tab.title.clone(), at, tab.link, panel)
}

/// The Panels left to right, each with its Tabs' titles.
fn dock(h: &Harness) -> Vec<Vec<String>> {
    h.panels().into_iter().map(|(_, titles)| titles).collect()
}

fn anchor(h: &mut Harness, t: TabId) -> crate::view::TextAnchor {
    let view = h.shell.st.tabs.get_mut(&t).unwrap().view.as_mut().unwrap();
    view.text_anchor().unwrap()
}

#[test]
fn three_closed_tabs_come_back_newest_first_with_ctrl_shift_t_where_they_were() {
    let mut h = Harness::new();
    settle_all(&mut h);
    // TB (Link set A) at Gen 31:10; a second TB beside it, unlinked, at Gen 30:20 and scrolled
    // a little past it; HNUM in a Panel of its own, Link set B, at its Gen 32:5.
    let tb = h.tab_id("TB");
    go(&mut h, tb, "Gen 31:10");
    let tb_panel = h.shell.st.session.workspace.panel_of(tb).unwrap();
    let out = h.shell.run(
        "resource.open",
        &json!({ "id": "test-bible", "panel": tb_panel.0 }),
    );
    let tb2 = TabId(out.unwrap()["tab"].as_u64().unwrap());
    link(&mut h, tb2, None);
    go(&mut h, tb2, "Gen 30:20");
    let out = h
        .shell
        .run("resource.open", &json!({ "id": "heb-num", "split": true }));
    let hn = TabId(out.unwrap()["tab"].as_u64().unwrap());
    settle_all(&mut h);
    link(&mut h, hn, Some('B'));
    go(&mut h, hn, "Gen 32:5");
    let view = h.shell.st.tabs.get_mut(&tb2).unwrap().view.as_mut().unwrap();
    view.scroll_by(9.0);
    settle_all(&mut h);
    let before_dock = dock(&h);
    let before = [seen(&mut h, tb2), seen(&mut h, hn), seen(&mut h, tb)];
    assert_eq!(before[0].1, Some(ge(30, 20)));
    assert_eq!((before[1].1, before[1].2), (Some(ge(32, 5)), Some('B')));
    assert_eq!((before[2].1, before[2].2), (Some(ge(31, 10)), Some('A')));
    let tb2_anchor = anchor(&mut h, tb2);
    assert!(tb2_anchor.into > 0.0, "scrolled into a line: {tb2_anchor:?}");

    // Close: the second TB with Ctrl+W, HNUM with a middle click on its Tab, then TB from
    // its ⋯ menu as the last Tab of its Panel.
    h.shell.st.session.workspace.user_focused(tb2);
    key(&mut h, Key::W, CTRL);
    assert!(h.shell.st.session.workspace.tab(tb2).is_none());
    let hnum = h.tab_at("HNUM");
    middle_click(&mut h, hnum);
    assert!(h.shell.st.session.workspace.tab(hn).is_none());
    h.shell.run("tab.close", &json!({ "tab": tb.0 }));
    settle_all(&mut h);
    assert!(h.shell.st.session.workspace.tab(tb).is_none());
    assert_eq!(dock(&h).len(), before_dock.len() - 2, "two Panels closed");

    // Window → Recently closed lists them newest first.
    h.open_menu("Window");
    h.click_text("Recently closed");
    for label in ["Genesis 31:10 · TB", "Genesis 32:5 · HNUM", "Genesis 30:20 · TB"] {
        assert!(h.find(label).is_some(), "{label} in {:?}", h.texts());
    }
    let order: Vec<f32> = ["Genesis 31:10 · TB", "Genesis 32:5 · HNUM", "Genesis 30:20 · TB"]
        .iter()
        .map(|l| h.find(l).unwrap().top())
        .collect();
    assert!(order.is_sorted(), "{order:?}");
    preview(&h, "recently-closed-menu");
    key(&mut h, Key::Escape, Modifiers::NONE);

    // Ctrl+Shift+T three times: TB, then HNUM, then the second TB.
    for expected in before.iter().rev() {
        key(&mut h, Key::T, CTRL_SHIFT);
        let t = h.shell.st.session.workspace.focused().unwrap();
        let now = seen(&mut h, t);
        assert_eq!((&now.0, now.1, now.2), (&expected.0, expected.1, expected.2));
        if now.0 == "TB" && now.1 == Some(ge(30, 20)) {
            // The text that was at the top of the view is again, as far into its line.
            let view = h.shell.st.tabs.get_mut(&t).unwrap().view.as_mut().unwrap();
            let top = view.text_top(tb2_anchor.unit, tb2_anchor.byte, 600.0);
            assert_eq!(top, Some(-tb2_anchor.into), "{:?}", view.text_anchor());
        }
    }
    // Every Panel is back where it was, with its Tabs in their order.
    assert_eq!(dock(&h), before_dock);
    preview(&h, "recently-closed-reopened");
    assert!(!h.shell.st.session.is_enabled("tab.reopen_closed"));
}

#[test]
fn close_panel_and_a_click_in_recently_closed_bring_a_panel_back_whole() {
    let mut h = Harness::new();
    settle_all(&mut h);
    let before = dock(&h);
    let library = h.tab_id("Library");
    let tb = h.tab_id("TB");
    go(&mut h, tb, "Gen 32:20");
    // Close Panel from the Library Tab's ⋯ menu closes Library and Word Info together.
    h.shell.run("panel.close", &json!({ "tab": library.0 }));
    settle_all(&mut h);
    assert_eq!(dock(&h), [vec!["TB".to_string()]]);
    // Then TB closes too, and the Panel is reopened from Window → Recently closed.
    h.shell.run("tab.close", &json!({ "tab": tb.0 }));
    settle_all(&mut h);
    h.open_menu("Window");
    h.click_text("Recently closed");
    h.click_text("Panel: Library, Word Info");
    settle_all(&mut h);
    assert_eq!(dock(&h), [vec!["Library".to_string(), "Word Info".to_string()]]);
    key(&mut h, Key::T, CTRL_SHIFT);
    assert_eq!(dock(&h), before);
    let tb = h.tab_id("TB");
    assert_eq!(location(&mut h, tb), Some(ge(32, 20)));
}
