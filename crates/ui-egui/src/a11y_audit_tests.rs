//! The accessibility audit of the surfaces added since #131 (#182), through the headless
//! harness with AccessKit on:
//!
//! - every Tab kind, toolbar pane, window and menu: no interactive AccessKit node without a
//!   name;
//! - each surface by keyboard alone: open, use, close, with a visible focus ring on the
//!   control that has the keyboard;
//! - every text the chrome paints, in each theme × Page tone, against what it is painted on:
//!   WCAG AA (4.5:1, 3:1 for large text).
//!
//! Findings and the gaps left are in `docs/dev/accessibility.md`.

use egui::Key;
use egui::accesskit::{Action, Node, NodeId, Role, TreeUpdate};
use egui::epaint::ClippedShape;
use egui::{Color32, Shape};
use obs_engine::{Guide, Launcher, PageTone, Tool};

use super::missing_resource::{Lab, release_set, settle, setup};
use super::*;

/// Ctrl as Linux and Windows report it.
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};

pub(super) fn key(h: &mut Harness, key: Key, modifiers: Modifiers) {
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

// ---------------------------------------------------------------------------------------
// The surfaces.

/// Every Tab kind, as the audit opens it. The match has no catch-all, so a new Tab kind does
/// not build until the audit covers it.
fn every_tab_kind() -> Vec<TabKind> {
    let mut kinds = Vec::new();
    for kind in [
        TabKind::Library,
        TabKind::Resource("tagnt-grace".into()),
        TabKind::About("kjv-strongs".into()),
        TabKind::ImportReport(String::new()),
        TabKind::GetBibles,
        TabKind::StarterLibrary,
        TabKind::Credits,
        TabKind::Home,
        TabKind::KeyboardShortcuts,
        TabKind::Settings,
        TabKind::Backups,
        TabKind::Guide(Guide::PassageGuide),
        TabKind::Tool(Tool::Search),
    ] {
        match &kind {
            TabKind::Guide(_) => kinds.extend(Guide::ALL.map(TabKind::Guide)),
            TabKind::Tool(_) => kinds.extend(
                [
                    Tool::Search,
                    Tool::TextComparison,
                    Tool::Notes,
                    Tool::WordInfo,
                    Tool::CrossReferences,
                    Tool::Favorites,
                    Tool::History,
                    Tool::ReadingPlans,
                    Tool::Atlas,
                    Tool::Timeline,
                ]
                .map(TabKind::Tool),
            ),
            TabKind::Library
            | TabKind::Resource(_)
            | TabKind::About(_)
            | TabKind::ImportReport(_)
            | TabKind::GetBibles
            | TabKind::StarterLibrary
            | TabKind::Credits
            | TabKind::Home
            | TabKind::KeyboardShortcuts
            | TabKind::Settings
            | TabKind::Backups => kinds.push(kind),
        }
    }
    kinds
}

/// Open a Tab of `kind` (with something in it to use) in its own Panel and draw it.
fn open_tab(h: &mut Harness, lab: &Lab, kind: &TabKind) {
    let split = json!({ "split": true });
    let (id, params) = match kind {
        TabKind::Library => ("library.open", split),
        TabKind::Resource(id) => {
            let tab = h
                .shell
                .run("resource.open", &json!({ "id": id, "split": true }))
                .unwrap()["tab"]
                .as_u64()
                .unwrap();
            h.shell
                .run("tab.go", &json!({ "tab": tab, "text": "Lk 1:30" }));
            ("tab.interlinear", json!({ "tab": tab, "on": true }))
        }
        TabKind::About(id) => ("resource.about", json!({ "id": id })),
        TabKind::ImportReport(_) => {
            let source = lab.root.join("usfm");
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(
                source.join("GEN.usfm"),
                "\\id GEN\n\\c 1\n\\p\n\\v 1 In the beginning.\n\\ip Dropped.\n",
            )
            .unwrap();
            let path = source.to_string_lossy().to_string();
            ("import.start", json!({ "path": path }))
        }
        TabKind::GetBibles => ("library.get_resources", split),
        TabKind::StarterLibrary => ("library.starter", split),
        TabKind::Credits => ("help.credits", split),
        TabKind::Home => {
            h.shell
                .run("reading_plan.create", &json!({ "template": "nt-90-days" }));
            ("home.open", split)
        }
        TabKind::KeyboardShortcuts => ("help.keyboard_shortcuts", split),
        TabKind::Settings => ("app.settings", split),
        TabKind::Backups => ("data.backups", split),
        TabKind::Guide(Guide::PassageGuide) => (
            "guide.passage_guide",
            json!({ "passage": "Jn 3:16", "split": true }),
        ),
        TabKind::Guide(Guide::WordStudy) => (
            "guide.word_study",
            json!({ "strongs": "G26", "split": true }),
        ),
        TabKind::Guide(Guide::Factbook) => {
            ("guide.factbook", json!({ "name": "John", "split": true }))
        }
        TabKind::Guide(Guide::ExegeticalGuide) => (
            "guide.exegetical_guide",
            json!({ "passage": "Jn 3:16", "split": true }),
        ),
        TabKind::Guide(Guide::TopicGuide) => (
            "guide.topic_guide",
            json!({ "topic": "Forgiveness", "split": true }),
        ),
        TabKind::Tool(Tool::Search) => {
            h.shell.run("tool.search", &split);
            ("search.run", json!({ "text": "God" }))
        }
        TabKind::Tool(Tool::TextComparison) => (
            "tool.text_comparison",
            json!({ "passage": "Gen 1:1", "split": true }),
        ),
        TabKind::Tool(Tool::Notes) => {
            h.shell.run(
                "annotation.note",
                &json!({ "resource": "kjv-strongs", "passage": "Gen 1:1", "body": "A note" }),
            );
            ("tool.notes", split)
        }
        TabKind::Tool(Tool::WordInfo) => ("tool.word_info", split),
        TabKind::Tool(Tool::CrossReferences) => (
            "tool.cross_references",
            json!({ "passage": "Jn 3:16", "split": true }),
        ),
        TabKind::Tool(Tool::Favorites) => {
            h.shell.run("favorite.add", &json!({}));
            ("tool.favorites", split)
        }
        TabKind::Tool(Tool::History) => ("tool.history", split),
        TabKind::Tool(Tool::ReadingPlans) => {
            h.shell
                .run("reading_plan.create", &json!({ "template": "nt-90-days" }));
            ("tool.reading_plans", split)
        }
        TabKind::Tool(Tool::Atlas) => {
            ("tool.atlas", json!({ "passage": "Acts 13", "split": true }))
        }
        TabKind::Tool(Tool::Timeline) => (
            "tool.timeline",
            json!({ "passage": "1 Kings 6", "split": true }),
        ),
    };
    h.shell.run(id, &params);
    settle(h);
    settle_all(h);
}

/// A launch with the Starter library's Resources and a topical index in the Library, and
/// AccessKit on.
fn audit_harness(name: &str) -> (Harness, Lab) {
    let mut set = release_set();
    set.push(obs_engine::testing::topical_index("nave"));
    // Events, so the Timeline (#185) draws its axis rather than its empty state, their people
    // matched to the TIPNR above so Abraham's Lifespan opens his Factbook.
    let tipnr = obs_engine::testing::tipnr();
    set.push(obs_engine::testing::theographic(Some(&tipnr)));
    let (mut h, lab) = setup(name, &set);
    h.ctx.enable_accesskit();
    h.events.push(Event::PointerMoved(pos2(1395.0, 895.0)));
    settle(&mut h);
    (h, lab)
}

// ---------------------------------------------------------------------------------------
// Names and roles.

fn nodes(h: &Harness) -> std::collections::HashMap<NodeId, Node> {
    tree_nodes(h.tree.as_ref().expect("an AccessKit tree"))
}

fn tree_nodes(tree: &TreeUpdate) -> std::collections::HashMap<NodeId, Node> {
    tree.nodes.iter().cloned().collect()
}

/// A node's accessible name: its label (a Label's is its value), else the names of the nodes
/// that label it.
fn name_of(nodes: &std::collections::HashMap<NodeId, Node>, node: &Node) -> String {
    if let Some(l) = node.label().filter(|l| !l.trim().is_empty()) {
        return l.to_string();
    }
    if node.role() == Role::Label {
        return node.value().unwrap_or_default().to_string();
    }
    (node.labelled_by().iter())
        .filter_map(|id| nodes.get(id))
        .filter_map(|n| n.label().or(n.value()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The interactive nodes of the tree (the keyboard can focus them or they can be clicked)
/// that have no accessible name, described by role and bounds.
fn unnamed(h: &Harness) -> Vec<String> {
    unnamed_in(h.tree.as_ref().expect("an AccessKit tree"))
}

/// [`unnamed`] in one window's AccessKit tree.
fn unnamed_in(tree: &TreeUpdate) -> Vec<String> {
    let nodes = tree_nodes(tree);
    let mut out: Vec<String> = (nodes.values())
        .filter(|n| n.supports_action(Action::Click) || n.supports_action(Action::Focus))
        // ARIA requires no name of a window or a scroll bar.
        .filter(|n| !matches!(n.role(), Role::Window | Role::ScrollBar))
        .filter(|n| name_of(&nodes, n).trim().is_empty())
        .map(|n| {
            let b = n.bounds().unwrap_or_default();
            format!(
                "{:?} at ({:.0}, {:.0})–({:.0}, {:.0}){}",
                n.role(),
                b.x0,
                b.y0,
                b.x1,
                b.y1,
                n.value()
                    .map(|v| format!(" value {v:?}"))
                    .unwrap_or_default()
            )
        })
        .collect();
    out.sort();
    out
}

/// Open each of `kinds` over the default layout and hand each state of it to `visit`, named:
/// the Tab as it opens, a Resource Tab also with its Contents pane open (#176), and the Search
/// Tab also in its Grid, Chart and Analysis views and in Library mode (the Resource and
/// Collection choosers, #153) and Notes mode.
fn each_tab_state(
    h: &mut Harness,
    lab: &Lab,
    kinds: &[TabKind],
    mut visit: impl FnMut(&mut Harness, String),
) {
    for kind in kinds {
        h.shell.reset_layout();
        open_tab(h, lab, kind);
        h.frames(2);
        assert!(
            focused_kind(h).as_ref() == Some(kind)
                || matches!(kind, TabKind::ImportReport(_))
                    && matches!(focused_kind(h), Some(TabKind::ImportReport(_))),
            "{kind:?} did not open: {:?}",
            focused_kind(h)
        );
        visit(h, format!("{kind:?}"));
        if matches!(kind, TabKind::Resource(_)) {
            let tab = h.shell.session().workspace.focused().unwrap().0;
            h.shell
                .run("tab.contents", &json!({ "tab": tab, "on": true }));
            settle(h);
            settle_all(h);
            let ws = &h.shell.session().workspace;
            assert!(ws.tab(obs_engine::TabId(tab)).is_some_and(|t| t.contents));
            visit(h, format!("{kind:?} with its Contents pane"));
        }
        if *kind != TabKind::Tool(Tool::Search) {
            continue;
        }
        let tab = h.shell.session().workspace.focused().unwrap().0;
        for view in ["grid", "chart", "analysis"] {
            h.shell
                .run("search.view", &json!({ "tab": tab, "value": view }));
            settle(h);
            visit(h, format!("{kind:?} in the {view} view"));
        }
        for mode in ["library", "notes"] {
            h.shell
                .run("search.mode", &json!({ "tab": tab, "value": mode }));
            h.shell
                .run("search.run", &json!({ "tab": tab, "text": "love" }));
            settle(h);
            let st = h.shell.session().search_tab(obs_engine::TabId(tab));
            assert_eq!(st.map(|st| st.mode.id()), Some(mode));
            visit(h, format!("{kind:?} in {mode} mode"));
        }
    }
}

/// What [`unnamed`] finds in each state of each of `kinds`.
fn audit_tabs(h: &mut Harness, lab: &Lab, kinds: &[TabKind]) -> Vec<String> {
    let mut failures = Vec::new();
    each_tab_state(h, lab, kinds, |h, what| {
        let bad = unnamed(h);
        if !bad.is_empty() {
            failures.push(format!("{what}: {bad:#?}"));
        }
    });
    failures
}

fn focused_kind(h: &Harness) -> Option<TabKind> {
    let ws = &h.shell.session().workspace;
    ws.focused().and_then(|t| ws.tab(t)).map(|t| t.kind.clone())
}

/// Acceptance (#182): every Tab kind, with the Starter library's Resources and with an empty
/// Library (the Guides' get-the-Resource buttons), has no unnamed interactive node.
#[test]
fn every_tab_kind_names_its_interactive_nodes() {
    let (mut h, lab) = audit_harness("a11y-tabs");
    let mut failures = audit_tabs(&mut h, &lab, &every_tab_kind());
    let (mut h, lab) = setup("a11y-tabs-empty", &[]);
    h.ctx.enable_accesskit();
    let without_resources: Vec<TabKind> = (every_tab_kind().into_iter())
        .filter(|k| !matches!(k, TabKind::Resource(_) | TabKind::About(_)))
        .collect();
    failures.extend(audit_tabs(&mut h, &lab, &without_resources));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The toolbar panes, the Command box, the menus and the windows, each opened over the default
/// layout: what opens it, and what shows it is open.
type Overlay = (&'static str, fn(&mut Harness), fn(&Harness) -> bool);

fn every_overlay() -> Vec<Overlay> {
    fn pane(h: &Harness, which: Launcher) -> bool {
        h.shell.st.toolbar.open == Some(which)
    }
    vec![
        (
            "Library pane",
            |h| key(h, Key::L, Modifiers::ALT),
            |h| pane(h, Launcher::Library),
        ),
        (
            "Library pane, searching",
            |h| {
                key(h, Key::L, Modifiers::ALT);
                type_text(h, "kjv");
            },
            |h| pane(h, Launcher::Library),
        ),
        (
            "Guides pane",
            |h| key(h, Key::G, Modifiers::ALT),
            |h| pane(h, Launcher::Guides),
        ),
        (
            "Tools pane",
            |h| key(h, Key::T, Modifiers::ALT),
            |h| pane(h, Launcher::Tools),
        ),
        (
            "Layouts pane",
            |h| key(h, Key::Y, Modifiers::ALT),
            |h| pane(h, Launcher::Layouts),
        ),
        (
            "Command box",
            |h| {
                key(h, Key::L, CTRL);
                type_text(h, "gen 1");
            },
            |h| h.shell.st.command_box.dropdown.is_some(),
        ),
        (
            "View menu",
            |h| h.open_menu("View"),
            |h| h.find("Toolbar position").is_some(),
        ),
        (
            "Help menu",
            |h| h.open_menu("Help"),
            |h| h.find("Credits").is_some(),
        ),
        (
            "Customize toolbar",
            |h| {
                h.shell.run("view.toolbar_customize", &json!({}));
            },
            |h| h.shell.st.toolbar.customizing,
        ),
        (
            "Fonts",
            |h| {
                h.shell.run("view.fonts", &json!({}));
            },
            |h| h.shell.st.fonts_ui.open,
        ),
        (
            "Highlight styles",
            |h| {
                h.shell.run("view.highlight_styles", &json!({}));
            },
            |h| h.shell.st.styles_ui.open,
        ),
        (
            "Copy verses",
            |h| {
                h.shell
                    .run("tool.copy_verses", &json!({ "passages": "Gen 1:1-3" }));
            },
            |h| h.shell.st.copy_ui.open,
        ),
        (
            "New Collection",
            |h| {
                tab_to(h, "New Collection…");
                key(h, Key::Enter, Modifiers::NONE);
            },
            |h| h.shell.st.collection_editor.is_some(),
        ),
        (
            "Right-click menu",
            |h| {
                let kjv = h.tab_id("KJV");
                let word = super::word_info::word_spot(h, kjv, "beginning");
                h.events.push(Event::PointerMoved(word));
                h.frame();
                key(h, Key::F10, Modifiers::SHIFT);
            },
            |h| egui::Popup::is_any_open(&h.ctx),
        ),
    ]
}

/// Acceptance (#182): every toolbar pane, the Command box's suggestions, the menus, the
/// windows and the right-click menu have no unnamed interactive node, and Esc closes each.
#[test]
fn every_pane_menu_and_window_names_its_interactive_nodes() {
    let (mut h, _lab) = audit_harness("a11y-overlays");
    let mut failures = Vec::new();
    for (name, open, is_open) in every_overlay() {
        h.shell.reset_layout();
        settle(&mut h);
        open(&mut h);
        h.frames(3);
        assert!(is_open(&h), "{name} did not open: {:?}", h.texts());
        let bad = unnamed(&h);
        if !bad.is_empty() {
            failures.push(format!("{name}: {bad:#?}"));
        }
        key(&mut h, Key::Escape, Modifiers::NONE);
        if is_open(&h) {
            // A field in it had the keyboard: the first Esc left the field.
            key(&mut h, Key::Escape, Modifiers::NONE);
        }
        assert!(!is_open(&h), "Esc leaves {name} open");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---------------------------------------------------------------------------------------
// The keyboard alone: open, use, close.

/// The node that has the keyboard: its name, role and bounds.
fn focused(h: &Harness) -> Option<(String, Role, egui::Rect)> {
    focused_in(h.tree.as_ref()?)
}

/// [`focused`] in one window's AccessKit tree.
fn focused_in(tree: &TreeUpdate) -> Option<(String, Role, egui::Rect)> {
    let nodes = tree_nodes(tree);
    let node = nodes.get(&tree.focus)?;
    let b = node.bounds()?;
    Some((
        name_of(&nodes, node),
        node.role(),
        egui::Rect::from_min_max(
            pos2(b.x0 as f32, b.y0 as f32),
            pos2(b.x1 as f32, b.y1 as f32),
        ),
    ))
}

/// A stroke in the focus colour is drawn round the focused widget: the shell's focus ring, or
/// a text field's own border.
fn assert_focus_ring(h: &Harness) {
    assert_focus_ring_in(h.tree.as_ref().expect("an AccessKit tree"), &h.last_shapes);
}

/// [`assert_focus_ring`] in one window: its AccessKit tree, and what it painted.
fn assert_focus_ring_in(tree: &TreeUpdate, shapes: &[ClippedShape]) {
    let (name, role, bounds) = focused_in(tree).expect("something has the keyboard");
    let lapis = crate::theme::c().lapis;
    let mut found = false;
    let mut check = |shape: &Shape| {
        if let Shape::Rect(r) = shape
            && r.stroke.color == lapis
            && r.stroke.width >= 1.0
            && r.rect.expand(4.0).contains_rect(bounds)
            && bounds.expand(8.0).contains_rect(r.rect)
        {
            found = true;
        }
    };
    fn walk(shape: &Shape, f: &mut impl FnMut(&Shape)) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, f)),
            s => f(s),
        }
    }
    for c in shapes {
        walk(&c.shape, &mut check);
    }
    assert!(found, "no focus ring round {role:?} {name:?} at {bounds:?}");
}

/// Press Tab until the widget named `name` has the keyboard, and check it shows it.
pub(super) fn tab_to(h: &mut Harness, name: &str) {
    let mut seen = Vec::new();
    for _ in 0..200 {
        if let Some((n, ..)) = focused(h) {
            if n == name {
                assert_focus_ring(h);
                return;
            }
            seen.push(n);
        }
        key(h, Key::Tab, Modifiers::NONE);
    }
    panic!("Tab never reached {name:?}; it went through {seen:?}");
}

/// Ctrl+L, `text`, Enter: the Command box runs its best suggestion.
fn command(h: &mut Harness, text: &str) {
    key(h, Key::L, CTRL);
    type_text(h, text);
    key(h, Key::Enter, Modifiers::NONE);
    settle(h);
}

/// Ctrl+W closes the focused Tab, of `kind`.
fn close_tab(h: &mut Harness, kind: &TabKind) {
    assert_eq!(focused_kind(h).as_ref(), Some(kind));
    let before = count_of(h, kind);
    key(h, Key::W, CTRL);
    settle(h);
    assert_eq!(count_of(h, kind), before - 1, "Ctrl+W left {kind:?} open");
}

fn count_of(h: &Harness, kind: &TabKind) -> usize {
    (h.shell.session().workspace.tabs())
        .filter(|t| t.kind == *kind)
        .count()
}

fn keyboard_harness(name: &str) -> (Harness, Lab) {
    let (h, lab) = audit_harness(name);
    (h, lab)
}

/// The Resource the focused Tab shows.
fn focused_resource(h: &Harness) -> Option<String> {
    match focused_kind(h)? {
        TabKind::Resource(id) => Some(id),
        _ => None,
    }
}

/// Starter library (#131): open it, open a Starter Resource from it, close it.
#[test]
fn keyboard_only_starter_library() {
    let (mut h, _lab) = keyboard_harness("kb-starter");
    command(&mut h, "starter library");
    assert_eq!(focused_kind(&h), Some(TabKind::StarterLibrary));
    tab_to(&mut h, "Open KJV (test)");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(focused_resource(&h).as_deref(), Some("kjv-strongs"));
    command(&mut h, "starter library");
    close_tab(&mut h, &TabKind::StarterLibrary);
}

/// Get Resources (#132, #133, #136): open it, filter to Lexicons, close it.
#[test]
fn keyboard_only_get_resources() {
    let (mut h, _lab) = keyboard_harness("kb-get-resources");
    command(&mut h, "get resources");
    assert_eq!(focused_kind(&h), Some(TabKind::GetBibles));
    tab_to(&mut h, "Lexicons");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(
        h.shell.st.session.get_bibles.filter(),
        obs_engine::get_bibles::ResourceFilter::Type(obs_model::ResourceType::Lexicon)
    );
    close_tab(&mut h, &TabKind::GetBibles);
}

/// The empty states' buttons (#135): an empty Library's Word Study offers Get Lexicons….
#[test]
fn keyboard_only_missing_resource_buttons() {
    let (mut h, _lab) = setup("kb-missing", &[]);
    h.ctx.enable_accesskit();
    command(&mut h, "G26");
    assert_eq!(focused_kind(&h), Some(TabKind::Guide(Guide::WordStudy)));
    tab_to(&mut h, "Get Lexicons…");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::GetBibles));
    close_tab(&mut h, &TabKind::GetBibles);
}

/// Fonts (#138): open the window, open a font list, close both with Esc.
#[test]
fn keyboard_only_fonts() {
    let (mut h, _lab) = keyboard_harness("kb-fonts");
    command(&mut h, "fonts");
    assert!(h.shell.st.fonts_ui.open);
    tab_to(&mut h, "Latin text font");
    key(&mut h, Key::Space, Modifiers::NONE);
    assert!(
        egui::Popup::is_any_open(&h.ctx),
        "the font list did not open"
    );
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!egui::Popup::is_any_open(&h.ctx));
    assert!(h.shell.st.fonts_ui.open, "one Esc closes the list only");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!h.shell.st.fonts_ui.open);
}

