//! Keyboard navigation across Panels and Tabs (#215), through the commands the keys run:
//! Ctrl+Tab's Tab switcher order, Ctrl+1…9 to the Panels in screen order, Ctrl+Page Up/Down
//! by chapter, section or entry, F6's areas, and the keys themselves as Help → Keyboard
//! shortcuts lists them, none in conflict.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;

use obs_model::{Key, KeyEntry, KeyScheme, Manifest, ResourceType, UnitId};
use obs_scripture::{Book, ListedVersification, Passage, Standard, Verse};
use serde_json::json;

use crate::resource_navigation::ResourceNavigation;
use crate::*;

fn session() -> Session {
    let mut s = Session::new(JobMode::Inline);
    for (id, ab) in [("kjv", "KJV"), ("web", "WEB"), ("asv", "ASV")] {
        s.library.add(LibraryEntry {
            manifest: Manifest::new(id, ab, ab, ResourceType::Bible, "en"),
            path: format!("/library/{id}.obr").into(),
        });
    }
    s
}

fn tab(v: &serde_json::Value) -> TabId {
    TabId(v["tab"].as_u64().unwrap())
}

fn open(s: &mut Session, id: &str, split: bool) -> TabId {
    tab(&s
        .run("resource.open", &json!({ "id": id, "split": split }))
        .unwrap())
}

fn focused_by_key(s: &mut Session, id: &str, params: serde_json::Value) -> TabId {
    s.workspace.take_effects();
    let out = s.run(id, &params).unwrap();
    let to = tab(&out);
    assert_eq!(s.workspace.focused(), Some(to), "{id}");
    assert!(
        (s.workspace.take_effects()).contains(&Effect::KeyboardToTab(to)),
        "{id} puts the keyboard on the Tab"
    );
    to
}

#[test]
fn ctrl_tab_goes_back_to_the_tab_used_before_and_walks_the_panel_most_recent_first() {
    let mut s = session();
    let a = open(&mut s, "kjv", false);
    let b = open(&mut s, "web", false);
    let c = open(&mut s, "asv", false);
    let panel = s.workspace.panel_of(a).unwrap();
    assert_eq!(s.workspace.panel_of(c), Some(panel), "one Panel");
    // Used A, then C: the switcher lists C, A, then B (least recently used).
    s.workspace.user_focused(a);
    s.workspace.user_focused(c);
    assert_eq!(s.workspace.tab_switcher_order(c), vec![c, a, b]);

    // Ctrl+Tab once: the Tab used before.
    let to = focused_by_key(&mut s, "tab.next_in_panel", json!({ "tab": c.0 }));
    assert_eq!(to, a);
    // Again: back to C (now the one used before), not on to B.
    let to = focused_by_key(&mut s, "tab.next_in_panel", json!({ "tab": a.0 }));
    assert_eq!(to, c);
    // Held while Tab is pressed twice: the UI runs it with steps 2.
    let to = focused_by_key(
        &mut s,
        "tab.next_in_panel",
        json!({ "tab": c.0, "steps": 2 }),
    );
    assert_eq!(to, b);
    // Ctrl+Shift+Tab goes up the list, wrapping: the least recently used first.
    assert_eq!(s.workspace.tab_switcher_order(b), vec![b, c, a]);
    let to = focused_by_key(&mut s, "tab.previous_in_panel", json!({ "tab": b.0 }));
    assert_eq!(to, a);
    // Past the end wraps round: 4 steps along 3 Tabs is 1.
    let order = s.workspace.tab_switcher_order(a);
    let to = focused_by_key(
        &mut s,
        "tab.next_in_panel",
        json!({ "tab": a.0, "steps": 4 }),
    );
    assert_eq!(to, order[1]);
}

#[test]
fn ctrl_tab_stays_in_the_focused_panel() {
    let mut s = session();
    let a = open(&mut s, "kjv", false);
    let b = open(&mut s, "web", true);
    let c = open(&mut s, "asv", false);
    assert_ne!(s.workspace.panel_of(a), s.workspace.panel_of(b));
    assert_eq!(s.workspace.panel_of(b), s.workspace.panel_of(c));
    assert_eq!(s.workspace.tab_switcher_order(c), vec![c, b]);
    // A Panel with one Tab: Ctrl+Tab stays on it.
    let to = focused_by_key(&mut s, "tab.next_in_panel", json!({ "tab": a.0 }));
    assert_eq!(to, a);
}

