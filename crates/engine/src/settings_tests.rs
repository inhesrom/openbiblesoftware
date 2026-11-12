//! The Settings Tab's engine side (#213): every preference is a Setting the search finds, each
//! Setting's Reset restores its default through the existing commands (and the default is
//! saved), and Ctrl+, opens the one Settings Tab.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::settings::{SETTINGS, SettingKind, SettingsSection, setting_by_id, settings_matching};
use crate::*;

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-settings-{name}-{}", std::process::id()));
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

/// The commands a Setting is read and written through.
fn commands_of(kind: SettingKind) -> Vec<&'static str> {
    match kind {
        SettingKind::Choice { command, .. }
        | SettingKind::Toggle { command, .. }
        | SettingKind::Action { command, .. } => vec![command],
        SettingKind::TextSize => vec!["view.text_larger", "view.text_smaller", "view.text_reset"],
        SettingKind::ScrollSpeed => vec!["view.scroll_speed"],
        SettingKind::Font(_) => vec!["view.fonts", "view.font"],
        SettingKind::NotebooksInText => vec!["notebook.show_in_text"],
        SettingKind::HomeCards => vec!["home.reset_cards"],
        SettingKind::GuideSections => vec!["guide.reset_arrangement"],
        SettingKind::ToolbarLaunchers => vec![
            "view.toolbar_customize",
            "toolbar.show_launcher",
            "toolbar.move_launcher",
        ],
        SettingKind::KeyboardShortcuts => vec![
            "help.keyboard_shortcuts",
            "keys.set",
            "keys.clear",
            "keys.reset",
            "keys.reset_all",
        ],
        SettingKind::LibraryFolder => vec![],
    }
}

fn covered() -> BTreeSet<&'static str> {
    SETTINGS.iter().flat_map(|s| commands_of(s.kind)).collect()
}

/// Acceptance: every existing preference is in Settings. Every View menu item (its choices,
/// toggles, text size keys, Fonts, Notebooks in text and the toolbar's), Help → Keyboard
/// shortcuts, and the preferences other Tabs set (Open Home at launch, Home cards, Guide
/// sections, toolbar labels, Word forms) are each a Setting, through the same command.
#[test]
fn every_preference_command_is_a_setting() {
    let covered = covered();
    let mut wanted: Vec<&str> = Vec::new();
    let view = MENUS.iter().find(|m| m.title == "View").unwrap();
    for item in view.items {
        match item {
            MenuItem::Command("app.settings") | MenuItem::Separator => {}
            // A Tab's own state, kept in Layouts with the Tab (#208), not a preference.
            MenuItem::Command("view.footnote_markers") => {}
            // Also a Tab's own choice (#210).
            MenuItem::Choice {
                command: "view.note_display",
                ..
            } => {}
            MenuItem::Command(id) | MenuItem::Choice { command: id, .. } => wanted.push(id),
            MenuItem::Notebooks => wanted.push("notebook.show_in_text"),
            // A Tab's own choices; their defaults are Settings.
            MenuItem::ReadingLayout => wanted.extend([
                "view.text_flow",
                "view.large_chapter_numbers",
                "view.section_headings",
                "view.column_width",
                "view.justified",
            ]),
            other => panic!("the View menu has {other:?}: give it a Setting"),
        }
    }
    wanted.extend([
        "help.keyboard_shortcuts",
        "home.open_at_launch",
        "home.reset_cards",
        "guide.reset_arrangement",
        "toolbar.labels",
        "view.toolbar_expanded",
        "toolbar.show_launcher",
        "toolbar.move_launcher",
        "search.word_forms",
        "history.clear",
        "search.clear_history",
        "data.backups",
        // Settings → Updates (#276).
        "updates.auto_check",
        "updates.prereleases",
        "help.check_for_updates",
    ]);
    let missing: Vec<&&str> = wanted.iter().filter(|c| !covered.contains(**c)).collect();
    assert!(missing.is_empty(), "no Setting for {missing:?}");
    let s = Session::new(JobMode::Inline);
    for c in &covered {
        assert!(s.commands().get(c).is_some(), "{c} is not a command");
    }
}

