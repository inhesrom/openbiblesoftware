//! A Resource's own Text size (#292) through the shell's real-event headless harness: the
//! Text size row in a Tab's ⋯ menu and the Aa button's popover change the Resource's size
//! live while the menu stays open, every Tab of the Resource follows and keeps its Current
//! location, the Ctrl+Alt keys change the focused Resource and the Ctrl keys the global size,
//! and multi-Resource Tabs draw each row at its Resource's size.

use egui::Key;
use egui::accesskit::{Node, NodeId, Role};
use obs_engine::TabId;

use super::*;

/// Ctrl as Linux and Windows report it.
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};
/// ⌘ as macOS reports it.
const MAC_CMD: Modifiers = Modifiers {
    mac_cmd: true,
    command: true,
    ..Modifiers::NONE
};

const TB: &str = "test-bible";
const HEB: &str = "heb-test";

fn press(h: &mut Harness, key: Key, physical: Key, modifiers: Modifiers) {
    h.modifiers = modifiers;
    for pressed in [true, false] {
        h.events.push(Event::Key {
            key,
            physical_key: Some(physical),
            pressed,
            repeat: false,
            modifiers,
        });
        h.frame();
    }
    h.modifiers = Modifiers::NONE;
    h.frame();
}

fn pane_rect(h: &Harness, tab: TabId) -> Rect {
    let tiles = &h.shell.tree.tiles;
    tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
}

/// Where `text` (or an icon, `[name]`) is painted inside Tab `tab`'s Panel.
fn in_pane(h: &Harness, tab: TabId, text: &str) -> Option<Pos2> {
    let pane = pane_rect(h, tab);
    (h.shapes.iter().rev())
        .find(|(r, t)| t == text && pane.contains(r.center()))
        .map(|(r, _)| r.center())
}

/// Click the ⋯ (Tab menu) button in Tab `tab`'s header.
fn open_tab_menu(h: &mut Harness, tab: TabId) {
    let dots = in_pane(h, tab, "[ellipsis]").expect("the Tab menu button");
    h.click(dots);
}

fn size_of(h: &Harness, resource: &str) -> f32 {
    h.shell.st.session.text_size_of(resource)
}

fn view_size(h: &Harness, tab: TabId) -> f32 {
    h.shell.st.tabs[&tab].view.as_ref().unwrap().settings().size
}

fn nodes(h: &Harness) -> std::collections::HashMap<NodeId, Node> {
    let tree = h.tree.as_ref().expect("an AccessKit tree");
    tree.nodes.iter().cloned().collect()
}

/// The bounds of the AccessKit node whose label starts with `name`, and its role and label.
fn node(h: &Harness, name: &str) -> Option<(Rect, Role, String)> {
    nodes(h).values().find_map(|n| {
        let label = n.label()?;
        let b = n.bounds()?;
        label.starts_with(name).then(|| {
            (
                Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32)),
                n.role(),
                label.to_string(),
            )
        })
    })
}

/// [`node`] inside Tab `tab`'s Panel.
fn node_in(h: &Harness, tab: TabId, name: &str) -> Option<(Rect, Role, String)> {
    let pane = pane_rect(h, tab);
    nodes(h).values().find_map(|n| {
        let label = n.label()?;
        let b = n.bounds()?;
        let r = Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32));
        (label.starts_with(name) && pane.contains(r.center()))
            .then(|| (r, n.role(), label.to_string()))
    })
}

/// The node that has the keyboard: its label.
fn focused_label(h: &Harness) -> Option<String> {
    let tree = h.tree.as_ref()?;
    let nodes = nodes(h);
    nodes.get(&tree.focus)?.label().map(str::to_string)
}