/// The Command box (#139): Ctrl+L, a reference, Enter; Esc leaves it.
#[test]
fn keyboard_only_command_box() {
    let (mut h, _lab) = keyboard_harness("kb-command-box");
    key(&mut h, Key::L, CTRL);
    assert_eq!(focused(&h).map(|f| f.0).as_deref(), Some("Command box"));
    assert_focus_ring(&h);
    type_text(&mut h, "gen 1:3");
    assert!(h.shell.st.command_box.dropdown.is_some());
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle_all(&mut h);
    let kjv = h.tab_id("KJV");
    assert_eq!(
        location(&mut h, kjv),
        Some(obs_scripture::Verse::new(
            obs_scripture::Book::Genesis,
            1,
            3
        ))
    );
    key(&mut h, Key::L, CTRL);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_ne!(focused(&h).map(|f| f.0).as_deref(), Some("Command box"));
}

/// A toolbar pane (#140) and History (#155): after reading Gen 1:2 then Gen 1:3, Alt+T,
/// filter, Enter opens History; its Gen 1:2 Visit, Enter, takes the Bible back there; Ctrl+W
/// closes History.
#[test]
fn keyboard_only_toolbar_pane_and_history() {
    let (mut h, _lab) = keyboard_harness("kb-toolbar-pane");
    let kjv = h.tab_id("KJV");
    command(&mut h, "gen 1:2");
    command(&mut h, "gen 1:3");
    let genesis_1 = |v| obs_scripture::Verse::new(obs_scripture::Book::Genesis, 1, v);
    assert_eq!(location(&mut h, kjv), Some(genesis_1(3)));
    key(&mut h, Key::T, Modifiers::ALT);
    assert_eq!(h.shell.st.toolbar.open, Some(Launcher::Tools));
    type_text(&mut h, "history");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(h.shell.st.toolbar.open, None);
    assert_eq!(focused_kind(&h), Some(TabKind::Tool(Tool::History)));
    tab_to(&mut h, "Genesis 1:2 · KJV");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle_all(&mut h);
    assert_eq!(
        location(&mut h, kjv),
        Some(genesis_1(2)),
        "the Visit went nowhere"
    );
    command(&mut h, "history");
    close_tab(&mut h, &TabKind::Tool(Tool::History));
}