/// Every field of the stored View and toolbar preferences has its Setting, so a preference
/// added to them later breaks this test until Settings shows it.
#[test]
fn every_stored_view_and_toolbar_field_has_a_setting() {
    let s = Session::new(JobMode::Inline);
    let view = crate::appearance::to_json(&s.appearance, &s.text, &s.fonts);
    let by_field = |field: &str| -> Vec<&str> {
        match field {
            "theme" => vec!["theme"],
            "page_tone" => vec!["page_tone"],
            "interface_size" => vec!["interface_size"],
            // The default a size was saved under is part of the Text size (#241).
            "text_size" | "text_size_default" => vec!["text_size"],
            "line_spacing" => vec!["line_spacing"],
            "red_letter" => vec!["red_letter"],
            "verse_numbers" => vec!["verse_numbers"],
            "highlights" => vec!["highlights"],
            "reduce_motion" => vec!["reduce_motion"],
            "scroll_speed" => vec!["scroll_speed"],
            "reading_layout" => vec![
                "text_flow",
                "large_chapter_numbers",
                "section_headings",
                "column_width",
                "justified",
            ],
            "fonts" => vec!["font_latin", "font_greek", "font_hebrew", "font_interface"],
            "position" => vec!["toolbar_position"],
            "auto_hide" => vec!["toolbar_auto_hide"],
            "expanded" => vec!["toolbar_expanded"],
            "labels" => vec!["toolbar_labels"],
            "launchers" => vec!["toolbar_launchers"],
            other => panic!("the stored field {other:?} has no Setting"),
        }
    };
    let toolbar = s.toolbar.to_json();
    for v in [view, toolbar] {
        for field in v.as_object().unwrap().keys() {
            for id in by_field(field) {
                assert!(setting_by_id(id).is_some(), "{field}: no Setting {id}");
            }
        }
    }
    assert_eq!(FontSlot::ALL.len(), 4);
}

/// Acceptance: every Setting can be found from the search by its label, in any case, and by
/// plain words for it.
#[test]
fn search_finds_every_setting_by_its_label_and_plain_words() {
    let s = Session::new(JobMode::Inline);
    for setting in SETTINGS {
        for query in [setting.label.to_string(), setting.label.to_uppercase()] {
            let found = settings_matching(&s, &query);
            assert!(
                found.iter().any(|f| f.id == setting.id),
                "{query:?} does not find {}",
                setting.id
            );
        }
        // Its section's title finds it too.
        assert!(
            settings_matching(&s, setting.section.title())
                .iter()
                .any(|f| f.id == setting.id)
        );
    }
    let ids = |q: &str| -> Vec<&str> { settings_matching(&s, q).iter().map(|f| f.id).collect() };
    assert_eq!(ids("sepia"), ["page_tone"]);
    assert_eq!(ids("dark mode"), ["theme"]);
    assert!(ids("dark").contains(&"theme") && ids("dark").contains(&"page_tone"));
    assert!(ids("zoom").contains(&"interface_size"));
    assert_eq!(ids("hotkeys"), ["keyboard_shortcuts"]);
    assert_eq!(ids("greek font"), ["font_greek"]);
    assert!(ids("privacy").contains(&"clear_history"));
    assert_eq!(ids("backup"), ["back_up_and_restore"]);
    assert!(ids("export").contains(&"back_up_and_restore"));
    assert_eq!(ids("superscript"), ["verse_numbers"]);
    assert!(ids("no such setting anywhere").is_empty());
    assert_eq!(settings_matching(&s, "").len(), SETTINGS.len());
    assert_eq!(settings_matching(&s, "   ").len(), SETTINGS.len());
}

/// Every section of the Tab has a Setting, ids are unique, and Settings are in section order.
#[test]
fn settings_are_grouped_in_section_order_with_unique_ids() {
    let ids: BTreeSet<&str> = SETTINGS.iter().map(|s| s.id).collect();
    assert_eq!(ids.len(), SETTINGS.len());
    let mut last = 0;
    for setting in SETTINGS {
        let i = (SettingsSection::ALL.iter())
            .position(|s| *s == setting.section)
            .unwrap();
        assert!(i >= last, "{} is out of section order", setting.id);
        last = i;
    }
    for section in SettingsSection::ALL {
        assert!(SETTINGS.iter().any(|s| s.section == section), "{section:?}");
    }
}

