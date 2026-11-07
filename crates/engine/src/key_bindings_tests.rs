//! Key bindings (#177): every binding listed by menu, the user's own keys kept in the User
//! database across launches, conflicts refused or swapped, reserved keys refused.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::key_bindings::{
    KEY_BINDINGS_PREF, OTHER_GROUP, SELECTION_GROUP, canonical_key, key_binding_groups,
    not_bindable,
};
use crate::*;

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-key-bindings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn db(&self) -> PathBuf {
        self.0.join("user.db")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn launch(db: &Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(db).unwrap();
    s.attach_user(Box::new(store));
    s
}

/// Acceptance: Larger text rebound to Ctrl+Up has it at once and after a restart; Reset
/// restores its default (Ctrl+Plus, pressed as Ctrl+= too).
#[test]
fn rebinding_larger_text_to_ctrl_up_survives_a_restart_and_reset_restores_the_default() {
    let t = Temp::new("larger");
    let mut s = launch(&t.db());
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+Plus"));
    let out = s
        .run(
            "keys.set",
            &json!({ "command": "view.text_larger", "key": "Ctrl+ArrowUp" }),
        )
        .unwrap();
    assert_eq!(out["key"], "Ctrl+Up");
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+Up"));
    let larger = |s: &Session| {
        s.key_bindings()
            .into_iter()
            .find(|b| b.id == "view.text_larger")
            .unwrap()
    };
    assert!(larger(&s).is_custom());
    assert_eq!(larger(&s).group, "View");
    drop(s);

    let mut s = launch(&t.db());
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+Up"));
    s.run("keys.reset", &json!({ "command": "view.text_larger" }))
        .unwrap();
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+Plus"));
    assert!(!larger(&s).is_custom());
    // Ctrl+= is the same key as Ctrl+Plus (#137).
    assert_eq!(
        s.key_conflict("view.text_smaller", "Ctrl+=").map(|c| c.id),
        Some("view.text_larger")
    );
    drop(s);
    let s = launch(&t.db());
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+Plus"));
    // A key equal to the default is not kept as the user's own.
    let saved = s.user.as_ref().unwrap().preference(KEY_BINDINGS_PREF);
    assert_eq!(saved.unwrap().as_deref(), Some("{}"));
}

/// Acceptance: a key already in use is refused, naming the command that has it (Cancel leaves
/// both as they were); with swap the two trade keys.
#[test]
fn binding_a_key_in_use_names_the_conflict_and_swap_trades_the_keys() {
    let t = Temp::new("conflict");
    let mut s = launch(&t.db());
    let conflict = s.key_conflict("view.text_larger", "Ctrl+F").unwrap();
    assert_eq!(
        (conflict.id, conflict.label),
        ("tab.find", "Find in Resource")
    );
    let err = s
        .run(
            "keys.set",
            &json!({ "command": "view.text_larger", "key": "Ctrl+F" }),
        )
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Ctrl+F is already the key of “Find in Resource”"),
        "{err}"
    );
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+Plus"));
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+F"));

    let out = s
        .run(
            "keys.set",
            &json!({ "command": "view.text_larger", "key": "Ctrl+F", "swap": true }),
        )
        .unwrap();
    assert_eq!(
        out["swapped"],
        json!({ "command": "tab.find", "key": "Ctrl+Plus" })
    );
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+F"));
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+Plus"));
    drop(s);
    let mut s = launch(&t.db());
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+F"));
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+Plus"));

    // Reset to a default another binding has now is a conflict too.
    let err = s
        .run("keys.reset", &json!({ "command": "view.text_larger" }))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("already the key of “Find in Resource”"),
        "{err}"
    );
    s.run(
        "keys.reset",
        &json!({ "command": "view.text_larger", "swap": true }),
    )
    .unwrap();
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+Plus"));
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+F"));

    // A Selection key conflicts like a command.
    let c = s.key_conflict("view.text_larger", "Ctrl+1").unwrap();
    assert_eq!(c.id, "selection.highlight_yellow");
}

