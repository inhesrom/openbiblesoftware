//! Floating windows (#152) through the headless harness. With no window system the floating
//! windows are egui windows inside the main one (egui's embedded viewports), drawn by the same
//! code as the OS windows the desktop gets: right-click → Float in new window, dragging a Tab
//! out, Link sets across windows, closing a window, Layouts and the Last session, a window
//! whose screen is gone, and no extra text work for the main window.

use obs_engine::{ScreenRect, WindowPlacement};

use super::*;

fn right_click(h: &mut Harness, p: Pos2) {
    h.events.push(Event::PointerMoved(p));
    h.frame();
    for pressed in [true, false] {
        h.events.push(Event::PointerButton {
            pos: p,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frames(2);
}

/// The Tab titles in each floating window, oldest window first.
fn floating(h: &Harness) -> Vec<Vec<String>> {
    let ws = &h.shell.st.session.workspace;
    h.shell
        .floating
        .iter()
        .map(|d| {
            ws.window_tabs(d.id)
                .into_iter()
                .filter_map(|t| ws.tab(t).map(|t| t.title.clone()))
                .collect()
        })
        .collect()
}

fn main_titles(h: &Harness) -> Vec<String> {
    h.panels().into_iter().flat_map(|(_, t)| t).collect()
}

fn second_screen() -> ScreenRect {
    ScreenRect {
        x: 1920.0,
        y: 0.0,
        width: 1440.0,
        height: 900.0,
    }
}

fn main_screen() -> ScreenRect {
    ScreenRect {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    }
}

#[test]
fn float_in_new_window_keeps_a_bible_following_its_link_set() {
    let (mut h, tb, hn) = linked_harness();
    // Right-click the HNUM Tab: the menu floats it.
    let title = h.tab_at("HNUM");
    right_click(&mut h, title);
    h.click_text("Float in new window");
    h.frames(3);
    assert_eq!(floating(&h), [vec!["HNUM".to_string()]]);
    assert!(
        !main_titles(&h).contains(&"HNUM".into()),
        "{:?}",
        h.panels()
    );
    assert!(h.shell.st.session.command_log().contains(&"tab.float"));
    // Its window is titled after it (an egui window here, an OS window on the desktop).
    assert!(
        h.find("HNUM — Open Bible Software").is_some(),
        "{:?}",
        h.texts()
    );
    // TB jumps in the main window; the floating HNUM follows in its own numbering.
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:55" }));
    let mut frames = 0;
    while !(location(&mut h, hn) == Some(ge(32, 1)) && settled(&h, hn)) {
        h.paced_frame();
        frames += 1;
        assert!(frames <= 3, "HNUM at {:?}", location(&mut h, hn));
    }
    assert!(h.find("Genesis 32:1").is_some(), "{:?}", h.texts());
    // And the other way: HNUM jumps in its window, TB follows in the main one.
    h.shell
        .run("tab.go", &json!({ "tab": hn.0, "text": "33:3" }));
    h.frames(3);
    assert_eq!(location(&mut h, tb), Some(ge(33, 3)));
    // Move to main window brings it back as a Panel of its own.
    // Its Tab is the first "HNUM" below the window's title.
    let window = h.find("HNUM — Open Bible Software").unwrap();
    let title = h
        .shapes
        .iter()
        .filter(|(r, t)| t == "HNUM" && r.top() > window.bottom())
        .map(|(r, _)| *r)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap()
        .center();
    right_click(&mut h, title);
    h.click_text("Move to main window");
    h.frames(2);
    assert!(h.shell.floating.is_empty());
    assert!(main_titles(&h).contains(&"HNUM".into()));
}

#[test]
fn dragging_a_tab_out_of_the_main_window_floats_it() {
    let mut h = Harness::new();
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    h.frames(3);
    assert_eq!(h.panels()[0].1, ["TB", "HEB"]);
    let from = h.tab_at("HEB");
    // Past the window's right edge (1400 points wide).
    h.drag(from, pos2(1500.0, 300.0));
    assert_eq!(floating(&h), [vec!["HEB".to_string()]]);
    assert_eq!(h.panels()[0].1, ["TB"]);
    let heb = h.tab_id("HEB");
    let ws = &h.shell.st.session.workspace;
    assert!(ws.window_of(heb).is_some());
    assert_eq!(ws.focused(), Some(heb));
    // A drag that ends inside the window still docks as before.
    let panel = h
        .shell
        .st
        .session
        .workspace
        .panel_of(h.tab_id("TB"))
        .unwrap();
    h.shell.run(
        "resource.open",
        &json!({ "id": "heb-num", "panel": panel.0 }),
    );
    h.frames(3);
    let (left, _) = h.panels()[0].clone();
    let from = h.tab_at("HNUM");
    h.drag(from, pos2(left.center().x, left.bottom() - 30.0));
    assert_eq!(floating(&h).len(), 1);
    assert!(main_titles(&h).contains(&"HNUM".into()));
}

#[test]
fn closing_a_floating_window_docks_its_tabs_back_in_the_main_window() {
    let mut h = Harness::new();
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    let heb = h.tab_id("HEB");
    h.shell.run("tab.float", &json!({ "tab": heb.0 }));
    // A second Tab opened from the floating one joins its Panel there.
    let panel = h.shell.st.session.workspace.panel_of(heb).unwrap();
    h.shell.run(
        "resource.open",
        &json!({ "id": "heb-num", "panel": panel.0 }),
    );
    h.frames(3);
    assert_eq!(floating(&h), [vec!["HEB".to_string(), "HNUM".to_string()]]);
    // The user closes the window: its Panel comes back whole, at the right.
    h.shell.floating[0].close = true;
    h.frames(2);
    assert!(h.shell.floating.is_empty());
    assert!(h.shell.st.session.workspace.floating.is_empty());
    let panels = h.panels();
    assert_eq!(panels.last().unwrap().1, ["HEB", "HNUM"], "{panels:?}");
    let ws = &h.shell.st.session.workspace;
    assert_eq!(ws.panel_of(heb), Some(panel), "the same Panel");
    assert!(ws.tab(heb).is_some(), "nothing was closed");
}

#[test]
fn a_layout_and_the_last_session_restore_the_floating_window() {
    let db = temp_db("floating-layout");
    let mut h = Harness::launch(Some(&db));
    let tb = h.tab_id("TB");
    h.shell.st.session.workspace.user_focused(tb);
    h.shell
        .run("resource.open", &json!({ "id": "heb-num", "split": true }));
    h.frames(3);
    let hn = h.tab_id("HNUM");
    let at = WindowPlacement {
        x: 2000.0,
        y: 80.0,
        width: 700.0,
        height: 800.0,
        screen: Some(second_screen()),
    };
    h.shell.st.session.workspace.float(hn, Some(at));
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:20" }));
    h.frames(3);
    h.shell
        .run("layout.save", &json!({ "name": "Two screens" }));
    let saved = workspace_state(&mut h);
    let saved_windows = floating(&h);
    // Change everything, then load the Layout.
    h.shell.run("layout.reset", &json!({}));
    h.frames(3);
    assert!(h.shell.floating.is_empty());
    h.shell
        .run("layout.restore", &json!({ "name": "Two screens" }));
    h.frames(3);
    assert_eq!(floating(&h), saved_windows);
    assert_eq!(workspace_state(&mut h), saved);
    assert_eq!(h.shell.floating[0].placement, Some(at), "where it was");
    assert_eq!(loc_of(&mut h, "HNUM"), Some(ge(31, 20)));
    // Quit and relaunch: the Last session brings the window back too.
    h.shell.save_session(true);
    drop(h);
    let mut h = Harness::launch(Some(&db));
    h.frames(3);
    assert_eq!(floating(&h), saved_windows);
    assert_eq!(h.shell.floating[0].placement, Some(at));
    // Its Link set still works across the windows.
    h.shell.run(
        "tab.go",
        &json!({ "tab": h.tab_id("TB").0, "text": "Gen 33:2" }),
    );
    h.frames(4);
    assert_eq!(loc_of(&mut h, "HNUM"), Some(ge(33, 2)));
}

#[test]
fn a_floating_window_whose_screen_is_gone_is_moved_on_screen_at_restore() {
    let db = temp_db("floating-screen");
    let mut h = Harness::launch(Some(&db));
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    let heb = h.tab_id("HEB");
    let at = WindowPlacement {
        x: 2000.0,
        y: 80.0,
        width: 700.0,
        height: 800.0,
        screen: Some(second_screen()),
    };
    h.shell.st.session.workspace.float(heb, Some(at));
    h.frames(2);
    h.shell.save_session(true);
    drop(h);
    // Relaunch with only the main screen connected.
    let mut session = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(&db).unwrap();
    session.attach_user(Box::new(store));
    let mut shell = Shell::new(session);
    add_doc(&mut shell, crate::tests::bible());
    add_doc(&mut shell, crate::tests::hebrew_doc());
    shell.restore_or_default();
    assert!(
        shell.wants_screens(),
        "a floating window asks for the screens"
    );
    shell.set_screens(vec![main_screen()]);
    let h = Harness::from_shell(shell);
    let placed = h.shell.floating[0].placement.unwrap();
    assert_eq!((placed.x, placed.y), (80.0, 80.0), "{placed:?}");
    assert_eq!(placed.screen, Some(main_screen()));
    assert_eq!(floating(&h), [vec!["HEB".to_string()]]);
}

#[test]
fn a_floating_window_adds_no_text_work_to_the_main_window() {
    let (mut h, _tb, hn) = linked_harness();
    settle_all(&mut h);
    // With no floating window, idle frames lay out and rasterize nothing.
    let before = text_work(&h);
    h.frames(30);
    assert_eq!(text_work(&h), before);
    h.shell.run("tab.float", &json!({ "tab": hn.0 }));
    settle_all(&mut h);
    // Floated: still nothing per idle frame, in either window.
    let before = text_work(&h);
    h.frames(30);
    assert_eq!(text_work(&h), before);
    assert_eq!(h.shell.floating.len(), 1);
}

/// Writes `target/previews/floating-window-kittest.ppm` when `UPDATE_PREVIEWS` is set: HNUM
/// floated out of the main window (an egui window here, an OS window on the desktop) and
/// following TB, its Link set, to KJV Gen 31:55 = its own 32:1.
#[test]
fn previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
    let (mut h, tb, hn) = linked_harness();
    h.shell.run("tab.float", &json!({ "tab": hn.0 }));
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:55" }));
    settle_all(&mut h);
    h.frames(3);
    h.screenshot(&out.join("floating-window-kittest.ppm"));
}