/// Customize toolbar… (#141): open it, pick Labels only, close it with Esc.
#[test]
fn keyboard_only_customize_toolbar() {
    let (mut h, _lab) = keyboard_harness("kb-customize");
    command(&mut h, "customize toolbar");
    assert!(h.shell.st.toolbar.customizing);
    tab_to(&mut h, "Labels only");
    key(&mut h, Key::Space, Modifiers::NONE);
    assert_eq!(
        h.shell.st.session.toolbar.labels,
        obs_engine::LauncherLabels::LabelsOnly
    );
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!h.shell.st.toolbar.customizing);
}

/// The right-click menu (#142) from the keyboard alone: Shift+F10 in a Bible acts on the
/// Verse at the top of the view; Passage Guide opens it.
#[test]
fn keyboard_only_right_click_menu() {
    let (mut h, _lab) = keyboard_harness("kb-menu");
    assert_eq!(focused_resource(&h).as_deref(), Some("kjv-strongs"));
    h.events.push(Event::PointerMoved(pos2(1395.0, 895.0)));
    key(&mut h, Key::F10, Modifiers::SHIFT);
    assert!(egui::Popup::is_any_open(&h.ctx), "Shift+F10 opened no menu");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!egui::Popup::is_any_open(&h.ctx), "Esc left the menu open");
    key(&mut h, Key::F10, Modifiers::SHIFT);
    tab_to(&mut h, "Verse: Gen 1:1");
    tab_to(&mut h, "Passage Guide");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::Guide(Guide::PassageGuide)));
    close_tab(&mut h, &TabKind::Guide(Guide::PassageGuide));
}

