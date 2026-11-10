//! The movable, customizable toolbar (#141) through the headless harness: each position from
//! the View menu (mouse and keyboard alone), the grip and the right-click menu, Auto-hide,
//! Customize toolbar…, hidden Launchers through their shortcut and the Command box, a small
//! window's » button, every position after a restart, and a side Toolbar's pop-out control
//! (Expanded and Collapsed, #291).

use egui::Key;
use obs_engine::{Launcher, ToolbarPosition};

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

fn position(h: &Harness) -> ToolbarPosition {
    h.shell.st.session.toolbar.position
}

fn bar(h: &Harness) -> Rect {
    h.shell.st.toolbar.bar.expect("the toolbar is drawn")
}

fn button(h: &Harness, which: Launcher) -> Rect {
    h.shell.st.toolbar.buttons[&which]
}

/// Texts painted on the toolbar.
fn bar_texts(h: &Harness) -> Vec<&str> {
    let bar = bar(h);
    (h.shapes.iter())
        .filter(|(r, _)| bar.contains(r.center()))
        .map(|(_, t)| t.as_str())
        .collect()
}

/// The topmost painted `text` (menu items are drawn over the Workspace).
fn topmost(h: &Harness, text: &str) -> Pos2 {
    h.shapes
        .iter()
        .rev()
        .find(|(_, t)| t == text)
        .map(|(r, _)| r.center())
        .unwrap_or_else(|| panic!("{text:?} not on screen: {:?}", h.texts()))
}

/// View → Toolbar position → `label`, with the mouse.
fn view_position(h: &mut Harness, label: &str) {
    h.open_menu("View");
    h.click_text("Toolbar position");
    let p = topmost(h, label);
    h.click(p);
    h.frames(2);
}

/// Every Panel's area, as egui_tiles drew it.
fn workspace(h: &Harness) -> Rect {
    (h.panels().into_iter())
        .map(|(r, _)| r)
        .reduce(|a, b| a.union(b))
        .expect("Panels")
}

/// The toolbar sits along `edge`, the Launchers in order along it, and the Workspace beside it.
fn assert_at(h: &Harness, edge: ToolbarPosition) {
    let screen = Rect::from_min_size(Pos2::ZERO, h.screen);
    let bar = bar(h);
    let buttons: Vec<Rect> = Launcher::ALL.iter().map(|w| button(h, *w)).collect();
    let ws = workspace(h);
    let menus = h.shell.st.toolbar.menus_bottom;
    match edge {
        ToolbarPosition::Top => {
            assert!(bar.bottom() <= menus, "in the menu bar: {bar:?}");
            assert!(ws.top() >= menus);
        }
        ToolbarPosition::Bottom => {
            assert!(bar.bottom() >= screen.bottom() - 10.0, "{bar:?}");
            assert!(ws.bottom() <= bar.top(), "{ws:?} {bar:?}");
        }
        ToolbarPosition::Left => {
            assert!(bar.left() <= 10.0 && bar.top() >= menus, "{bar:?}");
            assert!(ws.left() >= bar.right(), "{ws:?} {bar:?}");
        }
        ToolbarPosition::Right => {
            assert!(bar.right() >= screen.right() - 10.0, "{bar:?}");
            assert!(ws.right() <= bar.left(), "{ws:?} {bar:?}");
        }
    }
    for pair in buttons.windows(2) {
        if edge.is_vertical() {
            assert!(pair[0].bottom() <= pair[1].top(), "stacked: {buttons:?}");
        } else {
            assert!(pair[0].right() <= pair[1].left(), "in a row: {buttons:?}");
        }
    }
    // Vertical: icons only; the labels are in the tooltips.
    let labelled = bar_texts(h).contains(&"Library");
    assert_eq!(labelled, !edge.is_vertical(), "{:?}", bar_texts(h));
}

/// Alt+L drops the Library pane down from its button, on the side away from the edge.
fn assert_pane_beside_button(h: &mut Harness, edge: ToolbarPosition) {
    let name = format!("toolbar-{}", edge.id());
    assert_pane_beside(
        h,
        edge,
        (edge != ToolbarPosition::Top).then_some(&[name.as_str()][..]),
    );
}

