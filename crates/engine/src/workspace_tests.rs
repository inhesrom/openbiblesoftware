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
        ["Library", "Guides", "Tools", "Layouts", "View", "Window", "Help"]
    );
    // Import needs a local filesystem; not-yet-built items show but are disabled.
    assert_eq!(
        s.is_enabled("library.import"),
        cfg!(not(target_arch = "wasm32"))
    );
    assert!(s.is_enabled("tool.word_info"));
}

#[test]
fn view_and_help_commands_change_session_state() {
    let mut s = session();
    let size = s.text.size;
    s.run("view.text_larger", &json!({})).unwrap();
    assert_eq!(s.text.size, size + 1.0);
    s.run("view.page_tone", &json!({ "value": "sepia" }))
        .unwrap();
    assert_eq!(s.appearance.page_tone, PageTone::Sepia);
    s.run("view.theme", &json!({ "value": "dark" })).unwrap();
    assert_eq!(s.appearance.theme, Theme::Dark);
    assert!(s.run("view.theme", &json!({ "value": "sepia" })).is_err());
    // Credits is a keyless, single-instance Tab (#67).
    let credits = tab(&s.run("help.credits", &json!({})).unwrap());
    let t = s.workspace.tab(credits).unwrap();
    assert_eq!(
        (t.kind.clone(), t.title.as_str()),
        (TabKind::Credits, "Credits")
    );
    assert_eq!(t.kind.header_box(), HeaderBox::Keyless);
    assert!(!t.kind.has_link_chip());
    s.workspace.take_effects();
    assert_eq!(
        s.command_log(),
        [
            "view.text_larger",
            "view.page_tone",
            "view.theme",
            "view.theme",
            "help.credits"
        ]
    );
    s.run("layout.reset", &json!({})).unwrap();
    assert_eq!(s.workspace.take_effects(), vec![Effect::ResetLayout]);
}

