//! Key bindings (#177): every binding listed by menu, the user's own keys kept in the User
//! database across launches, conflicts refused or swapped, reserved keys refused.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::key_bindings::{
    KEY_BINDINGS_PREF, KeyStyle, OTHER_GROUP, SELECTION_GROUP, canonical_key, key_binding_groups,
    key_label, not_bindable,
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
    let c = s.key_conflict("view.text_larger", "Alt+1").unwrap();
    assert_eq!(c.id, "selection.highlight_1");
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
    assert!(why.contains("“Find in Resource” cannot take Esc:"), "{why}");
    let err = s
        .run(
            "keys.set",
            &json!({ "command": "word_info.unpin", "key": "Ctrl+F", "swap": true }),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot take Esc: Esc is kept"), "{err}");
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

/// #262: keys read as people write them: key names, not egui's ("Comma", "Plus", "Minus"),
/// on every platform, and Apple's modifier symbols in Apple's order on macOS.
#[test]
fn keys_are_shown_with_key_names_and_on_macos_with_modifier_symbols() {
    for (key, pc, mac) in [
        ("Ctrl+Comma", "Ctrl+,", "⌘,"),
        ("Ctrl+Plus", "Ctrl++", "⌘+"),
        ("Ctrl+Shift+Plus", "Ctrl++", "⌘+"),
        ("Ctrl+Minus", "Ctrl+−", "⌘−"),
        ("Ctrl+0", "Ctrl+0", "⌘0"),
        ("Ctrl+W", "Ctrl+W", "⌘W"),
        ("Ctrl+Shift+N", "Ctrl+Shift+N", "⇧⌘N"),
        ("Ctrl+Shift+Alt+X", "Ctrl+Shift+Alt+X", "⌥⇧⌘X"),
        ("Alt+Left", "Alt+←", "⌥←"),
        ("Alt+1", "Alt+1", "⌥1"),
        ("Ctrl+[", "Ctrl+[", "⌘["),
        ("Ctrl+PageDown", "Ctrl+Page Down", "⌘Page Down"),
        ("Ctrl+Enter", "Ctrl+Enter", "⌘Return"),
        ("Shift+F6", "Shift+F6", "⇧F6"),
        ("F6", "F6", "F6"),
        // Cmd+Tab switches apps on macOS: Ctrl+Tab is the Control key there.
        ("Ctrl+Tab", "Ctrl+Tab", "⌃Tab"),
        ("Ctrl+Shift+Tab", "Ctrl+Shift+Tab", "⌃⇧Tab"),
        ("Escape", "Esc", "Esc"),
    ] {
        assert_eq!(key_label(key, KeyStyle::Pc), pc, "{key}");
        assert_eq!(key_label(key, KeyStyle::Mac), mac, "{key}");
    }
    // Not a key: left as it is.
    assert_eq!(key_label("Ctrl+Wat", KeyStyle::Mac), "Ctrl+Wat");
    // Every default key reads without an egui key name.
    let s = Session::new(JobMode::Inline);
    for b in s.key_bindings() {
        let Some(key) = b.default else { continue };
        for style in [KeyStyle::Pc, KeyStyle::Mac] {
            let label = key_label(key, style);
            for name in [
                "Comma",
                "Plus",
                "Minus",
                "OpenBracket",
                "CloseBracket",
                "Left",
                "Right",
            ] {
                assert!(!label.contains(name), "{} shows {label}", b.id);
            }
        }
        assert!(!key_label(key, KeyStyle::Mac).contains("Ctrl"), "{key}");
    }
}

/// #262: a hint names a command by its real place in the menu bar, and only when it is in a
/// menu.
#[test]
fn menu_path_names_the_menu_a_command_is_in() {
    let s = Session::new(JobMode::Inline);
    assert_eq!(
        s.menu_path("help.keyboard_shortcuts").as_deref(),
        Some("Help → Keyboard shortcuts")
    );
    assert_eq!(
        s.menu_path("app.settings").as_deref(),
        Some("View → Settings")
    );
    assert_eq!(s.menu_path("no.such_command"), None);
    for b in s.key_bindings() {
        let path = s.menu_path(b.id);
        if b.group == OTHER_GROUP || b.group == SELECTION_GROUP {
            assert_eq!(path, None, "{}", b.id);
        } else {
            assert_eq!(path, Some(format!("{} → {}", b.group, b.label)), "{}", b.id);
        }
    }
}

/// #262 (WORKSPACE-MAC-9) found List commands doing nothing from Help or its key. It is All
/// commands now (#226): from Help or Ctrl+Shift+P it lists the commands in the Command box,
/// and it still answers scripts with the JSON list.
#[test]
fn all_commands_shows_the_list_and_answers_scripts() {
    let mut s = Session::new(JobMode::Inline);
    assert_eq!(
        s.menu_path("app.commands").as_deref(),
        Some("Help → All commands")
    );
    assert_eq!(s.shortcut("app.commands"), Some("Ctrl+Shift+P"));
    s.workspace.take_effects();
    assert!(s.run("app.commands", &json!({})).unwrap().is_array());
    assert!((s.workspace.take_effects()).contains(&crate::workspace::Effect::ListCommands));
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
    assert_eq!(group("selection.highlight_4"), Some(SELECTION_GROUP));
    assert_eq!(group("tab.find"), Some("Edit"));
    assert_eq!(group("toolbar.library"), Some(OTHER_GROUP));
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

/// The Highlight Selection keys follow the active palette's order (#181): Alt+1…9 by
/// default, and a key saved for one of the five v0.1 styles' old ids applies to its slot.
#[test]
fn highlight_keys_are_nine_palette_slots_and_old_saved_keys_carry_over() {
    let defaults: Vec<(&str, SelectionAction)> = SELECTION_KEYS
        .iter()
        .skip(1)
        .map(|k| (k.default, k.action))
        .collect();
    let want: Vec<(String, SelectionAction)> = (0..STYLE_SLOTS)
        .map(|i| (format!("Alt+{}", i + 1), SelectionAction::Highlight(i)))
        .collect();
    assert_eq!(
        defaults
            .iter()
            .map(|(k, a)| ((*k).to_string(), *a))
            .collect::<Vec<_>>(),
        want
    );
    let t = Temp::new("renamed");
    {
        let (store, _) = obs_store::UserStore::open(&t.db()).unwrap();
        store
            .set_preference(
                KEY_BINDINGS_PREF,
                r#"{"selection.highlight_green": "F7", "selection.highlight_red_underline": null}"#,
            )
            .unwrap();
    }
    let s = launch(&t.db());
    assert_eq!(s.shortcut("selection.highlight_2"), Some("F7"));
    assert_eq!(s.shortcut("selection.highlight_5"), None);
    assert_eq!(s.shortcut("selection.highlight_1"), Some("Alt+1"));
    assert_eq!(s.shortcut("selection.highlight_9"), Some("Alt+9"));
    assert!(s.key_conflicts().is_empty());
}