/// Alt+L drops the Library pane down from its button, on the side away from the edge; with
/// `previews`, its image is written under each name, in Light then Dark when two are given.
fn assert_pane_beside(h: &mut Harness, edge: ToolbarPosition, previews: Option<&[&str]>) {
    h.events.push(Event::PointerMoved(pos2(700.0, 500.0)));
    key(h, Key::L, Modifiers::ALT);
    h.frames(2);
    assert_eq!(h.shell.st.toolbar.open, Some(Launcher::Library));
    let pane = h.shell.st.toolbar.pane.expect("the pane");
    let b = button(h, Launcher::Library);
    match edge {
        ToolbarPosition::Top => assert!(pane.top() >= b.bottom(), "{pane:?} {b:?}"),
        ToolbarPosition::Bottom => assert!(pane.bottom() <= b.top(), "{pane:?} {b:?}"),
        ToolbarPosition::Left => assert!(pane.left() >= b.right(), "{pane:?} {b:?}"),
        ToolbarPosition::Right => assert!(pane.right() <= b.left(), "{pane:?} {b:?}"),
    }
    let screen = Rect::from_min_size(Pos2::ZERO, h.screen);
    assert!(screen.contains_rect(pane), "{pane:?}");
    for (i, name) in previews.unwrap_or_default().iter().enumerate() {
        let theme = if i == 0 { "light" } else { "dark" };
        if previews.is_some_and(|p| p.len() > 1) {
            h.shell.run("view.theme", &json!({ "value": theme }));
            h.frames(3);
        }
        preview(h, name);
    }
    if previews.is_some_and(|p| p.len() > 1) {
        h.shell.run("view.theme", &json!({ "value": "system" }));
        h.frames(2);
    }
    key(h, Key::Escape, Modifiers::NONE);
    assert_eq!(h.shell.st.toolbar.open, None);
}

#[test]
fn every_position_works_and_survives_a_restart() {
    let db = temp_db("toolbar-positions");
    let mut h = Harness::launch(Some(&db));
    assert_eq!(position(&h), ToolbarPosition::Top);
    assert_at(&h, ToolbarPosition::Top);
    for (edge, label) in [
        (ToolbarPosition::Bottom, "Bottom"),
        (ToolbarPosition::Left, "Left"),
        (ToolbarPosition::Right, "Right"),
        (ToolbarPosition::Top, "Top"),
    ] {
        view_position(&mut h, label);
        assert_eq!(position(&h), edge);
        assert_at(&h, edge);
        assert_pane_beside_button(&mut h, edge);
        // After a restart.
        drop(h);
        h = Harness::launch(Some(&db));
        assert_eq!(position(&h), edge, "{label} after a restart");
        assert_at(&h, edge);
    }
    drop(h);
    std::fs::remove_dir_all(db.parent().unwrap()).unwrap();
}

/// The focused widget's accessible name.
fn focused(h: &Harness) -> Option<String> {
    let tree = h.tree.as_ref()?;
    let focus = tree.focus;
    (tree.nodes.iter())
        .find(|(id, _)| *id == focus)
        .and_then(|(_, n)| n.label().map(str::to_string))
}

/// Tab until the widget named `name` has the keyboard.
fn tab_to(h: &mut Harness, name: &str) {
    for _ in 0..80 {
        if focused(h).as_deref() == Some(name) {
            return;
        }
        key(h, Key::Tab, Modifiers::NONE);
    }
    panic!("Tab never reached {name:?} (last: {:?})", focused(h));
}

#[test]
fn keyboard_users_change_the_position_from_the_view_menu() {
    let mut h = Harness::new();
    h.ctx.enable_accesskit();
    h.frames(2);
    tab_to(&mut h, "View");
    key(&mut h, Key::Enter, Modifiers::NONE);
    tab_to(&mut h, "Toolbar position");
    key(&mut h, Key::Enter, Modifiers::NONE);
    tab_to(&mut h, "Left");
    preview(&h, "toolbar-view-menu");
    key(&mut h, Key::Enter, Modifiers::NONE);
    h.frames(2);
    assert_eq!(position(&h), ToolbarPosition::Left);
    assert_at(&h, ToolbarPosition::Left);
    // And back, the same way.
    tab_to(&mut h, "View");
    key(&mut h, Key::Space, Modifiers::NONE);
    tab_to(&mut h, "Toolbar position");
    key(&mut h, Key::Space, Modifiers::NONE);
    tab_to(&mut h, "Top");
    key(&mut h, Key::Space, Modifiers::NONE);
    h.frames(2);
    assert_eq!(position(&h), ToolbarPosition::Top);
    assert_at(&h, ToolbarPosition::Top);
}

