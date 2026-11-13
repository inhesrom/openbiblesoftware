//! Floating windows (#152) through the headless harness. With no window system the floating
//! windows are egui windows inside the main one (egui's embedded viewports), drawn by the same
//! code as the OS windows the desktop gets: right-click → Float in new window, dragging a Tab
//! out, Link sets across windows, closing a window, Layouts and the Last session, a window
//! whose screen is gone, and no extra text work for the main window. With
//! [`Harness::native_windows`] they are OS windows as eframe draws them, each with input of its
//! own: keys, modifiers and the Note editor in the window they belong to, and window places on
//! screens of different scales.

use std::cell::{RefCell, RefMut};
use std::collections::HashMap;
use std::rc::Rc;

use obs_engine::{ScreenRect, TabId, WindowId, WindowPlacement};

use super::*;
use crate::DesktopUnits;

/// A floating window's title bar, as the window system draws it, in points.
const TITLE_BAR: f32 = 30.0;

/// Floating windows as the desktop has them: each an OS window with input of its own, drawn in
/// a pass of its own (egui's immediate viewports, not embedded), as eframe draws them.
#[derive(Default)]
pub(super) struct NativeWindows {
    windows: HashMap<egui::ViewportId, NativeWindow>,
    root: egui::ViewportInfo,
    time: f64,
    /// The operating system's appearance, the same in every window.
    os_theme: Option<egui::Theme>,
    /// The scale factor a new window opens at (that of the screen the system opens it on).
    open_scale: Option<f32>,
    /// What the app asked of the main window, oldest first.
    pub(super) root_commands: Vec<egui::ViewportCommand>,
    /// Texture uploads made in floating windows' frames, for the harness's screenshots.
    pub(super) textures: Vec<egui::TexturesDelta>,
}

/// One floating OS window.
pub(super) struct NativeWindow {
    /// Its place and scale, as the window system reports them.
    pub(super) info: egui::ViewportInfo,
    /// Input typed or clicked in it since its last frame.
    pub(super) events: Vec<Event>,
    modifiers: Modifiers,
    /// How the app last asked for it to be made.
    pub(super) builder: egui::ViewportBuilder,
    /// What the app asked of its window, oldest first.
    pub(super) commands: Vec<egui::ViewportCommand>,
    /// The text it painted last frame.
    pub(super) shapes: Vec<(Rect, String)>,
    /// What it painted last frame, and its last AccessKit tree update (when AccessKit is on).
    pub(super) last_shapes: Vec<egui::epaint::ClippedShape>,
    pub(super) tree: Option<egui::accesskit::TreeUpdate>,
}

impl NativeWindow {
    /// Made as egui-winit makes a window: at the builder's position in points, at the scale of
    /// the screen it opens on.
    fn open(builder: &egui::ViewportBuilder, scale: f32) -> Self {
        let at = builder.position.unwrap_or(Pos2::ZERO);
        let size = builder.inner_size.unwrap_or(vec2(640.0, 720.0));
        let info = egui::ViewportInfo {
            native_pixels_per_point: Some(scale),
            focused: Some(false),
            ..Default::default()
        };
        let mut w = Self {
            info,
            events: Vec::new(),
            modifiers: Modifiers::NONE,
            builder: builder.clone(),
            commands: Vec::new(),
            shapes: Vec::new(),
            last_shapes: Vec::new(),
            tree: None,
        };
        w.place(at, size);
        w
    }

    /// Its outer corner moves to `at` (points at its scale); its inside is `size` points.
    fn place(&mut self, at: Pos2, size: egui::Vec2) {
        self.info.outer_rect = Some(Rect::from_min_size(at, size + vec2(0.0, TITLE_BAR)));
        self.info.inner_rect = Some(Rect::from_min_size(at + vec2(0.0, TITLE_BAR), size));
    }

    /// Where the app asked its window to move, in points at its scale then.
    fn moves(&self) -> Vec<Pos2> {
        (self.commands.iter())
            .filter_map(|c| match c {
                egui::ViewportCommand::OuterPosition(at) => Some(*at),
                _ => None,
            })
            .collect()
    }

    pub(super) fn size(&self) -> egui::Vec2 {
        self.info
            .inner_rect
            .map_or(vec2(640.0, 720.0), |r| r.size())
    }

