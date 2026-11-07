//! Headless Workspace tests: a real egui Context, fed pointer events the way a window would,
//! with the shell drawn each frame. Widgets are found by the text they paint.

#[path = "reference_tests.rs"]
mod references;

use std::sync::Arc;

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape, pos2, vec2};
use egui_tiles::{Container, Tile};
use obs_engine::{JobMode, LibraryEntry, Session, TabKind, Tool};
use serde_json::json;

use super::Shell;
use crate::doc::ReadingDoc;
use crate::tests::{bible, hebrew_doc};

struct Harness {
    ctx: egui::Context,
    shell: Shell,
    events: Vec<Event>,
    modifiers: Modifiers,
    time: f64,
    shapes: Vec<(Rect, String)>,
    /// Vertex colours of textured meshes (glyph quads) drawn last frame.
    meshes: Vec<egui::Color32>,
    /// Glyph quads (textured-mesh rectangles) drawn last frame.
    glyphs: Vec<Rect>,
    /// Filled rectangles and line segments drawn last frame, with their colours.
    rects: Vec<(Rect, egui::Color32)>,
    /// The operating system's appearance, as the window would report it.
    os_theme: egui::Theme,
    /// The last AccessKit tree update (when AccessKit is on).
    tree: Option<egui::accesskit::TreeUpdate>,
    /// Textures as the GPU would hold them, and last frame's shapes (for [`Harness::screenshot`]).
    textures: std::collections::HashMap<egui::TextureId, egui::ColorImage>,
    last_shapes: Vec<egui::epaint::ClippedShape>,
    /// URLs the app asked the platform to open (hyperlinks clicked), oldest first.
    opened: Vec<String>,
    /// Text the app put on the clipboard, oldest first.
    copied: Vec<String>,
    /// The window's size in points (1400 × 900 unless a test makes it smaller).
    screen: egui::Vec2,
    /// The main window as the window system reports it (place, scale, focus).
    root_info: egui::ViewportInfo,
    /// Floating windows as OS windows of their own, when a test asks for them
    /// ([`Harness::native_windows`]); else egui embeds them in the main window.
    native: Option<std::rc::Rc<std::cell::RefCell<floating_windows::NativeWindows>>>,
    /// How soon last frame asked for the next one, unprompted (`Duration::MAX`: never).
    repaint_delay: std::time::Duration,
}

fn add_doc(shell: &mut Shell, doc: Arc<ReadingDoc>) {
    let id = doc.manifest.resource_id.clone();
    shell.st.session.library.add(LibraryEntry {
        manifest: doc.manifest.clone(),
        path: format!("/nonexistent/{id}.obr").into(),
    });
    shell.st.docs.insert(id, doc);
}

impl Harness {
    fn new() -> Self {
        Self::launch(None)
    }

    /// Start the app as a launch would: with a User database at `db`, restoring the Last
    /// session from it (or the default layout).
    fn launch(db: Option<&std::path::Path>) -> Self {
        Self::launch_with(db, JobMode::Inline)
    }

    /// Jobs run on real worker threads, as on the desktop.
    fn threaded() -> Self {
        Self::launch_with(None, JobMode::Threads(1))
    }

    fn launch_with(db: Option<&std::path::Path>, mode: JobMode) -> Self {
        Self::launch_full(db, mode, None)
    }

    /// With `obr_dir`, TB is a real `.obr` file there (written on first use), loaded from
    /// disk like a library Resource: annotating reads the Resource's text units from its file.
    fn launch_full(
        db: Option<&std::path::Path>,
        mode: JobMode,
        obr_dir: Option<&std::path::Path>,
    ) -> Self {
        Self::launch_with_fonts(db, mode, obr_dir, None)
    }

    /// Like [`Self::launch_full`], with `fonts` as the computer's installed fonts (#138).
    fn launch_with_fonts(
        db: Option<&std::path::Path>,
        mode: JobMode,
        obr_dir: Option<&std::path::Path>,
        fonts: Option<Arc<dyn crate::SystemFonts>>,
    ) -> Self {
        let mut session = Session::new(mode);
        if let Some(db) = db {
            let (store, _) = obs_store::UserStore::open(db).unwrap();
            session.attach_user(Box::new(store));
        }
        let mut shell = Shell::new(session);
        if let Some(fonts) = fonts {
            shell.set_system_fonts(fonts);
        }
        match obr_dir {
            Some(dir) => {
                let path = dir.join("test-bible.obr");
                if !path.exists() {
                    std::fs::create_dir_all(dir).unwrap();
                    let r = crate::tests::numbered_bible_resource(
                        "test-bible",
                        "Test Bible",
                        "TB",
                        false,
                    );
                    obs_store::write_obr(&r, &path).unwrap();
                }
                shell.st.session.library.add_file(&path).unwrap();
            }
            None => add_doc(&mut shell, bible()),
        }
        add_doc(&mut shell, hebrew_doc());
        add_doc(
            &mut shell,
            crate::tests::numbered_bible("heb-num", "Hebrew-numbered Bible", "HNUM", true),
        );
        shell.restore_or_default();
        Self::from_shell(shell)
    }

    /// Draw `shell` (already restored) in a fresh window, as the first frames of a launch.
    fn from_shell(shell: Shell) -> Self {
        Self::open(shell, false)
    }

    /// Like [`Self::from_shell`], with floating windows as OS windows of their own from the
    /// first frame ([`Self::native_windows`]).
    fn native_from_shell(shell: Shell) -> Self {
        Self::open(shell, true)
    }

    fn open(shell: Shell, native: bool) -> Self {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut h = Self {
            ctx,
            shell,
            events: Vec::new(),
            modifiers: Modifiers::NONE,
            time: 0.0,
            shapes: Vec::new(),
            meshes: Vec::new(),
            glyphs: Vec::new(),
            rects: Vec::new(),
            os_theme: egui::Theme::Light,
            tree: None,
            textures: std::collections::HashMap::new(),
            last_shapes: Vec::new(),
            opened: Vec::new(),
            copied: Vec::new(),
            screen: vec2(1400.0, 900.0),
            root_info: egui::ViewportInfo::default(),
            native: None,
            repaint_delay: std::time::Duration::MAX,
        };
        if native {
            h.native_windows();
        }
        h.frames(3);
        h
    }

    fn frame(&mut self) {
        self.events
            .insert(0, Event::ModifiersChanged(self.modifiers));
        self.run_frame();
    }

    /// A frame with no input at all, as when the window is idle: egui asks for no repaint of its
    /// own, so [`Harness::repaint_delay`] shows only what the app asked for.
    fn idle_frame(&mut self) {
        assert!(self.events.is_empty(), "pending input: {:?}", self.events);
        self.run_frame();
    }

    fn run_frame(&mut self) {
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(egui::ViewportId::ROOT, self.root_info.clone());
        if let Some(n) = &self.native {
            n.borrow_mut()
                .begin_frame(self.time, self.os_theme, &mut viewports);
        }
        let input = RawInput {
            viewports,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.screen)),
            time: Some(self.time),
            events: std::mem::take(&mut self.events),
            system_theme: Some(self.os_theme),
            ..Default::default()
        };
        self.time += 1.0 / 60.0;
        let shell = &mut self.shell;
        let mut out = self.ctx.run_ui(input, |ui| shell.ui(ui));
        if let Some(n) = &self.native {
            n.borrow_mut().end_frame(&out.viewport_output);
        }
        // No GPU here: texture uploads are kept as images, for screenshots only.
        for (id, deltas) in std::mem::take(&mut out.textures_delta.set) {
            for delta in deltas {
                let egui::ImageData::Color(img) = delta.image;
                match (delta.pos, self.textures.get_mut(&id)) {
                    (Some([x0, y0]), Some(tex)) => {
                        for y in 0..img.size[1] {
                            for x in 0..img.size[0] {
                                tex.pixels[(y0 + y) * tex.size[0] + x0 + x] =
                                    img.pixels[y * img.size[0] + x];
                            }
                        }
                    }
                    _ => {
                        self.textures.insert(id, (*img).clone());
                    }
                }
            }
        }
        out.textures_delta.clear();
        self.repaint_delay = out.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        self.last_shapes = out.shapes.clone();
        for c in &out.platform_output.commands {
            match c {
                egui::OutputCommand::OpenUrl(u) => self.opened.push(u.url.clone()),
                egui::OutputCommand::CopyText(t) => self.copied.push(t.clone()),
                _ => {}
            }
        }
        if let Some(t) = out.platform_output.accesskit_update.take() {
            self.tree = Some(t);
        }
        self.shapes.clear();
        self.meshes.clear();
        self.glyphs.clear();
        self.rects.clear();
        for c in out.shapes {
            collect(&c.shape, &mut self.shapes);
            collect_meshes(&c.shape, &mut self.meshes);
            collect_glyphs(&c.shape, &mut self.glyphs);
            collect_rects(&c.shape, &mut self.rects);
            collect_icons(&self.ctx, &c.shape, &mut self.shapes);
        }
    }

    /// A frame after a 60 Hz frame interval, so workers get the time a real frame gives them.
    fn paced_frame(&mut self) {
        std::thread::sleep(std::time::Duration::from_micros(16_667));
        self.frame();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    /// Where `text` was painted last frame.
    fn find(&self, text: &str) -> Option<Rect> {
        self.shapes
            .iter()
            .rev()
            .find(|(_, t)| t == text)
            .map(|(r, _)| *r)
    }

    fn at(&self, text: &str) -> Pos2 {
        self.find(text)
            .unwrap_or_else(|| panic!("{text:?} not on screen: {:?}", self.texts()))
            .center()
    }

    /// Open a top menu (its title is the topmost text with that name).
    fn open_menu(&mut self, title: &str) {
        let r = self
            .shapes
            .iter()
            .filter(|(_, t)| t == title)
            .map(|(r, _)| *r)
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap_or_else(|| panic!("no menu {title}"));
        self.click(r.center());
    }

    /// Where a Tab's title is painted in its Panel's tab bar (the same text can also be a
    /// Library row).
    fn tab_at(&self, title: &str) -> Pos2 {
        let bars: Vec<Rect> = self
            .panels()
            .into_iter()
            .map(|(r, _)| Rect::from_min_size(r.min, vec2(r.width(), 31.0)))
            .collect();
        self.shapes
            .iter()
            .rev()
            .find(|(r, t)| t == title && bars.iter().any(|b| b.contains(r.center())))
            .map(|(r, _)| r.center())
            .unwrap_or_else(|| panic!("no Tab {title:?} in a tab bar: {:?}", self.texts()))
    }

    fn texts(&self) -> Vec<&str> {
        self.shapes.iter().map(|(_, t)| t.as_str()).collect()
    }

    fn click(&mut self, p: Pos2) {
        self.events.push(Event::PointerMoved(p));
        self.frame();
        self.button(p, true);
        self.frame();
        self.button(p, false);
        self.frames(2);
    }

    fn click_text(&mut self, text: &str) {
        let p = self.at(text);
        self.click(p);
    }

    fn button(&mut self, pos: Pos2, pressed: bool) {
        self.events.push(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: self.modifiers,
        });
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.events.push(Event::PointerMoved(from));
        self.frame();
        self.button(from, true);
        self.frame();
        for i in 1..=20 {
            let t = i as f32 / 20.0;
            self.events
                .push(Event::PointerMoved(from + (to - from) * t));
            self.frame();
        }
        self.button(to, false);
        self.frames(3);
    }

    /// Tabs containers and the Tabs (titles) in each, left to right.
    fn panels(&self) -> Vec<(Rect, Vec<String>)> {
        let tiles = &self.shell.tree.tiles;
        let mut out: Vec<(Rect, Vec<String>)> = tiles
            .tile_ids()
            .filter_map(|id| match tiles.get(id) {
                Some(Tile::Container(Container::Tabs(t))) => {
                    let titles = t
                        .children
                        .iter()
                        .filter_map(|c| match tiles.get(*c) {
                            Some(Tile::Pane(tab)) => self
                                .shell
                                .st
                                .session
                                .workspace
                                .tab(*tab)
                                .map(|t| t.title.clone()),
                            _ => None,
                        })
                        .collect();
                    Some((tiles.rect(id)?, titles))
                }
                _ => None,
            })
            .collect();
        out.sort_by(|a, b| {
            a.0.left()
                .total_cmp(&b.0.left())
                .then(a.0.top().total_cmp(&b.0.top()))
        });
        out
    }

    /// Rasterize last frame as the GPU would (premultiplied alpha, nearest texels) into a PPM.
    fn screenshot(&self, path: &std::path::Path) {
        let ppp = 1.0;
        let (w, h) = (self.screen.x as usize, self.screen.y as usize);
        let prims = self.ctx.tessellate(self.last_shapes.clone(), ppp);
        let mut px = vec![[0.0f32; 3]; w * h];
        for p in &prims {
            let egui::epaint::Primitive::Mesh(m) = &p.primitive else {
                continue;
            };
            let tex = self.textures.get(&m.texture_id);
            let clip = p
                .clip_rect
                .intersect(Rect::from_min_size(Pos2::ZERO, vec2(w as f32, h as f32)));
            for tri in m.indices.chunks(3) {
                let [a, b, c] = [0, 1, 2].map(|i| m.vertices[tri[i] as usize]);
                let lo = a.pos.min(b.pos).min(c.pos).max(clip.min);
                let hi = a.pos.max(b.pos).max(c.pos).min(clip.max);
                let area =
                    (b.pos - a.pos).x * (c.pos - a.pos).y - (b.pos - a.pos).y * (c.pos - a.pos).x;
                if area.abs() < 1e-6 || lo.x >= hi.x || lo.y >= hi.y {
                    continue;
                }
                for y in lo.y.floor() as usize..hi.y.ceil() as usize {
                    for x in lo.x.floor() as usize..hi.x.ceil() as usize {
                        let q = pos2(x as f32 + 0.5, y as f32 + 0.5);
                        let edge = |p0: Pos2, p1: Pos2| {
                            (p1 - p0).x * (q - p0).y - (p1 - p0).y * (q - p0).x
                        };
                        let (wa, wb, wc) = (
                            edge(b.pos, c.pos) / area,
                            edge(c.pos, a.pos) / area,
                            edge(a.pos, b.pos) / area,
                        );
                        if wa < -1e-4 || wb < -1e-4 || wc < -1e-4 || x >= w || y >= h {
                            continue;
                        }
                        let mut src = [0.0f32; 4];
                        for (v, wt) in [(a, wa), (b, wb), (c, wc)] {
                            for (k, ch) in v.color.to_array().iter().enumerate() {
                                src[k] += f32::from(*ch) / 255.0 * wt;
                            }
                        }
                        if let Some(t) = tex {
                            let uv =
                                a.uv.to_vec2() * wa + b.uv.to_vec2() * wb + c.uv.to_vec2() * wc;
                            let tx = ((uv.x * t.size[0] as f32) as usize).min(t.size[0] - 1);
                            let ty = ((uv.y * t.size[1] as f32) as usize).min(t.size[1] - 1);
                            let texel = t.pixels[ty * t.size[0] + tx].to_array();
                            for k in 0..4 {
                                src[k] *= f32::from(texel[k]) / 255.0;
                            }
                        }
                        let d = &mut px[y * w + x];
                        for k in 0..3 {
                            d[k] = src[k] + d[k] * (1.0 - src[3]);
                        }
                    }
                }
            }
        }
        let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
        out.extend(
            px.iter()
                .flat_map(|p| p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)),
        );
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, out).unwrap();
    }

    fn tab_id(&self, title: &str) -> obs_engine::TabId {
        self.shell
            .st
            .session
            .workspace
            .tabs()
            .find(|t| t.title == title)
            .map(|t| t.id)
            .unwrap_or_else(|| panic!("no Tab {title}"))
    }
}

fn collect_meshes(shape: &Shape, out: &mut Vec<egui::Color32>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| collect_meshes(s, out)),
        Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => {
            out.extend(m.vertices.iter().map(|v| v.color));
        }
        _ => {}
    }
}

/// Each glyph quad of a textured mesh (4 vertices per quad, as the reading pane adds them).
fn collect_glyphs(shape: &Shape, out: &mut Vec<Rect>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| collect_glyphs(s, out)),
        Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => {
            for q in m.vertices.chunks(4) {
                out.push(Rect::from_points(
                    &q.iter().map(|v| v.pos).collect::<Vec<_>>(),
                ));
            }
        }
        _ => {}
    }
}

fn collect_rects(shape: &Shape, out: &mut Vec<(Rect, egui::Color32)>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| collect_rects(s, out)),
        Shape::Rect(r) => {
            out.push((r.rect, r.fill));
            // An outline (a Box Highlight style, #181) counts in its stroke's colour too.
            if r.fill == egui::Color32::TRANSPARENT && r.stroke.width > 0.0 {
                out.push((r.rect, r.stroke.color));
            }
        }
        Shape::LineSegment { points, stroke } => {
            out.push((Rect::from_two_pos(points[0], points[1]), stroke.color));
        }
        _ => {}
    }
}

/// Icons drawn, listed with the painted text as their [`crate::icons::Icon::marker`]
/// (`[x]`, `[pin]`), so tests find them as they find labels.
fn collect_icons(ctx: &egui::Context, shape: &Shape, out: &mut Vec<(Rect, String)>) {
    let (rect, tex) = match shape {
        Shape::Vec(v) => {
            v.iter().for_each(|s| collect_icons(ctx, s, out));
            return;
        }
        Shape::Rect(r) => match &r.brush {
            Some(b) => (r.rect, b.fill_texture_id),
            None => return,
        },
        Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => (
            Rect::from_points(&m.vertices.iter().map(|v| v.pos).collect::<Vec<_>>()),
            m.texture_id,
        ),
        _ => return,
    };
    if let Some(icon) = crate::icons::icon_of(ctx, tex) {
        out.push((rect, icon.marker()));
    }
}