#[test]
fn right_click_moves_the_toolbar_and_the_grip_drags_it_to_an_edge() {
    let mut h = Harness::new();
    let tools = button(&h, Launcher::Tools).center();
    right_click(&mut h, tools);
    h.click_text("Position");
    let p = topmost(&h, "Bottom");
    h.click(p);
    h.frames(2);
    assert_eq!(position(&h), ToolbarPosition::Bottom);
    assert_at(&h, ToolbarPosition::Bottom);

    // Drag the grip (the toolbar's start) towards the right edge: the edge is marked while
    // dragging, and letting go moves the toolbar there.
    let grip = bar(&h).left_center() + vec2(5.0, 0.0);
    let to = pos2(h.screen.x - 20.0, 450.0);
    h.events.push(Event::PointerMoved(grip));
    h.frame();
    h.button(grip, true);
    h.frame();
    for i in 1..=10 {
        let t = i as f32 / 10.0;
        h.events.push(Event::PointerMoved(grip + (to - grip) * t));
        h.frame();
    }
    assert_eq!(h.shell.st.toolbar.grip_target, Some(ToolbarPosition::Right));
    let marked = (h.rects.iter())
        .any(|(r, _)| r.right() >= h.screen.x - 1.0 && r.height() > 600.0 && r.width() < 60.0);
    assert!(marked, "the right edge is marked");
    assert_eq!(
        position(&h),
        ToolbarPosition::Bottom,
        "not moved before letting go"
    );
    h.button(to, false);
    h.frames(3);
    assert_eq!(position(&h), ToolbarPosition::Right);
    assert_eq!(h.shell.st.toolbar.grip_target, None);
    assert_at(&h, ToolbarPosition::Right);

    // The right-click menu on the docked toolbar's background too.
    let empty = pos2(bar(&h).center().x, bar(&h).bottom() - 20.0);
    right_click(&mut h, empty);
    h.click_text("Position");
    let p = topmost(&h, "Top");
    h.click(p);
    h.frames(2);
    assert_eq!(position(&h), ToolbarPosition::Top);
}

#[test]
fn auto_hide_shows_the_toolbar_at_its_edge_or_while_alt_is_held() {
    let mut h = Harness::new();
    h.shell
        .run("view.toolbar_position", &json!({ "value": "left" }));
    h.frames(2);
    let docked = workspace(&h);
    h.open_menu("View");
    h.click_text("Auto-hide toolbar");
    let middle = pos2(700.0, 450.0);
    h.events.push(Event::PointerMoved(middle));
    h.frames(3);
    assert!(h.shell.st.session.toolbar.auto_hide);
    assert_eq!(h.shell.st.toolbar.bar, None, "hidden");
    // The Workspace takes the toolbar's room.
    assert!(workspace(&h).left() < docked.left(), "{:?}", workspace(&h));

    // The pointer at the left edge: it shows, over the Workspace, and stays while the
    // pointer is on it.
    h.events.push(Event::PointerMoved(pos2(1.0, 450.0)));
    h.frames(2);
    let shown = bar(&h);
    assert!(shown.left() <= 10.0);
    let on = button(&h, Launcher::Guides).center();
    h.events.push(Event::PointerMoved(on));
    h.frames(2);
    assert!(h.shell.st.toolbar.bar.is_some(), "stays while pointed at");
    h.click(on);
    assert_eq!(h.shell.st.toolbar.open, Some(Launcher::Guides));
    key(&mut h, Key::Escape, Modifiers::NONE);
    h.events.push(Event::PointerMoved(middle));
    h.frames(3);
    assert_eq!(h.shell.st.toolbar.bar, None, "hidden again");

    // Alt held shows it.
    h.modifiers = Modifiers::ALT;
    h.frames(2);
    assert!(h.shell.st.toolbar.bar.is_some(), "shown while Alt is held");
    h.modifiers = Modifiers::NONE;
    h.frames(2);
    assert_eq!(h.shell.st.toolbar.bar, None);

    // Alt+T: the pane opens by its button, the toolbar showing while it is open.
    key(&mut h, Key::T, Modifiers::ALT);
    assert_eq!(h.shell.st.toolbar.open, Some(Launcher::Tools));
    let pane = h.shell.st.toolbar.pane.unwrap();
    assert!(pane.left() >= button(&h, Launcher::Tools).right());
    key(&mut h, Key::Escape, Modifiers::NONE);

    // Auto-hide at the Top: the Launchers leave the menu bar and show under it.
    h.shell
        .run("view.toolbar_position", &json!({ "value": "top" }));
    h.events.push(Event::PointerMoved(middle));
    h.frames(3);
    assert_eq!(h.shell.st.toolbar.bar, None);
    assert!(h.find("View").is_some(), "the menu bar stays");
    h.events.push(Event::PointerMoved(pos2(700.0, 1.0)));
    h.frames(2);
    assert!(bar(&h).top() >= h.shell.st.toolbar.menus_bottom - 1.0);
}