/// Interlinear lines (#143): open a tagged Bible, turn its lines on, close it.
#[test]
fn keyboard_only_interlinear() {
    let (mut h, _lab) = keyboard_harness("kb-interlinear");
    key(&mut h, Key::L, Modifiers::ALT);
    type_text(&mut h, "tagnt");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle_all(&mut h);
    let id = focused_resource(&h).expect("a Bible opened");
    assert!(id.starts_with("tagnt"), "{id}");
    tab_to(&mut h, "Interlinear");
    key(&mut h, Key::Space, Modifiers::NONE);
    let ws = &h.shell.session().workspace;
    let tab = ws.focused().unwrap();
    assert!(ws.tab(tab).unwrap().interlinear.on);
    close_tab(&mut h, &TabKind::Resource(id));
}

/// The Exegetical Guide (#144): open it at a Passage, follow a word to its Word Study.
#[test]
fn keyboard_only_exegetical_guide() {
    let (mut h, _lab) = keyboard_harness("kb-exegetical");
    command(&mut h, "eg Lk 1:30");
    assert_eq!(
        focused_kind(&h),
        Some(TabKind::Guide(Guide::ExegeticalGuide))
    );
    tab_to(&mut h, "Word Study on G5485 (χάριν)");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::Guide(Guide::WordStudy)));
    close_tab(&mut h, &TabKind::Guide(Guide::WordStudy));
    command(&mut h, "exegetical guide");
    close_tab(&mut h, &TabKind::Guide(Guide::ExegeticalGuide));
}

/// Search views (#146): search, switch to the Grid view, close.
#[test]
fn keyboard_only_search_views() {
    let (mut h, _lab) = keyboard_harness("kb-search");
    key(&mut h, Key::F, CTRL | Modifiers::SHIFT);
    assert_eq!(focused_kind(&h), Some(TabKind::Tool(Tool::Search)));
    tab_to(&mut h, "Search for");
    type_text(&mut h, "God");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    tab_to(&mut h, "Grid");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    let tab = h.shell.session().workspace.focused().unwrap();
    assert_eq!(
        h.shell.session().search_tab(tab).unwrap().view,
        obs_engine::ResultsView::Grid
    );
    close_tab(&mut h, &TabKind::Tool(Tool::Search));
}

/// Reading plans (#147): start a plan, close the Tool.
#[test]
fn keyboard_only_reading_plans() {
    let (mut h, _lab) = keyboard_harness("kb-reading-plans");
    command(&mut h, "reading plans");
    assert_eq!(focused_kind(&h), Some(TabKind::Tool(Tool::ReadingPlans)));
    tab_to(&mut h, "Start plan");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(h.shell.session().reading_plans().len(), 1);
    close_tab(&mut h, &TabKind::Tool(Tool::ReadingPlans));
}

