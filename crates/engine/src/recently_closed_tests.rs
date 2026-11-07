//! Recently closed and Reopen closed Tab (#214), through the commands a menu item or key runs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_model::{Manifest, ResourceType};
use obs_scripture::{Book, Verse};
use serde_json::json;

use crate::*;

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// KJV, WEB and ASV, with a User database for the Last session and Layouts.
fn session(name: &str) -> (Session, Temp) {
    let root = std::env::temp_dir().join(format!("obs-reopen-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut s = Session::new(JobMode::Inline);
    for (id, ab) in [("kjv", "KJV"), ("web", "WEB"), ("asv", "ASV")] {
        s.library.add(LibraryEntry {
            manifest: Manifest::new(id, ab, ab, ResourceType::Bible, "en"),
            path: format!("/library/{id}.obr").into(),
        });
    }
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    (s, Temp(root))
}

fn tab(v: &serde_json::Value) -> TabId {
    TabId(v["tab"].as_u64().unwrap())
}

fn open(s: &mut Session, id: &str, params: serde_json::Value) -> TabId {
    let mut p = params;
    p["id"] = json!(id);
    tab(&s.run("resource.open", &p).unwrap())
}

fn v(book: Book, c: u16, n: u16) -> Verse {
    Verse::new(book, c, n)
}

/// A Bible Tab at `at` in Link set `link`.
fn place(s: &mut Session, t: TabId, link: Option<char>, at: Verse) {
    s.workspace.set_link(t, link);
    s.workspace.navigate(t, at);
}

/// What a Tab is, where, and in which Link set and Panel.
fn state(s: &Session, t: TabId) -> (TabKind, Option<Verse>, Option<char>, PanelId) {
    let tab = s.workspace.tab(t).unwrap();
    (
        tab.kind.clone(),
        tab.history.current(),
        tab.link,
        s.workspace.panel_of(t).unwrap(),
    )
}

fn reopen(s: &mut Session) -> Vec<TabId> {
    let out = s.run("tab.reopen_closed", &json!({})).unwrap();
    (out["tabs"].as_array().unwrap().iter())
        .map(|t| TabId(t.as_u64().unwrap()))
        .collect()
}

/// The acceptance criterion: close three Tabs, then Reopen closed Tab three times; they come
/// back in reverse order, each at its old location and in its old Link set, and in its old
/// Panel (the two closed one by one from a Panel that closed with them return to it together).
#[test]
fn closing_three_tabs_then_reopening_three_times_brings_them_back_newest_first_where_they_were() {
    let (mut s, _t) = session("three");
    let kjv = open(&mut s, "kjv", json!({}));
    let first = s.workspace.panel_of(kjv).unwrap();
    let web = open(&mut s, "web", json!({ "panel": first.0 }));
    let asv = open(&mut s, "asv", json!({ "split": true }));
    let second = s.workspace.panel_of(asv).unwrap();
    assert_ne!(first, second);
    place(&mut s, kjv, Some('A'), v(Book::John, 3, 16));
    place(&mut s, web, Some('B'), v(Book::Romans, 8, 28));
    place(&mut s, asv, None, v(Book::Genesis, 1, 1));
    // The two Panels side by side, as the UI reports them.
    let leaf = |p: PanelId, tabs: &[TabId]| LayoutNode::Panel {
        panel: p.0,
        tabs: tabs.iter().map(|t| t.0).collect(),
        active: Some(tabs[0].0),
    };
    s.workspace.arrangement = Some(LayoutNode::Row {
        children: vec![(leaf(first, &[kjv, web]), 1.0), (leaf(second, &[asv]), 1.0)],
    });
    let before: Vec<_> = [web, kjv, asv].iter().map(|t| state(&s, *t)).collect();
    // Ctrl+W on each, WEB first: the first Panel closes with KJV, the second with ASV.
    for t in [web, kjv, asv] {
        s.run("tab.close", &json!({ "tab": t.0 })).unwrap();
    }
    assert_eq!(s.workspace.tabs().count(), 0);
    assert!(s.is_enabled("tab.reopen_closed"));
    assert_eq!(
        s.commands().get("tab.reopen_closed").unwrap().shortcut,
        Some("Ctrl+Shift+T")
    );
    s.workspace.take_effects();

    let mut back = Vec::new();
    for expected in [&before[2], &before[1], &before[0]] {
        let reopened = reopen(&mut s);
        assert_eq!(reopened.len(), 1);
        let t = reopened[0];
        let (kind, at, link, panel) = state(&s, t);
        assert_eq!((&kind, at, link), (&expected.0, expected.1, expected.2));
        assert_eq!(panel, expected.3, "{kind:?} returns to its old Panel");
        assert_eq!(s.workspace.focused(), Some(t));
        back.push(t);
    }
    assert!(!s.is_enabled("tab.reopen_closed"), "nothing left to reopen");
    assert!(s.run("tab.reopen_closed", &json!({})).is_err());
    // WEB went back into the Panel KJV reopened: Panel order as it was before the closes.
    let first_tabs: Vec<TabId> = (s.workspace.panels())
        .find(|(p, _)| *p == first)
        .map(|(_, t)| t.to_vec())
        .unwrap();
    assert_eq!(first_tabs, [back[1], back[2]]);
    // The UI is told to place each one and focus it; a reopened Bible joins its old Link set
    // without moving the set.
    let effects = s.workspace.take_effects();
    let opened: Vec<(TabId, Placement)> = (effects.iter())
        .filter_map(|e| match e {
            Effect::Opened { tab, placement } => Some((*tab, *placement)),
            _ => None,
        })
        .collect();
    assert_eq!(
        opened,
        [
            (
                back[0],
                Placement::NewPanel {
                    panel: second,
                    beside: None,
                    before: false
                }
            ),
            (
                back[1],
                Placement::NewPanel {
                    panel: first,
                    beside: Some(second),
                    before: true
                }
            ),
            (back[2], Placement::Panel(first)),
        ]
    );
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::Follow { .. })),
        "{effects:?}"
    );
}