#[test]
fn text_size_steps_undo_exactly_reset_to_the_default_and_show_the_size() {
    let mut s = session();
    let default = obs_layout::TextSettings::default().size;
    assert_eq!(s.text.size, default);
    for _ in 0..3 {
        s.run("view.text_larger", &json!({})).unwrap();
    }
    assert_eq!(s.text.size, default + 3.0);
    assert_eq!(
        s.workspace.take_effects().last(),
        Some(&Effect::Message(format!("Text {} pt", default + 3.0)))
    );
    for _ in 0..3 {
        s.run("view.text_smaller", &json!({})).unwrap();
    }
    assert_eq!(s.text.size, default);
    s.run("view.text_larger", &json!({})).unwrap();
    let out = s.run("view.text_reset", &json!({})).unwrap();
    assert_eq!(out, json!({ "size": default }));
    assert_eq!(s.text.size, default);
    assert_eq!(
        s.workspace.take_effects().last(),
        Some(&Effect::Message(format!("Text {default} pt")))
    );
    // Clamped at both ends.
    for _ in 0..60 {
        s.run("view.text_larger", &json!({})).unwrap();
    }
    assert_eq!(s.text.size, 40.0);
    for _ in 0..60 {
        s.run("view.text_smaller", &json!({})).unwrap();
    }
    assert_eq!(s.text.size, 10.0);
    // Its keys: Ctrl+0 resets; the shell also takes Ctrl+= and the numpad keys (#137).
    let key = |id: &str| s.commands().get(id).unwrap().shortcut;
    assert_eq!(key("view.text_larger"), Some("Ctrl+Plus"));
    assert_eq!(key("view.text_smaller"), Some("Ctrl+Minus"));
    assert_eq!(key("view.text_reset"), Some("Ctrl+0"));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_text_size_that_cannot_be_saved_is_an_error_not_a_size_message() {
    let dir = std::env::temp_dir().join(format!("obs-text-size-unsaved-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (store, _) = obs_store::UserStore::open(&dir.join("user.db")).unwrap();
    let mut s = session();
    s.attach_user(Box::new(crate::testing::WritesFail::preferences(store)));
    s.workspace.take_effects();
    for id in ["view.text_larger", "view.text_smaller", "view.text_reset"] {
        let err = s.run(id, &json!({})).unwrap_err().to_string();
        assert!(err.contains("disk full"), "{id}: {err}");
        assert_eq!(s.workspace.take_effects(), vec![], "{id}");
    }
    drop(s);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn interface_size_is_a_view_choice_kept_apart_from_text_size() {
    let mut s = session();
    assert_eq!(s.appearance.interface_size, InterfaceSize::Normal);
    assert_eq!(s.appearance.interface_size.zoom(), 1.0);
    let (options, current) = choices(&s, "view.interface_size");
    assert_eq!(
        options,
        [
            ("smaller", "Smaller"),
            ("default", "Default"),
            ("larger", "Larger")
        ]
    );
    assert_eq!(current, "default");
    let view = MENUS.iter().find(|m| m.title == "View").unwrap();
    assert!(view.items.contains(&MenuItem::Choice {
        title: "Interface size",
        command: "view.interface_size"
    }));
    assert!(view.items.contains(&MenuItem::Command("view.text_reset")));

    s.run("view.interface_size", &json!({ "value": "larger" }))
        .unwrap();
    assert_eq!(s.appearance.interface_size, InterfaceSize::Larger);
    assert_eq!(s.appearance.interface_size.zoom(), 1.25);
    assert!(
        s.run("view.interface_size", &json!({ "value": "huge" }))
            .is_err()
    );
    let size = s.text.size;
    for id in ["view.text_larger", "view.text_smaller", "view.text_reset"] {
        s.run(id, &json!({})).unwrap();
        assert_eq!(s.appearance.interface_size, InterfaceSize::Larger, "{id}");
    }
    assert_eq!(s.text.size, size);

    // Saved with the other View preferences, and read back (old records keep the default).
    let mut a = Appearance::default();
    let mut t = obs_layout::TextSettings::default();
    let mut f = FontChoices::default();
    let saved = crate::appearance::to_json(&s.appearance, &s.text, &s.fonts);
    assert_eq!(saved["interface_size"], "larger");
    crate::appearance::apply_json(&saved, &mut a, &mut t, &mut f);
    assert_eq!(a.interface_size, InterfaceSize::Larger);
    let mut a = Appearance::default();
    crate::appearance::apply_json(&json!({ "theme": "dark" }), &mut a, &mut t, &mut f);
    assert_eq!(a.interface_size, InterfaceSize::Normal);
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

#[test]
fn word_info_commands_follow_the_pin_state() {
    let mut s = session();
    // Nothing shown yet: nothing to copy, unpin, study or search for.
    assert!(!s.is_enabled("word_info.copy"));
    assert!(!s.is_enabled("word_info.unpin"));
    assert!(!s.is_enabled("word_info.word_study"));
    assert!(!s.is_enabled("word_info.search"));
    let part = obs_model::MorphemePart {
        strongs: "H1254A".parse().unwrap(),
        lemma: Some("בָּרָא".into()),
        morph: Some("HVqp3ms".into()),
        gloss: Some("he created".into()),
    };
    let word =
        ShownWord::new("בָּרָא", true, "Genesis 1:1 (TAHOT)".into(), &[part], |_| None).unwrap();
    assert!(s.word_info.hover(word.clone()));
    assert!(s.is_enabled("word_info.copy"));
    assert!(s.is_enabled("word_info.search"));
    assert!(!s.is_enabled("word_info.unpin"));
    let out = s.run("word_info.copy", &json!({})).unwrap();
    assert_eq!(out["text"], word.copy_text());
    s.word_info.pin(word);
    assert!(s.is_enabled("word_info.unpin"));
    assert_eq!(
        s.commands().get("word_info.unpin").unwrap().shortcut,
        Some("Escape")
    );
    s.run("word_info.unpin", &json!({})).unwrap();
    assert!(!s.word_info.pinned);
    assert!(s.word_info.shown.is_some(), "unpinning keeps the content");
}

#[test]
fn help_credits_opens_one_credits_tab_and_refocuses_it() {
    let mut s = session();
    let first = tab(&s.run("help.credits", &json!({})).unwrap());
    let kjv = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    assert_eq!(s.workspace.focused(), Some(kjv));
    let again = tab(&s.run("help.credits", &json!({})).unwrap());
    assert_eq!(again, first);
    assert_eq!(s.workspace.focused(), Some(first));
    let credits = s
        .workspace
        .tabs()
        .filter(|t| t.kind == TabKind::Credits)
        .count();
    assert_eq!(credits, 1);
    // It is not a reading Tab: it does not land in the reading Panel.
    assert_ne!(s.workspace.panel_of(first), s.workspace.panel_of(kjv));
}

/// View → Fonts (#138): font choices by family and style, kept in the User database.
#[test]
fn font_choices_are_saved_by_name_and_survive_a_restart() {
    let root = std::env::temp_dir().join(format!("obs-fonts-pref-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let db = root.join("user.db");
    let mut s = session();
    s.attach_user(Box::new(obs_store::UserStore::open(&db).unwrap().0));
    assert_eq!(s.fonts, FontChoices::default(), "bundled fonts by default");
    s.run("view.fonts", &json!({})).unwrap();
    assert!(s.workspace.take_effects().contains(&Effect::ShowFonts));
    let out = s
        .run(
            "view.font",
            &json!({ "slot": "latin", "family": "Charis SIL", "style": "Italic" }),
        )
        .unwrap();
    assert_eq!(out["family"], "Charis SIL");
    s.run(
        "view.font",
        &json!({ "slot": "interface", "family": "Inter" }),
    )
    .unwrap();
    assert!(
        s.run("view.font", &json!({ "slot": "arabic", "family": "X" }))
            .is_err()
    );
    let latin = s.fonts.get(FontSlot::Latin).unwrap();
    assert_eq!(
        (latin.family.as_str(), latin.style.as_str()),
        ("Charis SIL", "Italic")
    );
    assert_eq!(s.fonts.get(FontSlot::Interface).unwrap().style, "Regular");
    assert_eq!(s.fonts.get(FontSlot::Hebrew), None);
    // The font is missing here: tell the user once, and remember that across a restart.
    assert!(s.font_missing(FontSlot::Latin));
    assert!(!s.font_missing(FontSlot::Latin));
    assert!(
        !s.font_missing(FontSlot::Greek),
        "nothing chosen, nothing missing"
    );
    drop(s);

    let mut again = session();
    again.attach_user(Box::new(obs_store::UserStore::open(&db).unwrap().0));
    let latin = again.fonts.get(FontSlot::Latin).unwrap();
    assert_eq!(
        (latin.family.as_str(), latin.style.as_str()),
        ("Charis SIL", "Italic")
    );
    assert!(!again.font_missing(FontSlot::Latin), "already told");
    assert_eq!(
        again.fonts.get(FontSlot::Interface).unwrap().family,
        "Inter"
    );
    // Found again: a later absence is worth a notice again.
    again.font_found(FontSlot::Latin);
    assert!(again.font_missing(FontSlot::Latin));
    // Choosing the bundled font clears the choice.
    again
        .run("view.font", &json!({ "slot": "latin", "family": "" }))
        .unwrap();
    assert_eq!(again.fonts.get(FontSlot::Latin), None);
    drop(again);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn ctrl_f_opens_the_find_bar_of_a_resource_or_text_comparison_tab() {
    let mut s = session();
    let find = s.commands().get("tab.find").unwrap();
    assert_eq!(find.shortcut, Some("Ctrl+F"));
    // Nothing focused, or a Tab without a Find bar: the key is left alone.
    assert!(!s.is_enabled("tab.find"));
    let search = tab(&s.run("tool.search", &json!({})).unwrap());
    assert!(!s.is_enabled("tab.find"));
    assert!(s.run("tab.find", &json!({ "tab": search.0 })).is_err());

    let kjv = tab(&s.run("resource.open", &json!({ "id": "kjv" })).unwrap());
    assert!(s.is_enabled("tab.find"));
    s.workspace.take_effects();
    s.run("tab.find", &json!({ "tab": kjv.0, "text": "love" }))
        .unwrap();
    s.run("tab.find_next", &json!({ "tab": kjv.0 })).unwrap();
    s.run("tab.find_previous", &json!({ "tab": kjv.0 }))
        .unwrap();
    assert_eq!(
        s.workspace.take_effects(),
        [
            Effect::Find {
                tab: kjv,
                text: Some("love".into())
            },
            Effect::FindStep {
                tab: kjv,
                forward: true
            },
            Effect::FindStep {
                tab: kjv,
                forward: false
            },
        ]
    );

    let compare = tab(&s.run("tool.text_comparison", &json!({})).unwrap());
    assert!(s.is_enabled("tab.find"));
    s.workspace.take_effects();
    s.run("tab.find", &json!({ "tab": compare.0 })).unwrap();
    assert_eq!(
        s.workspace.take_effects(),
        [Effect::Find {
            tab: compare,
            text: None
        }]
    );
}