/// Home (#178): Alt+H opens it; a card moves down and Open Home at launch turns on from the
/// keyboard; a Quick start opens its Guide.
#[test]
fn keyboard_only_home() {
    let (mut h, _lab) = keyboard_harness("kb-home");
    key(&mut h, Key::H, Modifiers::ALT);
    settle(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::Home));
    tab_to(&mut h, "Open Home at launch");
    key(&mut h, Key::Space, Modifiers::NONE);
    settle(&mut h);
    assert!(h.shell.session().home_opens_at_launch());
    tab_to(&mut h, "Move Today's reading down");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(
        h.shell.session().home_settings().cards[1].card,
        obs_engine::HomeCard::TodaysReading
    );
    tab_to(&mut h, "Word Study");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::Guide(Guide::WordStudy)));
}

/// The Factbook (#149): from the Guides pane, a name, Enter; then the Summary's link to his
/// father opens the father's Factbook in the same Tab.
#[test]
fn keyboard_only_factbook() {
    let (mut h, _lab) = keyboard_harness("kb-factbook");
    key(&mut h, Key::G, Modifiers::ALT);
    assert_eq!(h.shell.st.toolbar.open, Some(Launcher::Guides));
    tab_to(&mut h, "Factbook: A person or place, e.g. Abraham");
    type_text(&mut h, "Abraham");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::Guide(Guide::Factbook)));
    let entry = |h: &Harness| {
        let ws = &h.shell.session().workspace;
        ws.tab(ws.focused().unwrap())
            .unwrap()
            .entry_history
            .current()
    };
    assert_eq!(
        entry(&h),
        Some(obs_model::Key::Headword("Abraham@Gen.11.26-1Pe".into()))
    );
    tab_to(&mut h, "Terah");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::Guide(Guide::Factbook)));
    assert_eq!(
        entry(&h),
        Some(obs_model::Key::Headword("Terah@Gen.11.24-Luk".into())),
        "the link went nowhere"
    );
    close_tab(&mut h, &TabKind::Guide(Guide::Factbook));
}

/// The Topic Guide (#148): Ctrl+L `topic forgiveness` lists the topics; Enter on one opens
/// it; its Search button hands the topic to the Search Tool.
#[test]
fn keyboard_only_topic_guide() {
    let (mut h, _lab) = keyboard_harness("kb-topic-guide");
    command(&mut h, "topic forgiveness");
    assert_eq!(focused_kind(&h), Some(TabKind::Guide(Guide::TopicGuide)));
    tab_to(&mut h, "Topic Guide: Forgiveness");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    let ws = &h.shell.session().workspace;
    assert_eq!(
        ws.tab(ws.focused().unwrap())
            .unwrap()
            .entry_history
            .current(),
        Some(obs_model::Key::Headword("Forgiveness".into()))
    );
    tab_to(&mut h, "Search your library for “Forgiveness”");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::Tool(Tool::Search)));
    let tab = h.shell.session().workspace.focused().unwrap();
    assert_eq!(
        h.shell.session().search_tab(tab).unwrap().query,
        "Forgiveness",
        "the Search button searched for something else"
    );
    command(&mut h, "topic guide");
    close_tab(&mut h, &TabKind::Guide(Guide::TopicGuide));
}

/// Parallel resources (#151): Ctrl+] shows the next Bible in the same Tab; Ctrl+[ goes back.
#[test]
fn keyboard_only_parallel_resources() {
    let (mut h, _lab) = keyboard_harness("kb-parallel");
    assert_eq!(focused_resource(&h).as_deref(), Some("kjv-strongs"));
    key(&mut h, Key::CloseBracket, CTRL);
    settle(&mut h);
    let next = focused_resource(&h).unwrap();
    assert_ne!(next, "kjv-strongs");
    tab_to(&mut h, "Previous of your Bibles");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(focused_resource(&h).as_deref(), Some("kjv-strongs"));
}

/// Copy verses (#154): open the window, copy plain text, close it with Esc.
#[test]
fn keyboard_only_copy_verses() {
    let (mut h, _lab) = keyboard_harness("kb-copy");
    command(&mut h, "copy verses");
    assert!(h.shell.st.copy_ui.open);
    tab_to(&mut h, "Copy plain text");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(
        h.copied
            .last()
            .is_some_and(|t| t.contains("In the beginning")),
        "{:?}",
        h.copied
    );
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!h.shell.st.copy_ui.open);
}

/// Keyboard shortcuts (#177): filter to Larger text, record Ctrl+Up for it, Reset, close.
#[test]
fn keyboard_only_keyboard_shortcuts() {
    let (mut h, _lab) = keyboard_harness("kb-keys");
    command(&mut h, "keyboard shortcuts");
    assert_eq!(focused_kind(&h), Some(TabKind::KeyboardShortcuts));
    tab_to(&mut h, "Filter");
    type_text(&mut h, "larger");
    tab_to(&mut h, "Larger text shortcut: Ctrl+Plus");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.shell.st.keys_ui.is_recording());
    key(&mut h, Key::ArrowUp, CTRL);
    let session = h.shell.session();
    assert_eq!(session.shortcut("view.text_larger"), Some("Ctrl+Up"));
    tab_to(&mut h, "Reset the Larger text shortcut");
    key(&mut h, Key::Enter, Modifiers::NONE);
    let session = h.shell.session();
    assert_eq!(session.shortcut("view.text_larger"), Some("Ctrl+Plus"));
    close_tab(&mut h, &TabKind::KeyboardShortcuts);
}

/// Settings (#213): Ctrl+, opens it with the search box ready; pick a Page tone, Reset it,
/// turn a toggle on and off, close.
#[test]
fn keyboard_only_settings() {
    let (mut h, _lab) = keyboard_harness("kb-settings");
    key(&mut h, Key::Comma, CTRL);
    assert_eq!(focused_kind(&h), Some(TabKind::Settings));
    // The search box has the keyboard already.
    assert_eq!(
        focused(&h).map(|f| f.0),
        Some("Search settings".to_string())
    );
    type_text(&mut h, "page tone");
    tab_to(&mut h, "Page tone: Sepia");
    key(&mut h, Key::Space, Modifiers::NONE);
    assert_eq!(h.shell.session().appearance.page_tone, PageTone::Sepia);
    tab_to(&mut h, "Reset Page tone");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.shell.session().appearance.page_tone, PageTone::Auto);
    tab_to(&mut h, "Search settings");
    key(&mut h, Key::A, CTRL);
    type_text(&mut h, "red letter");
    tab_to(&mut h, "Red letter");
    key(&mut h, Key::Space, Modifiers::NONE);
    assert!(!h.shell.session().text.red_letter);
    tab_to(&mut h, "Reset Red letter");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.shell.session().text.red_letter);
    close_tab(&mut h, &TabKind::Settings);
}

/// Esc closes one window (#182): with Fonts open behind Copy verses, the one with the
/// keyboard; with the keyboard in neither, the front one.
#[test]
fn escape_closes_the_window_with_the_keyboard_not_the_one_behind() {
    let (mut h, _lab) = keyboard_harness("kb-two-windows");
    command(&mut h, "fonts");
    command(&mut h, "copy verses");
    assert!(h.shell.st.fonts_ui.open && h.shell.st.copy_ui.open);
    tab_to(&mut h, "Copy plain text");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!h.shell.st.copy_ui.open, "Esc left Copy verses open");
    assert!(
        h.shell.st.fonts_ui.open,
        "Esc closed Fonts, behind Copy verses"
    );

    // The keyboard in Fonts, Copy verses in front: Esc closes Fonts.
    command(&mut h, "copy verses");
    tab_to(&mut h, "Filter installed fonts");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!h.shell.st.fonts_ui.open, "Esc left Fonts open");
    assert!(h.shell.st.copy_ui.open, "Esc closed Copy verses");

    // The keyboard in neither: the front window, then the one behind.
    command(&mut h, "fonts");
    h.ctx.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
    });
    h.frames(2);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!h.shell.st.fonts_ui.open, "Esc left the front window open");
    assert!(h.shell.st.copy_ui.open, "Esc closed the window behind");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!h.shell.st.copy_ui.open);
}