/// The defaults Settings resets to are the app's own defaults.
#[test]
fn choice_defaults_are_the_app_defaults() {
    let t = Temp::new("defaults");
    let mut s = launch(&t.db());
    for setting in SETTINGS {
        if let SettingKind::Choice { command, default } = setting.kind {
            let (choices, current) = choices(&s, command);
            assert_eq!(current, default, "{}", setting.id);
            assert!(choices.iter().any(|c| c.0 == default), "{}", setting.id);
        }
        if setting.has_reset() {
            assert_eq!(
                s.setting_is_default(setting.id),
                Some(true),
                "{}",
                setting.id
            );
        } else {
            assert_eq!(s.setting_is_default(setting.id), None, "{}", setting.id);
        }
    }
}

/// Change `id` away from its default through the command the Settings Tab runs for it.
pub(crate) fn change(s: &mut Session, id: &str) {
    let setting = setting_by_id(id).unwrap();
    let ok = |r: Result<Value, CommandError>| {
        r.unwrap_or_else(|e| panic!("{id}: {e}"));
    };
    match setting.kind {
        SettingKind::Choice { command, default } => {
            let (choices, _) = choices(s, command);
            let other = choices.iter().find(|c| c.0 != default).unwrap().0;
            ok(s.run(command, &json!({ "value": other })));
        }
        SettingKind::Toggle {
            command,
            param,
            default,
            ..
        } => match param {
            Some(name) => {
                let mut params = serde_json::Map::new();
                params.insert(name.into(), Value::Bool(!default));
                ok(s.run(command, &Value::Object(params)));
            }
            None => ok(s.run(command, &json!({}))),
        },
        SettingKind::TextSize => ok(s.run("view.text_larger", &json!({}))),
        SettingKind::ScrollSpeed => ok(s.run("view.scroll_speed", &json!({ "value": 2.5 }))),
        SettingKind::Font(slot) => ok(s.run(
            "view.font",
            &json!({ "slot": slot.id(), "family": "Charis SIL" }),
        )),
        SettingKind::NotebooksInText => {
            let n = s.default_notebook().unwrap();
            ok(s.run(
                "notebook.show_in_text",
                &json!({ "id": n.id, "show": false }),
            ));
        }
        SettingKind::HomeCards => {
            ok(s.run("home.hide_card", &json!({ "card": "history_favorites" })))
        }
        SettingKind::GuideSections => ok(s.run(
            "guide.hide_section",
            &json!({ "guide": "passage_guide", "section": "commentaries" }),
        )),
        SettingKind::ToolbarLaunchers => {
            ok(s.run(
                "toolbar.show_launcher",
                &json!({ "launcher": "guides", "shown": false }),
            ));
            ok(s.run(
                "toolbar.move_launcher",
                &json!({ "launcher": "layouts", "to": 0 }),
            ));
        }
        SettingKind::KeyboardShortcuts => ok(s.run(
            "keys.set",
            &json!({ "command": "view.text_larger", "key": "Ctrl+Up" }),
        )),
        SettingKind::LibraryFolder | SettingKind::Action { .. } => {}
    }
}

/// Acceptance: Reset restores each default, saved like any change, and leaves the other
/// Settings as they were.
#[test]
fn reset_restores_each_default_and_it_survives_a_restart() {
    let t = Temp::new("reset");
    let resettable: Vec<&str> = (SETTINGS.iter())
        .filter(|s| s.has_reset())
        .map(|s| s.id)
        .collect();
    {
        let mut s = launch(&t.db());
        for id in &resettable {
            change(&mut s, id);
            assert_eq!(s.setting_is_default(id), Some(false), "{id} did not change");
        }
    }
    let mut s = launch(&t.db());
    for id in &resettable {
        assert_eq!(s.setting_is_default(id), Some(false), "{id} was not saved");
    }
    for (i, id) in resettable.iter().enumerate() {
        let out = s.run("settings.reset", &json!({ "setting": id })).unwrap();
        assert_eq!(out["setting"], *id);
        assert_eq!(s.setting_is_default(id), Some(true), "{id} was not reset");
        // The ones not reset yet keep their values.
        for later in &resettable[i + 1..] {
            assert_eq!(
                s.setting_is_default(later),
                Some(false),
                "{id} reset {later}"
            );
        }
    }
    drop(s);
    let mut s = launch(&t.db());
    for id in &resettable {
        assert_eq!(
            s.setting_is_default(id),
            Some(true),
            "{id}'s reset was not saved"
        );
    }
    assert_eq!(s.text.size, obs_layout::TextSettings::default().size);
    assert_eq!(s.appearance, Appearance::default());
    assert_eq!(s.toolbar, ToolbarSettings::default());
    assert_eq!(s.shortcut("view.text_larger"), Some("Ctrl+Plus"));
}