    fn find(&self, text: &str) -> Option<Rect> {
        (self.shapes.iter().rev())
            .find(|(_, t)| t == text)
            .map(|(r, _)| *r)
    }
}

impl NativeWindows {
    /// Before the main window's frame: every window's info in its input, as eframe gives it.
    pub(super) fn begin_frame(
        &mut self,
        time: f64,
        os_theme: egui::Theme,
        viewports: &mut egui::ViewportIdMap<egui::ViewportInfo>,
    ) {
        self.time = time;
        self.os_theme = Some(os_theme);
        self.root = viewports[&egui::ViewportId::ROOT].clone();
        for (id, w) in &self.windows {
            viewports.insert(*id, w.info.clone());
        }
    }

    /// A floating window's input for its frame (opening it the first time).
    fn input(&mut self, id: egui::ViewportId, builder: egui::ViewportBuilder) -> RawInput {
        let scale = self.open_scale.unwrap_or(1.0);
        let w = (self.windows)
            .entry(id)
            .or_insert_with(|| NativeWindow::open(&builder, scale));
        w.builder = builder;
        let mut events = std::mem::take(&mut w.events);
        events.insert(0, Event::ModifiersChanged(w.modifiers));
        let size = w.size();
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(egui::ViewportId::ROOT, self.root.clone());
        for (id, w) in &self.windows {
            viewports.insert(*id, w.info.clone());
        }
        RawInput {
            viewport_id: id,
            viewports,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            time: Some(self.time),
            events,
            system_theme: self.os_theme,
            ..Default::default()
        }
    }

    /// After its frame: what it painted.
    fn output(&mut self, id: egui::ViewportId, mut out: egui::FullOutput) {
        // No GPU: texture uploads are kept for the harness's screenshots.
        self.textures.push(std::mem::take(&mut out.textures_delta));
        let Some(w) = self.windows.get_mut(&id) else {
            return;
        };
        w.shapes.clear();
        for c in &out.shapes {
            collect(&c.shape, &mut w.shapes);
        }
        w.last_shapes = out.shapes;
        if let Some(t) = out.platform_output.accesskit_update.take() {
            w.tree = Some(t);
        }
    }

    /// After the main window's frame, which carries what each floating window asked of its OS
    /// window: a move is made as egui-winit makes it, in points at the window's own scale.
    /// Returns the window the app brought to the front, if it asked to: that window gains the
    /// keyboard and every other loses it, as the window system tells them.
    pub(super) fn end_frame(
        &mut self,
        out: &egui::OrderedViewportIdMap<egui::ViewportOutput>,
    ) -> Option<egui::ViewportId> {
        let asked = |id: &egui::ViewportId| out.get(id).map(|v| v.commands.clone());
        let mut front = None;
        for c in asked(&egui::ViewportId::ROOT).unwrap_or_default() {
            if c == egui::ViewportCommand::Focus {
                front = Some(egui::ViewportId::ROOT);
            }
            self.root_commands.push(c);
        }
        for (id, w) in &mut self.windows {
            for c in asked(id).unwrap_or_default() {
                match c {
                    egui::ViewportCommand::OuterPosition(at) => {
                        let size = w.size();
                        w.place(at, size);
                    }
                    egui::ViewportCommand::Focus => front = Some(*id),
                    _ => {}
                }
                w.commands.push(c);
            }
        }
        if let Some(front) = front {
            for (id, w) in &mut self.windows {
                let now = *id == front;
                if w.info.focused != Some(now) {
                    w.events.push(Event::WindowFocused(now));
                }
                w.info.focused = Some(now);
            }
        }
        front
    }
}

impl Harness {
    /// From now on floating windows are OS windows of their own, each drawn in a pass of its
    /// own with its own input ([`NativeWindows`]).
    pub(super) fn native_windows(&mut self) {
        let shared = Rc::new(RefCell::new(NativeWindows::default()));
        self.ctx.set_embed_viewports(false);
        let windows = Rc::clone(&shared);
        egui::Context::set_immediate_viewport_renderer(move |ctx, viewport| {
            let egui::ImmediateViewport {
                ids,
                builder,
                mut viewport_ui_cb,
            } = viewport;
            let input = windows.borrow_mut().input(ids.this, builder);
            let out = ctx.run_ui(input, |ui| viewport_ui_cb(ui));
            windows.borrow_mut().output(ids.this, out);
        });
        self.native = Some(shared);
    }