/// Esc in the Note editor (#182) saves and closes it, and nothing else: Fonts, open behind
/// it, stays open for the next Esc.
#[test]
fn escape_in_the_note_editor_leaves_the_window_behind_open() {
    let (mut h, _lab) = keyboard_harness("kb-note-over-fonts");
    command(&mut h, "fonts");
    assert!(h.shell.st.fonts_ui.open);
    assert_eq!(focused_resource(&h).as_deref(), Some("kjv-strongs"));
    key(&mut h, Key::F10, Modifiers::SHIFT);
    assert!(egui::Popup::is_any_open(&h.ctx), "Shift+F10 opened no menu");
    tab_to(&mut h, "Verse: Gen 1:1");
    tab_to(&mut h, "Add Note on verse");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.shell.st.note_editor.is_some(), "no Note editor");
    type_text(&mut h, "Light first");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(
        h.shell.st.note_editor.is_none(),
        "Esc left the Note editor open"
    );
    assert!(
        h.shell.st.fonts_ui.open,
        "Esc closed Fonts, behind the Note editor"
    );
    let notes = h.shell.st.session.annotations(None).unwrap();
    assert!(
        notes.iter().any(|n| n.body() == "Light first"),
        "Esc did not save the Note"
    );
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!h.shell.st.fonts_ui.open, "the next Esc left Fonts open");
}

/// Favorites and History (#155): Ctrl+D, then the Favorites Tool opens the Favorite.
#[test]
fn keyboard_only_favorites() {
    let (mut h, _lab) = keyboard_harness("kb-favorites");
    key(&mut h, Key::D, CTRL);
    command(&mut h, "favorites");
    assert_eq!(focused_kind(&h), Some(TabKind::Tool(Tool::Favorites)));
    tab_to(&mut h, "Genesis 1:1 · KJV");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(focused_resource(&h).as_deref(), Some("kjv-strongs"));
    command(&mut h, "favorites");
    close_tab(&mut h, &TabKind::Tool(Tool::Favorites));
}

/// Collections (#153): New Collection… in the Library, a name, Save; Esc closes the editor.
#[test]
fn keyboard_only_collections() {
    let (mut h, _lab) = keyboard_harness("kb-collections");
    tab_to(&mut h, "New Collection…");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.shell.st.collection_editor.is_some());
    tab_to(&mut h, "Name");
    type_text(&mut h, "Mine");
    tab_to(&mut h, "Rules");
    type_text(&mut h, "type:lexicon");
    tab_to(&mut h, "Save");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert!(h.shell.st.collection_editor.is_none());
    assert!(
        (h.shell.session().library.collections().iter()).any(|c| c.name == "Mine"),
        "no Collection saved"
    );
    tab_to(&mut h, "New Collection…");
    key(&mut h, Key::Enter, Modifiers::NONE);
    key(&mut h, Key::Escape, Modifiers::NONE);
    if h.shell.st.collection_editor.is_some() {
        // The first Esc left the field that had the keyboard.
        key(&mut h, Key::Escape, Modifiers::NONE);
    }
    assert!(h.shell.st.collection_editor.is_none());
}

/// A separator between Panels (#182): Tab reaches it and the arrow keys move it.
#[test]
fn keyboard_only_panel_separator() {
    let (mut h, _lab) = keyboard_harness("kb-separator");
    let before = h.panels()[0].0.width();
    tab_to(&mut h, super::super::tile_handles::SEPARATOR);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    let after = h.panels()[0].0.width();
    assert!((after - before - 48.0).abs() < 2.0, "{before} -> {after}");
    assert_eq!(
        focused(&h).map(|f| f.0).as_deref(),
        Some(super::super::tile_handles::SEPARATOR),
        "the arrow keys moved the focus away"
    );
}

/// The Timeline (#185): Tab reaches every entry on the axis. Enter on Fit shows the whole
/// Timeline, on an Era fits the view to
/// it, on an Event moves the Link set's Bible to its Passage, on a Lifespan opens the person's
/// Factbook.
#[test]
fn keyboard_only_timeline() {
    let (mut h, _lab) = keyboard_harness("kb-timeline");
    let kjv = h.tab_id("KJV");
    command(&mut h, "timeline");
    assert_eq!(focused_kind(&h), Some(TabKind::Tool(Tool::Timeline)));
    settle(&mut h);
    let ws = &h.shell.session().workspace;
    assert_eq!(
        ws.tab(kjv).and_then(|t| t.history.current()),
        Some(obs_scripture::Verse::new(
            obs_scripture::Book::Genesis,
            1,
            1
        ))
    );
    let axis = |h: &Harness| {
        let nodes = nodes(h);
        let node = (nodes.values())
            .find(|n| n.role() == Role::Image && name_of(&nodes, n).starts_with("Timeline from"))
            .expect("the Timeline's axis");
        let b = node.bounds().unwrap();
        (b.x1 - b.x0) as f32
    };

    // The Bible is at Genesis 1:1, so the axis shows the Creation; Fit shows it all.
    tab_to(&mut h, "Fit");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    let patriarchs = "Era: Patriarchs, 1996–1635 BC";
    tab_to(&mut h, patriarchs);
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    let (name, _, band) = focused(&h).unwrap();
    assert_eq!(name, patriarchs, "Enter moved the keyboard off the Era");
    assert!(
        band.width() >= axis(&h) - 60.0,
        "the view did not fit the Era: {band:?} on an axis {} wide",
        axis(&h)
    );

    tab_to(&mut h, "Event: Birth of Abraham, 1996 BC");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle_all(&mut h);
    // The Link set moved there: the Timeline with it, and the Bible to the nearest Verse it
    // has (the test KJV ends at Genesis 1:3).
    let ws = &h.shell.session().workspace;
    let at = |t: obs_engine::TabId| ws.tab(t).and_then(|t| t.history.current());
    let genesis = |c, v| {
        Some(obs_scripture::Verse::new(
            obs_scripture::Book::Genesis,
            c,
            v,
        ))
    };
    let timeline = ws.focused().unwrap();
    assert_eq!(
        at(timeline),
        genesis(11, 26),
        "the Event's Passage did not open"
    );
    assert_eq!(
        at(kjv),
        genesis(1, 3),
        "the Bible did not follow from Genesis 1:1"
    );

    tab_to(&mut h, "Lifespan: Abraham, 1996–1821 BC");
    key(&mut h, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::Guide(Guide::Factbook)));
    let ws = &h.shell.session().workspace;
    assert_eq!(
        ws.tab(ws.focused().unwrap())
            .unwrap()
            .entry_history
            .current(),
        Some(obs_model::Key::Headword("Abraham@Gen.11.26-1Pe".into()))
    );
    close_tab(&mut h, &TabKind::Guide(Guide::Factbook));
}

