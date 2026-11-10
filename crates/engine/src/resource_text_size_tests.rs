//! A Resource's own Text size (#292): absolute, per Resource (every Tab of it), kept in the
//! User database across a restart, Layout loads and a Backup restore; a Resource without one
//! follows the global Text size, which never moves one it has.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::appearance::{TEXT_SIZE_DEFAULT, TEXT_SIZE_MAX, TEXT_SIZE_MIN};
use crate::resource_text_size::RESOURCE_TEXT_SIZES_PREF;
use crate::settings::{setting_by_id, settings_matching};
use crate::testing::bibles::{tahot, web, write_library};
use crate::testing::kjv_john;
use crate::*;

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// KJV (John), WEB and TAHOT as real `.obr` files, with a User database.
fn launch(name: &str) -> (Session, Temp) {
    let root = std::env::temp_dir().join(format!(
        "obs-resource-text-size-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    write_library(&root.join("library"), &[kjv_john(), web(), tahot()]);
    (relaunch(&root), Temp(root))
}

/// A new Session on the same library and User database.
fn relaunch(root: &Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    s
}

fn open(s: &mut Session, id: &str) -> TabId {
    let out = s
        .run("resource.open", &json!({ "id": id, "split": true }))
        .unwrap();
    TabId(out["tab"].as_u64().unwrap())
}

fn set(s: &mut Session, resource: &str, value: i64) {
    s.run(
        "view.resource_text_size",
        &json!({ "resource": resource, "value": value }),
    )
    .unwrap();
}

fn messages(s: &mut Session) -> Vec<String> {
    (s.workspace.take_effects().into_iter())
        .filter_map(|e| match e {
            Effect::Message(m) => Some(m),
            _ => None,
        })
        .collect()
}

/// Acceptance: the size is the Resource's, shared by all its Tabs; a Resource without one
/// follows the global Text size; the global keys never move an own size; Reset returns the
/// Resource to following the global size.
#[test]
fn a_resources_own_size_is_shared_by_its_tabs_and_the_global_size_does_not_move_it() {
    let (mut s, _t) = launch("shared");
    let kjv = open(&mut s, "kjv");
    let kjv_again = open(&mut s, "kjv");
    let tahot = open(&mut s, "tahot");
    assert_eq!(s.text_size_of("kjv"), TEXT_SIZE_DEFAULT);

    // The focused Tab's Resource, a step at a time.
    s.workspace.user_focused(tahot);
    s.run("view.resource_text_larger", &json!({})).unwrap();
    s.run("view.resource_text_larger", &json!({})).unwrap();
    assert_eq!(s.text_size_of("tahot"), TEXT_SIZE_DEFAULT + 2.0);
    assert_eq!(
        messages(&mut s).last().map(String::as_str),
        Some("TAHOT text 22 pt")
    );
    assert_eq!(
        s.text_size_of("kjv"),
        TEXT_SIZE_DEFAULT,
        "only that Resource"
    );

    // Every Tab of a Resource shares its size: either Tab's controls change it.
    s.run(
        "view.resource_text_size",
        &json!({ "tab": kjv_again.0, "value": 26 }),
    )
    .unwrap();
    for tab in [kjv, kjv_again] {
        assert_eq!(s.text_size_resource(tab).as_deref(), Some("kjv"));
    }
    assert_eq!(s.text_size_of("kjv"), 26.0);
    assert_eq!(s.text_settings_of("kjv").size, 26.0);
    assert_eq!(
        s.text_settings_of("kjv").line_spacing,
        s.text.line_spacing,
        "the other Text settings are the global ones"
    );

    // The global size moves the Resources that follow it, and none with their own.
    s.run("view.text_larger", &json!({})).unwrap();
    s.run("view.text_larger", &json!({})).unwrap();
    assert_eq!(s.text.size, TEXT_SIZE_DEFAULT + 2.0);
    assert_eq!(s.text_size_of("web"), TEXT_SIZE_DEFAULT + 2.0);
    assert_eq!(s.text_size_of("kjv"), 26.0);
    s.run("view.text_reset", &json!({})).unwrap();
    assert_eq!(s.text_size_of("kjv"), 26.0);

    // Reset: the Resource follows the global size again, wherever it goes.
    s.workspace.user_focused(kjv);
    let out = s.run("view.resource_text_reset", &json!({})).unwrap();
    assert_eq!(out["own"], false);
    assert_eq!(s.resource_text_sizes().get("kjv"), None);
    assert_eq!(s.text_size_of("kjv"), s.text.size);
    s.run("view.text_larger", &json!({})).unwrap();
    assert_eq!(s.text_size_of("kjv"), s.text.size);
    assert_eq!(
        messages(&mut s)
            .iter()
            .filter(|m| m.starts_with("KJV"))
            .count(),
        1,
        "Reset says the Resource follows the Text size; the slider's value says nothing"
    );
}

/// The size is absolute and stays in the readable range, in whole points.
#[test]
fn an_own_size_is_absolute_and_kept_in_the_readable_range() {
    let (mut s, _t) = launch("range");
    let kjv = open(&mut s, "kjv");
    set(&mut s, "kjv", 99);
    assert_eq!(s.text_size_of("kjv"), TEXT_SIZE_MAX);
    s.run("view.resource_text_larger", &json!({ "tab": kjv.0 }))
        .unwrap();
    assert_eq!(s.text_size_of("kjv"), TEXT_SIZE_MAX);
    set(&mut s, "kjv", 3);
    assert_eq!(s.text_size_of("kjv"), TEXT_SIZE_MIN);
    s.run("view.resource_text_smaller", &json!({ "tab": kjv.0 }))
        .unwrap();
    assert_eq!(s.text_size_of("kjv"), TEXT_SIZE_MIN);
    // Smaller from a Resource following the global size starts at the global size.
    s.run("view.resource_text_smaller", &json!({ "resource": "web" }))
        .unwrap();
    assert_eq!(s.text_size_of("web"), TEXT_SIZE_DEFAULT - 1.0);
}

/// Acceptance: kept in the User database, so it survives a restart and Layout loads (a
/// Layout saved with another size does not change it), and is the Resource's in the Last
/// session.
#[test]
fn an_own_size_survives_a_restart_and_layout_loads() {
    let (mut s, t) = launch("restart");
    open(&mut s, "kjv");
    s.run("layout.save", &json!({ "name": "Study" })).unwrap();
    set(&mut s, "kjv", 28);
    set(&mut s, "tahot", 30);
    s.save_last_session().unwrap();
    drop(s);

    let mut s = relaunch(&t.0);
    assert_eq!(s.text_size_of("kjv"), 28.0);
    assert_eq!(s.text_size_of("tahot"), 30.0);
    assert_eq!(s.text_size_of("web"), TEXT_SIZE_DEFAULT);
    let stored = s
        .user
        .as_ref()
        .unwrap()
        .preference(RESOURCE_TEXT_SIZES_PREF)
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&stored).unwrap(),
        json!({ "kjv": 28.0, "tahot": 30.0 })
    );

    // A Layout saved before the size was chosen keeps nothing of it: loading it leaves it.
    s.run("layout.restore", &json!({ "name": "Study" }))
        .unwrap();
    let kjv = (s.workspace.tabs())
        .find(|t| t.kind == TabKind::Resource("kjv".into()))
        .map(|t| t.id)
        .unwrap();
    assert_eq!(s.text_size_resource(kjv).as_deref(), Some("kjv"));
    assert_eq!(s.text_size_of("kjv"), 28.0);
    let snapshot = s.workspace.snapshot().to_json();
    assert!(!snapshot.contains("28"), "not kept in Layouts: {snapshot}");
}

