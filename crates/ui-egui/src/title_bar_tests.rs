//! macOS: the title bar merged into the menu bar (#297), through the headless harness. The
//! harness draws what the desktop draws once the app asks for it
//! ([`Shell::set_unified_title_bar`]): the menu bar leaves the traffic lights their room at
//! every Interface size (none in full screen), its empty space moves and zooms or minimizes
//! the window as the system setting says, the Toolbar's grip and the menus keep their drags
//! and clicks, and Floating windows do the same in their top tab bar. Without it (Linux,
//! Windows) the menu bar is drawn as before.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use egui::ViewportCommand;
use obs_engine::{ToolbarPosition, WindowId};

use super::floating_windows::{drag_in, native_heb};
use super::*;
use crate::tokens::{inset, size};
use crate::{SystemTitleBar, TitleBarDoubleClick};

/// The system's double-click setting, as a test sets it.
#[derive(Default)]
struct TestTitleBar(AtomicU8);

impl TestTitleBar {
    fn set(&self, action: TitleBarDoubleClick) {
        let code = match action {
            TitleBarDoubleClick::Zoom => 0,
            TitleBarDoubleClick::Minimize => 1,
            TitleBarDoubleClick::Nothing => 2,
        };
        self.0.store(code, Ordering::Relaxed);
    }
}

impl SystemTitleBar for TestTitleBar {
    fn double_click(&self) -> TitleBarDoubleClick {
        match self.0.load(Ordering::Relaxed) {
            1 => TitleBarDoubleClick::Minimize,
            2 => TitleBarDoubleClick::Nothing,
            _ => TitleBarDoubleClick::Zoom,
        }
    }
}

/// The app as the macOS desktop has it: every title bar is the app's own top row.
fn mac() -> (Harness, Arc<TestTitleBar>) {
    let mut h = Harness::new();
    let system = Arc::new(TestTitleBar::default());
    h.shell.set_unified_title_bar(system.clone());
    h.frames(3);
    (h, system)
}

fn menus(h: &Harness) -> Rect {
    h.shell.st.areas.menus.expect("the menu bar is drawn").1
}

fn command_box(h: &Harness) -> Rect {
    let id = egui::Id::unique(crate::shell::command_box::BOX_ID);
    h.ctx.read_response(id).expect("the Command box").rect
}

fn zoom(h: &Harness) -> f32 {
    h.ctx.zoom_factor()
}

/// The traffic lights' room in the window's points at its zoom.
fn lights(h: &Harness) -> Rect {
    let z = zoom(h);
    Rect::from_min_max(
        Pos2::ZERO,
        pos2(size::TRAFFIC_LIGHTS / z, size::TITLE_BAR / z),
    )
}

/// Whether `a` and `b` share more than an edge.
fn overlap(a: Rect, b: Rect) -> bool {
    let both = a.intersect(b);
    both.width() > 0.5 && both.height() > 0.5
}

/// Everything painted in the menu bar (texts and icons) that overlaps `room`.
fn painted_in(h: &Harness, room: Rect) -> Vec<String> {
    let bar = menus(h);
    (h.shapes.iter())
        .filter(|(r, _)| bar.contains(r.center()) && overlap(*r, room))
        .map(|(_, t)| t.clone())
        .collect()
}

fn press_drag(h: &mut Harness, from: Pos2, to: Pos2) {
    h.commands.clear();
    h.drag(from, to);
}

fn double_click(h: &mut Harness, p: Pos2) {
    h.commands.clear();
    // Long after any earlier click, so it is not counted as part of one.
    h.time += 1.0;
    h.events.push(Event::PointerMoved(p));
    h.frame();
    for _ in 0..2 {
        h.button(p, true);
        h.frame();
        h.button(p, false);
        h.frame();
    }
    h.frames(2);
}

fn asked(h: &Harness, c: &ViewportCommand) -> bool {
    h.commands.contains(c)
}