/// Press Tab in the floating OS window `w` until the widget named `name` has its keyboard, and
/// check the window shows it.
fn tab_to_in(h: &mut Harness, w: obs_engine::WindowId, name: &str) {
    let mut seen = Vec::new();
    for _ in 0..200 {
        let win = h.window(w);
        let tree = win.tree.as_ref().expect("the window's AccessKit tree");
        if let Some((n, ..)) = focused_in(tree) {
            if n == name {
                assert_focus_ring_in(tree, &win.last_shapes);
                return;
            }
            seen.push(n);
        }
        drop(win);
        h.key_in(w, Key::Tab, Modifiers::NONE);
    }
    panic!("Tab never reached {name:?} in the floating window; it went through {seen:?}");
}

/// The widths of the Panels of floating window `w`, left to right.
fn floating_panel_widths(h: &Harness, w: obs_engine::WindowId) -> Vec<f32> {
    let d = h.shell.floating.iter().find(|d| d.id == w).unwrap();
    let tiles = &d.tree.tiles;
    let mut rects: Vec<egui::Rect> = (tiles.tile_ids())
        .filter(|t| matches!(tiles.get(*t), Some(Tile::Container(Container::Tabs(_)))))
        .filter_map(|t| tiles.rect(t))
        .collect();
    rects.sort_by(|a, b| a.left().total_cmp(&b.left()));
    rects.iter().map(egui::Rect::width).collect()
}