/// Acceptance: a Backup holds the sizes and a restore brings them back; a restore of a
/// Backup without them leaves every Resource following the global size; a Backup with a
/// size the loader would change is refused, naming the setting.
#[test]
fn own_sizes_are_backed_up_and_restored() {
    let (mut s, t) = launch("backup");
    set(&mut s, "kjv", 24);
    let with = t.0.join("with.obsbackup");
    s.run("data.back_up", &json!({ "path": with.to_str().unwrap() }))
        .unwrap();
    s.run(
        "settings.reset",
        &json!({ "setting": "resource_text_sizes" }),
    )
    .unwrap();
    let without = t.0.join("without.obsbackup");
    s.run(
        "data.back_up",
        &json!({ "path": without.to_str().unwrap() }),
    )
    .unwrap();
    set(&mut s, "tahot", 32);

    s.run("data.restore", &json!({ "path": with.to_str().unwrap() }))
        .unwrap();
    assert_eq!(s.text_size_of("kjv"), 24.0);
    assert_eq!(s.text_size_of("tahot"), TEXT_SIZE_DEFAULT);
    s.run(
        "data.restore",
        &json!({ "path": without.to_str().unwrap() }),
    )
    .unwrap();
    assert!(s.resource_text_sizes().is_empty());

    let user = s.user.as_ref().unwrap();
    user.set_preference(RESOURCE_TEXT_SIZES_PREF, r#"{"kjv": 9.0}"#)
        .unwrap();
    let damaged = t.0.join("damaged.obsbackup");
    s.run(
        "data.back_up",
        &json!({ "path": damaged.to_str().unwrap() }),
    )
    .unwrap();
    let (opened, _) = s.user.as_ref().unwrap().open_backup(&damaged).unwrap();
    assert_eq!(
        s.trial_restore(&*opened).unwrap(),
        ["the setting `view.resource_text_sizes` (Resources' own Text sizes)"]
    );
}

/// Decision: in a Text Comparison Tab the size controls change the Resource last clicked
/// into while it is compared, else the first row's; other Tools have none.
#[test]
fn a_text_comparison_tab_sizes_the_bible_last_clicked_into() {
    let (mut s, _t) = launch("compare");
    let tab = TabId(
        s.run("tool.text_comparison", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    let compared = |s: &Session| s.workspace.tab(tab).unwrap().comparison.bibles(&s.library);
    let bibles = compared(&s);
    assert!(bibles.len() >= 2, "{bibles:?}");
    let (first, second) = (bibles[0].clone(), bibles[1].clone());
    assert_eq!(s.text_size_resource(tab), Some(first.clone()));
    s.workspace.clicked_into(tab, &second);
    assert_eq!(s.text_size_resource(tab), Some(second.clone()));
    s.workspace.user_focused(tab);
    s.run("view.resource_text_larger", &json!({})).unwrap();
    assert_eq!(s.text_size_of(&second), TEXT_SIZE_DEFAULT + 1.0);
    assert_eq!(s.text_size_of(&first), TEXT_SIZE_DEFAULT);
    // A Bible no longer compared is not the one.
    s.run("compare.bible", &json!({ "tab": tab.0, "id": second }))
        .unwrap();
    assert!(!compared(&s).contains(&second));
    assert_eq!(s.text_size_resource(tab), Some(first));

    let library = TabId(
        s.run("library.open", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    s.workspace.user_focused(library);
    assert_eq!(s.text_size_resource(library), None);
    assert!(!s.is_enabled("view.resource_text_larger"));
}

/// Acceptance: the commands have the Text size keys with Alt, can run from a key, and the
/// Command box finds them.
#[test]
fn the_commands_have_keys_and_the_command_box_finds_them() {
    let (mut s, _t) = launch("keys");
    // Enabled, so listed, while the focused Tab shows a Resource.
    open(&mut s, "kjv");
    for (id, key) in [
        ("view.resource_text_larger", "Ctrl+Alt+Plus"),
        ("view.resource_text_smaller", "Ctrl+Alt+Minus"),
        ("view.resource_text_reset", "Ctrl+Alt+0"),
    ] {
        assert_eq!(s.shortcut(id), Some(key), "{id}");
        let spec = s.commands().get(id).unwrap();
        assert!(crate::key_bindings::runs_from_a_key(spec), "{id}");
    }
    assert!(s.key_conflicts().is_empty(), "{:?}", s.key_conflicts());
    let listed: Vec<&str> = s.key_bindings().iter().map(|b| b.id).collect();
    for id in [
        "view.resource_text_larger",
        "view.resource_text_smaller",
        "view.resource_text_reset",
    ] {
        assert!(listed.contains(&id), "{id} is not in Keyboard shortcuts");
    }
    for (query, id) in [
        ("larger text for this resource", "view.resource_text_larger"),
        ("smaller text", "view.resource_text_smaller"),
        ("reset this resource", "view.resource_text_reset"),
    ] {
        let found = s.command_box_suggestions(&format!("> {query}"));
        assert!(
            (found.iter()).any(|f| f.action == CommandBoxAction::Command(id)),
            "{query}: {found:?}"
        );
    }
}

/// Acceptance: Settings → Text and fonts lists the own sizes; its Reset (Reset all) puts every
/// Resource back on the global size; the search finds it by "font size" and "zoom".
#[test]
fn settings_lists_own_sizes_resets_them_and_its_search_finds_them() {
    let (mut s, _t) = launch("settings");
    let setting = setting_by_id("resource_text_sizes").unwrap();
    assert_eq!(
        setting.section,
        crate::settings::SettingsSection::TextAndFonts
    );
    for query in ["font size", "zoom", "text size", "own"] {
        assert!(
            settings_matching(&s, query)
                .iter()
                .any(|m| m.id == "resource_text_sizes"),
            "{query}"
        );
    }
    assert_eq!(s.setting_is_default("resource_text_sizes"), Some(true));
    set(&mut s, "kjv", 24);
    set(&mut s, "tahot", 30);
    assert_eq!(s.setting_is_default("resource_text_sizes"), Some(false));
    let own: Vec<(&str, f32)> = s.resource_text_sizes().iter().collect();
    assert_eq!(own, [("kjv", 24.0), ("tahot", 30.0)]);
    // One Resource's Reset.
    s.run(
        "settings.reset",
        &json!({ "setting": "resource_text_sizes", "resource": "kjv" }),
    )
    .unwrap();
    assert_eq!(s.resource_text_sizes().get("kjv"), None);
    assert_eq!(s.resource_text_sizes().iter().count(), 1);
    // Reset all.
    s.run(
        "settings.reset",
        &json!({ "setting": "resource_text_sizes" }),
    )
    .unwrap();
    assert!(s.resource_text_sizes().is_empty());
    assert_eq!(s.setting_is_default("resource_text_sizes"), Some(true));
}
