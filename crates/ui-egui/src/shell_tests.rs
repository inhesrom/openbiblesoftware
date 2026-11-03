//! Headless Workspace tests: a real egui Context, fed pointer events the way a window would,
//! with the shell drawn each frame. Widgets are found by the text they paint.

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
    time: f64,
    shapes: Vec<(Rect, String)>,
    /// Vertex colours of textured meshes (glyph quads) drawn last frame.
    meshes: Vec<egui::Color32>,
    /// The operating system's appearance, as the window would report it.
    os_theme: egui::Theme,
    /// The last AccessKit tree update (when AccessKit is on).
    tree: Option<egui::accesskit::TreeUpdate>,
    /// Textures as the GPU would hold them, and last frame's shapes (for [`Harness::screenshot`]).
    textures: std::collections::HashMap<egui::TextureId, egui::ColorImage>,
    last_shapes: Vec<egui::epaint::ClippedShape>,
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
        let mut session = Session::new(JobMode::Inline);
        if let Some(db) = db {
            let (store, _) = obs_store::UserStore::open(db).unwrap();
            session.attach_user(Box::new(store));
        }
        let mut shell = Shell::new(session);
        add_doc(&mut shell, bible());
        add_doc(&mut shell, hebrew_doc());
        add_doc(
            &mut shell,
            crate::tests::numbered_bible("heb-num", "Hebrew-numbered Bible", "HNUM", true),
        );
        shell.restore_or_default();
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut h = Self {
            ctx,
            shell,
            events: Vec::new(),
            time: 0.0,
            shapes: Vec::new(),
            meshes: Vec::new(),
            os_theme: egui::Theme::Light,
            tree: None,
            textures: std::collections::HashMap::new(),
            last_shapes: Vec::new(),
        };
        h.frames(3);
        h
    }

    fn frame(&mut self) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            time: Some(self.time),
            events: std::mem::take(&mut self.events),
            system_theme: Some(self.os_theme),
            ..Default::default()
        };
        self.time += 1.0 / 60.0;
        let shell = &mut self.shell;
        let mut out = self.ctx.run_ui(input, |ui| shell.ui(ui));
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
        self.last_shapes = out.shapes.clone();
        if let Some(t) = out.platform_output.accesskit_update.take() {
            self.tree = Some(t);
        }
        self.shapes.clear();
        self.meshes.clear();
        for c in out.shapes {
            collect(&c.shape, &mut self.shapes);
            collect_meshes(&c.shape, &mut self.meshes);
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
            modifiers: Modifiers::NONE,
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
        let (w, h) = (1400usize, 900usize);
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

#[test]
fn default_layout_has_a_bible_and_a_tools_panel() {
    let h = Harness::new();
    let panels = h.panels();
    assert_eq!(panels.len(), 2, "{panels:?}");
    assert_eq!(panels[0].1, ["TB"]);
    assert_eq!(panels[1].1, ["Library", "Word Info"]);
    // The Bible's header: link chip slot, ‹ ›, reference box with the location, title.
    for t in ["A", "‹", "›", "Genesis 30:1", "Test Bible", "…"] {
        assert!(h.find(t).is_some(), "{t:?} missing: {:?}", h.texts());
    }
    // The launcher menus and the Search field.
    for t in [
        "Library", "Guides", "Tools", "Layouts", "View", "Help", "Search",
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
    let from = h.at("HEB");
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
    let from = h.at("HEB");
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
        .filter(|(_, t)| t == "‹")
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
    // It is the active Tab of its Panel: its body shows.
    assert!(h.find("Word Info arrives with #50").is_some());
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
    assert!(
        h.shell
            .st
            .window
            .as_ref()
            .is_some_and(|(_, b)| b.contains("TVTMS"))
    );
    h.open_menu("View");
    h.click_text("✓ Red letter");
    assert!(!h.shell.st.session.text.red_letter);
    h.open_menu("Guides");
    h.click_text("Passage Guide");
    assert!(h.find("Passage Guide arrives with #53").is_some());
    // The + on a tab bar runs the same commands.
    let plus = h
        .shapes
        .iter()
        .filter(|(_, t)| t.trim() == "+")
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
    let title = h.at("HNUM");
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
fn a_layout_round_trips_and_can_be_renamed_and_deleted_from_the_menu() {
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
    h.open_menu("Layouts");
    h.click_text("Two Bibles");
    h.click_text("Restore");
    h.frames(3);
    assert_eq!(workspace_state(&mut h), saved);
    assert_eq!(loc_of(&mut h, "TB"), Some(ge(31, 20)));
    // Rename through the prompt.
    h.open_menu("Layouts");
    h.click_text("Two Bibles");
    h.click_text("Rename…");
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
    h.click_text("Save");
    let user = h.shell.st.session.user.as_ref().unwrap();
    assert_eq!(user.layouts().unwrap(), ["Genesis study"]);
    // Delete.
    h.open_menu("Layouts");
    h.click_text("Genesis study");
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
        let chip = texts_in(&h, cmp)
            .into_iter()
            .find(|t| t.ends_with('×') && t.len() > 2)
            .unwrap_or_else(|| panic!("no range chip in {:?}", texts_in(&h, cmp)));
        h.click_text(&chip);
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

    /// ★ and ☆ are painted shapes: the chrome fonts lack them.
    #[test]
    fn chip_symbols_are_in_the_chrome_fonts() {
        let h = Harness::new();
        h.ctx.fonts_mut(|f| {
            for s in ["×", "—", "+ Bible"] {
                assert!(f.has_glyphs(&egui::FontId::proportional(12.0), s), "{s}");
            }
        });
    }
}
