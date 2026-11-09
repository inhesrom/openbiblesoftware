//! Reading layout (#209): defaults in Settings, a Tab's own choices over them, and both kept:
//! the defaults in the User database, a Tab's choices in Layouts and the Last session.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_layout::{ColumnWidth, ReadingLayout, TextFlow};
use serde_json::json;

use crate::testing::bibles::{tahot, write_library};
use crate::testing::kjv_john;
use crate::*;

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn root(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("obs-reading-layout-{name}-{}", std::process::id()))
}

/// KJV (John) and TAHOT as real `.obr` files, with a User database.
fn launch(name: &str) -> (Session, Temp) {
    let root = root(name);
    let _ = std::fs::remove_dir_all(&root);
    write_library(&root.join("library"), &[kjv_john(), tahot()]);
    (relaunch(&root), Temp(root))
}

/// A new Session on the same library and User database.
fn relaunch(root: &std::path::Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    s
}

fn open(s: &mut Session, id: &str, split: bool) -> TabId {
    let out = s
        .run("resource.open", &json!({ "id": id, "split": split }))
        .unwrap();
    TabId(out["tab"].as_u64().unwrap())
}

#[test]
fn a_tabs_own_choices_override_the_defaults_and_the_rest_follow_them() {
    let (mut s, _t) = launch("per-tab");
    let kjv = open(&mut s, "kjv", false);
    let tahot = open(&mut s, "tahot", true);
    assert_eq!(s.reading_layout(kjv), ReadingLayout::default());
    let out = s
        .run(
            "tab.reading_layout",
            &json!({ "tab": kjv.0, "flow": "verse_per_line", "justified": true }),
        )
        .unwrap();
    assert_eq!(out["flow"], "verse_per_line");
    assert_eq!(out["justified"], true);
    assert_eq!(out["column"], "medium", "the rest are the defaults");
    assert_eq!(s.reading_layout(kjv).flow, TextFlow::VersePerLine);
    assert_eq!(
        s.reading_layout(tahot),
        ReadingLayout::default(),
        "only that Tab"
    );

    // A default the Tab has not set reaches it; one it has set does not.
    s.run("view.column_width", &json!({ "value": "wide" }))
        .unwrap();
    s.run("view.justified", &json!({ "on": false })).unwrap();
    s.run("view.section_headings", &json!({})).unwrap();
    assert_eq!(s.reading_layout(kjv).column, ColumnWidth::Wide);
    assert!(s.reading_layout(kjv).justified, "the Tab's own choice");
    assert!(!s.reading_layout(kjv).section_headings);
    assert_eq!(s.reading_layout(tahot).column, ColumnWidth::Wide);
    assert!(!s.reading_layout(tahot).section_headings);

    // Back to the defaults.
    s.run(
        "tab.reading_layout",
        &json!({ "tab": kjv.0, "reset": true }),
    )
    .unwrap();
    assert_eq!(s.reading_layout(kjv), s.text.layout);
    assert!(s.workspace.tab(kjv).unwrap().reading_layout.is_default());
}