/// Resetting a Setting already at its default changes nothing (a toggle is not flipped).
#[test]
fn resetting_a_default_setting_keeps_it() {
    let t = Temp::new("keep");
    let mut s = launch(&t.db());
    for setting in SETTINGS.iter().filter(|s| s.has_reset()) {
        s.run("settings.reset", &json!({ "setting": setting.id }))
            .unwrap_or_else(|e| panic!("{}: {e}", setting.id));
        assert_eq!(
            s.setting_is_default(setting.id),
            Some(true),
            "{}",
            setting.id
        );
    }
}

/// Show in text's Reset reads the Notebooks: when that read fails (or there is no User
/// database) the Reset is an error, not a success that restored nothing, and the Setting is
/// not taken for its default.
#[test]
fn a_failed_notebook_read_is_not_a_default_or_a_reset() {
    let t = Temp::new("notebook-read");
    {
        let mut s = launch(&t.db());
        change(&mut s, "notebooks_in_text");
    }
    let (store, _) = obs_store::UserStore::open(&t.db()).unwrap();
    let mut s = Session::new(JobMode::Inline);
    s.attach_user(Box::new(crate::testing::WritesFail::notebook_reads(store)));
    assert_eq!(s.setting_is_default("notebooks_in_text"), None);
    let err = s
        .run("settings.reset", &json!({ "setting": "notebooks_in_text" }))
        .unwrap_err();
    assert!(err.to_string().contains("disk I/O error"), "{err}");
    drop(s);
    // Nothing was restored behind the error.
    assert_eq!(
        launch(&t.db()).setting_is_default("notebooks_in_text"),
        Some(false)
    );
    let mut none = Session::new(JobMode::Inline);
    assert_eq!(none.setting_is_default("notebooks_in_text"), None);
    let err = none
        .run("settings.reset", &json!({ "setting": "notebooks_in_text" }))
        .unwrap_err();
    assert!(err.to_string().contains("no User database"), "{err}");
}

#[test]
fn reset_refuses_unknown_settings_and_actions() {
    let mut s = Session::new(JobMode::Inline);
    let err = s
        .run("settings.reset", &json!({ "setting": "nope" }))
        .unwrap_err();
    assert!(err.to_string().contains("nope"), "{err}");
    for id in ["clear_history", "library_folder", "get_resources"] {
        assert!(!setting_by_id(id).unwrap().has_reset());
        let err = s
            .run("settings.reset", &json!({ "setting": id }))
            .unwrap_err();
        assert!(err.to_string().contains("no default"), "{err}");
    }
}

/// Acceptance: Ctrl+, opens Settings, one Tab however often, with the search box in its
/// header; it is in the View menu.
#[test]
fn ctrl_comma_opens_the_one_settings_tab() {
    let mut s = Session::new(JobMode::Inline);
    assert_eq!(s.shortcut("app.settings"), Some("Ctrl+Comma"));
    let view = MENUS.iter().find(|m| m.title == "View").unwrap();
    assert_eq!(view.items[0], MenuItem::Command("app.settings"));
    let a = s.run("app.settings", &json!({})).unwrap()["tab"].as_u64();
    let b = s.run("app.settings", &json!({})).unwrap()["tab"].as_u64();
    assert_eq!(a, b);
    let tab = s.workspace.tab(TabId(a.unwrap())).unwrap();
    assert_eq!(tab.kind, TabKind::Settings);
    assert_eq!(tab.title, "Settings");
    assert_eq!(tab.header_box(), HeaderBox::Filter);
    assert!(!tab.kind.has_link_chip());
    assert_eq!(
        (s.workspace.tabs())
            .filter(|t| t.kind == TabKind::Settings)
            .count(),
        1
    );
}

/// Word forms is set from Settings, without a Search Tab, and saved.
#[test]
fn word_forms_changes_without_a_search_tab() {
    let t = Temp::new("word-forms");
    let mut s = launch(&t.db());
    assert!(!s.search_word_forms());
    let out = s
        .run("search.word_forms", &json!({ "value": true }))
        .unwrap();
    assert_eq!(out["tab"], Value::Null);
    assert!(s.search_word_forms());
    drop(s);
    assert!(launch(&t.db()).search_word_forms());
}