/// A floating window that is an OS window of its own (#152) is as usable by keyboard and screen
/// reader as the main window (#182): no unnamed interactive node, the focus ring on the
/// control Tab reaches (the Search Tab's own view buttons), and a named separator between its
/// Panels that the arrow keys move.
#[test]
fn keyboard_only_floating_window() {
    let (mut h, _lab) = keyboard_harness("kb-floating");
    h.native_windows();
    key(&mut h, Key::F, CTRL | Modifiers::SHIFT);
    assert_eq!(focused_kind(&h), Some(TabKind::Tool(Tool::Search)));
    let search = h.shell.session().workspace.focused().unwrap();
    h.shell.run("tab.float", &json!({ "tab": search.0 }));
    settle(&mut h);
    let w = h.shell.floating[0].id;
    assert!(h.shell.floating[0].native, "an OS window of its own");
    // A Bible in a Panel beside the Search Tab, in the same window.
    h.shell.run(
        "resource.open",
        &json!({ "id": "kjv-strongs", "split": true }),
    );
    settle(&mut h);
    assert_eq!(h.shell.session().workspace.window_tabs(w).len(), 2);
    assert_eq!(floating_panel_widths(&h, w).len(), 2, "two Panels");
    let bad = unnamed_in(
        h.window(w)
            .tree
            .as_ref()
            .expect("the window's AccessKit tree"),
    );
    assert!(bad.is_empty(), "unnamed in the floating window: {bad:#?}");

    tab_to_in(&mut h, w, "Search for");
    h.window(w).events.push(Event::Text("God".into()));
    h.frames(2);
    h.key_in(w, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    tab_to_in(&mut h, w, "Grid");
    h.key_in(w, Key::Enter, Modifiers::NONE);
    settle(&mut h);
    assert_eq!(
        h.shell.session().search_tab(search).unwrap().view,
        obs_engine::ResultsView::Grid
    );

    let before = floating_panel_widths(&h, w)[0];
    tab_to_in(&mut h, w, super::super::tile_handles::SEPARATOR);
    h.key_in(w, Key::ArrowRight, Modifiers::NONE);
    h.key_in(w, Key::ArrowRight, Modifiers::NONE);
    let after = floating_panel_widths(&h, w)[0];
    assert!((after - before - 48.0).abs() < 2.0, "{before} -> {after}");
    let win = h.window(w);
    assert_eq!(
        focused_in(win.tree.as_ref().unwrap())
            .map(|f| f.0)
            .as_deref(),
        Some(super::super::tile_handles::SEPARATOR),
        "the arrow keys moved the focus away"
    );
}

// ---------------------------------------------------------------------------------------
// Contrast: every theme × Page tone.

/// `top` over `bottom` (sRGB, unmultiplied alpha), as an opaque colour.
fn over(top: Color32, bottom: Color32) -> Color32 {
    let [r, g, b, a] = top.to_srgba_unmultiplied();
    let a = f32::from(a) / 255.0;
    let mix = |t: u8, u: u8| (f32::from(t) * a + f32::from(u) * (1.0 - a)).round() as u8;
    Color32::from_rgb(mix(r, bottom.r()), mix(g, bottom.g()), mix(b, bottom.b()))
}

/// One run of text the chrome painted last frame: its text, colour and size, and the
/// colour under its middle.
struct PaintedText {
    text: String,
    color: Color32,
    background: Color32,
    size: f32,
    bold: bool,
    rect: egui::Rect,
}

impl PaintedText {
    /// WCAG AA: 4.5:1, or 3:1 for large text (18pt, or 14pt bold) and for symbols that are
    /// not words (icons drawn as glyphs: ★, ≡, ×), which count as graphics.
    fn required(&self) -> f32 {
        let large = self.size >= 24.0 || (self.bold && self.size >= 18.66);
        if large || !self.text.chars().any(char::is_alphanumeric) {
            3.0
        } else {
            4.5
        }
    }
}

/// Every text run painted last frame over what it is painted on: the solid fills drawn before
/// it (in paint order, within their clip) under the middle of its run.
fn painted_texts(h: &Harness, base: Color32) -> Vec<PaintedText> {
    fn flatten(shape: &Shape, clip: egui::Rect, out: &mut Vec<(egui::Rect, Shape)>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| flatten(s, clip, out)),
            s => out.push((clip, s.clone())),
        }
    }
    let mut shapes = Vec::new();
    for ClippedShape {
        clip_rect, shape, ..
    } in &h.last_shapes
    {
        flatten(shape, *clip_rect, &mut shapes);
    }
    let screen = egui::Rect::from_min_size(Pos2::ZERO, h.screen);
    let mut fills: Vec<(egui::Rect, Color32)> = Vec::new();
    let mut out = Vec::new();
    for (clip, shape) in shapes {
        match shape {
            Shape::Rect(r) if r.fill.a() > 0 => {
                fills.push((r.rect.intersect(clip), r.fill));
            }
            Shape::Text(t) => {
                let whole = egui::Rect::from_min_size(t.pos, t.galley.size());
                let shown = whole.intersect(clip).intersect(screen);
                if !shown.is_positive() {
                    continue;
                }
                let job = &t.galley.job;
                // Each row's glyphs are the job's characters in order, a newline between rows
                // that end with one; a glyph's section is the one holding its character.
                let chars: Vec<usize> = job.text.char_indices().map(|(b, _)| b).collect();
                let mut next = 0;
                for placed in &t.galley.rows {
                    let first = next;
                    next += placed.glyphs.len() + usize::from(placed.ends_with_newline);
                    let row_rect = placed.rect().translate(t.pos.to_vec2());
                    let visible = row_rect.intersect(clip).intersect(screen);
                    if !visible.is_positive() {
                        continue;
                    }
                    let at = visible.center();
                    let background = (fills.iter())
                        .filter(|(r, _)| r.contains(at))
                        .fold(base, |under, (_, fill)| over(*fill, under));
                    let mut sections: Vec<usize> = (first..first + placed.glyphs.len())
                        .filter_map(|c| chars.get(c))
                        .filter_map(|b| {
                            (job.sections.iter()).position(|s| {
                                (s.byte_range.start.0..s.byte_range.end.0).contains(b)
                            })
                        })
                        .collect();
                    sections.dedup();
                    for i in sections {
                        let section = &job.sections[i];
                        let range = section.byte_range.start.0..section.byte_range.end.0;
                        let text = job.text[range].trim().to_string();
                        if text.is_empty() {
                            continue;
                        }
                        let mut color = t.override_text_color.unwrap_or(section.format.color);
                        if color == Color32::PLACEHOLDER {
                            color = t.fallback_color;
                        }
                        let color = color.gamma_multiply(t.opacity_factor);
                        let font = &section.format.font_id;
                        let bold = matches!(&font.family, egui::FontFamily::Name(n) if n.as_ref() == "display");
                        out.push(PaintedText {
                            text,
                            color: over(color, background),
                            background,
                            size: font.size,
                            bold,
                            rect: visible,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The disabled controls' bounds: WCAG asks no contrast of an inactive control's text.
fn disabled(h: &Harness) -> Vec<egui::Rect> {
    (nodes(h).values())
        .filter(|n| n.is_disabled())
        .filter_map(Node::bounds)
        .map(|b| {
            egui::Rect::from_min_max(
                pos2(b.x0 as f32, b.y0 as f32),
                pos2(b.x1 as f32, b.y1 as f32),
            )
        })
        .collect()
}

/// The text runs of last frame below AA, described.
fn contrast_failures(h: &Harness) -> Vec<String> {
    let base = crate::theme::c().shell;
    let disabled = disabled(h);
    let mut out: Vec<String> = painted_texts(h, base)
        .into_iter()
        .filter(|t| !disabled.iter().any(|d| d.contains(t.rect.center())))
        .filter_map(|t| {
            let ratio = crate::palette::contrast(t.color, t.background);
            (ratio < t.required()).then(|| {
                format!(
                    "{:?} {:?} on {:?}: {ratio:.2}:1 (needs {}:1)",
                    t.text,
                    t.color,
                    t.background,
                    t.required()
                )
            })
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Every theme × Page tone, applied and drawn: (theme, tone).
fn each_theme_and_tone(h: &mut Harness, mut check: impl FnMut(&mut Harness, &str, &str)) {
    for theme in ["light", "dark"] {
        for (_, tone, _) in PageTone::ALL {
            h.shell.run("view.theme", &json!({ "value": theme }));
            h.shell.run("view.page_tone", &json!({ "value": tone }));
            // Past every fade (windows, menus, hover): colours as they settle.
            h.frames(15);
            check(h, theme, tone);
        }
    }
}

/// Acceptance (#182): no text on any Tab kind, toolbar pane, menu or window is below WCAG AA
/// in Light or Dark with any Page tone.
#[test]
fn every_surface_meets_wcag_aa_contrast_in_every_theme_and_page_tone() {
    let (mut h, lab) = audit_harness("a11y-contrast");
    let mut failures = std::collections::BTreeMap::<String, Vec<String>>::new();
    let mut checked = 0;
    let mut record = |h: &mut Harness, surface: &str, theme: &str, tone: &str| {
        checked += painted_texts(h, crate::theme::c().shell).len();
        for f in contrast_failures(h) {
            failures
                .entry(f)
                .or_default()
                .push(format!("{surface} ({theme}, {tone})"));
        }
    };
    each_tab_state(&mut h, &lab, &every_tab_kind(), |h, what| {
        each_theme_and_tone(h, |h, theme, tone| record(h, &what, theme, tone));
        h.shell.run("view.theme", &json!({ "value": "light" }));
        h.shell.run("view.page_tone", &json!({ "value": "auto" }));
    });
    for (name, open, is_open) in every_overlay() {
        h.shell.reset_layout();
        settle(&mut h);
        open(&mut h);
        h.frames(3);
        assert!(is_open(&h), "{name} did not open");
        each_theme_and_tone(&mut h, |h, theme, tone| record(h, name, theme, tone));
        key(&mut h, Key::Escape, Modifiers::NONE);
        if is_open(&h) {
            key(&mut h, Key::Escape, Modifiers::NONE);
        }
    }
    assert!(checked > 10_000, "only {checked} text runs checked");
    let report: Vec<String> = (failures.iter())
        .map(|(f, at)| format!("{f}\n    in {} (and {} more)", at[0], at.len() - 1))
        .collect();
    assert!(
        report.is_empty(),
        "{} failures:\n{}",
        report.len(),
        report.join("\n")
    );
}

/// The chrome's colours themselves, for states the rendered check does not catch (hover, a
/// selected row): every text colour on every background it can sit on meets AA in both
/// themes, symbols and the focus ring 3:1.
#[test]
fn chrome_text_colours_meet_wcag_aa_on_every_chrome_background() {
    use crate::palette::contrast;
    use crate::theme::{DARK, LIGHT};
    for ch in [LIGHT, DARK] {
        let backgrounds = [
            ("shell", ch.shell),
            ("surface", ch.surface),
            ("field", ch.field),
            ("hover", ch.hover),
            ("selection", ch.selection),
        ];
        for (bg_name, bg) in backgrounds {
            for (name, ink, needs) in [
                ("ink", ch.ink, 4.5),
                ("muted", ch.muted, 4.5),
                ("lapis", ch.lapis, 4.5),
                ("verdigris", ch.verdigris, 4.5),
                ("warn", ch.warn, 4.5),
                ("gilt (★ only)", ch.gilt, 3.0),
                ("focus ring", ch.lapis, 3.0),
            ] {
                let c = contrast(ink, bg);
                assert!(
                    c >= needs,
                    "{} theme: {name} on {bg_name} is {c:.2}:1, needs {needs}:1",
                    if ch.dark { "Dark" } else { "Light" }
                );
            }
        }
        // Chips and badges: their text is chosen for their fill.
        for link in [None, Some('A'), Some('B'), Some('C'), Some('D')] {
            crate::theme::set_for_test(ch);
            let fill = crate::theme::link_color(link);
            let c = contrast(crate::theme::text_on(fill), fill);
            assert!(c >= 4.5, "chip {link:?}: {c:.2}:1");
        }
        let c = contrast(crate::theme::text_on(ch.gilt), ch.gilt);
        assert!(c >= 4.5, "badge: {c:.2}:1");
    }
    crate::theme::set_for_test(LIGHT);
    // The heat cells of the Grid view, at every heat.
    for heat in 0..=20 {
        let fill = super::super::search_views::heat_fill(heat as f32 / 20.0);
        let c = contrast(crate::theme::text_on(fill), fill);
        assert!(c >= 4.5, "heat {heat}/20: {c:.2}:1");
    }
}

/// Visual evidence for `docs/previews/` when `UPDATE_PREVIEWS` is set: the focus ring on a
/// Tab reached with the keyboard, the right-click menu opened with Shift+F10 alone, and the
/// Tools pane's selected row in the Dark theme.
#[test]
fn previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
    let (mut h, _lab) = keyboard_harness("a11y-previews");
    command(&mut h, "get resources");
    tab_to(&mut h, "Lexicons");
    h.frames(15);
    h.screenshot(&dir.join("a11y-focus-ring-get-resources.ppm"));
    h.shell.reset_layout();
    settle(&mut h);
    key(&mut h, Key::F10, Modifiers::SHIFT);
    tab_to(&mut h, "Verse: Gen 1:1");
    h.frames(15);
    h.screenshot(&dir.join("a11y-keyboard-menu.ppm"));
    key(&mut h, Key::Escape, Modifiers::NONE);
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    key(&mut h, Key::T, Modifiers::ALT);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    h.frames(15);
    h.screenshot(&dir.join("a11y-dark-tools-pane.ppm"));
}