/// Swapping never hands a reserved key away: Unpin Word Info's default Esc stays with it, so
/// rebinding it to Find's Ctrl+F with swap is refused and both keep their keys, then and after
/// a restart.
#[test]
fn a_swap_never_moves_esc_off_unpin_word_info() {
    let t = Temp::new("reserved-swap");
    let mut s = launch(&t.db());
    assert_eq!(s.shortcut("word_info.unpin"), Some("Escape"));
    assert_eq!(s.swap_refused("view.text_larger", "tab.find"), None);
    let why = s.swap_refused("word_info.unpin", "tab.find").unwrap();
    assert!(
        why.contains("“Find in Resource” cannot take Escape"),
        "{why}"
    );
    let err = s
        .run(
            "keys.set",
            &json!({ "command": "word_info.unpin", "key": "Ctrl+F", "swap": true }),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot take Escape: Esc is kept"), "{err}");
    assert_eq!(s.shortcut("word_info.unpin"), Some("Escape"));
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+F"));
    drop(s);
    let mut s = launch(&t.db());
    assert_eq!(s.shortcut("word_info.unpin"), Some("Escape"));
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+F"));
    // Once Unpin Word Info has a key of its own, it can swap that one.
    s.run(
        "keys.set",
        &json!({ "command": "word_info.unpin", "key": "Alt+U" }),
    )
    .unwrap();
    s.run(
        "keys.set",
        &json!({ "command": "word_info.unpin", "key": "Ctrl+F", "swap": true }),
    )
    .unwrap();
    assert_eq!(s.shortcut("word_info.unpin"), Some("Ctrl+F"));
    assert_eq!(s.shortcut("tab.find"), Some("Alt+U"));
}

#[test]
fn clear_and_reset_all_survive_a_restart() {
    let t = Temp::new("clear");
    let mut s = launch(&t.db());
    s.run("keys.clear", &json!({ "command": "tab.find" }))
        .unwrap();
    s.run(
        "keys.set",
        &json!({ "command": "selection.note", "key": "Alt+N" }),
    )
    .unwrap();
    // A command with no default key can be given one.
    s.run(
        "keys.set",
        &json!({ "command": "help.credits", "key": "F1" }),
    )
    .unwrap();
    drop(s);
    let mut s = launch(&t.db());
    assert_eq!(s.shortcut("tab.find"), None);
    assert_eq!(s.shortcut("selection.note"), Some("Alt+N"));
    assert_eq!(s.shortcut("help.credits"), Some("F1"));
    // A cleared key is free for another binding.
    assert!(s.key_conflict("view.text_larger", "Ctrl+F").is_none());
    s.run("keys.reset_all", &json!({})).unwrap();
    drop(s);
    let s = launch(&t.db());
    assert_eq!(s.shortcut("tab.find"), Some("Ctrl+F"));
    assert_eq!(s.shortcut("selection.note"), Some("Ctrl+Shift+N"));
    assert_eq!(s.shortcut("help.credits"), None);
}

/// Esc, Enter and Tab are never bound; a key needs Ctrl or Alt unless it is a function key.
#[test]
fn reserved_keys_and_keys_that_type_text_are_refused() {
    let mut s = Session::new(JobMode::Inline);
    for (key, why) in [
        ("Escape", "Esc is kept"),
        ("Ctrl+Escape", "Esc is kept"),
        ("Enter", "Enter is kept"),
        ("Ctrl+Enter", "Enter is kept"),
        ("Shift+Tab", "Tab is kept"),
        ("A", "add Ctrl or Alt"),
        ("Shift+A", "add Ctrl or Alt"),
        ("Up", "add Ctrl or Alt"),
    ] {
        let err = s
            .run("keys.set", &json!({ "command": "tab.find", "key": key }))
            .unwrap_err()
            .to_string();
        assert!(err.contains(why), "{key}: {err}");
        assert_eq!(s.shortcut("tab.find"), Some("Ctrl+F"));
    }
    for key in ["F5", "Shift+F5", "Alt+Q", "Ctrl+Shift+Q"] {
        assert_eq!(not_bindable(&canonical_key(key).unwrap()), None, "{key}");
    }
    let err = s
        .run(
            "keys.set",
            &json!({ "command": "tab.find", "key": "Ctrl+Nope" }),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("“Nope” is not a key"), "{err}");
    // Commands that need more than their Tab cannot run from a key.
    for id in ["tab.go", "no.such_command"] {
        let err = s
            .run("keys.set", &json!({ "command": id, "key": "F6" }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a command that runs from a key"), "{err}");
    }
}

#[test]
fn keys_are_written_one_way() {
    for (given, want) in [
        ("ctrl+shift+n", "Ctrl+Shift+N"),
        ("Alt+Shift+Ctrl+x", "Ctrl+Shift+Alt+X"),
        ("Ctrl+Shift+Plus", "Ctrl+Plus"),
        ("Ctrl+[", "Ctrl+OpenBracket"),
        ("Ctrl+ArrowUp", "Ctrl+Up"),
        ("Ctrl+=", "Ctrl+Equals"),
        ("Ctrl+0", "Ctrl+0"),
        ("F12", "F12"),
    ] {
        assert_eq!(canonical_key(given).as_deref(), Ok(want), "{given}");
    }
    for bad in ["", "Ctrl", "Ctrl+A+B", "Ctrl+F0", "Ctrl+F36", "Ctrl+Wat"] {
        assert!(canonical_key(bad).is_err(), "{bad}");
    }
}

/// Every command a key can run is listed once, grouped by its menu, with the Selection keys
/// and the commands in no menu after the menus; the defaults never conflict.
#[test]
fn every_binding_is_listed_once_by_menu_and_the_defaults_do_not_conflict() {
    let s = Session::new(JobMode::Inline);
    let all = s.key_bindings();
    let groups = key_binding_groups();
    let mut last = 0;
    for b in &all {
        let g = groups.iter().position(|g| *g == b.group).unwrap();
        assert!(g >= last, "{} out of group order", b.id);
        last = g;
        assert_eq!(all.iter().filter(|o| o.id == b.id).count(), 1, "{}", b.id);
        assert!(!b.is_custom(), "{}", b.id);
        if let Some(k) = &b.key {
            assert!(canonical_key(k).is_ok(), "{} {k}", b.id);
            assert_eq!(s.key_conflict(b.id, k), None, "{} {k}", b.id);
        }
    }
    let group = |id: &str| all.iter().find(|b| b.id == id).map(|b| b.group);
    assert_eq!(group("view.text_larger"), Some("View"));
    assert_eq!(group("help.keyboard_shortcuts"), Some("Help"));
    assert_eq!(group("selection.highlight_pink"), Some(SELECTION_GROUP));
    assert_eq!(group("tab.find"), Some(OTHER_GROUP));
    assert_eq!(group("tab.go"), None);
    // Every command with a default key is listed.
    for spec in s.commands().iter().filter(|c| c.shortcut.is_some()) {
        assert!(group(spec.id).is_some(), "{} is not listed", spec.id);
    }
    // The Selection keys are not commands.
    for k in SELECTION_KEYS {
        assert!(s.commands().get(k.id).is_none(), "{}", k.id);
    }
}

#[test]
fn help_keyboard_shortcuts_opens_one_filterable_tab() {
    let mut s = Session::new(JobMode::Inline);
    let tab = |v: serde_json::Value| TabId(v["tab"].as_u64().unwrap());
    let first = tab(s.run("help.keyboard_shortcuts", &json!({})).unwrap());
    let again = tab(s.run("help.keyboard_shortcuts", &json!({})).unwrap());
    assert_eq!(first, again);
    let t = s.workspace.tab(first).unwrap();
    assert_eq!(
        (t.kind.clone(), t.title.as_str()),
        (TabKind::KeyboardShortcuts, "Keyboard shortcuts")
    );
    assert_eq!(t.kind.header_box(), HeaderBox::Filter);
    assert!(!t.kind.has_link_chip());
}

/// A key saved by the user that a default now also has (a newer version's default, or a
/// record written by hand) is listed as a conflict on both bindings, and a key that no
/// longer reads is dropped for the default.
#[test]
fn a_saved_key_that_meets_a_default_is_flagged_on_both_bindings() {
    let t = Temp::new("flagged");
    {
        let (store, _) = obs_store::UserStore::open(&t.db()).unwrap();
        store
            .set_preference(
                KEY_BINDINGS_PREF,
                r#"{"view.text_smaller": "Ctrl+F", "view.text_reset": "Ctrl+Bogus"}"#,
            )
            .unwrap();
    }
    let s = launch(&t.db());
    assert_eq!(s.shortcut("view.text_reset"), Some("Ctrl+0"));
    let conflicts = s.key_conflicts();
    assert_eq!(conflicts.len(), 2, "{conflicts:?}");
    assert_eq!(conflicts["view.text_smaller"].id, "tab.find");
    assert_eq!(conflicts["tab.find"].id, "view.text_smaller");
    assert_eq!(conflicts["tab.find"].key, "Ctrl+F");
    assert!(Session::new(JobMode::Inline).key_conflicts().is_empty());
}