#[test]
fn customize_hides_reorders_and_picks_icons_or_labels() {
    let mut h = Harness::new();
    let library_width = button(&h, Launcher::Library).width();
    let library = button(&h, Launcher::Library).center();
    right_click(&mut h, library);
    h.click_text("Customize…");
    assert!(h.shell.st.toolbar.customizing);
    assert!(h.find("Customize toolbar").is_some());
    h.events.push(Event::PointerMoved(pos2(1300.0, 800.0)));
    h.frames(20);
    preview(&h, "toolbar-customize");

    // Hide Guides.
    h.click_text("Guides");
    assert!(!h.shell.st.session.toolbar.is_shown(Launcher::Guides));
    assert!(!h.shell.st.toolbar.buttons.contains_key(&Launcher::Guides));
    // Layouts first: Move up on its row three times.
    for _ in 0..3 {
        let layouts = h.at("Layouts");
        let ups: Vec<Rect> = (h.shapes.iter())
            .filter(|(r, t)| t == "Move up" && (r.center().y - layouts.y).abs() < 8.0)
            .map(|(r, _)| *r)
            .collect();
        h.click(ups[0].center());
    }
    assert_eq!(
        h.shell.st.session.toolbar.shown(),
        [Launcher::Layouts, Launcher::Library, Launcher::Tools]
    );
    let order: Vec<f32> = [Launcher::Layouts, Launcher::Library, Launcher::Tools]
        .iter()
        .map(|w| button(&h, *w).left())
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");

    // Icons only: narrow buttons, no labels on the toolbar.
    h.click_text("Icons only");
    assert!(button(&h, Launcher::Library).width() < library_width - 30.0);
    assert!(!bar_texts(&h).contains(&"Library"), "{:?}", bar_texts(&h));
    // Labels only.
    h.click_text("Labels only");
    assert!(bar_texts(&h).contains(&"Library"));
    assert!(button(&h, Launcher::Library).width() < library_width);
    h.click_text("Done");
    assert!(!h.shell.st.toolbar.customizing);
    assert!(h.find("Customize toolbar").is_none());

    // Reset brings everything back; View → Customize toolbar… opens the window too.
    h.open_menu("View");
    h.click_text("Customize toolbar…");
    h.click_text("Reset");
    assert_eq!(
        h.shell.st.session.toolbar,
        obs_engine::ToolbarSettings::default()
    );
}

#[test]
fn hidden_launchers_stay_reachable_by_shortcut_and_the_command_box() {
    let mut h = Harness::new();
    for which in [Launcher::Guides, Launcher::Layouts] {
        h.shell.run(
            "toolbar.show_launcher",
            &json!({ "launcher": which.id(), "shown": false }),
        );
    }
    h.frames(2);
    assert!(!h.shell.st.toolbar.buttons.contains_key(&Launcher::Guides));
    assert!(h.find("Guides").is_none(), "{:?}", h.texts());

    // Its shortcut drops it down from the toolbar's start.
    key(&mut h, Key::G, Modifiers::ALT);
    assert_eq!(h.shell.st.toolbar.open, Some(Launcher::Guides));
    let pane = h.shell.st.toolbar.pane.unwrap();
    assert!(pane.top() >= bar(&h).bottom() && pane.left() <= bar(&h).left() + 1.0);
    key(&mut h, Key::Escape, Modifiers::NONE);

    // The Command box finds it by name.
    key(&mut h, Key::L, CTRL);
    h.events.push(Event::Text("Layouts launcher".into()));
    h.frames(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.shell.st.toolbar.open, Some(Launcher::Layouts));
    assert!(h.shell.st.toolbar.pane.is_some());
}