    /// A floating window's OS window.
    pub(super) fn window(&self, w: WindowId) -> RefMut<'_, NativeWindow> {
        let d = self.shell.floating.iter().find(|d| d.id == w).unwrap();
        let id = d.viewport_id();
        let native = self.native.as_ref().expect("native windows");
        RefMut::map(native.borrow_mut(), |n| n.windows.get_mut(&id).unwrap())
    }

    /// Rasterize floating window `w`'s last frame, as [`Harness::screenshot`] does the main
    /// window's, into a PPM.
    pub(super) fn window_screenshot(&self, w: WindowId, path: &std::path::Path) {
        let (shapes, size, ppp) = {
            let win = self.window(w);
            let ppp = win.info.native_pixels_per_point.unwrap_or(1.0) * self.ctx.zoom_factor();
            (win.last_shapes.clone(), win.size(), ppp)
        };
        let (width, height, rgb) = self.rasterize(&shapes, size, ppp);
        let mut out = format!("P6\n{width} {height}\n255\n").into_bytes();
        out.extend(rgb);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, out).unwrap();
    }

    /// The scale factor the next floating window opens at.
    fn open_windows_at(&mut self, scale: f32) {
        self.native.as_ref().unwrap().borrow_mut().open_scale = Some(scale);
    }

    /// A click (or two, a double click) in a floating window, with that window's modifiers.
    pub(super) fn click_in(&mut self, w: WindowId, p: Pos2, clicks: usize) {
        // Long after any earlier click, so a double click is not counted as a triple.
        self.time += 1.0;
        self.window(w).events.push(Event::PointerMoved(p));
        self.frame();
        for _ in 0..clicks {
            for pressed in [true, false] {
                let mut win = self.window(w);
                let modifiers = win.modifiers;
                win.events.push(Event::PointerButton {
                    pos: p,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers,
                });
                drop(win);
                self.frame();
            }
        }
        self.frames(2);
    }

    /// A key pressed in a floating window.
    pub(super) fn key_in(&mut self, w: WindowId, key: egui::Key, modifiers: Modifiers) {
        self.window(w).events.push(key_event(key, modifiers));
        self.frames(2);
    }
}

fn key_event(key: egui::Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
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
        scale: 1.0,
    }
}

fn main_screen() -> ScreenRect {
    ScreenRect {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
        scale: 1.0,
    }
}

