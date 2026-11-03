#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use obs_model::{Manifest, ResourceType};
use obs_scripture::{Book, ListedVersification, Verse};
use serde_json::json;

use crate::*;

fn bible(id: &str, abbrev: &str) -> LibraryEntry {
    LibraryEntry {
        manifest: Manifest::new(id, abbrev, abbrev, ResourceType::Bible, "en"),
        path: format!("/library/{id}.obr").into(),
    }
}

fn session() -> Session {
    let mut s = Session::new(JobMode::Inline);
    s.library.add(bible("kjv", "KJV"));
    s.library.add(bible("web", "WEB"));
    s
}

fn tab(v: &serde_json::Value) -> TabId {
    TabId(v["tab"].as_u64().unwrap())
}

fn panel_of(s: &Session, t: TabId) -> PanelId {
    s.workspace.panel_of(t).unwrap()
}

#[test]
fn opening_word_info_twice_focuses_the_existing_tab() {
    let mut s = session();
    let a = tab(&s.run("tool.word_info", &json!({})).unwrap());
    let kjv = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    assert_eq!(s.workspace.focused(), Some(kjv));
    s.workspace.take_effects();
    let b = tab(&s.run("tool.word_info", &json!({ "split": true })).unwrap());
    assert_eq!(a, b);
    assert_eq!(s.workspace.focused(), Some(a));
    assert_eq!(s.workspace.take_effects(), vec![Effect::Focus(a)]);
    let count = s
        .workspace
        .tabs()
        .filter(|t| t.kind == TabKind::Tool(Tool::WordInfo))
        .count();
    assert_eq!(count, 1);
    // Library and Notes are single-instance too; Bibles and other Tools are not.
    let l1 = tab(&s.run("library.open", &json!({})).unwrap());
    let l2 = tab(&s.run("library.open", &json!({})).unwrap());
    assert_eq!(l1, l2);
    let n1 = tab(&s.run("tool.notes", &json!({})).unwrap());
    assert_eq!(n1, tab(&s.run("tool.notes", &json!({})).unwrap()));
    let k2 = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    assert_ne!(kjv, k2);
    let x1 = tab(&s.run("tool.cross_references", &json!({})).unwrap());
    assert_ne!(
        x1,
        tab(&s.run("tool.cross_references", &json!({})).unwrap())
    );
}

#[test]
fn placement_follows_rule_2() {
    let mut s = session();
    // Nothing open: the first Tab gets a new Panel.
    let kjv = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    let reading = panel_of(&s, kjv);
    // A Tool with no non-reading Panel yet splits off to the right.
    let wi = tab(&s.run("tool.word_info", &json!({})).unwrap());
    let tools = panel_of(&s, wi);
    assert_ne!(tools, reading);
    let effects = s.workspace.take_effects();
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Opened { tab, placement: Placement::NewPanel { beside: Some(b), .. } }
            if *tab == wi && *b == reading
    )));
    // Another Resource goes to the last-active reading Panel, even with a Tool focused.
    let web = tab(&s.run("resource.open", &json!({ "id": "web" })).unwrap());
    assert_eq!(panel_of(&s, web), reading);
    // Another Tool goes to the last-active non-reading Panel.
    let xr = tab(&s.run("tool.cross_references", &json!({})).unwrap());
    assert_eq!(panel_of(&s, xr), tools);
    // The + of a Panel puts the Tab there.
    let xr2 = tab(&s
        .run("tool.cross_references", &json!({ "panel": reading.0 }))
        .unwrap());
    assert_eq!(panel_of(&s, xr2), reading);
    // Each kind remembers its Panel: Cross-references now goes to the reading Panel.
    s.workspace.user_focused(web);
    let xr3 = tab(&s.run("tool.cross_references", &json!({})).unwrap());
    assert_eq!(panel_of(&s, xr3), reading);
    // Ctrl/Cmd forces a new split.
    let k2 = tab(&s
        .run("resource.open", &json!({ "id": "kjv", "split": true }))
        .unwrap());
    let p = panel_of(&s, k2);
    assert!(p != reading && p != tools);
    // When the remembered Panel is gone, a Tool goes to the last-active non-reading Panel:
    // the one where Cross-references was last active.
    for t in [wi, xr] {
        s.run("tab.close", &json!({ "tab": t.0 })).unwrap();
    }
    let wi2 = tab(&s.run("tool.word_info", &json!({})).unwrap());
    assert_eq!(panel_of(&s, wi2), reading);
}