#[test]
fn closing_a_panel_is_undone_by_one_reopen_with_all_its_tabs() {
    let (mut s, _t) = session("panel");
    let kjv = open(&mut s, "kjv", json!({}));
    let panel = s.workspace.panel_of(kjv).unwrap();
    let web = open(&mut s, "web", json!({ "panel": panel.0 }));
    let asv = open(&mut s, "asv", json!({ "split": true }));
    place(&mut s, kjv, Some('A'), v(Book::John, 1, 1));
    place(&mut s, web, Some('A'), v(Book::John, 1, 1));
    s.workspace.focus(kjv);
    // Close Panel from the Window menu closes the focused Tab's Panel.
    s.run("panel.close", &json!({})).unwrap();
    assert!(s.workspace.tab(kjv).is_none() && s.workspace.tab(web).is_none());
    assert!(s.workspace.tab(asv).is_some());
    assert_eq!(s.recently_closed_labels().len(), 1);
    assert!(s.workspace.recently_closed().next().unwrap().is_panel());
    let back = reopen(&mut s);
    assert_eq!(back.len(), 2);
    let kinds: Vec<TabKind> = back.iter().map(|t| state(&s, *t).0).collect();
    assert_eq!(
        kinds,
        [
            TabKind::Resource("kjv".into()),
            TabKind::Resource("web".into())
        ]
    );
    for t in &back {
        let (_, at, link, p) = state(&s, *t);
        assert_eq!((at, link, p), (Some(v(Book::John, 1, 1)), Some('A'), panel));
    }
    // KJV was the Panel's active Tab: it is again, and focused.
    assert_eq!(s.workspace.active_in(panel), Some(back[0]));
    assert_eq!(s.workspace.focused(), Some(back[0]));
}

/// Tabs the UI stops showing (× or a middle click on the Tab) were closed by the user.
#[test]
fn tabs_closed_in_the_dock_go_into_recently_closed() {
    let (mut s, _t) = session("dock");
    let kjv = open(&mut s, "kjv", json!({}));
    let asv = open(&mut s, "asv", json!({ "split": true }));
    place(&mut s, asv, Some('B'), v(Book::Psalms, 23, 1));
    let kept = s.workspace.panel_of(kjv).unwrap();
    s.workspace.sync_panels(vec![(kept, vec![kjv])]);
    assert!(s.workspace.tab(asv).is_none());
    assert_eq!(s.recently_closed_labels()[0].1, "Psalms 23:1 · ASV");
    let back = reopen(&mut s);
    let (kind, at, link, _) = state(&s, back[0]);
    assert_eq!(
        (kind, at, link),
        (
            TabKind::Resource("asv".into()),
            Some(v(Book::Psalms, 23, 1)),
            Some('B')
        )
    );
    // Closing everything for a layout reset, or a Resource's Tabs as it is removed, is not
    // the user closing them.
    s.workspace.clear();
    assert_eq!(s.workspace.recently_closed().count(), 0);
}