#[test]
fn a_small_window_puts_the_launchers_it_has_no_room_for_in_a_more_button() {
    let mut h = Harness::new();
    h.screen = vec2(1100.0, 600.0);
    h.frames(4);
    let tb = &h.shell.st.toolbar;
    let more = tb.overflow.expect("a » button");
    // The Launchers come after the menus (#226).
    let help = h.find("Help").unwrap();
    assert!(more.left() >= help.right(), "no overlap: {more:?} {help:?}");
    let inside: Vec<Launcher> = (Launcher::ALL.into_iter())
        .filter(|w| tb.buttons[w] != more)
        .collect();
    let rest: Vec<Launcher> = (Launcher::ALL.into_iter())
        .filter(|w| tb.buttons[w] == more)
        .collect();
    assert!(
        !inside.is_empty() && !rest.is_empty(),
        "{inside:?} {rest:?}"
    );
    preview(&h, "toolbar-overflow");
    // » lists the rest; choosing one drops its pane down from ».
    h.click(more.center());
    let last = *rest.last().unwrap();
    let p = topmost(&h, last.title());
    h.click(p);
    assert_eq!(h.shell.st.toolbar.open, Some(last));
    let pane = h.shell.st.toolbar.pane.unwrap();
    assert!(pane.top() >= more.bottom());
    key(&mut h, Key::Escape, Modifiers::NONE);

    // A short window with the toolbar on the left: the » button at the bottom of the column.
    h.screen = vec2(1000.0, 170.0);
    h.shell
        .run("view.toolbar_position", &json!({ "value": "left" }));
    h.frames(4);
    let tb = &h.shell.st.toolbar;
    let more = tb.overflow.expect("a » button");
    assert!(more.bottom() <= h.screen.y);
    assert!(Launcher::ALL.iter().any(|w| tb.buttons[w] == more));
    // Room again: no » button.
    h.screen = vec2(1400.0, 900.0);
    h.frames(3);
    assert_eq!(h.shell.st.toolbar.overflow, None);
}

// ---------------------------------------------------------------------------------------------
// The pop-out control: Expanded and Collapsed side Toolbars (#291).

fn expanded(h: &Harness) -> bool {
    h.shell.st.session.toolbar.expanded
}

fn pop_out(h: &Harness) -> Rect {
    h.shell.st.toolbar.pop_out.expect("the pop-out control")
}

/// The text painted inside the Library's row on the toolbar.
fn library_row_texts(h: &Harness) -> Vec<&str> {
    let row = button(h, Launcher::Library);
    (h.shapes.iter())
        .filter(|(r, _)| row.contains(r.center()))
        .map(|(_, t)| t.as_str())
        .collect()
}

/// The toolbar is Expanded at `edge`: wider than Collapsed, each Launcher's row showing its
/// label and detail line in full, the Workspace beside the wider column, and the Library pane
/// opening beside it.
fn assert_expanded(h: &mut Harness, edge: ToolbarPosition, collapsed: Rect) {
    let bar = bar(h);
    assert!(
        bar.width() > collapsed.width() + 40.0,
        "wider: {bar:?} {collapsed:?}"
    );
    let texts = bar_texts(h);
    for which in Launcher::ALL {
        assert!(
            texts.contains(&which.title()),
            "{}: {texts:?}",
            which.title()
        );
        let key = crate::key_labels::label(h.shell.st.session.shortcut(which.command()).unwrap());
        assert!(texts.contains(&key.as_str()), "{key}: {texts:?}");
    }
    assert_uncut(h);
    assert!(texts.contains(&"Collapse"), "{texts:?}");
    assert_at_wide(h, edge);
    let light = format!("toolbar-{}-expanded-light", edge.id());
    let dark = format!("toolbar-{}-expanded-dark", edge.id());
    assert_pane_beside(h, edge, Some(&[light.as_str(), dark.as_str()]));
}

/// How wide a Collapsed side Toolbar's buttons are.
fn look_collapsed() -> f32 {
    crate::shell::toolbar_look::grip_size(true).x
}

/// The column is narrower than its maximum, so no row's text is cut short with "…".
fn assert_uncut(h: &Harness) {
    use crate::shell::toolbar_look as look;
    let most = look::bar_thickness_around(look::expanded_width(crate::tokens::size::FIELD));
    assert!(bar(h).width() < most, "{:?} {most}", bar(h));
}

/// Like [`assert_at`] for an Expanded side Toolbar: at its edge, stacked, the Workspace beside.
fn assert_at_wide(h: &Harness, edge: ToolbarPosition) {
    let bar = bar(h);
    let ws = workspace(h);
    match edge {
        ToolbarPosition::Left => assert!(ws.left() >= bar.right(), "{ws:?} {bar:?}"),
        ToolbarPosition::Right => assert!(ws.right() <= bar.left(), "{ws:?} {bar:?}"),
        _ => unreachable!(),
    }
    let rows: Vec<Rect> = Launcher::ALL.iter().map(|w| button(h, *w)).collect();
    for pair in rows.windows(2) {
        assert!(pair[0].bottom() <= pair[1].top(), "stacked: {rows:?}");
    }
    assert!(pop_out(h).top() >= rows[3].bottom(), "at the toolbar's end");
}