#[test]
fn each_tab_navigates_and_keeps_its_own_history() {
    let mut s = session();
    let a = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    let b = tab(&s.run("resource.open", &json!({ "id": "web" })).unwrap());
    let go = |s: &mut Session, t: TabId, r: &str| {
        s.run("tab.go", &json!({ "tab": t.0, "text": r })).unwrap()["verse"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(go(&mut s, a, "Jn 3:16"), "John.3.16");
    assert_eq!(go(&mut s, a, "Rom 8"), "Rom.8.1");
    // Relative forms read against the Tab's own location.
    assert_eq!(go(&mut s, a, "28"), "Rom.8.28");
    assert_eq!(go(&mut s, b, "Gen 1:1"), "Gen.1.1");
    assert_eq!(go(&mut s, b, "2:4"), "Gen.2.4");
    // Scrolling moves the location without a history entry.
    s.workspace.scrolled(b, Verse::new(Book::Genesis, 3, 1));
    let back = |s: &mut Session, t: TabId| {
        s.run("tab.back", &json!({ "tab": t.0 })).unwrap()["verse"].clone()
    };
    assert_eq!(back(&mut s, a), "Rom.8.1");
    assert_eq!(back(&mut s, a), "John.3.16");
    assert_eq!(back(&mut s, a), serde_json::Value::Null);
    assert_eq!(back(&mut s, b), "Gen.1.1");
    let fwd = s.run("tab.forward", &json!({ "tab": b.0 })).unwrap();
    assert_eq!(fwd["verse"], "Gen.3.1");
    assert_eq!(
        s.workspace.tab(a).unwrap().history.current(),
        Some(Verse::new(Book::John, 3, 16))
    );
    let navs: Vec<Effect> = s
        .workspace
        .take_effects()
        .into_iter()
        .filter(|e| matches!(e, Effect::Navigate { .. }))
        .collect();
    // 5 jumps, 3 Backs that moved, 1 Forward.
    assert_eq!(navs.len(), 9);
}

#[test]
fn references_are_read_in_the_tabs_own_numbering() {
    let mut s = session();
    let t = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    // A Bible with only Genesis 1-2 (Gen 1 has 31 verses).
    let v = ListedVersification::new(
        (1..=31)
            .map(|n| (Verse::new(Book::Genesis, 1, n), n))
            .chain((1..=25).map(|n| (Verse::new(Book::Genesis, 2, n), n))),
    );
    s.workspace.tab_mut(t).unwrap().versification = Some(Arc::new(v));
    let ok = s
        .run("tab.go", &json!({ "tab": t.0, "text": "Gen 2" }))
        .unwrap();
    assert_eq!(ok["text"], "Genesis 2");
    let err = s
        .run("tab.go", &json!({ "tab": t.0, "text": "Exod 3:1" }))
        .unwrap_err();
    assert!(err.to_string().contains("not in this Bible"), "{err}");
    let err = s
        .run("tab.go", &json!({ "tab": t.0, "text": "Gen 1:40" }))
        .unwrap_err();
    assert!(err.to_string().contains("Gen"), "{err}");
}

#[test]
fn every_menu_item_is_a_registered_command() {
    let s = session();
    let mut n = 0;
    for m in MENUS {
        for item in m.items {
            if let MenuItem::Command(id) = item {
                let spec = s.commands().get(id).unwrap_or_else(|| panic!("{id}"));
                assert!(spec.params.iter().all(|p| !p.required), "{id} needs params");
                n += 1;
            }
        }
    }
    assert!(n >= 17);
    let titles: Vec<&str> = MENUS.iter().map(|m| m.title).collect();
    assert_eq!(
        titles,
        ["Library", "Guides", "Tools", "Layouts", "View", "Help"]
    );
    // Not-yet-built items show but are disabled.
    assert!(!s.is_enabled("library.import"));
    assert!(s.is_enabled("tool.word_info"));
}

#[test]
fn view_and_help_commands_change_session_state() {
    let mut s = session();
    let size = s.text.size;
    s.run("view.text_larger", &json!({})).unwrap();
    assert_eq!(s.text.size, size + 1.0);
    s.run("view.dark_page", &json!({})).unwrap();
    assert!(s.dark_page);
    s.run("help.credits", &json!({})).unwrap();
    let e = s.workspace.take_effects();
    assert!(matches!(&e[..], [Effect::Show { body, .. }] if body.contains("TVTMS")));
    assert_eq!(
        s.command_log(),
        ["view.text_larger", "view.dark_page", "help.credits"]
    );
    s.run("layout.reset", &json!({})).unwrap();
    assert_eq!(s.workspace.take_effects(), vec![Effect::ResetLayout]);
}

#[test]
fn sync_panels_follows_drags_and_closes() {
    let mut s = session();
    let a = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    let b = tab(&s.run("resource.open", &json!({ "id": "web" })).unwrap());
    let p = panel_of(&s, a);
    // The user drags WEB out into a new Panel and closes nothing.
    let q = s.workspace.new_panel_id();
    s.workspace.sync_panels(vec![(p, vec![a]), (q, vec![b])]);
    assert_eq!(panel_of(&s, b), q);
    // Then closes KJV with its ×.
    s.workspace.sync_panels(vec![(q, vec![b])]);
    assert!(s.workspace.tab(a).is_none());
    assert_eq!(s.workspace.panels().count(), 1);
}

#[test]
fn library_keeps_preferred_order_within_types() {
    let mut lib = Library::new();
    lib.add(bible("kjv", "KJV"));
    let mut jfb = bible("jfb", "JFB");
    jfb.manifest.resource_type = ResourceType::Commentary;
    lib.add(jfb);
    lib.add(bible("web", "WEB"));
    let ids: Vec<&str> = lib
        .entries()
        .iter()
        .map(|e| e.manifest.resource_id.as_str())
        .collect();
    assert_eq!(ids, ["kjv", "web", "jfb"]);
    lib.add(bible("kjv", "KJV2"));
    assert_eq!(lib.entries().len(), 3);
    assert_eq!(lib.of_type(ResourceType::Bible).count(), 2);
}