fn collect(shape: &Shape, out: &mut Vec<(Rect, String)>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
        Shape::Text(t) => {
            let rect = Rect::from_min_size(t.pos, t.galley.size());
            out.push((rect, t.galley.text().to_string()));
        }
        _ => {}
    }
}

/// The count drawn on the Library launcher, if any.
fn library_badge(h: &Harness) -> Option<String> {
    let button = h.shell.st.toolbar.buttons[&obs_engine::Launcher::Library];
    h.shapes
        .iter()
        .find(|(r, t)| t != "Library" && t != "[library]" && button.contains(r.center()))
        .map(|(_, t)| t.clone())
}

#[test]
fn default_layout_has_a_bible_and_a_tools_panel() {
    let h = Harness::new();
    let panels = h.panels();
    assert_eq!(panels.len(), 2, "{panels:?}");
    assert_eq!(panels[0].1, ["TB"]);
    assert_eq!(panels[1].1, ["Library", "Word Info"]);
    // The Bible's header: link chip slot, ‹ ›, reference box with the location, title.
    for t in [
        "A",
        "[arrow-left]",
        "[arrow-right]",
        "Genesis 30:1",
        "Test Bible",
        "[ellipsis]",
    ] {
        assert!(h.find(t).is_some(), "{t:?} missing: {:?}", h.texts());
    }
    // The launcher menus and the Command box (#139).
    for t in [
        "Library",
        "Guides",
        "Tools",
        "Layouts",
        "View",
        "Help",
        &format!("{} (Ctrl+L)", crate::shell::command_box::HINT),
    ] {
        assert!(h.find(t).is_some(), "{t}");
    }
}

#[test]
fn tabs_can_be_dragged_to_split_dock_and_resize() {
    let mut h = Harness::new();
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    h.frames(3);
    assert_eq!(h.panels()[0].1, ["TB", "HEB"]);

    // Split: drag the HEB tab to the bottom edge of the TB Panel.
    let (left, _) = h.panels()[0].clone();
    let from = h.tab_at("HEB");
    h.drag(from, pos2(left.center().x, left.bottom() - 30.0));
    let panels = h.panels();
    assert_eq!(panels.len(), 3, "{panels:?}");
    let heb = h.tab_id("HEB");
    let tb = h.tab_id("TB");
    let ws = &h.shell.st.session.workspace;
    assert_ne!(ws.panel_of(heb), ws.panel_of(tb));
    assert_eq!(ws.panels().count(), 3, "the engine follows the dock");

    // Dock: drag HEB onto the tab bar of the Tools Panel.
    let tools_tab = h.at("Word Info");
    let from = h.tab_at("HEB");
    h.drag(from, tools_tab + vec2(60.0, 0.0));
    let panels = h.panels();
    assert_eq!(panels.len(), 2, "{panels:?}");
    let ws = &h.shell.st.session.workspace;
    assert_eq!(ws.panel_of(heb), ws.panel_of(h.tab_id("Word Info")));

    // Resize: drag the gap between the two Panels to the left.
    let (left, _) = h.panels()[0].clone();
    let gap = pos2(left.right() + 3.0, left.center().y);
    h.drag(gap, gap - vec2(200.0, 0.0));
    let (after, _) = h.panels()[0].clone();
    assert!(
        after.width() < left.width() - 150.0,
        "{} -> {}",
        left.width(),
        after.width()
    );
}