/// Acceptance: at the Left and Right one click on the pop-out control expands the toolbar to
/// icons, labels and detail lines, pushing the Workspace over, and one click collapses it;
/// the choice survives a restart; Top and Bottom have no control.
#[test]
fn one_click_on_the_pop_out_control_expands_and_collapses_a_side_toolbar() {
    let db = temp_db("toolbar-expanded");
    let mut h = Harness::launch(Some(&db));
    assert_eq!(h.shell.st.toolbar.pop_out, None, "not at the Top");
    for (edge, label) in [
        (ToolbarPosition::Left, "Left"),
        (ToolbarPosition::Right, "Right"),
    ] {
        view_position(&mut h, label);
        assert!(!expanded(&h));
        let collapsed = bar(&h);
        let narrow = workspace(&h);
        let control = pop_out(&h);
        assert!(bar(&h).contains_rect(control), "{control:?}");
        h.click(control.center());
        assert!(expanded(&h), "{label}: one click expands");
        // The control is still under the pointer: a second click there would undo it.
        assert!(pop_out(&h).contains(control.center()), "{:?}", pop_out(&h));
        assert!(
            workspace(&h).width() < narrow.width() - 40.0,
            "the Workspace is pushed over"
        );
        assert_expanded(&mut h, edge, collapsed);

        // After a restart.
        drop(h);
        h = Harness::launch(Some(&db));
        assert!(expanded(&h), "{label} after a restart");
        assert_eq!(position(&h), edge);
        assert!(library_row_texts(&h).contains(&"Library"));

        let control = pop_out(&h);
        h.click(control.center());
        assert!(!expanded(&h), "{label}: one click collapses");
        assert_at(&h, edge);
        assert!((bar(&h).width() - collapsed.width()).abs() < 1.0);
        assert!((workspace(&h).width() - narrow.width()).abs() < 1.0);
    }
    // Bottom: no control, and Expanded changes nothing there.
    h.shell.run("view.toolbar_expanded", &json!({ "on": true }));
    view_position(&mut h, "Bottom");
    assert_eq!(h.shell.st.toolbar.pop_out, None);
    assert_at(&h, ToolbarPosition::Bottom);
    drop(h);
    std::fs::remove_dir_all(db.parent().unwrap()).unwrap();
}

/// The Library's row paints its shortcut; its detail line counts updates, as the badge does
/// Collapsed; the column's width has a cap.
#[test]
fn the_expanded_library_row_paints_its_shortcut_and_its_detail_line_counts_updates() {
    let mut h = Harness::new();
    h.shell
        .run("view.toolbar_position", &json!({ "value": "left" }));
    h.shell.run("view.toolbar_expanded", &json!({ "on": true }));
    h.frames(2);
    let detail = h.shell.launcher_detail(Launcher::Library, 2);
    assert_eq!(detail, "Alt+L · 2 updates");
    assert_eq!(
        h.shell.launcher_detail(Launcher::Library, 1),
        "Alt+L · 1 update"
    );
    assert_eq!(h.shell.launcher_detail(Launcher::Guides, 0), "Alt+G");
    assert!(
        library_row_texts(&h).contains(&"Alt+L"),
        "{:?}",
        library_row_texts(&h)
    );
    // A Launcher with no key has no shortcut in its line.
    h.shell
        .run("keys.clear", &json!({ "command": "toolbar.tools" }));
    assert_eq!(h.shell.launcher_detail(Launcher::Tools, 0), "");
    // The column's width has a cap, whatever the text.
    use crate::shell::toolbar_look as look;
    assert_eq!(
        look::expanded_width(10_000.0),
        look::expanded_width(crate::tokens::size::FIELD)
    );
}