#[test]
fn recently_closed_lists_the_last_twenty_newest_first_and_reopens_any_of_them() {
    let (mut s, _t) = session("twenty");
    for n in 1..=25u16 {
        let t = open(&mut s, "kjv", json!({}));
        place(&mut s, t, None, v(Book::Psalms, n, 1));
        s.run("tab.close", &json!({ "tab": t.0 })).unwrap();
    }
    let labels = s.recently_closed_labels();
    assert_eq!(labels.len(), RECENTLY_CLOSED_LIMIT);
    assert_eq!(labels[0].1, "Psalms 25:1 · KJV");
    assert_eq!(labels[19].1, "Psalms 6:1 · KJV");
    // Window → Recently closed → "Psalms 10:1 · KJV".
    let (id, _) = labels.iter().find(|(_, l)| l == "Psalms 10:1 · KJV").unwrap();
    let out = s.run("tab.reopen_closed", &json!({ "closed": id })).unwrap();
    let t = TabId(out["tabs"][0].as_u64().unwrap());
    assert_eq!(state(&s, t).1, Some(v(Book::Psalms, 10, 1)));
    assert_eq!(s.recently_closed_labels().len(), 19);
    assert!(
        s.run("tab.reopen_closed", &json!({ "closed": id }))
            .is_err()
    );
    // The Window menu holds Reopen closed Tab, Recently closed and Close Panel.
    let window = MENUS.iter().find(|m| m.title == "Window").unwrap();
    assert_eq!(
        window.items,
        [
            MenuItem::Command("tab.reopen_closed"),
            MenuItem::RecentlyClosed,
            MenuItem::Separator,
            MenuItem::Command("panel.close"),
        ]
    );
}

#[test]
fn a_single_instance_tab_open_again_is_focused_instead_of_reopened() {
    let (mut s, _t) = session("single");
    let library = tab(&s.run("library.open", &json!({})).unwrap());
    s.run("tab.close", &json!({ "tab": library.0 })).unwrap();
    let again = tab(&s.run("library.open", &json!({})).unwrap());
    let _kjv = open(&mut s, "kjv", json!({ "split": true }));
    assert!(reopen(&mut s).is_empty());
    assert_eq!(s.workspace.focused(), Some(again));
    assert_eq!(
        s.workspace
            .tabs()
            .filter(|t| t.kind == TabKind::Library)
            .count(),
        1
    );
}

/// Recently closed is kept with the Last session, never with a Layout; restoring a Layout
/// keeps it, its Tabs then opening where Tabs of their kind open.
#[test]
fn recently_closed_survives_a_restart_but_is_never_saved_in_a_layout() {
    let (mut s, t) = session("restart");
    let kjv = open(&mut s, "kjv", json!({}));
    let web = open(&mut s, "web", json!({ "split": true }));
    place(&mut s, web, Some('C'), v(Book::Romans, 12, 1));
    s.run("tab.close", &json!({ "tab": web.0 })).unwrap();
    s.run("layout.save", &json!({ "name": "Study" })).unwrap();
    let saved = s.user.as_ref().unwrap().layout("Study").unwrap().unwrap();
    assert!(!saved.contains("recently_closed"), "{saved}");
    s.save_last_session().unwrap();
    drop(s);

    let mut s = Session::new(JobMode::Inline);
    for (id, ab) in [("kjv", "KJV"), ("web", "WEB"), ("asv", "ASV")] {
        s.library.add(LibraryEntry {
            manifest: Manifest::new(id, ab, ab, ResourceType::Bible, "en"),
            path: format!("/library/{id}.obr").into(),
        });
    }
    let (store, _) = obs_store::UserStore::open(&t.0.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    assert!(s.restore_last_session());
    assert_eq!(s.recently_closed_labels()[0].1, "Romans 12:1 · WEB");
    let kjv_panel = s.workspace.panel_of(kjv).unwrap();
    let back = reopen(&mut s);
    let (kind, at, link, panel) = state(&s, back[0]);
    assert_eq!(kind, TabKind::Resource("web".into()));
    assert_eq!((at, link), (Some(v(Book::Romans, 12, 1)), Some('C')));
    assert_ne!(panel, kjv_panel, "WEB's own Panel comes back");
    assert!(back[0] != kjv, "a reopened Tab gets a fresh id");

    // Closed again, then a Layout replaces the Workspace: the list stays.
    s.run("tab.close", &json!({ "tab": back[0].0 })).unwrap();
    s.run("layout.restore", &json!({ "name": "Study" }))
        .unwrap();
    assert_eq!(s.recently_closed_labels().len(), 1);
    let back = reopen(&mut s);
    assert_eq!(state(&s, back[0]).0, TabKind::Resource("web".into()));
}

#[test]
fn a_removed_resource_leaves_recently_closed() {
    let (mut s, _t) = session("removed");
    let kjv = open(&mut s, "kjv", json!({}));
    let asv = open(&mut s, "asv", json!({ "split": true }));
    for t in [kjv, asv] {
        s.run("tab.close", &json!({ "tab": t.0 })).unwrap();
    }
    s.run("library.remove", &json!({ "id": "asv", "confirm": true }))
        .unwrap();
    let labels: Vec<String> = (s.recently_closed_labels().into_iter())
        .map(|(_, l)| l)
        .collect();
    assert_eq!(labels, ["KJV"]);
}