#[test]
fn ctrl_1_to_9_go_to_the_panels_in_screen_order() {
    let mut s = session();
    let a = open(&mut s, "kjv", false);
    let b = open(&mut s, "web", true);
    let c = open(&mut s, "asv", true);
    let pa = s.workspace.panel_of(a).unwrap();
    let pb = s.workspace.panel_of(b).unwrap();
    let pc = s.workspace.panel_of(c).unwrap();
    // Without an arrangement from the UI, by id.
    assert_eq!(s.workspace.panels_in_order(), vec![pa, pb, pc]);
    // On screen: C on the left, then a column of B above A.
    let panel = |p: PanelId, t: TabId| LayoutNode::Panel {
        panel: p.0,
        tabs: vec![t.0],
        active: Some(t.0),
    };
    s.workspace.arrangement = Some(LayoutNode::Row {
        children: vec![
            (panel(pc, c), 0.5),
            (
                LayoutNode::Column {
                    children: vec![(panel(pb, b), 0.5), (panel(pa, a), 0.5)],
                },
                0.5,
            ),
        ],
    });
    assert_eq!(s.workspace.panels_in_order(), vec![pc, pb, pa]);
    assert_eq!(
        focused_by_key(&mut s, "panel.focus_1", json!({})),
        c,
        "Ctrl+1"
    );
    assert_eq!(focused_by_key(&mut s, "panel.focus_2", json!({})), b);
    assert_eq!(focused_by_key(&mut s, "panel.focus_3", json!({})), a);
    // A Panel that is not there: the key is left alone.
    assert!(!s.is_enabled("panel.focus_4"));
    assert!(s.run("panel.focus_4", &json!({})).is_err());
    assert_eq!(s.workspace.focused(), Some(a));
    // A Floating window's Panels come after the main window's.
    s.run("tab.float", &json!({ "tab": c.0 })).unwrap();
    let floated = s.workspace.panel_of(c).unwrap();
    s.workspace.arrangement = Some(LayoutNode::Column {
        children: vec![(panel(pb, b), 0.5), (panel(pa, a), 0.5)],
    });
    s.workspace.floating = vec![FloatingWindow {
        id: s.workspace.window_of(c).unwrap(),
        arrangement: Some(panel(floated, c)),
        placement: None,
    }];
    assert_eq!(s.workspace.panels_in_order(), vec![pb, pa, floated]);
    assert_eq!(focused_by_key(&mut s, "panel.focus_3", json!({})), c);
}

fn v(book: Book, chapter: u16, verse: u16) -> Verse {
    Verse::new(book, chapter, verse)
}

fn current(s: &Session, t: TabId) -> Verse {
    s.workspace.tab(t).unwrap().history.current().unwrap()
}