/// Acceptance: Expanded and Collapsed are reachable from the toolbar's right-click menu,
/// Customize toolbar…, the Command box and the shortcut (Ctrl+Shift+B).
#[test]
fn right_click_customize_the_command_box_and_the_shortcut_expand_and_collapse_it() {
    let mut h = Harness::new();
    h.shell
        .run("view.toolbar_position", &json!({ "value": "right" }));
    h.frames(2);
    // Right-click: Expanded, then Collapsed.
    for (item, on) in [("Expanded", true), ("Collapsed", false)] {
        let at = button(&h, Launcher::Guides).center();
        right_click(&mut h, at);
        let p = topmost(&h, item);
        h.click(p);
        assert_eq!(expanded(&h), on, "right-click {item}");
    }
    // Not offered at the Top.
    h.shell
        .run("view.toolbar_position", &json!({ "value": "top" }));
    h.frames(2);
    let at = button(&h, Launcher::Guides).center();
    right_click(&mut h, at);
    assert!(h.find("Expanded").is_none() && h.find("Collapsed").is_none());
    key(&mut h, Key::Escape, Modifiers::NONE);

    // Customize toolbar…: its two choices.
    h.shell
        .run("view.toolbar_position", &json!({ "value": "left" }));
    h.shell.run("view.toolbar_customize", &json!({}));
    h.frames(3);
    h.click_text("Expanded");
    assert!(expanded(&h), "Customize: Expanded");
    h.click_text("Collapsed");
    assert!(!expanded(&h), "Customize: Collapsed");
    h.click_text("Done");

    // The Command box: its label.
    key(&mut h, Key::L, CTRL);
    h.events.push(Event::Text("Expanded toolbar".into()));
    h.frames(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(expanded(&h), "the Command box");

    // Its shortcut flips it back.
    key(&mut h, Key::B, CTRL | Modifiers::SHIFT);
    assert!(!expanded(&h), "Ctrl+Shift+B");
    key(&mut h, Key::B, CTRL | Modifiers::SHIFT);
    assert!(expanded(&h), "Ctrl+Shift+B again");
}

/// Acceptance: F6 puts the keyboard on the toolbar, Tab reaches the pop-out control, whose
/// accessible name states the toolbar's state, and Enter flips it.
#[test]
fn the_keyboard_reaches_the_pop_out_control_and_hears_its_state() {
    let mut h = Harness::new();
    h.ctx.enable_accesskit();
    h.shell
        .run("view.toolbar_position", &json!({ "value": "left" }));
    h.frames(3);
    let collapsed = crate::shell::toolbar_bar::pop_out_name(false);
    let expanded_name = crate::shell::toolbar_bar::pop_out_name(true);
    assert!(collapsed.contains("collapsed") && expanded_name.contains("expanded"));
    // F6 from the menu bar to the toolbar's first Launcher.
    for _ in 0..4 {
        key(&mut h, Key::F6, Modifiers::NONE);
        if focused(&h).as_deref() == Some("Library") {
            break;
        }
    }
    assert_eq!(focused(&h).as_deref(), Some("Library"));
    tab_to(&mut h, collapsed);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(expanded(&h), "Enter expands");
    h.frames(2);
    assert_eq!(
        focused(&h).as_deref(),
        Some(expanded_name),
        "it keeps the keyboard"
    );
    let node = {
        let tree = h.tree.as_ref().unwrap();
        (tree.nodes.iter())
            .find(|(id, _)| *id == tree.focus)
            .map(|(_, n)| n.toggled())
            .unwrap()
    };
    assert_eq!(node, Some(egui::accesskit::Toggled::True));
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(!expanded(&h), "Enter collapses");
}

/// The open pane is beside an Expanded column at `edge`, never over it.
fn assert_pane_beside_column(h: &Harness, edge: ToolbarPosition) {
    let pane = h.shell.st.toolbar.pane.expect("the pane");
    let bar = bar(h);
    match edge {
        ToolbarPosition::Left => assert!(pane.left() >= bar.right(), "{pane:?} {bar:?}"),
        ToolbarPosition::Right => assert!(pane.right() <= bar.left(), "{pane:?} {bar:?}"),
        _ => unreachable!(),
    }
}

/// An Expanded column's » button and a hidden Launcher's fallback anchor are narrower than
/// the column; their panes still open beside it, at the Left and the Right.
#[test]
fn an_expanded_toolbar_opens_panes_from_more_and_for_hidden_launchers_beside_the_column() {
    let mut h = Harness::new();
    h.shell.run("view.toolbar_expanded", &json!({ "on": true }));
    for (edge, value) in [
        (ToolbarPosition::Left, "left"),
        (ToolbarPosition::Right, "right"),
    ] {
        h.shell
            .run("view.toolbar_position", &json!({ "value": value }));
        // A short window: choose the last Launcher in » from ».
        h.screen = vec2(1000.0, 230.0);
        h.frames(4);
        let tb = &h.shell.st.toolbar;
        let more = tb.overflow.expect("a » button");
        assert!(more.width() < bar(&h).width() - 40.0, "{more:?}");
        let last = (Launcher::ALL.into_iter())
            .rfind(|w| tb.buttons[w] == more)
            .expect("a Launcher in »");
        h.click(more.center());
        let p = topmost(&h, last.title());
        h.click(p);
        assert_eq!(h.shell.st.toolbar.open, Some(last), "{value}");
        assert_pane_beside_column(&h, edge);
        key(&mut h, Key::Escape, Modifiers::NONE);
        assert_eq!(h.shell.st.toolbar.open, None);

        // The Library hidden: Alt+L opens it from the toolbar's start, beside the column.
        h.screen = vec2(1400.0, 900.0);
        h.shell.run(
            "toolbar.show_launcher",
            &json!({ "launcher": Launcher::Library.id(), "shown": false }),
        );
        h.frames(3);
        assert!(!h.shell.st.toolbar.buttons.contains_key(&Launcher::Library));
        h.events.push(Event::PointerMoved(pos2(700.0, 500.0)));
        key(&mut h, Key::L, Modifiers::ALT);
        assert_eq!(h.shell.st.toolbar.open, Some(Launcher::Library), "{value}");
        assert_pane_beside_column(&h, edge);
        key(&mut h, Key::Escape, Modifiers::NONE);
        h.shell.run(
            "toolbar.show_launcher",
            &json!({ "launcher": Launcher::Library.id(), "shown": true }),
        );
    }
}

/// With every Launcher hidden, F6 still reaches a side Toolbar: its pop-out control.
#[test]
fn f6_lands_on_the_pop_out_control_when_every_launcher_is_hidden() {
    let mut h = Harness::new();
    h.ctx.enable_accesskit();
    h.shell
        .run("view.toolbar_position", &json!({ "value": "left" }));
    for which in Launcher::ALL {
        h.shell.run(
            "toolbar.show_launcher",
            &json!({ "launcher": which.id(), "shown": false }),
        );
    }
    h.frames(3);
    let name = crate::shell::toolbar_bar::pop_out_name(false);
    for _ in 0..4 {
        key(&mut h, Key::F6, Modifiers::NONE);
        if focused(&h).as_deref() == Some(name) {
            break;
        }
    }
    assert_eq!(focused(&h).as_deref(), Some(name));
}

/// Acceptance: with Auto-hide an Expanded toolbar is drawn over the Workspace at its edge;
/// a small window's » button and the pop-out control both stay in the window; a Larger
/// Interface size still fits every label.
#[test]
fn expanded_works_with_auto_hide_the_more_button_and_interface_size() {
    let mut h = Harness::new();
    h.shell
        .run("view.toolbar_position", &json!({ "value": "left" }));
    h.shell.run("view.toolbar_expanded", &json!({ "on": true }));
    h.shell.run("view.toolbar_auto_hide", &json!({}));
    let middle = pos2(700.0, 450.0);
    h.events.push(Event::PointerMoved(middle));
    h.frames(3);
    assert_eq!(h.shell.st.toolbar.bar, None, "hidden");
    let ws = workspace(&h);
    h.events.push(Event::PointerMoved(pos2(1.0, 450.0)));
    h.frames(2);
    let shown = bar(&h);
    assert!(
        shown.left() <= 10.0 && shown.width() > look_collapsed() + 40.0,
        "{shown:?}"
    );
    assert!(ws.left() < shown.right(), "drawn over the Workspace");
    assert_eq!(workspace(&h), ws, "the Workspace stays where it is");
    assert!(bar_texts(&h).contains(&"Layouts"));
    // The pop-out control works while revealed.
    let control = pop_out(&h);
    h.events.push(Event::PointerMoved(control.center()));
    h.frames(2);
    h.click(control.center());
    assert!(!expanded(&h));
    h.shell.run("view.toolbar_auto_hide", &json!({}));
    h.shell.run("view.toolbar_expanded", &json!({ "on": true }));
    h.frames(2);

    // A short window: Launchers go into », and the pop-out control stays in the window.
    h.screen = vec2(1000.0, 230.0);
    h.frames(4);
    let tb = &h.shell.st.toolbar;
    let more = tb.overflow.expect("a » button");
    assert!(Launcher::ALL.iter().any(|w| tb.buttons[w] == more));
    assert!(pop_out(&h).bottom() <= h.screen.y, "{:?}", pop_out(&h));
    assert!(more.bottom() <= pop_out(&h).top());
    preview(&h, "toolbar-left-expanded-overflow");
    h.screen = vec2(1400.0, 900.0);
    h.frames(3);
    assert_eq!(h.shell.st.toolbar.overflow, None);

    // Larger Interface size: every label and detail still in full.
    let normal = bar(&h);
    h.shell
        .run("view.interface_size", &json!({ "value": "larger" }));
    h.frames(4);
    let texts = bar_texts(&h);
    for which in Launcher::ALL {
        assert!(texts.contains(&which.title()), "{texts:?}");
    }
    assert_uncut(&h);
    assert_at_wide(&h, ToolbarPosition::Left);
    // Points stay points: the column keeps its width in points as the window zooms.
    assert!(
        (bar(&h).width() - normal.width()).abs() < 2.0,
        "{:?} {normal:?}",
        bar(&h)
    );
}

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}