/// Right of the main screen, a screen at twice its scale: 1440 × 900 points, 2880 × 1800
/// pixels, on a desktop measured in pixels.
fn retina_screen() -> ScreenRect {
    ScreenRect {
        x: 1920.0,
        y: 0.0,
        width: 2880.0,
        height: 1800.0,
        scale: 2.0,
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

/// HEB floated into an OS window of its own, TB focused in the main window first.
pub(super) fn native_heb() -> (Harness, TabId, TabId, WindowId) {
    let mut h = Harness::new();
    h.native_windows();
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    let (tb, heb) = (h.tab_id("TB"), h.tab_id("HEB"));
    h.shell.run("tab.float", &json!({ "tab": heb.0 }));
    h.frames(3);
    let w = h.shell.floating[0].id;
    assert!(h.shell.floating[0].native, "an OS window of its own");
    (h, tb, heb, w)
}

#[test]
fn keys_typed_in_a_window_act_on_that_windows_tab() {
    let (mut h, tb, heb, w) = native_heb();
    let ws = |h: &Harness| h.shell.st.session.workspace.focused();
    // The user clicks TB in the main window, then into the floating HEB.
    let tb_pane = h.panels()[0].0;
    h.click(tb_pane.center());
    assert_eq!(ws(&h), Some(tb));
    let centre = (h.window(w).size() / 2.0).to_pos2();
    h.click_in(w, centre, 1);
    assert_eq!(ws(&h), Some(heb));
    // Alt-Tab back to the main window, no click: Ctrl+W there closes TB, not HEB.
    h.root_info.focused = Some(true);
    h.events.push(Event::WindowFocused(true));
    h.frame();
    assert_eq!(ws(&h), Some(tb), "the main window's Tab is focused again");
    h.events.push(key_event(egui::Key::W, Modifiers::COMMAND));
    h.frames(2);
    let ws_now = &h.shell.st.session.workspace;
    assert!(ws_now.tab(tb).is_none(), "TB closed");
    assert!(ws_now.tab(heb).is_some(), "HEB stays");
    assert_eq!(floating(&h), [vec!["HEB".to_string()]]);
    // Even with the focus left in the main window, Ctrl+W typed in the floating window
    // closes its HEB (and its window, left empty).
    assert_ne!(ws(&h), Some(heb));
    h.key_in(w, egui::Key::W, Modifiers::COMMAND);
    assert!(
        h.shell.st.session.workspace.tab(heb).is_none(),
        "HEB closed"
    );
    assert!(h.shell.floating.is_empty());
}

#[test]
fn ctrl_held_in_a_floating_library_opens_in_a_new_panel() {
    let mut h = Harness::new();
    h.native_windows();
    let library = h.tab_id("Library");
    h.shell.run("tab.float", &json!({ "tab": library.0 }));
    h.frames(3);
    let w = h.shell.floating[0].id;
    // Ctrl/Cmd held in the floating window only.
    h.modifiers = Modifiers::NONE;
    h.window(w).modifiers = Modifiers::COMMAND;
    let row = h.window(w).find("HNUM").expect("HNUM's row").center();
    h.click_in(w, row, 2);
    h.window(w).modifiers = Modifiers::NONE;
    let ws = &h.shell.st.session.workspace;
    let (tb, hnum) = (h.tab_id("TB"), h.tab_id("HNUM"));
    assert_ne!(
        ws.panel_of(hnum),
        ws.panel_of(tb),
        "opened in a new Panel, not with the other Bible"
    );
}

/// A word of at least 5 letters shown on one line in a floating window: its rect there.
fn word_in_window(h: &Harness, tab: TabId, w: WindowId) -> Rect {
    use unicode_segmentation::UnicodeSegmentation;
    let v = h.shell.st.tabs[&tab].view.as_ref().unwrap();
    let [ox, oy] = v.text_origin;
    let bottom = h.window(w).size().y - 60.0;
    for (index, _, _) in v.laid_in_view(900.0) {
        let block = &v.doc.blocks[index];
        let text = &v.doc.units[block.unit].text;
        let range = block.source.range.clone();
        for (i, word) in text[range.clone()].split_word_bound_indices() {
            if word.len() < 5 || !word.chars().all(char::is_alphabetic) || word.contains('f') {
                continue;
            }
            let a = crate::selection::DocPos {
                unit: block.unit,
                byte: range.start + i,
            };
            let b = crate::selection::DocPos {
                byte: a.byte + word.len(),
                ..a
            };
            let rects = crate::selection::range_rects(v, &[(a, b)], 900.0).remove(0);
            if let [[x0, y0, x1, y1]] = rects[..] {
                let r = Rect::from_min_max(pos2(ox + x0, oy + y0), pos2(ox + x1, oy + y1));
                if r.top() > 120.0 && r.bottom() < bottom {
                    return r;
                }
            }
        }
    }
    panic!("no word in the floating window");
}

#[test]
fn a_note_on_a_floating_bible_is_written_in_its_window() {
    let dir = std::env::temp_dir().join(format!("obs-floating-note-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(&dir));
    h.native_windows();
    let tb = h.tab_id("TB");
    h.shell.run("tab.float", &json!({ "tab": tb.0 }));
    settle_all(&mut h);
    let w = h.shell.floating[0].id;
    // Select a word in the floating window, then Ctrl+Shift+N there.
    let word = word_in_window(&h, tb, w);
    h.click_in(w, word.center(), 2);
    assert!(h.shell.st.has_selection(tb));
    let cmd_shift = Modifiers {
        shift: true,
        ..Modifiers::COMMAND
    };
    h.key_in(w, egui::Key::N, cmd_shift);
    assert!(h.shell.st.note_editor.is_some());
    // The editor is in the floating window, not the main one.
    assert!(h.window(w).find("Write a note…").is_some());
    assert!(h.find("Write a note…").is_none(), "{:?}", h.texts());
    // Typed there, and saved with Ctrl+Enter there.
    h.window(w)
        .events
        .push(Event::Text("Written in the floating window".into()));
    h.frames(2);
    h.key_in(w, egui::Key::Enter, Modifiers::COMMAND);
    assert!(h.shell.st.note_editor.is_none());
    let notes = h.shell.st.session.annotations(None).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body(), "Written in the floating window");
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A drag with the primary button in a floating window, from `from` to `to`, with that
/// window's input only.
pub(super) fn drag_in(h: &mut Harness, w: WindowId, from: Pos2, to: Pos2) {
    h.window(w).events.push(Event::PointerMoved(from));
    h.frame();
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    h.window(w).events.push(button(from, true));
    h.frame();
    for i in 1..=20 {
        let t = i as f32 / 20.0;
        h.window(w)
            .events
            .push(Event::PointerMoved(from + (to - from) * t));
        h.frame();
    }
    h.window(w).events.push(button(to, false));
    h.frames(3);
}

/// The Panels of floating window `w` (its own points), and the Panel each is.
fn window_panels(h: &Harness, w: WindowId) -> Vec<(Rect, obs_engine::PanelId)> {
    let d = h.shell.floating.iter().find(|d| d.id == w).unwrap();
    let mut out: Vec<_> = (d.panels.iter())
        .filter_map(|(tile, panel)| Some((d.tree.tiles.rect(*tile)?, *panel)))
        .collect();
    out.sort_by(|a, b| a.0.left().total_cmp(&b.0.left()));
    out
}

#[test]
fn a_resource_dragged_out_of_a_floating_library_tab_opens_in_that_window() {
    let mut h = Harness::new();
    h.native_windows();
    let library = h.tab_id("Library");
    h.shell.run("tab.float", &json!({ "tab": library.0 }));
    h.frames(3);
    let w = h.shell.floating[0].id;
    let main_before = main_titles(&h);
    // HNUM's row onto the Library Panel's right edge, in the same window: a new Panel there.
    let (panel, _) = window_panels(&h, w)[0];
    let row = h.window(w).find("HNUM").expect("HNUM's row").center();
    drag_in(&mut h, w, row, pos2(panel.right() - 20.0, panel.center().y));
    let hnum = h.tab_id("HNUM");
    assert_eq!(h.shell.keys_window(hnum), Some(w), "{:?}", floating(&h));
    let ws = &h.shell.st.session.workspace;
    assert_ne!(
        ws.panel_of(hnum),
        ws.panel_of(library),
        "a Panel of its own"
    );
    assert_eq!(main_titles(&h), main_before, "the main window is as it was");
    assert!(h.shell.st.drag.dragging.is_none());
    // HEB's row onto HNUM's tab bar: a new Tab in that Panel.
    let panels = window_panels(&h, w);
    let (bar, hnum_panel) = (panels.iter())
        .find(|(_, p)| Some(*p) == ws.panel_of(hnum))
        .map(|(r, p)| (pos2(r.left() + 40.0, r.top() + 12.0), *p))
        .unwrap();
    let row = h.window(w).find("HEB").expect("HEB's row").center();
    drag_in(&mut h, w, row, bar);
    let heb = h.tab_id("HEB");
    assert_eq!(h.shell.st.session.workspace.panel_of(heb), Some(hnum_panel));
    assert_eq!(main_titles(&h), main_before);
    // Dropped back on the Library's own list, nothing opens.
    let (list, _) = (window_panels(&h, w).into_iter())
        .find(|(_, p)| Some(*p) == h.shell.st.session.workspace.panel_of(library))
        .unwrap();
    let tabs = h.shell.st.session.workspace.window_tabs(w).len();
    let row = h.window(w).find("TB").expect("TB's row").center();
    drag_in(
        &mut h,
        w,
        row,
        pos2(list.left() + 60.0, list.center().y + 40.0),
    );
    assert_eq!(h.shell.st.session.workspace.window_tabs(w).len(), tabs);
}

#[test]
fn the_move_note_box_for_a_floating_bible_is_shown_in_its_window() {
    let mut h = Harness::new();
    h.native_windows();
    let tb = h.tab_id("TB");
    h.shell.run("tab.float", &json!({ "tab": tb.0 }));
    h.frames(3);
    let w = h.shell.floating[0].id;
    // A Note dropped on TB's text, in its floating window.
    h.shell.st.confirm_reanchor(
        "n".into(),
        "Genesis 30:1 (TB)".into(),
        tb,
        json!({}),
        "Genesis 30:5 (TB)".into(),
    );
    h.frames(2);
    let ask = "Move this Note from Genesis 30:1 (TB) to Genesis 30:5 (TB)?";
    assert!(h.window(w).find(ask).is_some());
    assert!(h.find(ask).is_none(), "not in the main window");
    // Esc there cancels it.
    h.key_in(w, egui::Key::Escape, Modifiers::NONE);
    assert!(h.shell.st.drag.confirm.is_none());
    assert!(h.window(w).find(ask).is_none());
}

#[test]
fn a_tab_dragged_out_as_the_pointer_leaves_the_window_floats_where_it_left() {
    let mut h = Harness::new();
    // The main window's inside starts 100 × 50 points into the desktop.
    h.root_info.inner_rect = Some(Rect::from_min_size(pos2(100.0, 50.0), h.screen));
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    h.frames(3);
    let from = h.tab_at("HEB");
    let edge = pos2(1390.0, 300.0);
    h.events.push(Event::PointerMoved(from));
    h.frame();
    h.button(from, true);
    h.frame();
    for i in 1..=20 {
        let t = i as f32 / 20.0;
        h.events.push(Event::PointerMoved(from + (edge - from) * t));
        h.frame();
    }
    // The pointer leaves the window still dragging: its position goes with it.
    h.events.push(Event::PointerGone);
    h.frames(3);
    assert_eq!(floating(&h), [vec!["HEB".to_string()]]);
    let size = h.shell.floating[0].placement.unwrap();
    // Where it left (1490 × 350 on the desktop), on the new window's title bar.
    assert_eq!(
        (size.x, size.y),
        (
            100.0 + 1390.0 - size.width.min(160.0) / 2.0,
            50.0 + 300.0 - 12.0
        )
    );
}

#[test]
fn a_window_on_a_screen_of_another_scale_is_placed_and_restored_in_desktop_units() {
    let db = temp_db("floating-mixed-scale");
    let mut h = Harness::launch(Some(&db));
    h.native_windows();
    h.shell.screens.units = DesktopUnits::Pixels;
    h.shell.set_screens(vec![main_screen(), retina_screen()]);
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    let heb = h.tab_id("HEB");
    // 240 × 50 points into the 2× screen: 2400 × 100 pixels.
    let at = WindowPlacement {
        x: 2400.0,
        y: 100.0,
        width: 700.0,
        height: 800.0,
        screen: Some(retina_screen()),
    };
    h.open_windows_at(2.0);
    h.shell.st.session.workspace.float(heb, Some(at));
    h.frames(3);
    let w = h.shell.floating[0].id;
    // egui-winit makes the window at its position times its scale: asked for in the 2×
    // screen's points.
    assert_eq!(h.window(w).builder.position, Some(pos2(1200.0, 50.0)));
    assert!(h.window(w).moves().is_empty(), "it opened in place");
    // Its place, read back from the window at its own scale, is the same desktop place on the
    // same screen (at the main window's scale it would read as 1200 pixels: the main screen).
    assert_eq!(h.shell.floating[0].placement, Some(at));
    // A window the system opens on the 1× screen first is moved where it belongs, in that
    // window's points.
    h.shell.run("resource.open", &json!({ "id": "heb-num" }));
    let hn = h.tab_id("HNUM");
    let at2 = WindowPlacement { y: 300.0, ..at };
    h.open_windows_at(1.0);
    h.shell.st.session.workspace.float(hn, Some(at2));
    h.frames(3);
    let w2 = h.shell.floating[1].id;
    assert_eq!(h.window(w2).builder.position, Some(pos2(1200.0, 150.0)));
    assert_eq!(h.window(w2).moves(), [pos2(2400.0, 300.0)]);
    assert_eq!(h.shell.floating[1].placement, Some(at2));
    // Saved, then restored with only the main screen: 240 × 50 points into it, its size kept.
    h.shell.save_session(true);
    drop(h);
    let mut session = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(&db).unwrap();
    session.attach_user(Box::new(store));
    let mut shell = Shell::new(session);
    add_doc(&mut shell, crate::tests::bible());
    add_doc(&mut shell, crate::tests::hebrew_doc());
    add_doc(
        &mut shell,
        crate::tests::numbered_bible("heb-num", "Hebrew-numbered Bible", "HNUM", true),
    );
    shell.restore_or_default();
    shell.screens.units = DesktopUnits::Pixels;
    shell.set_screens(vec![main_screen()]);
    let h = Harness::native_from_shell(shell);
    let placed = h.shell.floating[0].placement.unwrap();
    assert_eq!((placed.x, placed.y), (240.0, 50.0), "{placed:?}");
    assert_eq!((placed.width, placed.height), (700.0, 800.0));
    assert_eq!(placed.screen, Some(main_screen()));
    let w = h.shell.floating[0].id;
    assert_eq!(h.window(w).builder.position, Some(pos2(240.0, 50.0)));
}

#[test]
fn monitors_are_listed_in_the_desktops_own_units() {
    // Windows and Linux: pixels, the same on every monitor; a 2× monitor is 2 pixels a point.
    let px = DesktopUnits::Pixels.screen((2880, 0), (2880, 1800), 2.0, 1.0);
    assert_eq!(
        px,
        ScreenRect {
            x: 2880.0,
            y: 0.0,
            width: 2880.0,
            height: 1800.0,
            scale: 2.0
        }
    );
    // macOS: points (winit's pixels over the monitor's own scale).
    let pt = DesktopUnits::Points.screen((2880, 0), (2880, 1800), 2.0, 1.0);
    assert_eq!(
        pt,
        ScreenRect {
            x: 1440.0,
            y: 0.0,
            width: 1440.0,
            height: 900.0,
            scale: 1.0
        }
    );
    // A window drawn at 2 pixels a point and egui zoom 1.5 (3 pixels a point).
    assert_eq!(DesktopUnits::Pixels.per_point(3.0, 1.5), 3.0);
    assert_eq!(DesktopUnits::Points.per_point(3.0, 1.5), 1.5);
}

/// #293 acceptance: a Floating window, an OS window with input of its own, scrolls at the
/// Scroll speed too: a mouse-wheel step (lines) and a trackpad swipe (points) go half and
/// twice as far at 0.5× and 2×, with Reduce motion on and off.
#[test]
fn scroll_speed_scales_wheel_and_trackpad_in_a_floating_window() {
    for reduce in [true, false] {
        let mut h = Harness::new();
        h.native_windows();
        if !reduce {
            h.system_motion(false);
        }
        let tb = h.tab_id("TB");
        h.shell.run("tab.float", &json!({ "tab": tb.0 }));
        h.frames(3);
        let w = h.shell.floating[0].id;
        assert!(h.shell.floating[0].native, "an OS window of its own");
        let centre = (h.window(w).size() / 2.0).to_pos2();
        let position = |h: &mut Harness| {
            (h.shell.st.tabs.get_mut(&tb).unwrap().view.as_mut())
                .unwrap()
                .position()
                .0
        };
        let wheeled = |h: &mut Harness, unit: egui::MouseWheelUnit, dy: f32| {
            let before = position(h);
            let mut window = h.window(w);
            window.events.push(Event::PointerMoved(centre));
            window.events.push(Event::MouseWheel {
                unit,
                delta: vec2(0.0, -dy),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Modifiers::NONE,
            });
            drop(window);
            h.frames(60);
            position(h) - before
        };
        for (unit, dy) in [
            (egui::MouseWheelUnit::Line, 3.0),
            (egui::MouseWheelUnit::Point, 90.0),
        ] {
            h.shell.run("view.scroll_speed", &json!({ "value": 1.0 }));
            let one = wheeled(&mut h, unit, dy);
            assert!(one > 50.0, "reduce {reduce}, {unit:?}: {one}");
            for speed in [0.5, 2.0] {
                h.shell.run("view.scroll_speed", &json!({ "value": speed }));
                let moved = wheeled(&mut h, unit, dy);
                assert!(
                    (moved - one * speed).abs() < 0.5,
                    "reduce {reduce}, {unit:?} at {speed}×: {moved}, at 1×: {one}"
                );
            }
        }
    }
}