#[test]
fn ctrl_page_down_and_up_move_a_bible_by_chapter_in_its_own_numbering() {
    let mut s = session();
    let kjv = open(&mut s, "kjv", false);
    let web = open(&mut s, "web", true);
    s.workspace.set_link(kjv, Some('A'));
    s.workspace.set_link(web, Some('A'));
    s.workspace.navigate(kjv, v(Book::Genesis, 1, 5));
    let page = |s: &mut Session, id: &str, t: TabId| {
        s.run(id, &json!({ "tab": t.0 })).unwrap();
        current(s, t)
    };
    assert_eq!(
        page(&mut s, "tab.next_chapter", kjv),
        v(Book::Genesis, 2, 1)
    );
    // A jump: the Link set follows, and Back returns.
    assert_eq!(current(&s, web), v(Book::Genesis, 2, 1));
    s.run("tab.back", &json!({ "tab": kjv.0 })).unwrap();
    assert_eq!(current(&s, kjv), v(Book::Genesis, 1, 5));
    // Into the next Bible book, and back to the start of the previous chapter.
    s.workspace.navigate(kjv, v(Book::Genesis, 50, 3));
    assert_eq!(page(&mut s, "tab.next_chapter", kjv), v(Book::Exodus, 1, 1));
    assert_eq!(
        page(&mut s, "tab.previous_chapter", kjv),
        v(Book::Genesis, 50, 1)
    );
    // Mid-chapter, Ctrl+Page Up goes to the chapter before, not this one's start.
    s.workspace.navigate(kjv, v(Book::Genesis, 3, 9));
    assert_eq!(
        page(&mut s, "tab.previous_chapter", kjv),
        v(Book::Genesis, 2, 1)
    );
    // Nothing before Genesis 1.
    s.workspace.navigate(kjv, v(Book::Genesis, 1, 2));
    assert!(
        s.run("tab.previous_chapter", &json!({ "tab": kjv.0 }))
            .is_err()
    );
    assert_eq!(current(&s, kjv), v(Book::Genesis, 1, 2));

    // A Bible numbered with three chapters of Malachi goes from Malachi 3 to Matthew 1.
    let own = ListedVersification::new(
        Standard
            .verses()
            .filter(|x| !(x.book == Book::Malachi && x.chapter == 4))
            .map(|x| (x, x.verse)),
    );
    s.workspace.tab_mut(web).unwrap().versification = Some(Arc::new(own));
    s.workspace.navigate(web, v(Book::Malachi, 3, 18));
    assert_eq!(
        page(&mut s, "tab.next_chapter", web),
        v(Book::Matthew, 1, 1)
    );
    assert_eq!(
        page(&mut s, "tab.previous_chapter", web),
        v(Book::Malachi, 3, 1)
    );
}

fn entry(key: Key, unit: &str) -> KeyEntry {
    KeyEntry {
        key,
        unit: UnitId::new(unit).unwrap(),
        range: None,
        alias: false,
    }
}

fn open_with(
    s: &mut Session,
    id: &str,
    ty: ResourceType,
    scheme: KeyScheme,
    nav: ResourceNavigation,
) -> TabId {
    let mut manifest = Manifest::new(id, id, id, ty, "en");
    manifest.key_schemes = vec![scheme];
    s.library.add(LibraryEntry {
        manifest,
        path: format!("/{id}.obr").into(),
    });
    let t = open(s, id, false);
    s.workspace.tab_mut(t).unwrap().navigation = Some(Arc::new(nav));
    t
}

#[test]
fn ctrl_page_down_and_up_move_a_commentary_by_section_and_a_lexicon_by_entry() {
    let mut s = session();
    let section =
        |a: Verse, b: Verse, unit: &str| entry(Key::Passage(Passage::new(a, b).unwrap()), unit);
    let ge = |c, x| v(Book::Genesis, c, x);
    let comm = open_with(
        &mut s,
        "mhc",
        ResourceType::Commentary,
        KeyScheme::Passage,
        ResourceNavigation::new(vec![
            section(ge(1, 1), ge(2, 3), "creation"),
            section(ge(2, 4), ge(2, 25), "eden"),
            section(ge(3, 1), ge(3, 24), "fall"),
        ]),
    );
    s.workspace.navigate(comm, ge(1, 1));
    s.run("tab.next_chapter", &json!({ "tab": comm.0 }))
        .unwrap();
    assert_eq!(current(&s, comm), ge(2, 4), "the next section");
    s.run("tab.next_chapter", &json!({ "tab": comm.0 }))
        .unwrap();
    assert_eq!(current(&s, comm), ge(3, 1));
    assert!(
        s.run("tab.next_chapter", &json!({ "tab": comm.0 }))
            .is_err()
    );
    s.run("tab.previous_chapter", &json!({ "tab": comm.0 }))
        .unwrap();
    assert_eq!(current(&s, comm), ge(2, 4));

    let strong = |n: &str| Key::StrongsNumber(n.parse().unwrap());
    let lex = open_with(
        &mut s,
        "dodson",
        ResourceType::Lexicon,
        KeyScheme::StrongsNumber,
        ResourceNavigation::new(vec![
            entry(strong("G25"), "G25"),
            entry(strong("G26"), "G26"),
            entry(strong("G27"), "G27"),
        ]),
    );
    s.workspace.navigate_entry(lex, strong("G25"));
    let out = s.run("tab.next_chapter", &json!({ "tab": lex.0 })).unwrap();
    assert_eq!(out["key"], json!(strong("G26")));
    let t = s.workspace.tab(lex).unwrap();
    assert_eq!(t.entry_history.current(), Some(strong("G26")));
    s.run("tab.previous_chapter", &json!({ "tab": lex.0 }))
        .unwrap();
    assert_eq!(
        s.workspace.tab(lex).unwrap().entry_history.current(),
        Some(strong("G25"))
    );

    // A Tab with nothing to page through leaves the keys alone.
    s.run("library.open", &json!({})).unwrap();
    assert!(!s.is_enabled("tab.next_chapter"));
    assert!(!s.is_enabled("tab.previous_chapter"));
}