#[test]
fn tab_reading_layout_refuses_bad_choices_and_tabs_without_a_resource() {
    let (mut s, _t) = launch("refuse");
    let kjv = open(&mut s, "kjv", false);
    for params in [
        json!({ "tab": kjv.0, "flow": "columns" }),
        json!({ "tab": kjv.0, "column": "huge" }),
        json!({ "tab": 9999, "justified": true }),
    ] {
        assert!(s.run("tab.reading_layout", &params).is_err(), "{params}");
    }
    assert!(s.workspace.tab(kjv).unwrap().reading_layout.is_default());
    let search = TabId(
        s.run("tool.search", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    assert!(
        s.run(
            "tab.reading_layout",
            &json!({ "tab": search.0, "justified": true })
        )
        .is_err()
    );
    assert!(
        s.run("view.text_flow", &json!({ "value": "columns" }))
            .is_err()
    );
}

#[test]
fn layouts_and_the_last_session_keep_a_tabs_reading_layout() {
    let (mut s, _t) = launch("layouts");
    let kjv = open(&mut s, "kjv", false);
    let tahot = open(&mut s, "tahot", true);
    s.run(
        "tab.reading_layout",
        &json!({ "tab": kjv.0, "flow": "verse_per_line", "large_chapter_numbers": true }),
    )
    .unwrap();
    s.run(
        "tab.reading_layout",
        &json!({ "tab": tahot.0, "column": "narrow", "section_headings": false }),
    )
    .unwrap();
    s.run("layout.save", &json!({ "name": "Study" })).unwrap();
    s.run(
        "tab.reading_layout",
        &json!({ "tab": kjv.0, "reset": true }),
    )
    .unwrap();
    s.run("layout.restore", &json!({ "name": "Study" }))
        .unwrap();
    let by_kind = |s: &Session, id: &str| {
        let tab = s
            .workspace
            .tabs()
            .find(|t| t.kind == TabKind::Resource(id.into()))
            .unwrap()
            .id;
        s.reading_layout(tab)
    };
    let kjv_layout = by_kind(&s, "kjv");
    assert_eq!(kjv_layout.flow, TextFlow::VersePerLine);
    assert!(kjv_layout.large_chapter_numbers);
    assert_eq!(by_kind(&s, "tahot").column, ColumnWidth::Narrow);
    assert!(!by_kind(&s, "tahot").section_headings);

    // The Last session round-trips it; a Tab without its own choices saves nothing extra.
    let snap = s.workspace.snapshot();
    let back = WorkspaceSnapshot::from_json(&snap.to_json()).unwrap();
    assert_eq!(back, snap);
    let plain = TabSnapshot {
        reading_layout: TabReadingLayout::default(),
        ..snap.tabs[0].clone()
    };
    assert!(
        !serde_json::to_string(&plain)
            .unwrap()
            .contains("reading_layout")
    );
    let kjv = (snap.tabs.iter())
        .find(|t| t.kind == TabKind::Resource("kjv".into()))
        .unwrap();
    assert!(serde_json::to_value(kjv).unwrap()["reading_layout"].is_object());
}

#[test]
fn the_defaults_are_kept_in_the_user_database() {
    let (mut s, t) = launch("defaults");
    s.run("view.text_flow", &json!({ "value": "verse_per_line" }))
        .unwrap();
    s.run("view.large_chapter_numbers", &json!({ "on": true }))
        .unwrap();
    s.run("view.section_headings", &json!({ "on": false }))
        .unwrap();
    s.run("view.column_width", &json!({ "value": "full" }))
        .unwrap();
    s.run("view.justified", &json!({})).unwrap();
    let want = ReadingLayout {
        flow: TextFlow::VersePerLine,
        large_chapter_numbers: true,
        section_headings: false,
        column: ColumnWidth::Full,
        justified: true,
    };
    assert_eq!(s.text.layout, want);
    drop(s);
    let s = relaunch(&t.0);
    assert_eq!(s.text.layout, want);
    // The choices show in View menus and Settings.
    assert_eq!(choices(&s, "view.text_flow").1, "verse_per_line");
    assert_eq!(choices(&s, "view.column_width").1, "full");
    assert_eq!(
        choices(&s, "view.column_width").0,
        [
            ("narrow", "Narrow"),
            ("medium", "Medium"),
            ("wide", "Wide"),
            ("full", "Full width")
        ]
    );
}

#[test]
fn settings_reset_each_reading_layout_default() {
    let (mut s, _t) = launch("reset");
    s.run("view.text_flow", &json!({ "value": "verse_per_line" }))
        .unwrap();
    s.run("view.large_chapter_numbers", &json!({ "on": true }))
        .unwrap();
    s.run("view.section_headings", &json!({ "on": false }))
        .unwrap();
    s.run("view.column_width", &json!({ "value": "narrow" }))
        .unwrap();
    s.run("view.justified", &json!({ "on": true })).unwrap();
    for id in [
        "text_flow",
        "large_chapter_numbers",
        "section_headings",
        "column_width",
        "justified",
    ] {
        assert_eq!(s.setting_is_default(id), Some(false), "{id}");
        s.run("settings.reset", &json!({ "setting": id })).unwrap();
        assert_eq!(s.setting_is_default(id), Some(true), "{id}");
    }
    assert_eq!(s.text.layout, ReadingLayout::default());
}

#[test]
fn an_older_preference_record_reads_as_the_default_layout() {
    let mut a = Appearance::default();
    let mut t = obs_layout::TextSettings::default();
    let mut f = FontChoices::default();
    crate::appearance::apply_json(
        &json!({ "text_size": 20.0, "reading_layout": { "justified": true } }),
        &mut a,
        &mut t,
        &mut f,
    );
    assert_eq!(
        t.layout,
        ReadingLayout {
            justified: true,
            ..ReadingLayout::default()
        },
        "a missing field keeps its default"
    );
    let mut t = obs_layout::TextSettings::default();
    crate::appearance::apply_json(&json!({ "text_size": 20.0 }), &mut a, &mut t, &mut f);
    assert_eq!(t.layout, ReadingLayout::default());
}