#[test]
fn the_menu_bar_leaves_the_traffic_lights_room_at_every_interface_size() {
    let (mut h, _) = mac();
    for (id, z) in [("smaller", 0.9), ("default", 1.0), ("larger", 1.25)] {
        h.shell.run("view.interface_size", &json!({ "value": id }));
        h.frames(3);
        assert_eq!(zoom(&h), z);
        let room = lights(&h);
        assert!(
            painted_in(&h, room).is_empty(),
            "{id}: {:?} under the traffic lights",
            painted_in(&h, room)
        );
        // The app's name, then the menus, start just after them.
        let name = h.find("Open Bible").expect("the app's name");
        assert!(
            name.left() >= room.right() - 0.5,
            "{id}: {name:?} vs {room:?}"
        );
        assert!(name.left() < room.right() + 12.0, "{id}: {name:?}");
        // The menu bar is the title bar: at the window's top, as tall as the system's (or its
        // controls), with its menus centred on the traffic lights when they fit.
        let bar = menus(&h);
        assert_eq!(bar.top(), 0.0, "{id}");
        let controls = h.shell.menu_row_height;
        assert!(controls >= size::CONTROL, "{id}: {controls}");
        let tall = (size::TITLE_BAR / z).max(controls);
        // Within pixel rounding at the zoom.
        assert!(
            (bar.height() - tall).abs() <= 1.5,
            "{id}: {bar:?}, controls {controls}"
        );
        let file = h.at("File");
        let centre = (size::TITLE_BAR / z / 2.0).max(controls / 2.0);
        assert!((file.y - centre).abs() < 1.5, "{id}: File at {file:?}");
        // The Command box (and Home, Today's reading) are still on the right, in the row.
        let commands = command_box(&h);
        assert!(
            commands.top() >= bar.top() && commands.bottom() <= bar.bottom(),
            "{id}: {commands:?} in {bar:?}"
        );
        assert!(commands.left() > room.right(), "{id}: {commands:?}");
    }
}

#[test]
fn without_it_the_menu_bar_is_drawn_as_before() {
    let mut h = Harness::new();
    let bar = menus(&h);
    let name = h.find("Open Bible").unwrap();
    assert_eq!(name.left(), f32::from(inset::BAND.left));
    assert_eq!(name.top(), f32::from(inset::BAND.top));
    assert_eq!(bar.height(), 39.0, "its controls and margins: {bar:?}");
    // Its empty space is not the window's to drag.
    let empty = pos2(30.0, bar.center().y);
    press_drag(
        &mut h,
        pos2(5.0, bar.center().y),
        empty + vec2(200.0, 100.0),
    );
    assert!(!asked(&h, &ViewportCommand::StartDrag));
}

#[test]
fn in_full_screen_the_menu_bar_starts_at_the_left_edge() {
    let (mut h, system) = mac();
    h.root_info.fullscreen = Some(true);
    h.frames(3);
    let name = h.find("Open Bible").unwrap();
    assert_eq!(name.left(), f32::from(inset::BAND.left));
    // Its empty space is no title bar there: the system has none to drag, and a zoom asked
    // for in full screen would be applied on leaving it.
    let bar = menus(&h);
    let launchers = h
        .shell
        .st
        .toolbar
        .bar
        .expect("the Launchers in the menu bar");
    let gap = pos2(launchers.right() + 8.0, bar.center().y);
    press_drag(&mut h, gap, gap + vec2(-200.0, 300.0));
    assert!(h.commands.is_empty(), "{:?}", h.commands);
    for action in [TitleBarDoubleClick::Zoom, TitleBarDoubleClick::Minimize] {
        system.set(action);
        double_click(&mut h, gap);
        assert!(h.commands.is_empty(), "{action:?}: {:?}", h.commands);
    }
    // Out of full screen (or Split View) again, the traffic lights' room is back, and the
    // same space zooms the window.
    h.root_info.fullscreen = Some(false);
    h.frames(3);
    assert!(h.find("Open Bible").unwrap().left() >= lights(&h).right());
    let launchers = h
        .shell
        .st
        .toolbar
        .bar
        .expect("the Launchers in the menu bar");
    let gap = pos2(launchers.right() + 8.0, bar.center().y);
    system.set(TitleBarDoubleClick::Zoom);
    double_click(&mut h, gap);
    assert!(
        asked(&h, &ViewportCommand::Maximized(true)),
        "{:?}",
        h.commands
    );
}