fn launch(name: &str) -> (Harness, std::path::PathBuf) {
    let db = temp_db(name);
    let mut h = Harness::launch(Some(&db));
    h.ctx.enable_accesskit();
    // TB with a Panel of HEB beside it.
    let tb = h.tab_id("TB");
    h.shell.st.session.workspace.user_focused(tb);
    h.shell
        .run("resource.open", &json!({ "id": HEB, "split": true }));
    settle_all(&mut h);
    h.shell.st.session.workspace.user_focused(tb);
    h.frames(2);
    (h, db)
}

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// Acceptance: the ⋯ menu has the Text size row (A−, slider, A+, the size, Reset); a change
/// applies live, the menu stays open, and dragging the slider re-lays the text out on the
/// frames of the drag; only that Resource changes; Reset returns it to the global size.
#[test]
fn the_tab_menu_row_changes_the_resources_size_live_and_the_menu_stays_open() {
    let (mut h, db) = launch("menu-row");
    let tb = h.tab_id("TB");
    let heb = h.tab_id("HEB");
    let default = h.shell.st.session.text.size;
    open_tab_menu(&mut h, tb);
    for text in ["Text size · TB", "follows Text size", "A−", "A+", "Reset"] {
        assert!(h.find(text).is_some(), "{text:?} not in {:?}", h.texts());
    }
    assert!(h.find(&format!("{default:.0} pt")).is_some());
    // The menu's own items are still there below it.
    assert!(h.find("Close Tab").is_some());
    preview(&h, "resource-text-size-menu-follows");

    h.click_text("A+");
    assert_eq!(size_of(&h, TB), default + 1.0);
    assert!(h.find("Close Tab").is_some(), "the menu stays open");
    assert!(h.find("own size").is_some(), "{:?}", h.texts());
    h.click_text("A+");
    settle_all(&mut h);
    assert_eq!(size_of(&h, TB), default + 2.0);
    assert_eq!(view_size(&h, tb), default + 2.0, "the text is laid out at it");
    assert_eq!(view_size(&h, heb), default, "only TB's Resource");
    assert!(h.find("Close Tab").is_some(), "the menu stays open");

    // Drag the slider to its right end: the size grows frame by frame, and the open view
    // follows on the same frame, so the reader sees it while dragging.
    let (slider, role, label) = node(&h, "Text size for TB,").expect("the slider");
    assert_eq!(role, Role::Slider);
    assert_eq!(label, format!("Text size for TB, {:.0} points", default + 2.0));
    let (from, to) = (slider.center(), pos2(slider.right() + 40.0, slider.center().y));
    h.events.push(Event::PointerMoved(from));
    h.frame();
    h.button(from, true);
    h.frame();
    let mut seen = Vec::new();
    for i in 1..=10 {
        h.events
            .push(Event::PointerMoved(from + (to - from) * (i as f32 / 10.0)));
        h.frame();
        let size = size_of(&h, TB);
        assert_eq!(view_size(&h, tb), size, "the view is at the size on every frame");
        if seen.last() != Some(&size) {
            seen.push(size);
        }
    }
    h.button(to, false);
    h.frames(2);
    assert!(seen.len() >= 3, "the size changed while dragging: {seen:?}");
    assert_eq!(size_of(&h, TB), obs_engine::appearance::TEXT_SIZE_MAX);
    assert!(h.find("Close Tab").is_some(), "the menu stays open");
    settle_all(&mut h);
    preview(&h, "resource-text-size-menu-own");

    // Reset: TB follows the global size again.
    h.click_text("Reset");
    settle_all(&mut h);
    assert_eq!(h.shell.st.session.resource_text_sizes().get(TB), None);
    assert_eq!(view_size(&h, tb), default);
    assert!(h.find("follows Text size").is_some());
    drop(h);
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

/// Decision: the Aa button opens the Text size row alone in one click; the slider is keyboard
/// operable with an accessible name, and the size survives a restart.
#[test]
fn the_aa_button_opens_the_slider_in_one_click_and_the_keyboard_works_it() {
    let (mut h, db) = launch("aa");
    let tb = h.tab_id("TB");
    let default = h.shell.st.session.text.size;
    let (aa, role, _) = node_in(&h, tb, "Text size for TB").expect("the Aa button");
    assert_eq!(role, Role::Button);
    h.click(aa.center());
    assert!(h.find("Text size · TB").is_some(), "{:?}", h.texts());
    assert!(h.find("Close Tab").is_none(), "only the Text size row");
    preview(&h, "resource-text-size-aa");

    // The keyboard: Tab to the slider, arrows change the size.
    let slider = format!("Text size for TB, {default:.0} points");
    for _ in 0..40 {
        if focused_label(&h).as_deref() == Some(slider.as_str()) {
            break;
        }
        press(&mut h, Key::Tab, Key::Tab, Modifiers::NONE);
    }
    assert_eq!(focused_label(&h).as_deref(), Some(slider.as_str()));
    press(&mut h, Key::ArrowRight, Key::ArrowRight, Modifiers::NONE);
    press(&mut h, Key::ArrowRight, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(size_of(&h, TB), default + 2.0);
    assert_eq!(
        focused_label(&h),
        Some(format!("Text size for TB, {:.0} points", default + 2.0))
    );
    press(&mut h, Key::ArrowLeft, Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(size_of(&h, TB), default + 1.0);
    settle_all(&mut h);
    assert_eq!(view_size(&h, tb), default + 1.0);
    drop(h);

    // Kept in the User database: a restart opens TB at it.
    let mut h = Harness::launch(Some(&db));
    settle_all(&mut h);
    let tb = h.tab_id("TB");
    assert_eq!(view_size(&h, tb), default + 1.0);
    drop(h);
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

/// Acceptance: Ctrl+Alt with +, = or numpad +; − ; 0 changes the focused Tab's Resource
/// (⌘⌥ on macOS), as a US keyboard reports them (+ is Shift+=); Ctrl+Plus and Ctrl+Minus
/// still change the global size, which no longer moves a Resource with its own.
#[test]
fn ctrl_alt_keys_size_the_focused_resource_and_ctrl_keys_the_global_size() {
    for held in [CTRL, MAC_CMD] {
        let (mut h, db) = launch("keys");
        let tb = h.tab_id("TB");
        let default = h.shell.st.session.text.size;
        let alt = Modifiers { alt: true, ..held };
        let shift_alt = Modifiers {
            shift: true,
            ..alt
        };
        press(&mut h, Key::Equals, Key::Equals, alt);
        assert_eq!(size_of(&h, TB), default + 1.0, "Ctrl+Alt+=");
        press(&mut h, Key::Plus, Key::Equals, shift_alt);
        assert_eq!(size_of(&h, TB), default + 2.0, "Ctrl+Alt+Shift+= (+)");
        press(&mut h, Key::Plus, Key::Plus, alt);
        assert_eq!(size_of(&h, TB), default + 3.0, "Ctrl+Alt+numpad +");
        press(&mut h, Key::Minus, Key::Minus, alt);
        assert_eq!(size_of(&h, TB), default + 2.0, "Ctrl+Alt+−");
        assert!(h.find("TB text 22 pt").is_some(), "{:?}", h.texts());
        assert_eq!(h.shell.st.session.text.size, default, "not the global size");
        assert_eq!(size_of(&h, HEB), default, "not another Resource");

        // The global keys still work, with Shift as a US keyboard holds it for +, and move
        // HEB, which follows, but not TB.
        press(&mut h, Key::Plus, Key::Equals, Modifiers { shift: true, ..held });
        press(&mut h, Key::Equals, Key::Equals, held);
        assert_eq!(h.shell.st.session.text.size, default + 2.0, "Ctrl+Plus");
        assert_eq!(size_of(&h, HEB), default + 2.0);
        assert_eq!(size_of(&h, TB), default + 2.0, "its own size, not moved");
        press(&mut h, Key::Minus, Key::Minus, held);
        assert_eq!(h.shell.st.session.text.size, default + 1.0, "Ctrl+Minus");
        assert_eq!(size_of(&h, TB), default + 2.0);

        press(&mut h, Key::Num0, Key::Num0, alt);
        assert_eq!(h.shell.st.session.resource_text_sizes().get(TB), None, "Ctrl+Alt+0");
        assert_eq!(size_of(&h, TB), default + 1.0, "follows the global size again");
        press(&mut h, Key::Num0, Key::Num0, held);
        assert_eq!(size_of(&h, TB), default, "Ctrl+0");
        settle_all(&mut h);
        assert_eq!(view_size(&h, tb), default);
        drop(h);
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
    }
}

/// Acceptance: a size change lays the text out again at the same Current location in every
/// Tab of the Resource.
#[test]
fn every_tab_of_the_resource_keeps_its_current_location() {
    let (mut h, tb, hn) = linked_harness();
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:20" }));
    settle_all(&mut h);
    // A second TB Tab, elsewhere.
    h.shell.st.session.workspace.user_focused(hn);
    h.shell
        .run("resource.open", &json!({ "id": TB, "split": true }));
    settle_all(&mut h);
    let tb2 = (h.shell.st.session.workspace.tabs())
        .filter(|t| t.kind == TabKind::Resource(TB.into()))
        .map(|t| t.id)
        .find(|t| *t != tb)
        .unwrap();
    h.shell
        .run("tab.go", &json!({ "tab": tb2.0, "text": "Gen 2:4" }));
    settle_all(&mut h);
    let before = [location(&mut h, tb), location(&mut h, tb2), location(&mut h, hn)];
    assert!(before.iter().all(Option::is_some), "{before:?}");
    for value in [30, 14, 26] {
        h.shell.run(
            "view.resource_text_size",
            &json!({ "resource": TB, "value": value }),
        );
        settle_all(&mut h);
        for t in [tb, tb2] {
            assert_eq!(view_size(&h, t), value as f32);
        }
        assert_eq!(
            [location(&mut h, tb), location(&mut h, tb2), location(&mut h, hn)],
            before,
            "at {value} pt"
        );
    }
    assert_eq!(view_size(&h, hn), h.shell.st.session.text.size);
}

/// Decision: a Text Comparison Tab draws each row at its Bible's own size, and its Aa
/// button and ⋯ row change the Bible last clicked into, named.
#[test]
fn a_text_comparison_draws_each_row_at_its_bibles_size_and_sizes_the_one_clicked_into() {
    let mut h = Harness::new();
    h.ctx.enable_accesskit();
    let tb = h.tab_id("TB");
    h.shell.st.session.workspace.user_focused(tb);
    h.shell
        .run("tool.text_comparison", &json!({ "split": true }));
    h.frames(2);
    let cmp = h.tab_id("Text Comparison");
    h.shell
        .run("tab.link", &json!({ "tab": cmp.0, "set": "A" }));
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 31:20" }));
    settle_all(&mut h);
    h.frames(10);
    let rows = |h: &Harness| {
        let v = h.shell.st.tabs[&cmp].comparison.as_ref().unwrap();
        (0..3).map(|r| v.row_settings(r).size).collect::<Vec<_>>()
    };
    let default = h.shell.st.session.text.size;
    assert_eq!(rows(&h), [default; 3]);
    // The first compared Bible until one is clicked into.
    assert!(node_in(&h, cmp, "Text size for TB").is_some());

    // Click into HEB's row (the second): the Aa button now names HEB.
    let heb_row = in_pane(&h, cmp, "HEB").expect("HEB's row label");
    h.click(heb_row + vec2(LABEL_COLUMN_OFFSET, 0.0));
    assert_eq!(
        h.shell.st.session.text_size_resource(cmp).as_deref(),
        Some(HEB)
    );
    let (aa, ..) = node_in(&h, cmp, "Text size for HEB").expect("the Aa button names HEB");
    h.click(aa.center());
    assert!(h.find("Text size · HEB").is_some(), "{:?}", h.texts());
    h.click_text("A+");
    h.click_text("A+");
    h.frames(5);
    assert_eq!(rows(&h), [default, default + 2.0, default]);
    preview(&h, "resource-text-size-comparison");
}

/// How far right of a row's label to click into its text.
const LABEL_COLUMN_OFFSET: f32 = crate::compare_view::LABEL_WIDTH;

/// Acceptance: Settings → Text and fonts lists the Resources with their own size, each with
/// Reset, and Reset all; its search finds it by "font size" and "zoom".
#[test]
fn settings_lists_own_sizes_with_reset_and_reset_all() {
    let (mut h, db) = launch("settings");
    let default = h.shell.st.session.text.size;
    h.shell.run(
        "view.resource_text_size",
        &json!({ "resource": TB, "value": 24 }),
    );
    h.shell.run(
        "view.resource_text_size",
        &json!({ "resource": HEB, "value": 30 }),
    );
    h.shell.run("app.settings", &json!({}));
    h.frames(3);
    for query in ["font size", "zoom"] {
        let tab = h.tab_id("Settings");
        if let Some(t) = h.shell.st.tabs.get_mut(&tab) {
            t.filter = query.to_string();
        }
        h.frames(2);
        assert!(
            h.find("Resources with their own Text size").is_some(),
            "{query}: {:?}",
            h.texts()
        );
    }
    let tab = h.tab_id("Settings");
    if let Some(t) = h.shell.st.tabs.get_mut(&tab) {
        t.filter = "own text size".to_string();
    }
    h.frames(2);
    for text in ["TB — 24 pt", "HEB — 30 pt", "Reset all"] {
        assert!(h.find(text).is_some(), "{text:?} not in {:?}", h.texts());
    }
    preview(&h, "resource-text-size-settings");
    let (reset, ..) = node(&h, "Reset the Text size of TB").expect("TB's Reset");
    h.click(reset.center());
    assert_eq!(size_of(&h, TB), default);
    assert_eq!(size_of(&h, HEB), 30.0);
    assert!(h.find("TB — 24 pt").is_none());
    h.click_text("Reset all");
    assert!(h.shell.st.session.resource_text_sizes().is_empty());
    assert!(h.find("HEB — 30 pt").is_none());
    drop(h);
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}
