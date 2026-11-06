#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use obs_model::{Manifest, ResourceType};
use obs_scripture::{Book, ListedVersification, Passage, Standard, Verse, VerseMap, VerseMapEntry};
use serde_json::json;

use crate::*;

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// KJV and a Hebrew-numbered Bible, with a User database for Layouts.
fn session(name: &str) -> (Session, Temp) {
    let root = std::env::temp_dir().join(format!("obs-floating-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut s = Session::new(JobMode::Inline);
    for (id, ab) in [("kjv", "KJV"), ("heb", "HEB")] {
        s.library.add(LibraryEntry {
            manifest: Manifest::new(id, ab, ab, ResourceType::Bible, "en"),
            path: format!("/library/{id}.obr").into(),
        });
    }
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    (s, Temp(root))
}

fn ge(c: u16, v: u16) -> Verse {
    Verse::new(Book::Genesis, c, v)
}

/// Genesis 31-32 numbered the Hebrew way: KJV 31:55 is 32:1, KJV 32:n is 32:n+1.
fn hebrew_map() -> VerseMap {
    let entry = |native: Verse, std: Verse| VerseMapEntry {
        verse: native,
        last: native.verse,
        standard: Some(Passage::verse(std)),
    };
    let mut e: Vec<VerseMapEntry> = (1..=54).map(|v| entry(ge(31, v), ge(31, v))).collect();
    e.push(entry(ge(32, 1), ge(31, 55)));
    e.extend((2..=33).map(|v| entry(ge(32, v), ge(32, v - 1))));
    VerseMap::new(e, vec![], &Standard)
}

fn open(s: &mut Session, id: &str) -> TabId {
    TabId(
        s.run("resource.open", &json!({ "id": id, "split": true }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    )
}

fn placement(x: f32, y: f32, screen: Option<ScreenRect>) -> WindowPlacement {
    WindowPlacement {
        x,
        y,
        width: 800.0,
        height: 600.0,
        screen,
    }
}

fn screen(x: f32, y: f32, width: f32, height: f32) -> ScreenRect {
    ScreenRect {
        x,
        y,
        width,
        height,
        scale: 1.0,
    }
}

/// A screen of `width` × `height` points at `scale` desktop units per point (a 2× monitor on
/// a desktop measured in pixels), its corner at `x`, `y` in desktop units.
fn scaled_screen(x: f32, y: f32, width: f32, height: f32, scale: f32) -> ScreenRect {
    ScreenRect {
        x,
        y,
        width: width * scale,
        height: height * scale,
        scale,
    }
}

fn window_tab_ids(s: &Session) -> Vec<Vec<u64>> {
    s.workspace
        .floating
        .iter()
        .map(|w| s.workspace.window_tabs(w.id).iter().map(|t| t.0).collect())
        .collect()
}

#[test]
fn tab_float_moves_a_tab_into_a_new_window_of_its_own() {
    let (mut s, _t) = session("float");
    let kjv = open(&mut s, "kjv");
    let heb = open(&mut s, "heb");
    let main_panel = s.workspace.panel_of(kjv).unwrap();
    // The main window's arrangement, as the UI keeps it, holds both Tabs side by side.
    let panel_node = |tab: TabId| LayoutNode::Panel {
        panel: s.workspace.panel_of(tab).unwrap().0,
        tabs: vec![tab.0],
        active: Some(tab.0),
    };
    s.workspace.arrangement = Some(LayoutNode::Row {
        children: vec![(panel_node(kjv), 0.5), (panel_node(heb), 0.5)],
    });
    s.workspace.take_effects();
    let out = s.run("tab.float", &json!({ "tab": heb.0 })).unwrap();
    let window = WindowId(out["window"].as_u64().unwrap());
    assert_eq!(s.workspace.window_of(heb), Some(window));
    assert_eq!(
        s.workspace.window_of(kjv),
        None,
        "KJV stays in the main window"
    );
    let panel = s.workspace.panel_of(heb).unwrap();
    assert_ne!(panel, main_panel);
    assert_eq!(
        s.workspace.take_effects(),
        vec![
            Effect::Float {
                tab: heb,
                window,
                panel,
                placement: None
            },
            Effect::Focus(heb),
        ]
    );
    // The main window's arrangement holds only KJV now; the window's holds only HEB.
    let main: Vec<u64> = s
        .workspace
        .arrangement
        .as_ref()
        .map(LayoutNode::panels)
        .unwrap_or_default()
        .into_iter()
        .flat_map(|(_, tabs, _)| tabs)
        .collect();
    assert_eq!(main, [kjv.0]);
    assert_eq!(window_tab_ids(&s), [vec![heb.0]]);
    // Floating it again leaves it where it is; Move to main window brings it back.
    s.run("tab.float", &json!({ "tab": heb.0 })).unwrap();
    assert_eq!(s.workspace.floating.len(), 1);
    assert!(s.run("tab.dock", &json!({ "tab": kjv.0 })).is_err());
    s.run("tab.dock", &json!({ "tab": heb.0 })).unwrap();
    assert!(s.workspace.floating.is_empty());
    assert_eq!(s.workspace.window_of(heb), None);
    assert!(matches!(
        s.workspace.take_effects()[0],
        Effect::Dock { tab, .. } if tab == heb
    ));
}

#[test]
fn closing_a_floating_window_docks_its_panels_in_the_main_window() {
    let (mut s, _t) = session("dock-window");
    let kjv = open(&mut s, "kjv");
    let heb = open(&mut s, "heb");
    let window = s.workspace.float(heb, None).unwrap();
    s.workspace.take_effects();
    assert!(s.workspace.dock_window(window));
    assert_eq!(s.workspace.take_effects(), vec![Effect::DockWindow(window)]);
    assert!(s.workspace.floating.is_empty());
    let main: Vec<u64> = s
        .workspace
        .arrangement
        .as_ref()
        .unwrap()
        .panels()
        .into_iter()
        .flat_map(|(_, tabs, _)| tabs)
        .collect();
    assert!(main.contains(&heb.0), "{main:?}");
    assert!(s.workspace.tab(kjv).is_some());
    assert!(!s.workspace.dock_window(window), "already closed");
}

#[test]
fn a_floating_bible_keeps_following_its_link_set() {
    let (mut s, _t) = session("link");
    let kjv = open(&mut s, "kjv");
    let heb = open(&mut s, "heb");
    let tab = s.workspace.tab_mut(heb).unwrap();
    let map = hebrew_map();
    tab.versification = Some(Arc::new(ListedVersification::new(
        map.entries().iter().map(|e| (e.verse, e.last)),
    )));
    tab.verse_map = Some(Arc::new(map));
    for t in [kjv, heb] {
        s.run("tab.link", &json!({ "tab": t.0, "set": "A" }))
            .unwrap();
    }
    s.run("tab.float", &json!({ "tab": heb.0 })).unwrap();
    s.workspace.take_effects();
    s.run("tab.go", &json!({ "tab": kjv.0, "text": "Gen 31:55" }))
        .unwrap();
    assert_eq!(
        s.workspace.tab(heb).unwrap().history.current(),
        Some(ge(32, 1)),
        "the floating Bible follows, in its own numbering"
    );
    assert!(s.workspace.take_effects().contains(&Effect::Follow {
        tab: heb,
        verse: ge(32, 1),
        flash: true
    }));
}

#[test]
fn a_layout_saves_and_restores_floating_windows_with_their_places() {
    let (mut s, _t) = session("layout");
    let kjv = open(&mut s, "kjv");
    let heb = open(&mut s, "heb");
    let second = screen(1920.0, 0.0, 1440.0, 900.0);
    let at = placement(2000.0, 80.0, Some(second));
    s.workspace.float(heb, Some(at)).unwrap();
    s.run("layout.save", &json!({ "name": "Two screens" }))
        .unwrap();
    let saved = s.workspace.snapshot();
    assert_eq!(saved.windows.len(), 1);
    assert_eq!(saved.windows[0].placement, Some(at));
    // Everything changes, then the Layout comes back with its window.
    s.workspace.dock(heb);
    s.workspace.close(kjv);
    assert!(s.workspace.floating.is_empty());
    s.workspace.take_effects();
    s.run("layout.restore", &json!({ "name": "Two screens" }))
        .unwrap();
    assert!(s.workspace.take_effects().contains(&Effect::Rebuild));
    let heb = s
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("heb".into()))
        .unwrap()
        .id;
    let window = s.workspace.window_of(heb).expect("HEB floats again");
    let w = s
        .workspace
        .floating
        .iter()
        .find(|w| w.id == window)
        .unwrap();
    assert_eq!(w.placement, Some(at));
    assert!(s.workspace.panel_of(heb).is_some(), "its Panel is known");
    let restored = s.workspace.snapshot();
    assert_eq!(restored.windows, saved.windows);
    assert_eq!(restored.tabs, saved.tabs);
    // A new window after the restore gets a fresh id.
    let kjv = s
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("kjv".into()))
        .unwrap()
        .id;
    let other = s.workspace.float(kjv, None).unwrap();
    assert_ne!(other, window);
}

#[test]
fn snapshots_without_floating_windows_are_unchanged() {
    let (mut s, _t) = session("compat");
    open(&mut s, "kjv");
    let json = s.workspace.snapshot().to_json();
    assert!(!json.contains("windows"), "{json}");
    // A snapshot saved before floating windows existed still loads.
    let back = WorkspaceSnapshot::from_json(&json).unwrap();
    assert!(back.windows.is_empty());
}

#[test]
fn a_floating_window_whose_resources_are_gone_is_dropped_on_restore() {
    let (mut s, _t) = session("missing");
    open(&mut s, "kjv");
    let heb = open(&mut s, "heb");
    s.workspace.float(heb, None).unwrap();
    let snap = s.workspace.snapshot().without_missing(|id| id != "heb");
    assert!(snap.windows.is_empty());
    assert_eq!(snap.tabs.len(), 1);
}

#[test]
fn a_window_whose_screen_is_gone_is_moved_on_screen() {
    let main = screen(0.0, 0.0, 1920.0, 1080.0);
    let second = screen(1920.0, 0.0, 1440.0, 900.0);
    let at = placement(2000.0, 80.0, Some(second));
    // Both screens there: it stays put.
    assert_eq!(at.on_screen(&[main, second]), at);
    // No screens known (a backend that cannot list them): it stays as saved.
    assert_eq!(at.on_screen(&[]), at);
    // The second screen is gone: it moves to the main window's screen, at the same offset
    // from the corner.
    let moved = at.on_screen(&[main]);
    assert_eq!((moved.x, moved.y), (80.0, 80.0));
    assert_eq!((moved.width, moved.height), (800.0, 600.0));
    assert_eq!(moved.screen, Some(main));
    // A smaller screen: it shrinks to fit and stays inside.
    let small = screen(0.0, 0.0, 640.0, 480.0);
    let far = placement(3000.0, 700.0, Some(second));
    let moved = far.on_screen(&[small]);
    assert_eq!((moved.width, moved.height), (640.0, 480.0));
    assert_eq!((moved.x, moved.y), (0.0, 0.0));
    // A window only a sliver of whose title bar shows is not grabbable: moved too.
    let sliver = placement(1900.0, 100.0, None);
    let moved = sliver.on_screen(&[main]);
    assert!(moved.x + moved.width <= 1920.0, "{moved:?}");
    assert_eq!(
        (moved.x, moved.y),
        ((1920.0 - 800.0) / 2.0, (1080.0 - 600.0) / 2.0)
    );
}

#[test]
fn a_window_on_a_screen_of_another_scale_keeps_its_place_in_that_screens_points() {
    // A 1× main screen and, right of it, a 2× screen, on a desktop measured in pixels.
    let main = scaled_screen(0.0, 0.0, 1920.0, 1080.0, 1.0);
    let second = scaled_screen(1920.0, 0.0, 1440.0, 900.0, 2.0);
    // 240 × 50 points from the 2× screen's corner: 480 × 100 pixels.
    let at = placement(2400.0, 100.0, Some(second));
    assert_eq!(at.units_per_point(), 2.0);
    assert_eq!(at.on_screen(&[main, second]), at, "both there: it stays");
    // The 2× screen is gone: 240 × 50 points from the main screen's corner, same size.
    let moved = at.on_screen(&[main]);
    assert_eq!((moved.x, moved.y), (240.0, 50.0));
    assert_eq!((moved.width, moved.height), (800.0, 600.0));
    assert_eq!(moved.screen, Some(main));
    // And the other way: from the 1× screen onto a 2× one, its offset kept in points as far
    // as it fits in the 2× screen's 1440 × 900 points.
    let from_main = placement(100.0, 700.0, Some(main));
    let moved = from_main.on_screen(&[second]);
    assert_eq!((moved.width, moved.height), (800.0, 600.0));
    assert_eq!((moved.x, moved.y), (1920.0 + 100.0 * 2.0, 300.0 * 2.0));
    // A snapshot from before screens had a scale reads as 1×.
    let old: ScreenRect =
        serde_json::from_str(r#"{"x":0.0,"y":0.0,"width":10.0,"height":10.0}"#).unwrap();
    assert_eq!(old.units_per_point(), 1.0);
}