#[test]
fn dragging_empty_menu_bar_space_moves_the_window_and_nothing_else_does() {
    let (mut h, _) = mac();
    let bar = menus(&h);
    // Beside the traffic lights, and between the Launchers and the right-hand controls.
    let beside_lights = pos2(lights(&h).right() - 4.0, bar.center().y);
    press_drag(&mut h, beside_lights, beside_lights + vec2(300.0, 200.0));
    assert!(asked(&h, &ViewportCommand::StartDrag), "{:?}", h.commands);
    let launchers = h
        .shell
        .st
        .toolbar
        .bar
        .expect("the Launchers in the menu bar");
    let gap = pos2(launchers.right() + 8.0, bar.center().y);
    let right = command_box(&h);
    assert!(
        gap.x < right.left() - 100.0,
        "room between {launchers:?} and {right:?}"
    );
    press_drag(&mut h, gap, gap + vec2(-200.0, 300.0));
    assert!(asked(&h, &ViewportCommand::StartDrag), "{:?}", h.commands);
    // A drag that starts on a menu's title does not.
    let file = h.at("File");
    press_drag(&mut h, file, file + vec2(150.0, 300.0));
    assert!(!asked(&h, &ViewportCommand::StartDrag), "{:?}", h.commands);
    h.events.push(Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(3);
    // The Toolbar's grip still moves the Toolbar, and not the window.
    assert_eq!(h.shell.st.session.toolbar.position, ToolbarPosition::Top);
    let grip = launchers.left_center() + vec2(5.0, 0.0);
    let to = pos2(h.screen.x / zoom(&h) - 20.0, 450.0);
    h.commands.clear();
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
    h.button(to, false);
    h.frames(3);
    assert_eq!(h.shell.st.session.toolbar.position, ToolbarPosition::Right);
    assert!(!asked(&h, &ViewportCommand::StartDrag), "{:?}", h.commands);
}

#[test]
fn auto_hide_at_the_top_shows_the_toolbar_under_the_title_bar() {
    let (mut h, _) = mac();
    h.shell.run("view.toolbar_auto_hide", &json!({}));
    let middle = pos2(700.0, 450.0);
    h.events.push(Event::PointerMoved(middle));
    h.frames(3);
    assert!(h.shell.st.session.toolbar.auto_hide);
    assert_eq!(h.shell.st.toolbar.bar, None, "hidden");
    assert!(h.find("Open Bible").unwrap().left() >= lights(&h).right() - 0.5);
    // The pointer at the window's top edge, in the title bar: it shows under the menu bar.
    h.events.push(Event::PointerMoved(pos2(700.0, 1.0)));
    h.frames(2);
    let bar = h.shell.st.toolbar.bar.expect("shown");
    assert!(bar.top() >= menus(&h).bottom() - 1.0, "{bar:?}");
}

#[test]
fn double_clicking_the_menu_bar_zooms_or_minimizes_as_the_system_says() {
    let (mut h, system) = mac();
    let empty = pos2(lights(&h).right() - 4.0, menus(&h).center().y);
    double_click(&mut h, empty);
    assert!(
        asked(&h, &ViewportCommand::Maximized(true)),
        "{:?}",
        h.commands
    );
    // Zoomed, a double-click puts back the size it had.
    h.root_info.maximized = Some(true);
    double_click(&mut h, empty);
    assert!(
        asked(&h, &ViewportCommand::Maximized(false)),
        "{:?}",
        h.commands
    );
    // The app's name is part of the title bar too.
    h.root_info.maximized = Some(false);
    let name = h.at("Open Bible");
    double_click(&mut h, name);
    assert!(
        asked(&h, &ViewportCommand::Maximized(true)),
        "{:?}",
        h.commands
    );
    system.set(TitleBarDoubleClick::Minimize);
    double_click(&mut h, empty);
    assert!(
        asked(&h, &ViewportCommand::Minimized(true)),
        "{:?}",
        h.commands
    );
    assert!(!asked(&h, &ViewportCommand::Maximized(true)));
    system.set(TitleBarDoubleClick::Nothing);
    double_click(&mut h, empty);
    assert!(h.commands.is_empty(), "{:?}", h.commands);
    // A double-click on a menu's title is the menu's.
    system.set(TitleBarDoubleClick::Zoom);
    let file = h.at("File");
    double_click(&mut h, file);
    let window = |c: &ViewportCommand| {
        matches!(
            c,
            ViewportCommand::Maximized(_) | ViewportCommand::Minimized(_)
        )
    };
    assert!(!h.commands.iter().any(window), "{:?}", h.commands);
}

/// Where floating window `w` painted `text` topmost (a Tab's title in its top tab bar).
fn topmost_in(h: &Harness, w: WindowId, text: &str) -> Rect {
    (h.window(w).shapes.iter())
        .filter(|(_, t)| t == text)
        .map(|(r, _)| *r)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap_or_else(|| panic!("no {text:?} in the window"))
}

#[test]
fn a_floating_windows_top_tab_bar_is_its_title_bar() {
    let (mut h, _, _, w) = native_heb();
    // As before without it: the system's title bar, the Tab after the window's margin.
    let plain = h.window(w).builder.clone();
    assert_eq!(plain.fullsize_content_view, None);
    let before = topmost_in(&h, w, "HEB");
    assert!(before.left() < size::TRAFFIC_LIGHTS, "{before:?}");
    let system = Arc::new(TestTitleBar::default());
    h.shell.set_unified_title_bar(system.clone());
    h.frames(3);
    {
        let win = h.window(w);
        let b = &win.builder;
        assert_eq!(b.fullsize_content_view, Some(true));
        assert_eq!(b.titlebar_shown, Some(false));
        assert_eq!(b.title_shown, Some(false));
        // The title is kept, for the Window menu, Mission Control and VoiceOver.
        assert_eq!(b.title.as_deref(), Some("HEB — Open Bible Software"));
    }
    // Its tab bar is at the top, the Tab after the traffic lights.
    let tab = topmost_in(&h, w, "HEB");
    // The room beside its traffic lights, in its own points (the main window's zoom).
    let room = lights(&h);
    assert!(tab.left() >= room.right(), "{tab:?}");
    assert!(
        tab.top() < room.bottom(),
        "the tab bar is the top row: {tab:?}"
    );
    let centre = size::TITLE_BAR / 2.0;
    assert!((tab.center().y - centre).abs() < 2.0, "{tab:?}");
    // The room beside the traffic lights moves the window, and double-clicking it zooms.
    let beside = pos2(room.right() - 4.0, centre);
    drag_in(&mut h, w, beside, beside + vec2(200.0, 150.0));
    assert!(
        h.window(w).commands.contains(&ViewportCommand::StartDrag),
        "{:?}",
        h.window(w).commands
    );
    h.window(w).commands.clear();
    h.commands.clear();
    h.click_in(w, beside, 2);
    assert!(
        h.window(w)
            .commands
            .contains(&ViewportCommand::Maximized(true)),
        "{:?}",
        h.window(w).commands
    );
    // Only the Floating window zooms, not the main window.
    assert!(h.commands.is_empty(), "{:?}", h.commands);
    system.set(TitleBarDoubleClick::Minimize);
    h.window(w).commands.clear();
    h.click_in(w, beside, 2);
    assert!(
        h.window(w)
            .commands
            .contains(&ViewportCommand::Minimized(true))
    );
    // In full screen its tab bar starts at the window's margin.
    h.window(w).info.fullscreen = Some(true);
    h.frames(3);
    let tab = topmost_in(&h, w, "HEB");
    assert!(tab.left() < room.right() - 20.0, "{tab:?}");
    // And no room there moves or zooms the window.
    system.set(TitleBarDoubleClick::Zoom);
    h.window(w).commands.clear();
    h.click_in(w, beside, 2);
    let window = |c: &ViewportCommand| {
        matches!(
            c,
            ViewportCommand::StartDrag
                | ViewportCommand::Maximized(_)
                | ViewportCommand::Minimized(_)
        )
    };
    assert!(
        !h.window(w).commands.iter().any(window),
        "{:?}",
        h.window(w).commands
    );
}

/// Writes `target/previews/title-bar-floating-{before,after}-{light,dark}-kittest.ppm` when
/// `UPDATE_PREVIEWS` is set: HEB floated into an OS window of its own, with the system's title
/// bar (before) and with its top tab bar as the title bar (after), for the PR's screenshots
/// (the real app's OBS_SHOT captures only the main window).
#[test]
fn previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
    for theme in ["light", "dark"] {
        let (mut h, _, _, w) = native_heb();
        h.shell.run("view.theme", &json!({ "value": theme }));
        h.frames(5);
        h.window_screenshot(
            w,
            &out.join(format!("title-bar-floating-before-{theme}-kittest.ppm")),
        );
        h.shell
            .set_unified_title_bar(Arc::new(TestTitleBar::default()));
        h.frames(5);
        h.window_screenshot(
            w,
            &out.join(format!("title-bar-floating-after-{theme}-kittest.ppm")),
        );
    }
}