#[test]
fn f6_and_shift_f6_ask_the_ui_for_the_next_or_previous_area() {
    let mut s = session();
    s.workspace.take_effects();
    s.run("window.next_area", &json!({})).unwrap();
    s.run("window.previous_area", &json!({})).unwrap();
    assert_eq!(
        s.workspace.take_effects(),
        vec![
            Effect::NextArea { forward: true },
            Effect::NextArea { forward: false }
        ]
    );
}

/// The acceptance criterion's key list: every binding is in Help → Keyboard shortcuts with
/// its default, and no default meets another.
#[test]
fn every_navigation_key_is_listed_rebindable_and_conflict_free() {
    let s = session();
    let listed = s.key_bindings();
    let key = |id: &str| {
        let b = (listed.iter()).find(|b| b.id == id);
        b.unwrap_or_else(|| panic!("{id} is not in Keyboard shortcuts"))
            .key
            .clone()
    };
    let expected = [
        ("tab.next_in_panel", "Ctrl+Tab"),
        ("tab.previous_in_panel", "Ctrl+Shift+Tab"),
        ("panel.focus_1", "Ctrl+1"),
        ("panel.focus_5", "Ctrl+5"),
        ("panel.focus_9", "Ctrl+9"),
        ("tab.back", "Alt+Left"),
        ("tab.forward", "Alt+Right"),
        ("tab.previous_chapter", "Ctrl+PageUp"),
        ("tab.next_chapter", "Ctrl+PageDown"),
        ("window.next_area", "F6"),
        ("window.previous_area", "Shift+F6"),
        // The Highlight keys moved off Ctrl+1…9.
        ("selection.highlight_1", "Alt+1"),
        ("selection.highlight_9", "Alt+9"),
    ];
    for (id, k) in expected {
        assert_eq!(key(id).as_deref(), Some(k), "{id}");
        assert_eq!(key_bindings::canonical_key(k).as_deref(), Ok(k), "{id}");
        assert_eq!(key_bindings::not_bindable(k), None, "{id}: {k}");
    }
    for n in 1..=9 {
        assert_eq!(key(&format!("panel.focus_{n}")), Some(format!("Ctrl+{n}")));
        assert_eq!(
            key(&format!("selection.highlight_{n}")),
            Some(format!("Alt+{n}"))
        );
    }
    assert!(s.key_conflicts().is_empty(), "{:?}", s.key_conflicts());
}

#[test]
fn ctrl_tab_can_be_bound_but_tab_alone_and_alt_tab_cannot() {
    let mut s = session();
    assert_eq!(key_bindings::not_bindable("Ctrl+Tab"), None);
    assert!(key_bindings::not_bindable("Tab").is_some());
    assert!(key_bindings::not_bindable("Shift+Tab").is_some());
    assert!(key_bindings::not_bindable("Alt+Tab").is_some());
    // Rebinding works like any key, and a key in use is refused by name.
    s.run(
        "keys.set",
        &json!({ "command": "tab.next_in_panel", "key": "Ctrl+E" }),
    )
    .unwrap();
    assert_eq!(s.shortcut("tab.next_in_panel"), Some("Ctrl+E"));
    let err = s
        .run(
            "keys.set",
            &json!({ "command": "window.next_area", "key": "Ctrl+1" }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("Go to Panel 1"), "{err}");
}