#[test]
fn each_tab_navigates_and_keeps_its_own_history() {
    let mut h = Harness::new();
    h.shell
        .run("resource.open", &json!({ "id": "heb-test", "split": true }));
    h.frames(2);
    let tb = h.tab_id("TB");
    let heb = h.tab_id("HEB");
    // HEB joined TB's Link set when it opened from it; unlink it so each Tab moves alone.
    h.shell.run("tab.link", &json!({ "tab": heb.0, "set": "" }));
    h.frames(2);
    let heb_start = h
        .shell
        .st
        .tabs
        .get_mut(&heb)
        .unwrap()
        .view
        .as_mut()
        .unwrap()
        .current_location();
    // Type into TB's reference box: click it, select all, type, Enter.
    h.click_text("Genesis 30:1");
    h.events.push(Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.events.push(Event::Text("Gen 32:5".into()));
    h.frame();
    h.events.push(Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(3);
    let loc = |h: &mut Harness, t| {
        h.shell
            .st
            .tabs
            .get_mut(&t)
            .unwrap()
            .view
            .as_mut()
            .unwrap()
            .current_location()
    };
    assert_eq!(
        loc(&mut h, tb),
        Some(obs_scripture::Verse::new(
            obs_scripture::Book::Genesis,
            32,
            5
        ))
    );
    assert!(h.find("Genesis 32:5").is_some(), "{:?}", h.texts());
    // A relative reference, then Back with the header's ‹.
    h.shell.run("tab.go", &json!({ "tab": tb.0, "text": "12" }));
    h.frames(2);
    assert_eq!(
        loc(&mut h, tb),
        Some(obs_scripture::Verse::new(
            obs_scripture::Book::Genesis,
            32,
            12
        ))
    );
    let back = h
        .shapes
        .iter()
        .filter(|(_, t)| t == "[arrow-left]")
        .map(|(r, _)| *r)
        .min_by(|a, b| a.left().total_cmp(&b.left()))
        .unwrap();
    h.click(back.center());
    assert_eq!(
        loc(&mut h, tb),
        Some(obs_scripture::Verse::new(
            obs_scripture::Book::Genesis,
            32,
            5
        ))
    );
    // The other Bible did not move and has no history.
    assert_eq!(loc(&mut h, heb), heb_start);
    let ws = &h.shell.st.session.workspace;
    assert!(!ws.tab(heb).unwrap().history.can_go_back());
    assert!(ws.tab(tb).unwrap().history.can_go_forward());
}

#[test]
fn opening_word_info_twice_focuses_the_existing_tab() {
    let mut h = Harness::new();
    // Make Library the active Tab of the Tools Panel, then open Word Info from the menu.
    h.click_text("Library");
    h.open_menu("Tools");
    h.click_text("Word Info");
    h.open_menu("Tools");
    // Two "Word Info" texts now: the tab title and the menu item; the menu item is on top.
    h.click_text("Word Info");
    let wi: Vec<_> = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .filter(|t| t.kind == TabKind::Tool(Tool::WordInfo))
        .map(|t| t.id)
        .collect();
    assert_eq!(wi.len(), 1);
    assert_eq!(h.shell.st.session.workspace.focused(), Some(wi[0]));
    // It is the active Tab of its Panel: its body shows (empty until a word is hovered).
    assert!(
        h.texts()
            .iter()
            .any(|t| t.contains("Rest the pointer on a word")),
        "{:?}",
        h.texts()
    );
    // Both menu clicks ran the command (the default layout ran it once before).
    let log = h.shell.st.session.command_log();
    assert_eq!(log.iter().filter(|c| **c == "tool.word_info").count(), 3);
}

#[test]
fn menu_items_are_dispatched_through_the_command_registry() {
    let mut h = Harness::new();
    let before = h.shell.st.session.command_log().len();
    h.open_menu("Help");
    h.click_text("Credits");
    assert!(h.find("Credits").is_some());
    assert_eq!(
        h.shell
            .st
            .session
            .workspace
            .focused()
            .and_then(|t| h.shell.st.session.workspace.tab(t))
            .map(|t| t.kind.clone()),
        Some(TabKind::Credits)
    );
    h.open_menu("View");
    h.click_text("Red letter");
    assert!(!h.shell.st.session.text.red_letter);
    h.open_menu("Guides");
    h.click_text("Passage Guide");
    // The Passage Guide (#53) opens as itself, no longer a placeholder. The focused Credits
    // Tab has no Passage, so the Guide asks for one.
    assert!(
        h.texts()
            .iter()
            .any(|t| t.starts_with("Type a Passage in the box above")),
        "{:?}",
        h.texts()
    );
    assert!(h.find("Passage Guide arrives with #53").is_none());
    // The + on a tab bar runs the same commands.
    let plus = h
        .shapes
        .iter()
        .filter(|(_, t)| t == "[plus]")
        .map(|(r, _)| *r)
        .min_by(|a, b| a.left().total_cmp(&b.left()))
        .unwrap();
    h.click(plus.center());
    h.click_text("Cross-references");
    let log = h.shell.st.session.command_log()[before..].to_vec();
    assert_eq!(
        log,
        [
            "help.credits",
            "view.red_letter",
            "guide.passage_guide",
            "tool.cross_references"
        ]
    );
    // Opened from the TB Panel's +, Cross-references lands in that Panel.
    let ws = &h.shell.st.session.workspace;
    assert_eq!(
        ws.panel_of(h.tab_id("Cross-references")),
        ws.panel_of(h.tab_id("TB"))
    );
    // Every menu item is a registered command or the Resource list.
    for m in obs_engine::MENUS {
        h.open_menu(m.title);
        for item in m.items {
            if let obs_engine::MenuItem::Command(id) = item {
                let spec = h.shell.st.session.commands().get(id).unwrap();
                let shown = h.texts().iter().any(|t| t.ends_with(spec.label));
                assert!(shown, "{id} ({}) not in the {} menu", spec.label, m.title);
            }
        }
        h.events.push(Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.frames(2);
    }
}

#[test]
fn reading_text_keeps_a_comfortable_measure() {
    let h = Harness::new();
    // The TB Panel is far wider than 38 ems of 17 pt; no text line runs past the measure.
    let (left, _) = h.panels()[0].clone();
    assert!(left.width() > 800.0, "{left:?}");
    let laid = h
        .shell
        .st
        .tabs
        .values()
        .find_map(|t| t.view.as_ref())
        .unwrap();
    assert!(
        laid.width() <= crate::MEASURE_EMS * 17.0 + 0.5,
        "{}",
        laid.width()
    );
}

#[test]
fn every_command_shortcut_parses() {
    let s = Session::new(JobMode::Inline);
    for c in s.commands().iter() {
        if let Some(sc) = c.shortcut {
            assert!(super::parse_shortcut(sc).is_some(), "{} {sc}", c.id);
        }
    }
}

fn linked_harness() -> (Harness, obs_engine::TabId, obs_engine::TabId) {
    let mut h = Harness::new();
    add_doc(
        &mut h.shell,
        crate::tests::numbered_bible("heb-num", "Hebrew-numbered Bible", "HNUM", true),
    );
    // Opened from the focused TB (Link set A) with Ctrl held: a new Panel, same Link set.
    let tb = h.tab_id("TB");
    h.shell.st.session.workspace.user_focused(tb);
    h.shell
        .run("resource.open", &json!({ "id": "heb-num", "split": true }));
    h.frames(3);
    let hn = h.tab_id("HNUM");
    let ws = &h.shell.st.session.workspace;
    assert_eq!(ws.tab(hn).unwrap().link, Some('A'));
    (h, tb, hn)
}

fn location(h: &mut Harness, t: obs_engine::TabId) -> Option<obs_scripture::Verse> {
    h.shell
        .st
        .tabs
        .get_mut(&t)?
        .view
        .as_mut()?
        .current_location()
}

fn loc_of(h: &mut Harness, title: &str) -> Option<obs_scripture::Verse> {
    let t = h.tab_id(title);
    location(h, t)
}

/// The follower has its target block laid out and is showing it. (Prefetch around it may
/// still be in flight; that never shows as placeholders while the view stays put.)
fn settled(h: &Harness, t: obs_engine::TabId) -> bool {
    h.shell.st.tabs[&t]
        .view
        .as_ref()
        .is_some_and(|v| !v.is_following())
}

fn ge(c: u16, v: u16) -> obs_scripture::Verse {
    obs_scripture::Verse::new(obs_scripture::Book::Genesis, c, v)
}

#[test]
fn scrolling_to_gen_31_55_moves_a_linked_hebrew_numbered_bible_to_32_1() {
    let (mut h, tb, hn) = linked_harness();
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:53" }));
    h.frames(4);
    assert_eq!(location(&mut h, hn), Some(ge(31, 53)));
    // Wheel-scroll TB a little at a time until it reports 31:55.
    let over = h.panels()[0].0.center();
    h.events.push(Event::PointerMoved(over));
    let mut steps = 0;
    while location(&mut h, tb) != Some(ge(31, 55)) {
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -6.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        h.frame();
        steps += 1;
        assert!(steps < 400, "TB stuck at {:?}", location(&mut h, tb));
    }
    // The follower settles within 3 frames on 32:1, its own number for KJV 31:55.
    let mut frames = 0;
    while !(location(&mut h, hn) == Some(ge(32, 1)) && settled(&h, hn)) {
        h.paced_frame();
        frames += 1;
        assert!(
            frames <= 3,
            "HNUM at {:?} after {frames} frames",
            location(&mut h, hn)
        );
    }
    // The header shows the follower's own numbering.
    assert!(h.find("Genesis 32:1").is_some(), "{:?}", h.texts());
}

#[test]
fn a_jump_moves_every_member_and_flashes_the_target() {
    let (mut h, tb, hn) = linked_harness();
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 32:10" }));
    let mut frames = 0;
    while !(location(&mut h, hn) == Some(ge(32, 11)) && settled(&h, hn)) {
        h.paced_frame();
        frames += 1;
        assert!(frames <= 3, "HNUM at {:?}", location(&mut h, hn));
    }
    for t in [tb, hn] {
        assert!(h.shell.st.tabs[&t].flash.is_some(), "no flash in {t:?}");
    }
    // Scrolling the follower drives the origin back the other way, with no flash.
    h.shell
        .run("tab.go", &json!({ "tab": hn.0, "text": "33:3" }));
    h.frames(3);
    assert_eq!(location(&mut h, tb), Some(ge(33, 3)));
    // Back in HNUM (where that jump happened) moves the set back too. Following adds no
    // history: TB's own Back still leads to where TB itself last jumped from.
    h.shell.run("tab.back", &json!({ "tab": hn.0 }));
    h.frames(3);
    assert_eq!(location(&mut h, hn), Some(ge(32, 11)));
    assert_eq!(location(&mut h, tb), Some(ge(32, 10)));
    let ws = &h.shell.st.session.workspace;
    assert!(!ws.tab(tb).unwrap().history.can_go_forward());
}

#[test]
fn the_link_chip_and_the_tab_title_change_the_link_set() {
    let (mut h, tb, hn) = linked_harness();
    // HNUM's chip is the right-most "A" on screen; its menu offers the sets.
    let chip = h
        .shapes
        .iter()
        .filter(|(_, t)| t == "A")
        .map(|(r, _)| *r)
        .max_by(|a, b| a.left().total_cmp(&b.left()))
        .unwrap();
    h.click(chip.center());
    h.click_text("Link set B");
    assert_eq!(
        h.shell.st.session.workspace.tab(hn).unwrap().link,
        Some('B')
    );
    // Now TB jumps alone.
    let before = location(&mut h, hn);
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 33:1" }));
    h.frames(3);
    assert_eq!(location(&mut h, hn), before);
    // Right-click the HNUM tab title: back into A, and it moves to A's location.
    let title = h.tab_at("HNUM");
    h.events.push(Event::PointerMoved(title));
    h.frame();
    for pressed in [true, false] {
        h.events.push(Event::PointerButton {
            pos: title,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frames(2);
    h.click_text("Link set A");
    h.frames(3);
    assert_eq!(
        h.shell.st.session.workspace.tab(hn).unwrap().link,
        Some('A')
    );
    assert_eq!(location(&mut h, hn), Some(ge(33, 1)));
}

#[test]
fn verse_numbers_show_the_standard_equivalent_where_it_differs() {
    let (mut h, _tb, hn) = linked_harness();
    h.shell
        .run("tab.go", &json!({ "tab": hn.0, "text": "Gen 32:1" }));
    h.frames(4);
    // Find HNUM's "2" verse number (32:2, Standard 32:1) on screen and hover it.
    let view = h.shell.st.tabs[&hn].view.as_ref().unwrap();
    let [ox, oy] = view.text_origin;
    let mut spot = None;
    for (_, top, laid) in view.laid_in_view(800.0) {
        for line in &laid.lines {
            for item in &line.items {
                for (bytes, x0, x1) in item.clusters() {
                    if let Some(obs_layout::UnitPos::Object {
                        object: obs_layout::InlineObject::VerseNumber(v),
                        ..
                    }) = laid.to_unit(bytes.start)
                        && v == ge(32, 2)
                    {
                        spot = Some(pos2(
                            ox + (x0 + x1) / 2.0,
                            oy + top + (line.top + line.bottom) / 2.0,
                        ));
                    }
                }
            }
        }
    }
    let spot = spot.expect("verse number 32:2 on screen");
    h.events.push(Event::PointerMoved(spot));
    h.frames(3);
    assert!(
        h.find("Standard numbering: Genesis 32:1").is_some(),
        "{:?}",
        h.texts()
    );
    // TB numbers like the Standard: no tooltip over its verse numbers.
    let view = h
        .shell
        .st
        .tabs
        .values()
        .find_map(|t| t.view.as_ref().filter(|v| v.doc.verse_map.is_none()))
        .unwrap();
    assert!(view.doc.verse_map.is_none());
}

fn temp_db(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-shell-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("user.db")
}

/// What a user would call "the Workspace": Panels (Tab titles in order, left to right),
/// each Tab's location and Link set, and the focused Tab.
type TabState = (String, Option<obs_scripture::Verse>, Option<char>);
type WorkspaceState = (Vec<Vec<String>>, Vec<TabState>, Option<String>);

fn workspace_state(h: &mut Harness) -> WorkspaceState {
    let panels: Vec<Vec<String>> = h.panels().into_iter().map(|(_, t)| t).collect();
    let ws = &h.shell.st.session.workspace;
    let mut tabs: Vec<TabState> = ws
        .tabs()
        .map(|t| (t.title.clone(), t.history.current(), t.link))
        .collect();
    tabs.sort_by(|a, b| a.0.cmp(&b.0));
    let focused = ws
        .focused()
        .and_then(|f| ws.tab(f))
        .map(|t| t.title.clone());
    (panels, tabs, focused)
}

#[test]
fn quit_and_relaunch_restores_panels_tabs_locations_and_link_sets() {
    let db = temp_db("relaunch");
    let mut h = Harness::launch(Some(&db));
    // Arrange: TB at Gen 32:10 in A; HNUM split off in A (follows to 32:11); HEB in Link set
    // B at Gen 1:2; Cross-references docked with TB; the gap dragged.
    let tb = h.tab_id("TB");
    h.shell.st.session.workspace.user_focused(tb);
    h.shell
        .run("resource.open", &json!({ "id": "heb-num", "split": true }));
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 32:10" }));
    h.shell.st.session.workspace.user_focused(tb);
    h.shell.run(
        "tool.cross_references",
        &json!({ "panel": h.shell.st.session.workspace.panel_of(tb).unwrap().0 }),
    );
    h.shell
        .run("resource.open", &json!({ "id": "heb-test", "split": true }));
    let heb = h.tab_id("HEB");
    h.shell
        .run("tab.link", &json!({ "tab": heb.0, "set": "B" }));
    h.shell
        .run("tab.go", &json!({ "tab": heb.0, "text": "Gen 1:2" }));
    h.frames(4);
    let (left, _) = h.panels()[0].clone();
    let gap = pos2(left.right() + 3.0, left.center().y);
    h.drag(gap, gap + vec2(120.0, 0.0));
    let width = h.panels()[0].0.width();
    let before = workspace_state(&mut h);
    assert_eq!(before.0.len(), 4, "{:?}", before.0);
    // Quit: the window's close request saves the Last session.
    h.shell.save_session(true);
    drop(h);

    let mut h = Harness::launch(Some(&db));
    h.frames(4);
    let after = workspace_state(&mut h);
    assert_eq!(after, before);
    // The views are where the locations say, and the Panel kept its size.
    assert_eq!(loc_of(&mut h, "TB"), Some(ge(32, 10)));
    assert_eq!(loc_of(&mut h, "HNUM"), Some(ge(32, 11)));
    assert!(
        (h.panels()[0].0.width() - width).abs() < 2.0,
        "{width} vs {}",
        h.panels()[0].0.width()
    );
    // Link sets still work after the restore: TB jumps, HNUM follows, HEB (set B) does not.
    let heb_at = loc_of(&mut h, "HEB");
    h.shell.run(
        "tab.go",
        &json!({ "tab": h.tab_id("TB").0, "text": "Gen 33:2" }),
    );
    h.frames(4);
    assert_eq!(loc_of(&mut h, "HNUM"), Some(ge(33, 2)));
    assert_eq!(loc_of(&mut h, "HEB"), heb_at);
}

#[test]
fn a_layout_round_trips_and_can_be_renamed_and_deleted_from_the_layouts_launcher() {
    let db = temp_db("layouts");
    let mut h = Harness::launch(Some(&db));
    let tb = h.tab_id("TB");
    h.shell
        .run("resource.open", &json!({ "id": "heb-num", "split": true }));
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:20" }));
    h.frames(3);
    let saved = workspace_state(&mut h);
    // Layouts → Save layout as… asks for a name.
    h.open_menu("Layouts");
    h.click_text("Save layout as…");
    assert!(h.find("Save layout as").is_some(), "{:?}", h.texts());
    h.events.push(Event::Text("Two Bibles".into()));
    h.frame();
    h.click_text("Save");
    let user = h.shell.st.session.user.as_ref().unwrap();
    assert_eq!(user.layouts().unwrap(), ["Two Bibles"]);
    // Change everything, then restore it from the menu.
    h.shell.run("layout.reset", &json!({}));
    h.shell.run(
        "tab.go",
        &json!({ "tab": h.tab_id("TB").0, "text": "Gen 30:5" }),
    );
    h.frames(3);
    assert_ne!(workspace_state(&mut h), saved);
    // One click on its card loads it.
    h.open_menu("Layouts");
    h.click_text("Two Bibles");
    h.frames(3);
    assert_eq!(workspace_state(&mut h), saved);
    assert_eq!(loc_of(&mut h, "TB"), Some(ge(31, 20)));
    assert!(h.shell.st.toolbar.open.is_none(), "loading closes the pane");
    // Rename in place: the picked card's Rename edits its name; Enter keeps it.
    h.open_menu("Layouts");
    h.click_text("Rename");
    for _ in 0.."Two Bibles".len() {
        h.events.push(Event::Key {
            key: egui::Key::Backspace,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
    }
    h.events.push(Event::Text("Genesis study".into()));
    h.frame();
    h.events.push(Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(2);
    let user = h.shell.st.session.user.as_ref().unwrap();
    assert_eq!(user.layouts().unwrap(), ["Genesis study"]);
    assert!(
        h.find("Genesis study").is_some(),
        "the pane stays open, renamed"
    );
    // Delete in place.
    h.click_text("Delete");
    let user = h.shell.st.session.user.as_ref().unwrap();
    assert!(user.layouts().unwrap().is_empty());
    let log = h.shell.st.session.command_log();
    for id in [
        "layout.save",
        "layout.restore",
        "layout.rename",
        "layout.delete",
    ] {
        assert!(log.contains(&id), "{id} not run: {log:?}");
    }
}

#[test]
fn settings_persist_across_restarts() {
    use obs_engine::{PageTone, Theme};
    use obs_layout::VerseNumbers;
    let db = temp_db("prefs");
    let mut h = Harness::launch(Some(&db));
    let size = h.shell.st.session.text.size;
    h.shell.run("view.text_larger", &json!({}));
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.shell.run("view.page_tone", &json!({ "value": "sepia" }));
    h.shell
        .run("view.line_spacing", &json!({ "value": "relaxed" }));
    h.shell
        .run("view.verse_numbers", &json!({ "value": "inline" }));
    h.shell.run("view.red_letter", &json!({}));
    drop(h);
    let mut h = Harness::launch(Some(&db));
    let s = &h.shell.st.session;
    assert_eq!(s.text.size, size + 1.0);
    assert_eq!(s.appearance.theme, Theme::Dark);
    assert_eq!(s.appearance.page_tone, PageTone::Sepia);
    assert_eq!(s.text.line_spacing, 1.45);
    assert_eq!(s.text.verse_numbers, VerseNumbers::Inline);
    assert!(!s.text.red_letter);
    // And they are in effect: dark chrome, sepia pages.
    h.frame();
    assert_eq!(h.ctx.theme(), egui::Theme::Dark);
    assert_eq!(h.shell.st.palette, crate::Palette::SEPIA);
}

/// Layout and rasterization work done so far, across every open view and the atlas.
fn text_work(h: &Harness) -> (u64, u64) {
    let layouts = h
        .shell
        .st
        .tabs
        .values()
        .filter_map(|t| t.view.as_ref())
        .map(|v| v.layouts)
        .sum();
    (layouts, h.shell.st.atlas.atlas.stats().rasters)
}

/// Run paced frames until no view has layout work in flight for three frames running, so
/// nothing a worker finishes later can be mistaken for work caused by the test.
fn settle_all(h: &mut Harness) {
    let mut idle = 0;
    for _ in 0..400 {
        h.paced_frame();
        let busy = h
            .shell
            .st
            .tabs
            .values()
            .filter_map(|t| t.view.as_ref())
            .any(|v| v.has_pending() || v.is_following());
        idle = if busy { 0 } else { idle + 1 };
        if idle >= 3 {
            return;
        }
    }
}

/// The colours of the glyph quads drawn last frame (vertex colours of textured meshes).
fn glyph_colors(h: &Harness) -> Vec<egui::Color32> {
    h.meshes.clone()
}

#[test]
fn switching_theme_or_tone_is_instant_and_never_relays_out_or_rerasters() {
    let mut h = Harness::new();
    h.shell
        .run("resource.open", &json!({ "id": "heb-test", "split": true }));
    settle_all(&mut h);
    let work = text_work(&h);
    let before = glyph_colors(&h);
    assert!(!before.is_empty());
    assert_eq!(h.shell.st.palette, crate::Palette::PAPER);
    // View → Theme → Dark, through the menus.
    h.open_menu("View");
    h.click_text("Theme");
    h.click_text("Dark");
    // On the very next frame: dark chrome, the Auto tone resolves to Dark, new glyph colours.
    assert_eq!(h.shell.st.session.appearance.theme, obs_engine::Theme::Dark);
    assert_eq!(h.ctx.theme(), egui::Theme::Dark);
    assert_eq!(h.shell.st.palette, crate::Palette::DARK);
    let dark = glyph_colors(&h);
    assert_eq!(dark.len(), before.len());
    assert_ne!(dark, before);
    // Page tone Sepia inside dark chrome (#21 decision 2).
    h.shell.run("view.page_tone", &json!({ "value": "sepia" }));
    h.frame();
    assert_eq!(h.shell.st.palette, crate::Palette::SEPIA);
    assert_eq!(h.ctx.theme(), egui::Theme::Dark);
    let sepia = glyph_colors(&h);
    assert_ne!(sepia, dark);
    // Back to Follow system and Auto: the OS is light, so are the chrome and pages …
    h.shell.run("view.theme", &json!({ "value": "system" }));
    h.shell.run("view.page_tone", &json!({ "value": "auto" }));
    h.frame();
    assert_eq!(h.ctx.theme(), egui::Theme::Light);
    assert_eq!(h.shell.st.palette, crate::Palette::PAPER);
    // … and when the OS turns dark, the app follows on the next frame.
    h.os_theme = egui::Theme::Dark;
    h.frame();
    assert_eq!(h.ctx.theme(), egui::Theme::Dark);
    assert_eq!(h.shell.st.palette, crate::Palette::DARK);
    h.frames(2);
    assert_eq!(
        text_work(&h),
        work,
        "a theme or tone change re-laid out or re-rastered text"
    );
}

#[test]
fn text_settings_relayout_and_take_effect() {
    let mut h = Harness::new();
    settle_all(&mut h);
    let lines = |h: &Harness| -> usize {
        let v = h
            .shell
            .st
            .tabs
            .values()
            .find_map(|t| t.view.as_ref())
            .unwrap();
        v.laid_in_view(800.0)
            .iter()
            .map(|(_, _, l)| l.lines.len())
            .sum()
    };
    let height = |h: &Harness| -> f32 {
        let v = h
            .shell
            .st
            .tabs
            .values()
            .find_map(|t| t.view.as_ref())
            .unwrap();
        v.laid_in_view(800.0)
            .first()
            .map(|(_, _, l)| l.height)
            .unwrap()
    };
    let h0 = height(&h);
    h.shell
        .run("view.line_spacing", &json!({ "value": "loose" }));
    settle_all(&mut h);
    assert!(height(&h) > h0 * 1.2, "{} vs {h0}", height(&h));
    let n = lines(&h);
    h.shell.run("view.text_larger", &json!({}));
    h.shell.run("view.text_larger", &json!({}));
    settle_all(&mut h);
    assert!(lines(&h) <= n);
    // Hiding verse numbers lays the text out again without them.
    let before = text_work(&h).0;
    h.shell
        .run("view.verse_numbers", &json!({ "value": "hidden" }));
    settle_all(&mut h);
    assert!(text_work(&h).0 > before);
    assert!(
        h.shell
            .run("view.verse_numbers", &json!({ "value": "nope" }))
            .is_none()
    );
}

mod a11y {
    use egui::accesskit::{Node, NodeId, Role};

    use super::*;

    fn nodes(h: &Harness) -> std::collections::HashMap<NodeId, Node> {
        h.tree.as_ref().unwrap().nodes.iter().cloned().collect()
    }

    fn children_with(
        nodes: &std::collections::HashMap<NodeId, Node>,
        id: NodeId,
        role: Role,
    ) -> Vec<NodeId> {
        nodes[&id]
            .children()
            .iter()
            .copied()
            .filter(|c| nodes.get(c).is_some_and(|n| n.role() == role))
            .collect()
    }

    fn harness() -> Harness {
        let mut h = Harness::new();
        add_doc(&mut h.shell, crate::tests::tagged_bible());
        let tb = h.tab_id("TB");
        h.shell.run("tab.close", &json!({ "tab": tb.0 }));
        h.shell.run("resource.open", &json!({ "id": "tagged" }));
        h.ctx.enable_accesskit();
        settle_all(&mut h);
        h
    }

    fn document(h: &Harness) -> NodeId {
        let n = nodes(h);
        let docs: Vec<NodeId> = n
            .iter()
            .filter(|(_, n)| n.role() == Role::Document)
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(docs.len(), 1, "one reading pane, one Document");
        docs[0]
    }

    #[test]
    fn the_tree_has_paragraphs_and_links_for_visible_blocks_only() {
        let mut h = harness();
        let n = nodes(&h);
        let doc = document(&h);
        assert_eq!(n[&doc].label(), Some("Tagged Bible"));
        let paras = children_with(&n, doc, Role::Paragraph);
        let view = h
            .shell
            .st
            .tabs
            .values()
            .find_map(|t| t.view.as_ref())
            .unwrap();
        let visible = view.laid_in_view(900.0);
        assert!(!paras.is_empty());
        assert!(
            paras.len() <= visible.len(),
            "{} paragraphs for {} visible blocks",
            paras.len(),
            visible.len()
        );
        assert!(
            paras.len() < view.doc.blocks.len(),
            "the whole document was published"
        );
        // Each paragraph's runs are its lines, and together they are the block's text.
        let mut words = 0;
        for (p, (_, _, laid)) in paras.iter().zip(&visible) {
            let runs = children_with(&n, *p, Role::TextRun);
            assert_eq!(runs.len(), laid.lines.len());
            let text: String = runs.iter().map(|r| n[r].value().unwrap()).collect();
            assert_eq!(text, laid.text);
            for r in &runs {
                let v = n[r].value().unwrap();
                assert_eq!(n[r].character_lengths().len(), v.chars().count());
                assert!(n[r].bounds().is_some());
            }
            let links = children_with(&n, *p, Role::Link);
            for l in &links {
                let node = &n[l];
                assert!(node.bounds().is_some());
                let d = node.description().unwrap();
                assert!(
                    d.starts_with("Strong's H1") || d == "Go to Romans 8:28",
                    "{d}"
                );
            }
            words += links.len();
        }
        assert!(words > 50, "{words} links");
        // Scroll far away: the published text follows the view.
        let old: String = paras
            .iter()
            .flat_map(|p| children_with(&n, *p, Role::TextRun))
            .filter_map(|r| n[&r].value())
            .collect();
        h.shell.run(
            "tab.go",
            &json!({ "tab": h.tab_id("TAG").0, "text": "Gen 31:20" }),
        );
        settle_all(&mut h);
        let n2 = nodes(&h);
        let all_text: String = n2
            .values()
            .filter(|x| x.role() == Role::TextRun)
            .filter_map(|x| x.value())
            .collect();
        assert_ne!(all_text, old);
        assert!(all_text.contains("Romans 8:28"));
    }

    #[test]
    fn moving_the_selection_updates_the_exposed_text_selection() {
        let mut h = harness();
        assert!(nodes(&h)[&document(&h)].text_selection().is_none());
        let selected = |h: &Harness| -> String {
            let n = nodes(h);
            let doc = document(h);
            let sel = n[&doc].text_selection().expect("a text selection");
            // Walk the runs in order from anchor to focus and collect the characters.
            let runs: Vec<NodeId> = children_with(&n, doc, Role::Paragraph)
                .into_iter()
                .flat_map(|p| children_with(&n, p, Role::TextRun))
                .collect();
            let pos = |p: &egui::accesskit::TextPosition| {
                (
                    runs.iter().position(|r| *r == p.node).unwrap(),
                    p.character_index,
                )
            };
            let (a, f) = (pos(&sel.anchor), pos(&sel.focus));
            let (s, e) = if a <= f { (a, f) } else { (f, a) };
            let mut out = String::new();
            for (i, r) in runs.iter().enumerate().take(e.0 + 1).skip(s.0) {
                let chars: Vec<char> = n[r].value().unwrap().chars().collect();
                let from = if i == s.0 { s.1 } else { 0 };
                let to = if i == e.0 { e.1 } else { chars.len() };
                out.extend(&chars[from..to]);
            }
            out
        };
        // Drag across text in the pane.
        let view = h
            .shell
            .st
            .tabs
            .values()
            .find_map(|t| t.view.as_ref())
            .unwrap();
        let [ox, oy] = view.text_origin;
        h.drag(pos2(ox + 40.0, oy + 12.0), pos2(ox + 260.0, oy + 80.0));
        let first = selected(&h);
        let view = h
            .shell
            .st
            .tabs
            .values()
            .find_map(|t| t.view.as_ref())
            .unwrap();
        let sel = view.selection.unwrap();
        let want = crate::selected_text(&view.doc, &sel);
        assert!(first.len() > 20, "{first:?}");
        // The exposed text has the verse numbers the reader sees; the source has none.
        let strip = |s: &str| s.chars().filter(|c| c.is_alphabetic()).collect::<String>();
        let shown = strip(&first);
        assert!(
            shown.ends_with(&strip(&want)[strip(&want).len().saturating_sub(10)..]),
            "{first:?} vs {want:?}"
        );
        // Move it: a backward drag lower down.
        h.drag(pos2(ox + 300.0, oy + 160.0), pos2(ox + 30.0, oy + 110.0));
        let second = selected(&h);
        assert_ne!(first, second);
        let n = nodes(&h);
        let sel = n[&document(&h)].text_selection().unwrap();
        assert_ne!(sel.anchor, sel.focus);
        // Clearing the selection (a click) clears it in the tree.
        h.click(pos2(ox + 100.0, oy + 200.0));
        assert!(nodes(&h)[&document(&h)].text_selection().is_none());
    }

    #[test]
    fn without_a_screen_reader_nothing_is_built() {
        let mut h = Harness::new();
        h.frames(2);
        assert!(h.tree.is_none());
    }

    /// Whether the open menu's `label` item is on, as a screen reader hears it: the check
    /// beside a toggle is an image, so the state is the node's (#220).
    fn toggled(h: &Harness, label: &str) -> Option<bool> {
        use egui::accesskit::Toggled;
        let n = nodes(h);
        let item = n
            .values()
            .find(|n| n.role() == Role::Button && n.label() == Some(label))
            .unwrap_or_else(|| panic!("no {label} item"));
        item.toggled().map(|t| t == Toggled::True)
    }

    #[test]
    fn toggle_menu_items_tell_screen_readers_whether_they_are_on() {
        let mut h = Harness::new();
        h.ctx.enable_accesskit();
        h.frames(2);
        for (label, before) in [
            ("Red letter", true),
            ("Show Highlights", true),
            ("Auto-hide toolbar", false),
        ] {
            h.open_menu("View");
            assert_eq!(toggled(&h, label), Some(before), "{label}");
            h.click_text(label);
            h.frames(2);
            h.open_menu("View");
            assert_eq!(toggled(&h, label), Some(!before), "{label} after a click");
            h.events.push(Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
            h.frames(2);
        }
        // A command that is not a toggle is not announced as one.
        h.open_menu("View");
        assert_eq!(toggled(&h, "Customize toolbar…"), None);

        // The toolbar's right-click menu: Auto-hide (now on) reads as on.
        h.events.push(Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.frames(2);
        h.shell.run("view.toolbar_auto_hide", &json!({}));
        h.frames(2);
        assert!(!h.shell.st.session.toolbar.auto_hide);
        let tools = h.shell.st.toolbar.buttons[&obs_engine::Launcher::Tools].center();
        h.events.push(Event::PointerMoved(tools));
        h.frame();
        for pressed in [true, false] {
            h.events.push(Event::PointerButton {
                pos: tools,
                button: PointerButton::Secondary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.frame();
        }
        h.frames(2);
        assert_eq!(toggled(&h, "Auto-hide"), Some(false));
        h.click_text("Auto-hide");
        h.frames(2);
        assert!(h.shell.st.session.toolbar.auto_hide);
    }
}

/// The Library Tab and the About Tab (#46).
mod library {
    use super::*;

    use super::super::library_tab::ROW_H;

    const ABBREVS: [&str; 5] = ["TB", "HEB", "HNUM", "KJV", "DICT"];

    /// The Panel holding the Library Tab.
    fn library_panel(h: &Harness) -> Rect {
        h.panels()
            .into_iter()
            .find(|(_, t)| t.iter().any(|t| t == "Library"))
            .map(|(r, _)| r)
            .unwrap()
    }

    /// Library rows top to bottom: (abbreviation, row centre y).
    fn rows(h: &Harness) -> Vec<(String, f32)> {
        let panel = library_panel(h);
        let mut out: Vec<(String, f32)> = h
            .shapes
            .iter()
            .filter(|(r, t)| ABBREVS.contains(&t.as_str()) && panel.contains(r.center()))
            .map(|(r, t)| (t.clone(), r.center().y))
            .collect();
        out.sort_by(|a, b| a.1.total_cmp(&b.1));
        out
    }

    fn row_names(h: &Harness) -> Vec<String> {
        rows(h).into_iter().map(|(t, _)| t).collect()
    }

    /// The row the ★ is on.
    fn starred(h: &Harness) -> String {
        let star = h.at("[star-filled]");
        rows(h)
            .into_iter()
            .find(|(_, y)| (y - star.y).abs() < 6.0)
            .map(|(t, _)| t)
            .unwrap_or_else(|| panic!("★ on no row: {:?}", rows(h)))
    }

    fn row_pos(h: &Harness, abbrev: &str) -> Pos2 {
        let panel = library_panel(h);
        let r = h
            .shapes
            .iter()
            .find(|(r, t)| t == abbrev && panel.contains(r.center()))
            .map(|(r, _)| *r)
            .unwrap_or_else(|| panic!("no row {abbrev}: {:?}", rows(h)));
        r.center()
    }

    /// Where a row's drag handle is.
    fn handle(h: &Harness, abbrev: &str) -> Pos2 {
        let y = row_pos(h, abbrev).y;
        let (r, _) = h
            .shapes
            .iter()
            .rfind(|(r, t)| t == "[grip-vertical]" && (r.center().y - y).abs() < 6.0)
            .unwrap();
        r.center()
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

    fn double_click(h: &mut Harness, p: Pos2) {
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

    fn key(h: &mut Harness, key: egui::Key) {
        h.events.push(Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.frames(2);
    }

    #[test]
    fn rows_are_grouped_by_type_with_language_and_licence() {
        let mut h = Harness::new();
        h.ctx.fonts_mut(|fonts| {
            assert!(fonts.has_glyph(&egui::FontId::proportional(14.0), '★'));
            assert!(fonts.has_glyph(&egui::FontId::monospace(14.0), '≡'));
        });
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM"]);
        let panel = library_panel(&h);
        for t in [
            "BIBLES",
            "Test Bible",
            "hbo",
            "LicenseRef-PD",
            "English",
            "Hebrew",
        ] {
            assert!(
                h.shapes
                    .iter()
                    .any(|(r, s)| s == t && panel.contains(r.center())),
                "{t:?} missing: {:?}",
                h.texts()
            );
        }
        assert_eq!(starred(&h), "TB");

        h.shell.st.session.library.add(LibraryEntry {
            manifest: obs_model::Manifest::new(
                "test-dictionary",
                "Test Dictionary",
                "DICT",
                obs_model::ResourceType::Dictionary,
                "en",
            ),
            path: "/nonexistent/test-dictionary.obr".into(),
        });
        h.frames(2);
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM", "DICT"]);
        let bibles = h.find("BIBLES").unwrap();
        let dictionaries = h.find("DICTIONARIES").unwrap();
        assert!(bibles.bottom() < row_pos(&h, "TB").y);
        assert!(row_pos(&h, "HNUM").y < dictionaries.top());
        assert!(dictionaries.bottom() < row_pos(&h, "DICT").y);
        assert!(h.find("Test Dictionary").is_some());
    }

    #[test]
    fn reordering_persists_across_restarts_and_changes_which_bible_is_starred() {
        let db = temp_db("preferred-order");
        let mut h = Harness::launch(Some(&db));
        // Drag HNUM's handle onto the upper half of TB: HNUM becomes first, and ★.
        let from = handle(&h, "HNUM");
        let to = row_pos(&h, "TB") - vec2(0.0, 8.0);
        h.drag(from, to);
        assert_eq!(row_names(&h), ["HNUM", "TB", "HEB"]);
        assert_eq!(starred(&h), "HNUM");
        // Move down from the right-click menu.
        let p = row_pos(&h, "HNUM");
        right_click(&mut h, p);
        h.click_text("Move down");
        assert_eq!(row_names(&h), ["TB", "HNUM", "HEB"]);
        let p = row_pos(&h, "HEB");
        right_click(&mut h, p);
        h.click_text("Move up");
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM"]);
        h.drag(handle(&h, "HNUM"), row_pos(&h, "TB") - vec2(0.0, 8.0));
        assert_eq!(row_names(&h), ["HNUM", "TB", "HEB"]);
        let log = h.shell.st.session.command_log();
        for id in ["library.move", "library.move_up", "library.move_down"] {
            assert!(log.contains(&id), "{id} not run: {log:?}");
        }
        h.shell.save_session(true);
        drop(h);

        // Relaunch: the same order, the same ★, and the ★ Bible is the one lookups use.
        let mut h = Harness::launch(Some(&db));
        assert_eq!(row_names(&h), ["HNUM", "TB", "HEB"]);
        assert_eq!(starred(&h), "HNUM");
        h.shell.run("layout.reset", &json!({}));
        h.frames(3);
        assert_eq!(h.panels()[0].1, ["HNUM"]);
    }

    #[test]
    fn a_dragged_row_lands_in_the_gap_the_drop_line_marks() {
        let mut h = Harness::new();
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM"]);
        // Dropped in the gap between TB and HEB: between them, so TB keeps ★.
        let (tb, heb) = (row_pos(&h, "TB"), row_pos(&h, "HEB"));
        h.drag(handle(&h, "HNUM"), pos2(tb.x, (tb.y + heb.y) / 2.0));
        assert_eq!(row_names(&h), ["TB", "HNUM", "HEB"]);
        assert_eq!(starred(&h), "TB");
        // Dropped on the lower half of TB: below TB, not above it.
        h.drag(handle(&h, "HEB"), row_pos(&h, "TB") + vec2(0.0, 8.0));
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM"]);
        assert_eq!(starred(&h), "TB");
        // TB on the upper half of HEB stays put; on its lower half it goes below HEB.
        h.drag(handle(&h, "TB"), row_pos(&h, "HEB") - vec2(0.0, 8.0));
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM"]);
        h.drag(handle(&h, "TB"), row_pos(&h, "HEB") + vec2(0.0, 8.0));
        assert_eq!(row_names(&h), ["HEB", "TB", "HNUM"]);
        assert_eq!(starred(&h), "HEB");
        // Dropped well away from the rows: cancelled.
        let far = library_panel(&h).bottom() - 10.0;
        assert!(far > row_pos(&h, "HNUM").y + 3.0 * ROW_H, "panel too short");
        h.drag(handle(&h, "HNUM"), pos2(tb.x, far));
        assert_eq!(row_names(&h), ["HEB", "TB", "HNUM"]);
        // Escape cancels a drag in progress.
        let from = handle(&h, "HNUM");
        let to = row_pos(&h, "HEB") - vec2(0.0, 8.0);
        h.events.push(Event::PointerMoved(from));
        h.frame();
        h.button(from, true);
        h.frame();
        for i in 1..=10 {
            h.events
                .push(Event::PointerMoved(from + (to - from) * (i as f32 / 10.0)));
            h.frame();
        }
        key(&mut h, egui::Key::Escape);
        h.button(to, false);
        h.frames(3);
        assert_eq!(row_names(&h), ["HEB", "TB", "HNUM"]);
        let moves = h
            .shell
            .st
            .session
            .command_log()
            .iter()
            .filter(|c| **c == "library.move")
            .count();
        assert_eq!(moves, 3, "one library.move per drop that moved a row");
    }

    #[test]
    fn dropping_over_another_panel_does_not_change_or_persist_preferred_order() {
        let db = temp_db("outside-library-drop");
        let mut h = Harness::launch(Some(&db));
        let panel = library_panel(&h);
        let y = row_pos(&h, "TB").y - 8.0;
        let other = h
            .panels()
            .into_iter()
            .map(|(r, _)| r)
            .find(|r| !r.intersects(panel) && r.y_range().contains(y))
            .expect("another Panel alongside Library");
        h.drag(handle(&h, "HNUM"), pos2(other.center().x, y));
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM"]);
        assert_eq!(starred(&h), "TB");
        assert!(!h.shell.st.session.command_log().contains(&"library.move"));
        h.shell.save_session(true);
        drop(h);
        let h = Harness::launch(Some(&db));
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM"]);
        assert_eq!(starred(&h), "TB");
    }

    #[test]
    fn filtering_by_text_and_by_language_works_together() {
        let mut h = Harness::new();
        // Text: "hebrew" matches the Hebrew test Bible and the Hebrew-numbered one.
        h.click_text("Filter");
        h.events.push(Event::Text("hebrew".into()));
        h.frames(2);
        assert_eq!(row_names(&h), ["HEB", "HNUM"]);
        // With the Hebrew chip on as well, only the Hebrew-language one is left.
        h.click_text("Hebrew");
        assert_eq!(row_names(&h), ["HEB"]);
        // The English chip adds English Resources that also match the text.
        h.click_text("English");
        assert_eq!(row_names(&h), ["HEB", "HNUM"]);
        // Clearing the text leaves the chips filtering.
        let t = h.tab_id("Library");
        h.shell.st.tabs.get_mut(&t).unwrap().filter.clear();
        h.frames(2);
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM"]);
        h.click_text("English");
        assert_eq!(row_names(&h), ["HEB"]);
        h.click_text("Hebrew");
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM"]);
        // A filter that matches nothing says so.
        h.shell.st.tabs.get_mut(&t).unwrap().filter = "zzz".into();
        h.frames(2);
        assert!(row_names(&h).is_empty());
        assert!(h.find("Nothing matches the filter.").is_some());
    }

    #[test]
    fn removing_the_last_resource_of_a_selected_language_restores_unfiltered_rows() {
        let mut h = Harness::new();
        h.click_text("Hebrew");
        assert_eq!(row_names(&h), ["HEB"]);
        let p = row_pos(&h, "HEB");
        right_click(&mut h, p);
        h.click_text("Remove…");
        h.click_text("Cancel");
        assert_eq!(row_names(&h), ["HEB"], "Cancel keeps the language filter");

        let p = row_pos(&h, "HEB");
        right_click(&mut h, p);
        h.click_text("Remove…");
        h.click_text("Remove");
        assert!(h.shell.st.session.library.get("heb-test").is_none());
        assert_eq!(h.shell.st.session.library.languages(), ["English"]);
        assert_eq!(row_names(&h), ["TB", "HNUM"]);
        let tab = h.tab_id("Library");
        assert!(h.shell.st.tabs[&tab].languages.is_empty());
        assert!(h.find("Nothing matches the filter.").is_none());

        // The remaining chip still toggles normally; no hidden selection survives.
        h.click_text("English");
        h.click_text("English");
        assert_eq!(row_names(&h), ["TB", "HNUM"]);
        assert!(h.shell.st.tabs[&tab].languages.is_empty());
    }

    #[test]
    fn enter_double_click_and_right_click_open() {
        let mut h = Harness::new();
        let count = |h: &Harness, id: &str| {
            h.shell
                .st
                .session
                .workspace
                .tabs()
                .filter(|t| t.kind == TabKind::Resource(id.into()))
                .count()
        };
        let p = row_pos(&h, "HEB");
        double_click(&mut h, p);
        assert_eq!(count(&h, "heb-test"), 1);
        // Back to the Library: select HNUM with a click, then Enter.
        h.click_text("Library");
        let p = row_pos(&h, "HNUM");
        h.click(p);
        assert_eq!(count(&h, "heb-num"), 0, "a single click only selects");
        key(&mut h, egui::Key::Enter);
        assert_eq!(count(&h, "heb-num"), 1);
        h.click_text("Library");
        let p = row_pos(&h, "HEB");
        right_click(&mut h, p);
        h.click_text("Open");
        assert_eq!(count(&h, "heb-test"), 2);
    }

    #[test]
    fn tab_focused_row_opens_on_enter_without_activating_the_filter() {
        let mut h = Harness::new();
        h.click(row_pos(&h, "HNUM"));
        let library = h.tab_id("Library");
        let opens = |h: &Harness| {
            h.shell
                .st
                .session
                .command_log()
                .iter()
                .filter(|c| **c == "resource.open")
                .count()
        };
        let before = opens(&h);
        h.click_text("Filter");
        assert!(h.ctx.memory(|m| m.focused().is_some()));
        key(&mut h, egui::Key::Enter);
        assert_eq!(opens(&h), before, "Enter in the filter must not open a row");

        // Traverse actual widgets, not a synthetic request_focus: HEB has keyboard focus
        // while HNUM remains the mouse-selected row.
        let target = row_pos(&h, "HEB");
        let mut focused_row = false;
        // Tab wraps round through the menu bar (the Launchers, Home, the Command box).
        for _ in 0..64 {
            key(&mut h, egui::Key::Tab);
            if h.ctx
                .memory(|m| m.focused())
                .and_then(|id| h.ctx.read_response(id))
                .is_some_and(|r| r.has_focus() && r.rect.contains(target))
            {
                focused_row = true;
                break;
            }
        }
        assert!(focused_row, "Tab must reach the HEB row");
        assert_eq!(
            h.shell.st.tabs[&library].selected.as_deref(),
            Some("heb-num")
        );
        assert_eq!(opens(&h), before, "Tab only moves focus");

        key(&mut h, egui::Key::Enter);
        assert_eq!(
            opens(&h),
            before + 1,
            "Enter runs resource.open exactly once"
        );
        let workspace = &h.shell.st.session.workspace;
        let opened = workspace.tab(workspace.focused().unwrap()).unwrap();
        assert_eq!(opened.kind, TabKind::Resource("heb-test".into()));
        assert_eq!(opened.title, "HEB");
    }

    #[test]
    fn enter_with_a_context_menu_open_does_not_open_the_selected_row() {
        let mut h = Harness::new();
        let opens = |h: &Harness| {
            h.shell
                .st
                .session
                .command_log()
                .iter()
                .filter(|c| **c == "resource.open")
                .count()
        };
        let before = opens(&h);
        let p = row_pos(&h, "HEB");
        right_click(&mut h, p);
        assert!(egui::Popup::is_any_open(&h.ctx));
        key(&mut h, egui::Key::Enter);
        assert_eq!(opens(&h), before, "Enter belongs to the open menu");
        assert_eq!(
            h.shell.st.session.workspace.focused(),
            Some(h.tab_id("Library"))
        );
    }

    #[test]
    fn sword_import_licence_attribution_and_source_url_render_in_about_tab() {
        let source = obs_formats::SwordSource::open(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../formats/tests/assets/sword"),
        )
        .unwrap();
        let (resource, _) =
            obs_formats::import_sword(&source, "ObsNotes", &obs_formats::SwordOptions::default())
                .unwrap();
        let manifest = resource.manifest;
        let mut h = Harness::new();
        h.shell.st.session.library.add(LibraryEntry {
            manifest: manifest.clone(),
            path: "/nonexistent/obsnotes.obr".into(),
        });
        h.shell
            .run("resource.about", &json!({ "id": manifest.resource_id }));
        h.frames(5);
        let about = h.tab_id("About · ObsNotes");
        assert_eq!(h.shell.st.session.workspace.focused(), Some(about));
        for text in [
            "Licence",
            "CC0-1.0",
            "Source",
            "https://github.com/inhesrom/openbiblesoftware",
            "ATTRIBUTION",
        ] {
            assert!(h.find(text).is_some(), "{text:?} missing: {:?}", h.texts());
        }
        let view = h.shell.st.tabs[&about].view.as_ref().unwrap();
        assert_eq!(view.doc.units[0].text, manifest.attribution);
        assert!(
            view.doc.units[0]
                .text
                .contains("Licence: Creative Commons: CC0")
        );
        assert!(
            view.doc.units[0]
                .text
                .contains("Copyright holder: Open Bible Software contributors")
        );
        assert!(
            !h.meshes.is_empty(),
            "Attribution glyphs painted through reading pipeline"
        );
        // Visual evidence for docs/previews/sword-about.png when `UPDATE_PREVIEWS` is set.
        if std::env::var_os("UPDATE_PREVIEWS").is_some() {
            let dir =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
            std::fs::create_dir_all(&dir).unwrap();
            h.screenshot(&dir.join("sword-about.ppm"));
        }
    }

    #[test]
    fn the_about_tab_shows_manifest_licence_attribution_and_annotation_count() {
        let mut h = Harness::new();
        let mut e = h
            .shell
            .st
            .session
            .library
            .get("test-bible")
            .unwrap()
            .clone();
        e.manifest.attribution = "Test Bible text: public domain.\nכתב — ἀγάπη".into();
        e.manifest.changes = Some("Reformatted for the app.".into());
        e.manifest.source_file = Some("tb.usfm".into());
        e.manifest.source_format = "usfm".into();
        h.shell.st.session.library.add(e.clone());
        let p = row_pos(&h, "TB");
        right_click(&mut h, p);
        h.click_text("About");
        h.frames(2);
        let about = h.tab_id("About · TB");
        assert_eq!(h.shell.st.session.workspace.focused(), Some(about));
        for t in [
            "Test Bible",
            "Licence",
            "LicenseRef-PD",
            "ATTRIBUTION",
            "CHANGES WE MADE",
            "Reformatted for the app.",
            "tb.usfm (usfm)",
            "No Annotations refer to this Resource.",
            "Remove…",
        ] {
            assert!(h.find(t).is_some(), "{t:?} missing: {:?}", h.texts());
        }
        // The Attribution is drawn by the text pipeline, verbatim.
        let view = h.shell.st.tabs[&about].view.as_ref().unwrap();
        assert_eq!(view.doc.units[0].text, e.manifest.attribution);
        assert_eq!(view.doc.blocks.len(), 2, "one paragraph per line");
        // Opening About again (from the Bible Tab's ⋯) focuses the same Tab.
        let tb = h.tab_id("TB");
        h.shell
            .run("resource.about", &json!({ "id": "test-bible" }));
        assert_eq!(h.shell.st.session.workspace.focused(), Some(about));
        h.shell.st.session.workspace.user_focused(tb);
        let abouts = h
            .shell
            .st
            .session
            .workspace
            .tabs()
            .filter(|t| matches!(t.kind, TabKind::About(_)))
            .count();
        assert_eq!(abouts, 1);
    }

    #[test]
    fn the_about_tab_applies_live_text_size_and_line_spacing() {
        let mut h = Harness::new();
        let mut entry = h
            .shell
            .st
            .session
            .library
            .get("test-bible")
            .unwrap()
            .clone();
        entry.manifest.attribution = "Public domain text.\nAttribution shown verbatim.".into();
        h.shell.st.session.library.add(entry);
        h.shell
            .run("resource.about", &json!({ "id": "test-bible" }));
        settle_all(&mut h);
        let about = h.tab_id("About · TB");
        let height = |h: &Harness| -> f32 {
            let view = h.shell.st.tabs[&about].view.as_ref().unwrap();
            let laid = view.laid_in_view(600.0);
            assert!(!laid.is_empty(), "Attribution must be laid out");
            laid.iter().map(|(_, _, block)| block.height).sum()
        };
        let initial = height(&h);
        h.shell.run("view.text_larger", &json!({}));
        settle_all(&mut h);
        let larger = height(&h);
        let view = h.shell.st.tabs[&about].view.as_ref().unwrap();
        assert_eq!(view.settings().size, h.shell.st.session.text.size);
        assert!(larger > initial, "larger text: {larger} <= {initial}");
        h.shell
            .run("view.line_spacing", &json!({ "value": "loose" }));
        settle_all(&mut h);
        let view = h.shell.st.tabs[&about].view.as_ref().unwrap();
        assert_eq!(
            view.settings().line_spacing,
            h.shell.st.session.text.line_spacing
        );
        assert!(height(&h) > larger * 1.2, "line spacing must change layout");
        assert_eq!(h.tab_id("About · TB"), about, "About was never reopened");
    }

    #[test]
    fn the_about_tab_shows_none_recorded_for_empty_attribution() {
        let mut h = Harness::new();
        let mut entry = h
            .shell
            .st
            .session
            .library
            .get("test-bible")
            .unwrap()
            .clone();
        entry.manifest.attribution.clear();
        entry.manifest.changes = Some("No text changes.".into());
        h.shell.st.session.library.add(entry);
        h.shell
            .run("resource.about", &json!({ "id": "test-bible" }));
        h.frames(2);
        assert!(h.find("None recorded.").is_some());
        assert!(h.find("No text changes.").is_some());
    }

    #[test]
    fn remove_deletes_the_obr_from_the_library_after_confirmation() {
        let dir = std::env::temp_dir().join(format!("obs-shell-remove-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("kjv-sample.obr");
        obs_store::write_obr(&obs_store::sample_resource(), &file).unwrap();
        let mut h = Harness::new();
        assert!(h.shell.st.session.library.scan(&dir).is_empty());
        h.frames(2);
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM", "KJV"]);

        // Remove… asks first, stating the Annotation count; Cancel keeps everything.
        let p = row_pos(&h, "KJV");
        right_click(&mut h, p);
        h.click_text("Remove…");
        let message = h.shell.st.confirm.as_ref().unwrap().2.clone();
        assert!(message.contains("No Annotations refer"), "{message}");
        assert!(h.find(&message).is_some(), "{:?}", h.texts());
        h.click_text("Cancel");
        assert!(h.shell.st.confirm.is_none());
        assert!(file.is_file());
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM", "KJV"]);

        // From the About Tab this time, and confirmed: the file is deleted and the row goes.
        h.shell
            .run("resource.about", &json!({ "id": "kjv-sample" }));
        h.frames(2);
        h.click_text("Remove…");
        assert!(h.shell.st.confirm.is_some());
        h.click_text("Remove");
        assert!(!file.exists(), "the .obr is deleted");
        assert!(h.shell.st.session.library.get("kjv-sample").is_none());
        assert!(
            h.shell
                .st
                .session
                .workspace
                .tabs()
                .all(|t| !t.title.contains("KJV"))
        );
        h.click_text("Library");
        assert_eq!(row_names(&h), ["TB", "HEB", "HNUM"]);
        drop(h);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

mod text_comparison {
    use super::*;
    use crate::compare_view::{ABSENT, NOT_COMPARED};
    use obs_engine::{Cell, Marks, TabId};

    /// TB (Link set A) and a Text Comparison beside it in Link set A, comparing TB, HEB and
    /// HNUM (the default: the first Bibles in Preferred order).
    fn harness() -> (Harness, TabId, TabId) {
        let mut h = Harness::new();
        let tb = h.tab_id("TB");
        h.shell.st.session.workspace.user_focused(tb);
        h.shell
            .run("tool.text_comparison", &json!({ "split": true }));
        h.frames(2);
        let cmp = h.tab_id("Text Comparison");
        h.shell
            .run("tab.link", &json!({ "tab": cmp.0, "set": "A" }));
        h.frames(2);
        (h, tb, cmp)
    }

    fn settle(h: &mut Harness, cmp: TabId) {
        for _ in 0..400 {
            h.paced_frame();
            let pending = h.shell.st.tabs[&cmp]
                .comparison
                .as_ref()
                .is_some_and(|v| v.has_pending());
            if !pending {
                h.frames(2);
                return;
            }
        }
        panic!("comparison never settled");
    }

    fn view(h: &Harness, cmp: TabId) -> &crate::compare_view::ComparisonView {
        h.shell.st.tabs[&cmp].comparison.as_ref().unwrap()
    }

    fn pane_rect(h: &Harness, tab: TabId) -> Rect {
        let tiles = &h.shell.tree.tiles;
        tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
    }

    /// Texts painted inside the comparison pane.
    fn texts_in(h: &Harness, tab: TabId) -> Vec<String> {
        let r = pane_rect(h, tab);
        h.shapes
            .iter()
            .filter(|(s, _)| r.contains(s.center()))
            .map(|(_, t)| t.clone())
            .collect()
    }

    #[test]
    fn gen_31_32_compares_kjv_and_a_hebrew_numbered_bible_on_the_same_rows() {
        let (mut h, tb, cmp) = harness();
        h.shell
            .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:55" }));
        settle(&mut h, cmp);
        let v = view(&h, cmp);
        assert_eq!(v.current_location(), Some(ge(31, 55)));
        // The whole chapter, one block per Standard Verse.
        assert_eq!(v.blocks.len(), 55);
        assert_eq!(v.blocks[0].verse, ge(31, 1));
        let row = |b: &obs_engine::VerseBlock, i: usize| b.cells[i].cell.clone();
        let b = v.blocks.iter().find(|b| b.verse == ge(31, 55)).unwrap();
        // TB 31:55 and HNUM 32:1 share the row; HEB (Gen 1 only) is absent.
        assert!(matches!(row(b, 0), Cell::Text { natives, .. } if natives == [(ge(31, 55), 55)]));
        assert_eq!(row(b, 1), Cell::Absent);
        assert!(matches!(row(b, 2), Cell::Text { natives, .. } if natives == [(ge(32, 1), 1)]));
        let texts = texts_in(&h, cmp);
        for want in ["Genesis 31:55", "TB", "HEB", "HNUM", "32:1", ABSENT] {
            assert!(texts.iter().any(|t| t == want), "{want:?} not in {texts:?}");
        }

        // Chapter 32 the same way: KJV 32:1 is HNUM 32:2.
        h.shell
            .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 32:1" }));
        settle(&mut h, cmp);
        let v = view(&h, cmp);
        assert_eq!(v.blocks.len(), 32);
        let b = &v.blocks[0];
        assert_eq!(b.verse, ge(32, 1));
        assert!(matches!(row(b, 2), Cell::Text { natives, .. } if natives == [(ge(32, 2), 2)]));
        assert!(texts_in(&h, cmp).iter().any(|t| t == "32:2"));
    }

    #[test]
    fn scrolling_the_comparison_drives_linked_bibles() {
        let (mut h, tb, cmp) = harness();
        h.shell.st.session.workspace.user_focused(tb);
        h.shell
            .run("resource.open", &json!({ "id": "heb-num", "split": true }));
        h.frames(3);
        let hn = h.tab_id("HNUM");
        h.shell
            .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:50" }));
        settle(&mut h, cmp);
        assert_eq!(view(&h, cmp).current_location(), Some(ge(31, 50)));
        // Wheel over the comparison until its top row is Standard Gen 31:55.
        let over = pane_rect(&h, cmp).center();
        h.events.push(Event::PointerMoved(over));
        let mut steps = 0;
        while view(&h, cmp).current_location() != Some(ge(31, 55)) {
            h.events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -8.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Modifiers::NONE,
            });
            h.paced_frame();
            steps += 1;
            assert!(
                steps < 600,
                "stuck at {:?}",
                view(&h, cmp).current_location()
            );
        }
        let ws = &h.shell.st.session.workspace;
        assert_eq!(ws.tab(cmp).unwrap().history.current(), Some(ge(31, 55)));
        // The linked Bibles follow: TB to 31:55, HNUM to its own 32:1.
        h.frames(3);
        assert_eq!(location(&mut h, tb), Some(ge(31, 55)));
        assert_eq!(location(&mut h, hn), Some(ge(32, 1)));
        assert_eq!(view(&h, cmp).current_location(), Some(ge(31, 55)));
    }

    #[test]
    fn chips_choose_the_base_and_differences_are_marked_against_it() {
        let (mut h, tb, cmp) = harness();
        h.shell
            .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:10" }));
        settle(&mut h, cmp);
        let marks = |h: &Harness| -> Vec<Marks> {
            let v = view(h, cmp);
            let b = v.blocks.iter().find(|b| b.verse == ge(31, 10)).unwrap();
            b.cells.iter().map(|c| c.marks.clone()).collect()
        };
        // HNUM's text at Standard 31:10 is its own 31:10 here (same words), so nothing differs.
        let m = marks(&h);
        assert_eq!(m[0], Marks::Base);
        assert_eq!(m[1], Marks::None, "HEB is absent here");
        assert_eq!(m[2], Marks::Diff(Default::default()));

        // ☆ on the HNUM chip makes it the ★ base.
        let r = pane_rect(&h, cmp);
        let chip = h
            .shapes
            .iter()
            .filter(|(s, t)| t == "HNUM" && r.contains(s.center()))
            .map(|(s, _)| *s)
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap();
        h.click(pos2(chip.left() - 9.0, chip.center().y));
        let c = &h.shell.st.session.workspace.tab(cmp).unwrap().comparison;
        assert_eq!(c.base.as_deref(), Some("heb-num"));
        assert_eq!(marks(&h)[2], Marks::Base);
        assert_eq!(marks(&h)[0], Marks::Diff(Default::default()));

        // In chapter 32 the numbering differs, so the texts do: words are tinted.
        h.shell
            .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 32:3" }));
        settle(&mut h, cmp);
        let v = view(&h, cmp);
        let b = v.blocks.iter().find(|b| b.verse == ge(32, 3)).unwrap();
        assert!(matches!(&b.cells[0].marks, Marks::Diff(d) if !d.changed.is_empty()));

        // The Hebrew Bible's rows are not compared with an English base.
        h.shell
            .run("tab.go", &json!({ "tab": cmp.0, "text": "Gen 1:1" }));
        settle(&mut h, cmp);
        let v = view(&h, cmp);
        assert_eq!(v.blocks[0].cells[1].marks, Marks::NotCompared);
        assert!(texts_in(&h, cmp).iter().any(|t| t == NOT_COMPARED));
    }

    #[test]
    fn a_typed_range_shows_as_a_removable_chip() {
        let (mut h, _, cmp) = harness();
        h.shell.st.session.workspace.user_focused(cmp);
        h.shell
            .run("tab.go", &json!({ "tab": cmp.0, "text": "Gen 31:50-32:2" }));
        settle(&mut h, cmp);
        assert_eq!(view(&h, cmp).blocks.len(), 8);
        // The range's chip: its Passage and a Close icon.
        assert!(texts_in(&h, cmp).iter().any(|t| t == "Gen 31:50-32:2"));
        let chip = h.at("Gen 31:50-32:2");
        h.click(chip);
        assert_eq!(
            h.shell
                .st
                .session
                .workspace
                .tab(cmp)
                .unwrap()
                .comparison
                .range,
            None
        );
        h.frames(2);
        assert_eq!(view(&h, cmp).blocks.len(), 55);
    }

    const KJV_JN_3_16: &str = "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.";
    const WEB_JN_3_16: &str = "For God so loved the world, that he gave his one and only Son, that whoever believes in him should not perish, but have eternal life.";

    /// A Bible holding only John 3:16.
    fn john(id: &str, abbrev: &str, text: &str) -> Arc<ReadingDoc> {
        use obs_model::{
            KeyScheme, Manifest, ParaStyle, ResourceBuilder, ResourceType, SpanKind,
            TextAccumulator, UnitId, UnitKind,
        };
        let mut m = Manifest::new(id, abbrev, abbrev, ResourceType::Bible, "en");
        m.key_schemes = vec![KeyScheme::Passage];
        m.license = "LicenseRef-PD".into();
        let mut b = ResourceBuilder::new(m);
        let unit = UnitId::book(obs_scripture::Book::John);
        let mut t = TextAccumulator::new();
        let r = t.push(text);
        b.add_unit(unit.clone(), UnitKind::BibleBook, None, t.into_string());
        b.add_span(
            &unit,
            r.clone(),
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
        b.add_verse(
            &unit,
            r,
            obs_scripture::Verse::new(obs_scripture::Book::John, 3, 16),
            16,
        );
        Arc::new(ReadingDoc::from_resource(b.build().unwrap()))
    }

    /// The comparison narrowed to KJV and WEB at John 3:16.
    fn kjv_web_harness() -> (Harness, TabId) {
        let (mut h, _, cmp) = harness();
        add_doc(&mut h.shell, john("kjv", "KJV", KJV_JN_3_16));
        add_doc(&mut h.shell, john("web", "WEB", WEB_JN_3_16));
        for id in ["test-bible", "heb-test", "heb-num"] {
            h.shell
                .run("compare.bible", &json!({ "tab": cmp.0, "id": id }));
        }
        h.shell
            .run("tab.go", &json!({ "tab": cmp.0, "text": "Jn 3:16" }));
        settle(&mut h, cmp);
        (h, cmp)
    }

    #[test]
    fn diff_marks_known_kjv_web_wording_differences_in_jn_3_16() {
        let (h, cmp) = kjv_web_harness();
        let v = view(&h, cmp);
        let b = v
            .blocks
            .iter()
            .find(|b| b.verse == obs_scripture::Verse::new(obs_scripture::Book::John, 3, 16))
            .unwrap();
        assert_eq!(b.cells.len(), 2);
        assert_eq!(b.cells[0].marks, Marks::Base);
        let Marks::Diff(d) = &b.cells[1].marks else {
            panic!("{:?}", b.cells[1].marks)
        };
        let words: Vec<&str> = d.changed.iter().map(|r| &WEB_JN_3_16[r.clone()]).collect();
        assert_eq!(words, ["one", "and", "whoever", "believes", "eternal"]);
        assert_eq!(d.missing, [WEB_JN_3_16.find("Son").unwrap()]);
        let texts = texts_in(&h, cmp);
        assert!(texts.iter().any(|t| t == "KJV") && texts.iter().any(|t| t == "WEB"));
    }

    #[test]
    fn unavailable_selected_bibles_keep_rows_and_retry_only_on_request() {
        for (id, is_base) in [("heb-num", false), ("test-bible", true)] {
            let (mut h, tb, cmp) = harness();
            h.shell
                .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:55" }));
            let doc = h.shell.st.docs.remove(id).unwrap();
            settle(&mut h, cmp);
            assert_eq!(view(&h, cmp).blocks.last().unwrap().cells.len(), 3);
            let errors = &h.shell.st.tabs[&cmp].comparison_errors;
            assert_eq!(errors.len(), 1);
            let error = errors[id].clone();
            let message = if is_base {
                "Base Bible unavailable — differences not shown:"
            } else {
                "Bible unavailable:"
            };
            assert!(
                texts_in(&h, cmp)
                    .iter()
                    .any(|s| s.starts_with(message) && s.contains(&error))
            );
            assert!(h.find("Retry unavailable Bibles").is_some());
            assert!(
                texts_in(&h, cmp)
                    .iter()
                    .any(|s| s == &doc.manifest.abbreviation)
            );
            if is_base {
                assert!(
                    view(&h, cmp)
                        .blocks
                        .iter()
                        .flat_map(|b| &b.cells)
                        .all(|c| c.marks == Marks::None),
                    "no diff against an unavailable base"
                );
            }
            // Even if loading could now succeed, frames must retain the cached failure.
            h.shell.st.docs.insert(id.to_string(), doc);
            h.frames(4);
            assert_eq!(h.shell.st.tabs[&cmp].comparison_errors[id], error);
            assert!(texts_in(&h, cmp).iter().any(|s| s.starts_with(message)));
            h.click_text("Retry unavailable Bibles");
            settle(&mut h, cmp);
            assert!(h.shell.st.tabs[&cmp].comparison_errors.is_empty());
            assert!(!texts_in(&h, cmp).iter().any(|s| s.starts_with(message)));
            let block = view(&h, cmp).blocks.last().unwrap();
            assert_eq!(block.cells[0].marks, Marks::Base);
            assert!(matches!(block.cells[2].marks, Marks::Diff(_)));
        }
    }

    #[test]
    fn comparison_publishes_visible_bible_text_and_reference_labels_to_accesskit() {
        use egui::accesskit::Role;
        let (mut h, cmp) = kjv_web_harness();
        h.ctx.enable_accesskit();
        settle(&mut h, cmp);
        let tree = h.tree.as_ref().unwrap();
        let nodes: std::collections::HashMap<_, _> =
            tree.nodes.iter().map(|(id, n)| (*id, n)).collect();
        let document = nodes
            .values()
            .find(|n| n.role() == Role::Document && n.label() == Some("Text Comparison"))
            .unwrap();
        let paragraphs: Vec<_> = document
            .children()
            .iter()
            .filter_map(|id| nodes.get(id))
            .filter(|n| n.role() == Role::Paragraph)
            .collect();
        assert!(
            !paragraphs.is_empty() && paragraphs.len() < 36 * 2,
            "only visible Verse rows published"
        );
        for (bible, text) in [("KJV", KJV_JN_3_16), ("WEB", WEB_JN_3_16)] {
            let paragraph = paragraphs
                .iter()
                .find(|n| {
                    n.label()
                        .is_some_and(|s| s.starts_with(&format!("{bible} — John 3:16")))
                })
                .unwrap();
            assert!(paragraph.bounds().is_some());
            let runs: Vec<_> = paragraph
                .children()
                .iter()
                .map(|id| nodes[id])
                .filter(|n| n.role() == Role::TextRun)
                .collect();
            let shown: String = runs.iter().map(|n| n.value().unwrap()).collect();
            assert_eq!(shown, text);
            for run in runs {
                assert_eq!(
                    run.character_lengths().len(),
                    run.value().unwrap().chars().count()
                );
                assert!(run.bounds().is_some());
            }
        }
        h.shell
            .run("tab.go", &json!({ "tab": cmp.0, "text": "Jn 3:1" }));
        settle(&mut h, cmp);
        assert!(
            !h.tree
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .any(|(_, n)| n.role() == Role::Paragraph
                    && n.label().is_some_and(|s| s.contains("John 3:16")))
        );
    }

    #[test]
    fn comparison_body_labels_use_page_colours_for_every_theme_and_tone() {
        fn check(
            shape: &Shape,
            body_top: f32,
            pane: Rect,
            palette: crate::Palette,
            found: &mut Vec<String>,
        ) {
            match shape {
                Shape::Vec(shapes) => {
                    for s in shapes {
                        check(s, body_top, pane, palette, found);
                    }
                }
                Shape::Text(t) if pane.contains(t.pos) && t.pos.y > body_top => {
                    let text = t.galley.text();
                    let expected = if ["TB", "HEB", "HNUM"].contains(&text) {
                        palette.ink
                    } else {
                        palette.verse_number
                    };
                    for section in &t.galley.job.sections {
                        assert_eq!(
                            t.override_text_color.unwrap_or(section.format.color),
                            expected,
                            "{text}"
                        );
                    }
                    assert!(
                        crate::palette::contrast(expected, palette.page) >= 4.5,
                        "{text}"
                    );
                    found.push(text.to_string());
                }
                _ => {}
            }
        }
        let (mut h, _, cmp) = harness();
        for (_, theme, _) in obs_engine::Theme::ALL {
            for os in [egui::Theme::Light, egui::Theme::Dark] {
                h.os_theme = os;
                h.shell.run("view.theme", &json!({ "value": theme }));
                for (_, tone, _) in obs_engine::PageTone::ALL {
                    h.shell.run("view.page_tone", &json!({ "value": tone }));
                    let mut found = Vec::new();
                    for passage in ["Gen 31:55", "Gen 1:1"] {
                        h.shell
                            .run("tab.go", &json!({ "tab": cmp.0, "text": passage }));
                        settle(&mut h, cmp);
                        let pane = pane_rect(&h, cmp);
                        let top = h
                            .shapes
                            .iter()
                            .filter(|(r, s)| {
                                pane.contains(r.center())
                                    && (s == "Genesis 31:55" || s == "Genesis 1:1")
                            })
                            .max_by(|a, b| a.0.top().total_cmp(&b.0.top()))
                            .unwrap()
                            .0
                            .top();
                        for clipped in &h.last_shapes {
                            check(
                                &clipped.shape,
                                top - 1.0,
                                pane,
                                h.shell.st.palette,
                                &mut found,
                            );
                        }
                    }
                    for want in ["TB", "HEB", "HNUM", "32:1", ABSENT, NOT_COMPARED] {
                        assert!(
                            found.iter().any(|s| s == want),
                            "{theme}/{tone}/{os:?}: {want} missing"
                        );
                    }
                }
            }
        }
    }

    /// Writes `target/previews/text-comparison-*.ppm` (the whole window, rasterized from the
    /// real frame) when `UPDATE_PREVIEWS` is set.
    #[test]
    fn previews() {
        if std::env::var_os("UPDATE_PREVIEWS").is_none() {
            return;
        }
        let (mut h, tb, cmp) = harness();
        h.shell
            .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:55" }));
        settle(&mut h, cmp);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        h.screenshot(&dir.join("text-comparison-gen-31-55.ppm"));
        let (h, _) = kjv_web_harness();
        h.screenshot(&dir.join("text-comparison-jn-3-16.ppm"));
    }

    /// The chips' text is in the chrome fonts; their stars and Close buttons are icons.
    #[test]
    fn chip_symbols_are_in_the_chrome_fonts() {
        let h = Harness::new();
        h.ctx.fonts_mut(|f| {
            for s in ["—", "+ Bible"] {
                assert!(f.has_glyphs(&egui::FontId::proportional(12.0), s), "{s}");
            }
        });
    }
}

#[path = "resource_tabs_tests.rs"]
mod resource_tabs;

#[path = "contents_pane_tests.rs"]
mod contents_pane;

#[path = "annotation_tests.rs"]
mod annotation;

#[path = "text_size_tests.rs"]
mod text_size;

#[path = "annotations_across_tests.rs"]
mod annotations_across;

#[path = "notes_tests.rs"]
mod notes;

#[path = "highlight_styles_tests.rs"]
mod highlight_styles;

/// Import from the Library menu and the live Import report Tab (#47).
mod import {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, mpsc};
    use std::time::{Duration, Instant};

    use obs_engine::{
        ImportIndexStatus, ImportIndexer, ImportPhase, JobContext, UNAVAILABLE_INDEX,
    };
    use obs_model::ResourceType;

    use super::*;

    /// A temporary library folder and a USFM folder `WEB` of two Bible books, one with
    /// markup the span vocabulary drops.
    fn fixture(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("obs-shell-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let source = root.join("WEB");
        let lib = root.join("library");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(
            source.join("GEN.usfm"),
            "\\id GEN\n\\c 1\n\\p\n\\v 1 In the beginning God created the heavens and the earth.\n\\ip An introduction the report must list as dropped.\n",
        )
        .unwrap();
        std::fs::write(
            source.join("JHN.usfm"),
            "\\id JHN\n\\c 1\n\\p\n\\v 1 In the beginning was the Word.\n",
        )
        .unwrap();
        (root, source, lib)
    }

    /// Bible rows in the Library Tab, top to bottom, by abbreviation.
    fn bible_rows(h: &Harness) -> Vec<String> {
        let panel = h
            .panels()
            .into_iter()
            .find(|(_, t)| t.iter().any(|t| t == "Library"))
            .map(|(r, _)| r)
            .unwrap();
        let mut rows: Vec<(f32, String)> = h
            .shapes
            .iter()
            .filter(|(r, t)| {
                ["TB", "HEB", "HNUM", "WEB-1"].contains(&t.as_str()) && panel.contains(r.center())
            })
            .map(|(r, t)| (r.center().y, t.clone()))
            .collect();
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
        rows.into_iter().map(|(_, t)| t).collect()
    }

    /// Hand the shell a picked path the way the `rfd` thread does, without a native dialog.
    fn pick(h: &mut Harness, path: &Path) -> String {
        let (tx, rx) = mpsc::channel();
        h.shell.st.picker = Some(rx);
        tx.send(Some(path.to_path_buf())).unwrap();
        h.frames(2);
        h.shell
            .st
            .session
            .workspace
            .tabs()
            .find_map(|t| match &t.kind {
                TabKind::ImportReport(id) => Some(id.clone()),
                _ => None,
            })
            .expect("the report Tab opens as soon as the Import starts")
    }

    fn record(h: &Harness, id: &str) -> obs_engine::ImportRecord {
        h.shell.st.session.imports.report(id).unwrap().clone()
    }

    /// #58's seam, standing in: reports one step of index progress, then waits to be released.
    #[derive(Debug)]
    struct HeldIndexer {
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl ImportIndexer for HeldIndexer {
        fn build(
            &self,
            _: &obs_model::Resource,
            _: &Path,
            ctx: &JobContext,
        ) -> Result<ImportIndexStatus, String> {
            ctx.progress(1, 3);
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(20))
                .unwrap();
            ctx.progress(3, 3);
            Ok(ImportIndexStatus::Built)
        }
    }

    #[test]
    fn library_import_opens_a_chooser_then_a_report_tab_that_fills_in_live_without_freezing_frames()
    {
        let (root, source, lib) = fixture("live");
        let mut h = Harness::threaded();
        assert!(h.shell.st.session.library.scan(&lib).is_empty());
        let (entered, held) = mpsc::channel();
        let (release, rx) = mpsc::channel();
        h.shell
            .st
            .session
            .imports
            .set_indexer(Arc::new(HeldIndexer {
                entered,
                release: Mutex::new(rx),
            }));
        h.frames(2);

        // Library → Import… is enabled and asks whether to pick a file or a folder.
        h.open_menu("Library");
        h.click_text("Import…");
        assert!(
            h.find("Choose a USFM file or a folder of Bible books.")
                .is_some(),
            "{:?}",
            h.texts()
        );
        assert!(h.find("USFM folder…").is_some());
        assert!(h.find("SWORD module…").is_some());
        h.shell.st.pick_import = false;

        // The picked folder starts an engine job and opens the report Tab at once; nothing is
        // parsed on the UI thread, so the Resource is not in the Library yet.
        let id = pick(&mut h, &source);
        assert_eq!(id, "web-1");
        assert!(
            h.panels()
                .iter()
                .any(|(_, t)| t.iter().any(|t| t == "Import report"))
        );
        assert!(h.find("Import in progress").is_some(), "{:?}", h.texts());
        assert!(h.find(&source.display().to_string()).is_some());
        held.recv_timeout(Duration::from_secs(20)).unwrap();

        // The worker is now held inside the Index phase. Frames keep coming at interactive
        // speed and show the live phase: Parse done, Search index 1 / 3.
        let started = Instant::now();
        let mut saw_index_progress = false;
        for _ in 0..20 {
            let t = Instant::now();
            h.paced_frame();
            assert!(
                t.elapsed() < Duration::from_millis(500),
                "a frame blocked on the Import"
            );
            saw_index_progress |= h.find("Search index: 1 / 3").is_some();
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(saw_index_progress, "{:?}", h.texts());
        assert_eq!(record(&h, &id).phase, ImportPhase::Index);
        if std::env::var_os("UPDATE_PREVIEWS").is_some() {
            let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
            h.screenshot(&dir.join("import-report-in-progress.ppm"));
        }
        assert!(h.find("Parse complete").is_some());
        assert!(h.shell.st.session.library.get(&id).is_none());
        // Menus still answer while the job runs.
        h.open_menu("Library");
        assert!(h.find("Import…").is_some());
        h.click_text("Import…");
        h.shell.st.pick_import = false;
        h.frames(2);

        // Released, the report completes in place and the Bible is the last of its type.
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while record(&h, &id).phase != ImportPhase::Complete && Instant::now() < deadline {
            h.paced_frame();
        }
        h.frames(2);
        assert!(h.find("Import complete").is_some(), "{:?}", h.texts());
        assert!(h.find("Search index: built").is_some());
        assert_eq!(record(&h, &id).index, ImportIndexStatus::Built);
        h.click_text("Library");
        assert_eq!(bible_rows(&h), ["TB", "HEB", "HNUM", "WEB-1"]);
        assert!(lib.join("web-1.obr").is_file());
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// An Import whose indexer left the Search index to the background (#58).
    #[derive(Debug)]
    struct LeaveItIndexer;
    impl ImportIndexer for LeaveItIndexer {
        fn build(
            &self,
            _: &obs_model::Resource,
            _: &Path,
            _: &JobContext,
        ) -> Result<ImportIndexStatus, String> {
            Ok(ImportIndexStatus::Deferred)
        }
    }

    #[test]
    fn a_report_shows_the_background_search_index_build_until_it_is_ready() {
        let (root, source, lib) = fixture("background");
        let mut h = Harness::new();
        assert!(h.shell.st.session.library.scan(&lib).is_empty());
        h.shell.st.session.set_search_cache_dir(root.join("cache"));
        h.shell
            .st
            .session
            .imports
            .set_indexer(Arc::new(LeaveItIndexer));
        h.frames(2);
        let id = pick(&mut h, &source);
        h.shell.st.session.jobs().pump(1); // the Import
        h.frames(2);
        assert_eq!(record(&h, &id).index, ImportIndexStatus::Deferred);
        assert!(
            matches!(
                h.shell.st.session.search.status(&id),
                Some(obs_engine::IndexStatus::Indexing { .. })
            ),
            "queued for the background build"
        );
        assert!(
            h.find(obs_engine::DEFERRED_INDEX).is_some(),
            "{:?}",
            h.texts()
        );
        // Idle, the app still wakes up to move the background build along.
        assert_eq!(
            h.shell.background_repaint(),
            Some(std::time::Duration::from_millis(250))
        );
        h.shell.st.session.jobs().pump(1); // the background index build
        h.frames(2);
        assert_eq!(
            h.shell.st.session.search.status(&id),
            Some(&obs_engine::IndexStatus::Ready)
        );
        assert!(h.find("Search index: built").is_some(), "{:?}", h.texts());
        assert_eq!(h.shell.background_repaint(), None, "nothing left to build");
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_picked_sword_conf_imports_through_the_report_tab_and_opens() {
        let (root, _, lib) = fixture("sword");
        let conf = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../formats/tests/assets/sword/mods.d/obskjvosis.conf");
        let mut h = Harness::new();
        assert!(h.shell.st.session.library.scan(&lib).is_empty());
        h.frames(2);
        let id = pick(&mut h, &conf);
        assert_eq!(id, "obskjvosis-1");
        h.shell.st.session.jobs().pump(1);
        h.frames(2);
        let rec = record(&h, &id);
        assert_eq!(rec.phase, ImportPhase::Complete, "{:?}", rec.error);
        assert!(!rec.report.unwrap().license_missing);
        for want in ["Import complete", "1 source file(s) · sword (OSIS)"] {
            assert!(h.find(want).is_some(), "{want:?} missing: {:?}", h.texts());
        }
        h.click_text("Open");
        assert!(
            h.panels()
                .iter()
                .any(|(_, t)| t.iter().any(|t| t == "KJV-O")),
            "{:?}",
            h.panels()
        );
        assert!(lib.join("obskjvosis-1.obr").is_file());
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_finished_report_shows_the_summary_warnings_and_actions_and_remove_import_deletes_the_resource()
     {
        let (root, source, lib) = fixture("report");
        let mut h = Harness::new();
        assert!(h.shell.st.session.library.scan(&lib).is_empty());
        h.frames(2);
        let id = pick(&mut h, &source);
        h.shell.st.session.jobs().pump(1);
        h.frames(2);
        let rec = record(&h, &id);
        assert_eq!(rec.phase, ImportPhase::Complete);
        let report = rec.report.clone().unwrap();

        // Summary, Unmapped/Absent counts, the licence warning and the Search index (this
        // Session has no cache folder to build it in).
        for want in [
            "Import complete",
            UNAVAILABLE_INDEX,
            "2 source file(s) · usfm",
            "2 Bible book(s) · 2 Verses · 0 notes · 0 Tagged words",
            "Unmapped verses: 0",
            &format!("Absent verses: {}", report.verse_map.unwrap().absent),
            "Dropped markup (1)",
            "Source files and Bible books",
        ] {
            assert!(h.find(want).is_some(), "{want:?} missing: {:?}", h.texts());
        }
        assert!(report.verse_map.unwrap().absent > 0);
        assert!(report.license_missing);
        assert!(h.texts().iter().any(|t| t.starts_with("Licence: unknown")));
        // Dropped markup expands to the markers and their counts.
        assert!(h.find("\\ip ×1").is_none());
        h.click_text("Dropped markup (1)");
        h.frames(8); // the header's open animation
        assert!(h.find("\\ip ×1").is_some(), "{:?}", h.texts());
        h.click_text("Source files and Bible books");
        h.frames(8);
        assert!(h.find("GEN.usfm").is_some(), "{:?}", h.texts());
        assert!(
            h.find("JHN · 1 chapters · 1 Verses").is_some(),
            "{:?}",
            h.texts()
        );
        if std::env::var_os("UPDATE_PREVIEWS").is_some() {
            let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
            h.screenshot(&dir.join("import-report.ppm"));
        }

        // Open, About (which links back to the latest report) and Remove import.
        h.click_text("Open");
        assert!(
            h.panels()
                .iter()
                .any(|(_, t)| t.iter().any(|t| t == "WEB-1")),
            "{:?}",
            h.panels()
        );
        h.click_text("Import report");
        h.click_text("About");
        assert!(
            h.panels()
                .iter()
                .any(|(_, t)| t.iter().any(|t| t == "About · WEB-1")),
            "{:?}",
            h.panels()
        );
        assert!(h.find("Latest Import report").is_some(), "{:?}", h.texts());
        let reports_before = h
            .shell
            .st
            .session
            .workspace
            .tabs()
            .filter(|t| t.kind == TabKind::ImportReport(id.clone()))
            .count();
        h.click_text("Latest Import report");
        assert_eq!(
            h.shell
                .st
                .session
                .workspace
                .tabs()
                .filter(|t| t.kind == TabKind::ImportReport(id.clone()))
                .count(),
            reports_before
        );
        assert!(
            h.find("Import complete").is_some(),
            "the report Tab is focused again"
        );
        assert_eq!(
            h.shell
                .st
                .session
                .library
                .of_type(ResourceType::Bible)
                .last()
                .unwrap()
                .manifest
                .resource_id,
            id
        );

        h.click_text("Remove import…");
        assert!(h.shell.st.confirm.is_some());
        h.click_text("Remove");
        h.frames(2);
        assert!(!lib.join("web-1.obr").exists(), "the .obr is deleted");
        assert!(h.shell.st.session.library.get(&id).is_none());
        assert!(h.shell.st.session.imports.report(&id).is_none());
        assert!(h.shell.st.session.workspace.tabs().all(|t| !matches!(&t.kind, TabKind::Resource(r) | TabKind::About(r) | TabKind::ImportReport(r) if *r == id)));
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[path = "word_info_tests.rs"]
mod word_info;

#[path = "atlas_tests.rs"]
mod atlas;
#[path = "credits_tests.rs"]
mod credits;
#[path = "guide_tests.rs"]
mod guide;
#[path = "open_catalogue_tests.rs"]
mod open_catalogue;
#[path = "starter_library_tests.rs"]
mod starter_library;
#[path = "timeline_tests.rs"]
mod timeline;
#[path = "word_study_tests.rs"]
mod word_study;

#[path = "missing_resource_tests.rs"]
mod missing_resource;

#[path = "search_tests.rs"]
mod search;

#[path = "fonts_tests.rs"]
mod fonts;

#[path = "favorites_tests.rs"]
mod favorites;

#[path = "reading_plans_tests.rs"]
mod reading_plans;

#[path = "home_tests.rs"]
mod home;

#[path = "context_menu_tests.rs"]
mod context_menu;

#[path = "parallel_resources_tests.rs"]
mod parallel_resources;

#[path = "icon_previews_tests.rs"]
mod icon_previews;

#[path = "collections_tests.rs"]
mod collections;
#[path = "command_box_tests.rs"]
mod command_box;
#[path = "copy_export_tests.rs"]
mod copy_export;
#[path = "drag_drop_tests.rs"]
mod drag_drop;
#[path = "find_tests.rs"]
mod find_bar;

#[path = "a11y_audit_tests.rs"]
mod a11y_audit;
#[path = "floating_windows_tests.rs"]
mod floating_windows;
#[path = "interlinear_tests.rs"]
mod interlinear;
#[path = "keyboard_shortcuts_tests.rs"]
mod keyboard_shortcuts;
#[path = "settings_tests.rs"]
mod settings;
#[path = "toolbar_tests.rs"]
mod toolbar;
#[path = "toolbar_position_tests.rs"]
mod toolbar_position;

/// Get Bibles (#91) against mock `file://` SWORD repositories built from open texts.
mod get_bibles {
    use std::path::PathBuf;

    use obs_engine::ImportPhase;
    use obs_engine::testing::MockRepos;

    use super::*;

    const NASB: &str = "New American Standard Bible - NASB 1995";

    fn setup(name: &str) -> (Harness, Arc<MockRepos>, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("obs-shell-getbibles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let repos = Arc::new(MockRepos::new(&root.join("repos")));
        let mut h = Harness::new();
        let s = &mut h.shell.st.session;
        s.set_fetch(repos.clone());
        s.get_bibles.set_cache_path(root.join("sword-catalog.json"));
        s.get_bibles.set_sword_dir(root.join("sword"));
        // These tests count the SWORD catalog's rows; the Open catalogue has its own tests.
        s.get_bibles.set_open_catalogue(Vec::new());
        std::fs::create_dir_all(root.join("library")).unwrap();
        s.library.set_directory(root.join("library"));
        h.frames(2);
        (h, repos, root)
    }

    /// Run queued jobs (inline mode) and draw.
    fn settle(h: &mut Harness) {
        h.shell.st.session.jobs().pump(4);
        h.frames(3);
    }

    /// The topmost place `text` is painted (Featured rows repeat under All Bibles).
    fn first(h: &Harness, text: &str) -> Rect {
        h.shapes
            .iter()
            .filter(|(_, t)| t == text)
            .map(|(r, _)| *r)
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .unwrap_or_else(|| panic!("{text:?} not on screen: {:?}", h.texts()))
    }

    /// The control labelled `label` on the topmost row whose title is `title`.
    fn row_button(h: &Harness, title: &str, label: &str) -> Pos2 {
        let row = first(h, title);
        h.shapes
            .iter()
            .filter(|(r, t)| {
                t == label && r.center().y > row.top() - 4.0 && r.center().y < row.top() + 58.0
            })
            .map(|(r, _)| r.center())
            .next()
            .unwrap_or_else(|| panic!("no {label:?} on row {title:?}: {:?}", h.texts()))
    }

    fn open_get_bibles(h: &mut Harness) {
        h.open_menu("Library");
        h.click_text("Get Bibles…");
        settle(h);
        assert!(
            h.panels()
                .iter()
                .any(|(_, t)| t.iter().any(|t| t == "Get Resources")),
            "{:?}",
            h.panels()
        );
    }

    #[test]
    fn library_get_bibles_shows_nasb_1995_first_with_licence_and_size_then_one_click_installs_it() {
        let (mut h, repos, root) = setup("flow");
        open_get_bibles(&mut h);
        assert!(h.find("FEATURED BIBLES · 1").is_some(), "{:?}", h.texts());
        assert!(h.find("ALL BIBLES · 7").is_some(), "{:?}", h.texts());
        // NASB 1995 heads Featured, with the conf's licence and InstallSize.
        let nasb = first(&h, NASB);
        let featured = h.find("FEATURED BIBLES · 1").unwrap();
        let all = h.find("ALL BIBLES · 7").unwrap();
        assert!(featured.top() < nasb.top() && nasb.top() < all.top());
        assert!(h.find("Copyrighted").is_some());
        assert!(
            h.find("NASB95 · English · 4.5 MB · from Lockman Foundation")
                .is_some(),
            "{:?}",
            h.texts()
        );
        assert!(
            h.find(
                "Used by Word Study › Translated as and Occurrences · Copyrighted; Permission \
                 to distribute granted to CrossWire"
            )
            .is_some(),
            "{:?}",
            h.texts()
        );
        assert!(
            h.find("Public domain").is_some(),
            "open-text rows badge too"
        );
        if std::env::var_os("UPDATE_PREVIEWS").is_some() {
            let dir =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
            h.screenshot(&dir.join("get-bibles.ppm"));
        }

        // One click: download, import, report Tab; no picker, no prompt.
        let at = row_button(&h, NASB, "Download");
        h.click(at);
        assert!(!h.shell.st.pick_import);
        assert!(h.shell.st.confirm.is_none());
        let id = "nasb1995-1";
        assert_eq!(
            h.shell.st.session.imports.report(id).unwrap().phase,
            ImportPhase::Download
        );
        settle(&mut h);
        let record = h.shell.st.session.imports.report(id).unwrap().clone();
        assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
        assert!(h.shell.st.session.library.get(id).is_some());
        assert!(root.join("library/nasb1995-1.obr").is_file());
        let fetched = repos.fetched.lock().unwrap().clone();
        assert!(fetched.iter().all(|u| u.starts_with("file://")
            || u.starts_with("HEAD file://")
            || u == obs_engine::sword_repo::MASTER_REPO_LIST_URL));

        // Back on Get Bibles the row says Installed and opens the Bible.
        let tab = h.tab_at("Get Resources");
        h.click(tab);
        assert!(h.find("Installed").is_some(), "{:?}", h.texts());
        let open = row_button(&h, NASB, "Open");
        h.click(open);
        assert!(
            h.panels()
                .iter()
                .any(|(_, t)| t.iter().any(|t| t == "NASB95")),
            "{:?}",
            h.panels()
        );
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn all_bibles_filters_by_search_and_language() {
        let (mut h, _repos, root) = setup("filter");
        open_get_bibles(&mut h);
        h.shell.st.get_bibles_ui.language = Some("grc".into());
        h.frames(2);
        assert!(h.find("ALL BIBLES · 2").is_some(), "{:?}", h.texts());
        assert!(h.find("Greek test Bible").is_some());
        assert!(h.find(NASB).is_some(), "Featured is not filtered");
        h.shell.st.get_bibles_ui.language = None;
        h.shell.st.get_bibles_ui.query = "gbf".into();
        h.frames(2);
        assert!(h.find("ALL BIBLES · 1").is_some(), "{:?}", h.texts());
        h.shell.st.get_bibles_ui.query = "nothing like this".into();
        h.frames(2);
        assert!(
            h.find("Nothing matches the search, type, language and licence.")
                .is_some()
        );
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn offline_the_tab_shows_the_cached_catalog_and_says_so() {
        let (mut h, repos, root) = setup("offline");
        open_get_bibles(&mut h);
        assert!(h.find(NASB).is_some());
        repos.set_offline(true);
        h.shell.run("getbibles.refresh", &json!({}));
        settle(&mut h);
        assert!(
            h.texts()
                .iter()
                .any(|t| t.starts_with("Offline: showing the catalog from")),
            "{:?}",
            h.texts()
        );
        assert!(h.find(NASB).is_some(), "cached catalog still listed");
        assert!(h.find("Try again").is_some());
        // The rest of the app is unaffected: the Library still opens.
        h.open_menu("Library");
        h.click_text("Library");
        assert!(
            h.panels()
                .iter()
                .any(|(_, t)| t.iter().any(|t| t == "Library"))
        );
        // Back online, Try again clears the banner.
        repos.set_offline(false);
        let tab = h.tab_at("Get Resources");
        h.click(tab);
        h.click_text("Try again");
        settle(&mut h);
        assert!(
            !h.texts().iter().any(|t| t.starts_with("Offline:")),
            "{:?}",
            h.texts()
        );
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The live repositories over FTP only: the SWORD crate's built-in transport (plain
    /// `std::net`), so this test needs no networking crate. HTTPS sources fail at once and
    /// the installer falls back to each repository's FTP source.
    #[derive(Debug)]
    struct FtpOnly;
    impl obs_engine::sword_repo::Fetch for FtpOnly {
        fn get(
            &self,
            url: &str,
            progress: obs_engine::sword_repo::Progress<'_>,
        ) -> Result<Vec<u8>, obs_engine::sword_repo::FetchError> {
            if !url.starts_with("ftp://") {
                return Err(obs_engine::sword_repo::FetchError::Network(format!(
                    "{url}: FTP only in this test"
                )));
            }
            let bytes = obs_engine::sword_repo::builtin_get(url)?;
            let n = bytes.len() as u64;
            if progress(n, Some(n)) {
                Ok(bytes)
            } else {
                Err(obs_engine::sword_repo::FetchError::Cancelled)
            }
        }
    }

    /// No network at all.
    #[derive(Debug)]
    struct Offline;
    impl obs_engine::sword_repo::Fetch for Offline {
        fn get(
            &self,
            url: &str,
            _: obs_engine::sword_repo::Progress<'_>,
        ) -> Result<Vec<u8>, obs_engine::sword_repo::FetchError> {
            Err(obs_engine::sword_repo::FetchError::Network(format!(
                "{url}: failed to lookup address information"
            )))
        }
    }

    /// Click-to-reading against the real Lockman repository. Ignored: it needs the network
    /// and downloads a copyrighted module into a temp folder that is deleted afterwards.
    /// Screenshots go to `target/previews/` (never committed). Run by hand:
    /// `cargo test -p obs-ui-egui --release -- --ignored --nocapture live_get_bibles`
    #[test]
    #[ignore = "network; downloads a copyrighted module to a temp folder"]
    fn live_get_bibles_click_download_on_nasb_1995_then_open_and_read_it() {
        let root = std::env::temp_dir().join(format!("obs-live-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut h = Harness::new();
        let s = &mut h.shell.st.session;
        s.set_fetch(Arc::new(FtpOnly));
        s.get_bibles.set_cache_path(root.join("sword-catalog.json"));
        s.get_bibles.set_sword_dir(root.join("sword"));
        std::fs::create_dir_all(root.join("library")).unwrap();
        s.library.set_directory(root.join("library"));
        h.frames(2);
        let shots = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");

        let t = std::time::Instant::now();
        open_get_bibles(&mut h);
        println!("catalog loaded in {:.1}s", t.elapsed().as_secs_f64());
        let featured = first(&h, "FEATURED BIBLES · 13");
        let nasb = first(&h, NASB);
        assert!(featured.top() < nasb.top(), "{:?}", h.texts());
        assert!(
            h.texts()
                .iter()
                .any(|t| t.ends_with("Copyrighted; Permission to distribute granted to CrossWire")),
            "{:?}",
            h.texts()
        );
        h.screenshot(&shots.join("live-get-bibles.ppm"));

        let t = std::time::Instant::now();
        let at = row_button(&h, NASB, "Download");
        h.click(at);
        assert!(!h.shell.st.pick_import && h.shell.st.confirm.is_none());
        settle(&mut h);
        let id = "nasb1995-1";
        let record = h.shell.st.session.imports.report(id).unwrap().clone();
        assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
        println!(
            "downloaded and imported in {:.1}s: {} verses",
            t.elapsed().as_secs_f64(),
            record.report.as_ref().unwrap().verses
        );
        h.screenshot(&shots.join("live-import-report.ppm"));

        let tab = h.tab_at("Get Resources");
        h.click(tab);
        assert!(h.find("Installed").is_some(), "{:?}", h.texts());
        let open = row_button(&h, NASB, "Open");
        h.click(open);
        settle_all(&mut h);
        assert!(
            h.panels()
                .iter()
                .any(|(_, t)| t.iter().any(|t| t == "NASB1995")),
            "{:?}",
            h.panels()
        );
        h.screenshot(&shots.join("live-nasb-reading.ppm"));

        // Now offline: every fetch fails. The Tab keeps the real catalog it cached, says so,
        // and the Bible just downloaded still reads.
        h.shell.st.session.set_fetch(Arc::new(Offline));
        let tab = h.tab_at("Get Resources");
        h.click(tab);
        h.shell.run("getbibles.refresh", &json!({}));
        settle(&mut h);
        assert!(
            h.texts()
                .iter()
                .any(|t| t.starts_with("Offline: showing the catalog from")),
            "{:?}",
            h.texts()
        );
        assert!(h.find(NASB).is_some() && h.find("Try again").is_some());
        h.screenshot(&shots.join("live-offline.ppm"));
        let tab = h.tab_at("NASB1995");
        h.click(tab);
        settle_all(&mut h);
        println!("screenshots in {}", shots.display());
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    const STRONGS: &str = "Strong's Greek Bible Dictionary";
    const MT_NOTES: &str = "Synthetic ObsHebrewNotes regression fixture";

    /// Visual evidence for docs/previews/ when `UPDATE_PREVIEWS` is set.
    fn preview(h: &Harness, name: &str) {
        if std::env::var_os("UPDATE_PREVIEWS").is_some() {
            let dir =
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
            std::fs::create_dir_all(&dir).unwrap();
            h.screenshot(&dir.join(format!("{name}.ppm")));
        }
    }

    fn open_get_resources(h: &mut Harness) {
        h.open_menu("Library");
        h.click_text("Get Resources…");
        settle(h);
        assert!(
            h.panels()
                .iter()
                .any(|(_, t)| t.iter().any(|t| t == "Get Resources")),
            "{:?}",
            h.panels()
        );
    }

    #[test]
    fn get_resources_lists_every_type_and_the_lexicons_filter_installs_strongs_greek_in_one_click()
    {
        let (mut h, _repos, root) = setup("types");
        open_get_resources(&mut h);
        // All types: a Featured section per type that has one, then every listing.
        assert!(h.find("FEATURED BIBLES · 1").is_some(), "{:?}", h.texts());
        assert!(h.find("FEATURED LEXICONS · 1").is_some(), "{:?}", h.texts());
        assert!(h.find("FEATURED COMMENTARIES · 0").is_none());
        assert!(
            h.find("FEATURED HEBREW & GREEK · 2").is_some(),
            "{:?}",
            h.texts()
        );
        for t in [
            "Bibles",
            "Hebrew & Greek",
            "Lexicons",
            "Commentaries",
            "Dictionaries",
            "All",
        ] {
            assert!(h.find(t).is_some(), "type filter {t:?}: {:?}", h.texts());
        }
        assert!(
            h.find("Lexicon · StrongsGreek · English · 0.9 MB · from CrossWire")
                .is_some(),
            "{:?}",
            h.texts()
        );
        preview(&h, "get-resources-all");

        h.click_text("Lexicons");
        h.frames(2);
        assert_eq!(
            h.shell.st.session.get_bibles.type_filter(),
            Some(obs_model::ResourceType::Lexicon)
        );
        assert!(h.find("FEATURED LEXICONS · 1").is_some(), "{:?}", h.texts());
        assert!(h.find("ALL LEXICONS · 1").is_some(), "{:?}", h.texts());
        assert!(h.find(NASB).is_none(), "Bibles filtered out");
        assert!(
            h.find("Used by Word Study › Lexicon entries and Word Info · Public Domain")
                .is_some(),
            "{:?}",
            h.texts()
        );
        preview(&h, "get-resources-lexicons");

        let at = row_button(&h, STRONGS, "Download");
        h.click(at);
        assert!(!h.shell.st.pick_import && h.shell.st.confirm.is_none());
        settle(&mut h);
        let id = "strongsgreek-1";
        let record = h.shell.st.session.imports.report(id).unwrap().clone();
        assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
        assert_eq!(
            h.shell
                .st
                .session
                .library
                .get(id)
                .unwrap()
                .manifest
                .resource_type,
            obs_model::ResourceType::Lexicon
        );
        let tab = h.tab_at("Get Resources");
        h.click(tab);
        assert!(h.find("Installed").is_some(), "{:?}", h.texts());
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_commentary_import_refuses_is_shown_disabled_with_the_reason() {
        let (mut h, _repos, root) = setup("refused");
        open_get_resources(&mut h);
        h.click_text("Commentaries");
        h.frames(2);
        assert!(h.find("ALL COMMENTARIES · 3").is_some(), "{:?}", h.texts());
        assert!(
            h.find("Used by Passage Guide › Commentaries · Creative Commons: CC0")
                .is_some(),
            "{:?}",
            h.texts()
        );
        // Shown, not hidden; its Download does nothing.
        let at = row_button(&h, MT_NOTES, "Not supported yet");
        let download = row_button(&h, MT_NOTES, "Download");
        h.click(download);
        settle(&mut h);
        assert!(h.shell.st.session.imports.records().next().is_none());
        // The reason is on hover.
        // The tooltip shows once the pointer has rested (a second of frames).
        h.events.push(Event::PointerMoved(at));
        h.frames(60);
        assert!(
            h.texts()
                .iter()
                .any(|t| t.contains("Commentary numbering MT")),
            "{:?}",
            h.texts()
        );
        preview(&h, "get-resources-not-supported");
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_type_filter_search_and_language_filter_combine() {
        let (mut h, _repos, root) = setup("combine");
        open_get_resources(&mut h);
        h.shell.st.get_bibles_ui.query = "greek".into();
        h.frames(2);
        // With All, the Featured sections push "All Resources" below the fold.
        let catalog = h.shell.st.session.get_bibles.catalog().unwrap();
        assert_eq!(
            catalog
                .all("greek", None, obs_engine::get_bibles::ResourceFilter::All)
                .len(),
            3
        );
        h.click_text("Lexicons");
        h.frames(2);
        assert!(h.find("ALL LEXICONS · 1").is_some(), "{:?}", h.texts());
        h.shell.st.get_bibles_ui.language = Some("grc".into());
        h.frames(2);
        assert!(h.find("ALL LEXICONS · 0").is_some(), "{:?}", h.texts());
        assert!(
            h.find("Nothing matches the search, type, language and licence.")
                .is_some()
        );
        h.click_text("Bibles");
        h.frames(2);
        assert!(h.find("ALL BIBLES · 2").is_some(), "{:?}", h.texts());
        assert!(h.find("Greek test Bible").is_some());
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn offline_the_tab_opens_from_the_cache_with_lexicons_commentaries_and_dictionaries() {
        let (mut h, repos, root) = setup("offline-types");
        open_get_resources(&mut h);
        repos.set_offline(true);
        h.shell.run("getbibles.refresh", &json!({}));
        settle(&mut h);
        assert!(
            h.texts()
                .iter()
                .any(|t| t.starts_with("Offline: showing the catalog from")),
            "{:?}",
            h.texts()
        );
        for (filter, header, title) in [
            ("Lexicons", "ALL LEXICONS · 1", STRONGS),
            (
                "Commentaries",
                "ALL COMMENTARIES · 3",
                "Open Bible test notes (ThML commentary)",
            ),
            (
                "Dictionaries",
                "ALL DICTIONARIES · 1",
                "Open Bible test topics (OSIS dictionary)",
            ),
        ] {
            h.click_text(filter);
            h.frames(2);
            assert!(h.find(header).is_some(), "{filter}: {:?}", h.texts());
            assert!(h.find(title).is_some(), "{filter}: {:?}", h.texts());
        }
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    const MORPHGNT: &str = "Morphologically Parsed Greek New Testament based on the SBLGNT";
    const OSHB: &str = "Open Scriptures Hebrew Bible";

    /// #133: the Hebrew & Greek filter lists the Featured original-language Bibles with their
    /// Strong's and Morphology badges, then every Hebrew, Aramaic and Greek Bible; one click
    /// downloads a tagged Greek NT.
    #[test]
    fn the_hebrew_and_greek_filter_shows_tag_badges_and_downloads_a_tagged_greek_nt_in_one_click() {
        let (mut h, _repos, root) = setup("hebrew-greek");
        open_get_resources(&mut h);
        h.click_text("Hebrew & Greek");
        h.frames(2);
        assert_eq!(
            h.shell.st.session.get_bibles.filter(),
            obs_engine::get_bibles::ResourceFilter::HebrewAndGreek
        );
        assert!(
            h.find("FEATURED HEBREW & GREEK · 2").is_some(),
            "{:?}",
            h.texts()
        );
        assert!(
            h.find("ALL HEBREW & GREEK · 3").is_some(),
            "{:?}",
            h.texts()
        );
        assert!(h.find(NASB).is_none(), "English Bibles filtered out");
        assert!(h.find(STRONGS).is_none(), "Lexicons filtered out");
        // Featured order: Hebrew first. Each tagged row has both badges.
        assert!(first(&h, OSHB).top() < first(&h, MORPHGNT).top());
        for title in [OSHB, MORPHGNT] {
            row_button(&h, title, "Strong's");
            row_button(&h, title, "Morphology");
        }
        // The untagged-by-Morphology test Bible has the Strong's badge only.
        row_button(&h, "Greek test Bible", "Strong's");
        assert!(
            h.find(
                "Used by Word Info, Word Study › Occurrences and Forms, and the Exegetical \
                 Guide · Creative Commons: BY 4.0"
            )
            .is_some(),
            "{:?}",
            h.texts()
        );
        // The badge says what it is for.
        let badge = row_button(&h, MORPHGNT, "Morphology");
        h.events.push(Event::PointerMoved(badge));
        h.frames(60);
        assert!(
            h.texts()
                .iter()
                .any(|t| t.contains("Word Study › Forms fills in")),
            "{:?}",
            h.texts()
        );
        h.events.push(Event::PointerMoved(pos2(5.0, 5.0)));
        h.frames(2);
        preview(&h, "get-resources-hebrew-greek");

        let at = row_button(&h, MORPHGNT, "Download");
        h.click(at);
        settle(&mut h);
        let record = h
            .shell
            .st
            .session
            .imports
            .report("morphgnt-1")
            .unwrap()
            .clone();
        assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
        assert!(record.report.unwrap().tagged_words > 0);
        let tab = h.tab_at("Get Resources");
        h.click(tab);
        assert!(h.find("Installed").is_some(), "{:?}", h.texts());
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_failed_download_offers_retry_on_its_row() {
        let (mut h, repos, root) = setup("retry");
        open_get_bibles(&mut h);
        repos.set_offline(true);
        let at = row_button(&h, NASB, "Download");
        h.click(at);
        settle(&mut h);
        let tab = h.tab_at("Get Resources");
        h.click(tab);
        assert!(h.find("Failed").is_some(), "{:?}", h.texts());
        repos.set_offline(false);
        let retry = row_button(&h, NASB, "Retry");
        h.click(retry);
        settle(&mut h);
        let tab = h.tab_at("Get Resources");
        h.click(tab);
        assert!(h.find("Installed").is_some(), "{:?}", h.texts());
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    // ---- Updates (#136) ------------------------------------------------------------------

    const GBF: &str = "King James Version (GBF excerpt)";

    /// Download the GBF test Bible from the Tab's command and show only its row.
    fn downloaded_gbf(name: &str) -> (Harness, Arc<MockRepos>, PathBuf) {
        let (mut h, repos, root) = setup(name);
        open_get_bibles(&mut h);
        h.shell
            .run("getbibles.download", &json!({"module": "ObsKjvGbf"}));
        settle(&mut h);
        let tab = h.tab_at("Get Resources");
        h.click(tab);
        h.shell.st.get_bibles_ui.query = "gbf".into();
        h.frames(2);
        assert!(h.find("Installed").is_some(), "{:?}", h.texts());
        (h, repos, root)
    }

    fn says_up_to_date(h: &Harness) -> bool {
        h.texts().iter().any(|t| t.contains("up to date"))
    }

    #[test]
    fn an_update_shows_on_its_row_and_in_the_library_menu_and_update_all_reloads_open_tabs() {
        let (mut h, repos, root) = downloaded_gbf("update");
        let id = "obskjvgbf-1";
        assert!(
            h.find("Your downloaded Resources are up to date (checked just now).")
                .is_some(),
            "{:?}",
            h.texts()
        );
        // The Bible is open in a Tab while it is updated.
        h.shell.run("resource.open", &json!({"id": id}));
        h.frames(2);
        let bible = h
            .shell
            .st
            .session
            .workspace
            .tabs()
            .find(|t| t.kind == TabKind::Resource(id.into()))
            .unwrap()
            .id;
        let revision = |h: &Harness| {
            h.shell.st.tabs[&bible]
                .view
                .as_ref()
                .unwrap()
                .doc
                .manifest
                .revision
        };
        assert_eq!(revision(&h), 1);

        // CrossWire publishes 1.1; the next catalog fetch offers it.
        repos.publish(
            "crosswire",
            "obskjvgbf.conf",
            "1.1",
            Some(("In<WG1722> the beginning", "In<WG1722> the very beginning")),
        );
        h.shell.run("getbibles.refresh", &json!({}));
        settle(&mut h);
        let tab = h.tab_at("Get Resources");
        h.click(tab);
        assert!(h.find("Update available").is_some(), "{:?}", h.texts());
        assert!(h.find("1 update available: ObsKjvGbf 1.1").is_some());
        assert!(h.find("Update all").is_some());
        assert!(!says_up_to_date(&h), "{:?}", h.texts());
        // (Its title also heads the open Bible's Tab, so the row is found by its label.)
        let label = h.find("Update available").unwrap();
        for button in ["Update", "Open"] {
            assert!(
                h.shapes
                    .iter()
                    .any(|(r, t)| t == button && (r.center().y - label.center().y).abs() < 20.0),
                "{button}: {:?}",
                h.texts()
            );
        }
        preview(&h, "get-resources-update-available");

        // The Library launcher counts it, and so does Get Resources… in its pane.
        assert_eq!(library_badge(&h), Some("1".to_string()));
        h.open_menu("Library");
        assert!(
            h.find("Get Resources… (1 update)").is_some(),
            "{:?}",
            h.texts()
        );
        h.open_menu("Library");

        // Update all: the same Resource, its next revision, read again in its open Tab.
        h.click_text("Update all");
        settle(&mut h);
        let tab = h.tab_at("Get Resources");
        h.click(tab);
        assert_eq!(
            h.shell
                .st
                .session
                .library
                .get(id)
                .unwrap()
                .manifest
                .revision,
            2
        );
        assert_eq!(revision(&h), 2, "the open Tab shows the new revision");
        assert!(h.find("Update available").is_none());
        assert!(h.find("Installed").is_some(), "{:?}", h.texts());
        assert!(
            h.find("Your downloaded Resources are up to date (checked just now).")
                .is_some(),
            "{:?}",
            h.texts()
        );
        assert!(h.find("Library").is_some() && library_badge(&h).is_none());
        preview(&h, "get-resources-updated");
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn one_click_on_update_replaces_the_resource() {
        let (mut h, repos, root) = downloaded_gbf("update-one");
        let resources = h.shell.st.session.library.entries().len();
        repos.publish("crosswire", "obskjvgbf.conf", "2", None);
        h.shell.run("getbibles.refresh", &json!({}));
        settle(&mut h);
        let at = row_button(&h, GBF, "Update");
        h.click(at);
        settle(&mut h);
        let s = &h.shell.st.session;
        assert_eq!(s.library.get("obskjvgbf-1").unwrap().manifest.revision, 2);
        assert_eq!(s.library.entries().len(), resources, "no second Resource");
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_update_with_new_copyright_notes_refreshes_the_open_about_tab() {
        let (mut h, repos, root) = downloaded_gbf("update-about");
        let id = "obskjvgbf-1";
        h.shell.run("resource.about", &json!({ "id": id }));
        h.frames(2);
        let about = h
            .shell
            .st
            .session
            .workspace
            .tabs()
            .find(|t| t.kind == TabKind::About(id.into()))
            .unwrap()
            .id;
        let attribution = |h: &Harness| {
            h.shell.st.tabs[&about].view.as_ref().unwrap().doc.units[0]
                .text
                .clone()
        };
        assert!(!attribution(&h).contains("Copyright notes"));
        repos.publish("crosswire", "obskjvgbf.conf", "1.1", None);
        repos.add_conf_line(
            "crosswire",
            "obskjvgbf.conf",
            "CopyrightNotes=Corrected in 1.1",
        );
        h.shell.run("getbibles.refresh", &json!({}));
        settle(&mut h);
        h.shell
            .run("getbibles.update", &json!({"module": "ObsKjvGbf"}));
        settle(&mut h);
        let manifest = &h.shell.st.session.library.get(id).unwrap().manifest;
        assert_eq!(manifest.revision, 2);
        assert!(
            manifest
                .attribution
                .contains("Copyright notes: Corrected in 1.1")
        );
        assert_eq!(
            attribution(&h),
            manifest.attribution.clone(),
            "the open About Tab shows the new revision's Attribution"
        );
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn offline_the_tab_never_says_the_downloads_are_up_to_date() {
        let (mut h, repos, root) = downloaded_gbf("update-offline");
        assert!(says_up_to_date(&h));
        repos.set_offline(true);
        h.shell.run("getbibles.refresh", &json!({}));
        settle(&mut h);
        let tab = h.tab_at("Get Resources");
        h.click(tab);
        assert!(
            h.find("Updates to your downloaded Resources could not be checked while offline.")
                .is_some(),
            "{:?}",
            h.texts()
        );
        assert!(!says_up_to_date(&h), "{:?}", h.texts());
        // The offline banner and the Featured Hebrew & Greek group (#133) push the row down:
        // scroll the list to it.
        let over = h
            .panels()
            .into_iter()
            .find(|(_, t)| t.iter().any(|t| t == "Get Resources"))
            .unwrap()
            .0
            .center();
        h.events.push(Event::PointerMoved(over));
        for _ in 0..40 {
            if h.find("Installed").is_some() {
                break;
            }
            h.events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -40.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: Modifiers::NONE,
            });
            h.frame();
        }
        h.frames(2);
        assert!(h.find("Installed").is_some(), "{:?}", h.texts());
        preview(&h, "get-resources-updates-offline");
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[path = "saved_searches_tests.rs"]
mod saved_searches;
