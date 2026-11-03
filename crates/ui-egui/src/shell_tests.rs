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
        let mut shell = Shell::new(Session::new(JobMode::Inline));
        add_doc(&mut shell, bible());
        add_doc(&mut shell, hebrew_doc());
        shell.reset_layout();
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut h = Self {
            ctx,
            shell,
            events: Vec::new(),
            time: 0.0,
            shapes: Vec::new(),
        };
        h.frames(3);
        h
    }

    fn frame(&mut self) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
            time: Some(self.time),
            events: std::mem::take(&mut self.events),
            ..Default::default()
        };
        self.time += 1.0 / 60.0;
        let shell = &mut self.shell;
        let mut out = self.ctx.run_ui(input, |ui| shell.ui(ui));
        // No GPU here: the texture uploads are dropped on purpose.
        out.textures_delta.clear();
        self.shapes.clear();
        for c in out.shapes {
            collect(&c.shape, &mut self.shapes);
        }
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
    for t in ["–", "‹", "›", "Genesis 30:1", "Test Bible", "…"] {
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
    assert_eq!(
        loc(&mut h, heb),
        Some(obs_scripture::Verse::new(
            obs_scripture::Book::Genesis,
            1,
            1
        ))
    );
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
    h.click_text("Dark page");
    assert!(h.shell.st.session.dark_page);
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
            "view.dark_page",
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
