//! The Custom theme (#224): starting from a built-in theme, changing and resetting colour
//! roles, export and import as a JSON file, refused files, and the theme across a restart.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::*;

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-custom-theme-{name}-{}", std::process::id()));
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

fn rgb(hex: &str) -> Rgb {
    hex.parse().unwrap()
}

#[test]
fn the_themes_have_stable_ids_and_high_contrast_comes_light_and_dark() {
    let ids: Vec<&str> = Theme::ALL.iter().map(|t| t.1).collect();
    assert_eq!(
        ids,
        [
            "system",
            "light",
            "dark",
            "high_contrast_light",
            "high_contrast_dark",
            "custom"
        ]
    );
    assert!(Theme::HighContrastLight.is_high_contrast());
    assert!(Theme::HighContrastDark.is_high_contrast());
    assert!(!Theme::Dark.is_high_contrast());
    let mut s = Session::new(JobMode::Inline);
    s.run("view.theme", &json!({ "value": "high_contrast_dark" }))
        .unwrap();
    assert_eq!(s.appearance.theme, Theme::HighContrastDark);
    assert_eq!(
        crate::choices(&s, "view.theme").1,
        "high_contrast_dark",
        "the View menu shows it chosen"
    );
}

#[test]
fn editing_starts_from_a_built_in_theme_and_makes_the_custom_theme_the_theme() {
    let t = Temp::new("edit");
    let mut s = launch(&t.db());
    assert_eq!(s.custom_theme, CustomTheme::default());
    s.run("theme.start_from", &json!({ "base": "high_contrast_dark" }))
        .unwrap();
    assert_eq!(s.appearance.theme, Theme::Custom);
    assert_eq!(s.custom_theme.base, Theme::HighContrastDark);
    assert!(s.custom_theme.colors.is_empty());

    s.run(
        "theme.set_color",
        &json!({ "role": "lapis", "color": "#FFD000" }),
    )
    .unwrap();
    s.run(
        "theme.set_color",
        &json!({ "role": "shell", "color": "101010" }),
    )
    .unwrap();
    assert_eq!(s.custom_theme.colors[&ThemeRole::Lapis], rgb("#ffd000"));
    assert_eq!(s.custom_theme.colors[&ThemeRole::Shell], rgb("#101010"));

    // No colour puts the base's back.
    s.run("theme.set_color", &json!({ "role": "shell" }))
        .unwrap();
    assert!(!s.custom_theme.colors.contains_key(&ThemeRole::Shell));

    // Starting again drops the changes.
    s.run("theme.start_from", &json!({ "base": "light" }))
        .unwrap();
    assert_eq!(s.custom_theme, CustomTheme::default());

    for (cmd, params, says) in [
        ("theme.start_from", json!({ "base": "system" }), "system"),
        ("theme.start_from", json!({ "base": "custom" }), "custom"),
        (
            "theme.set_color",
            json!({ "role": "paper", "color": "#000000" }),
            "paper",
        ),
        (
            "theme.set_color",
            json!({ "role": "ink", "color": "blue" }),
            "blue",
        ),
    ] {
        let err = s.run(cmd, &params).unwrap_err().to_string();
        assert!(err.contains(says), "{cmd} {params}: {err}");
    }
    assert_eq!(
        s.custom_theme,
        CustomTheme::default(),
        "refusals change nothing"
    );
}

/// Acceptance (#224): a Custom theme round-trips through export and import, and survives a
/// restart.
#[test]
fn a_custom_theme_round_trips_through_export_and_import_and_survives_a_restart() {
    let t = Temp::new("round-trip");
    let file = t.0.join("mine.theme.json");
    let mut s = launch(&t.db());
    s.run("theme.start_from", &json!({ "base": "dark" }))
        .unwrap();
    for (role, color) in [
        ("ink", "#f0e8d8"),
        ("lapis", "#8fb8ff"),
        ("warn", "#ffa080"),
    ] {
        s.run("theme.set_color", &json!({ "role": role, "color": color }))
            .unwrap();
    }
    let made = s.custom_theme.clone();
    let out = s
        .run("theme.export", &json!({ "path": file.to_string_lossy() }))
        .unwrap();
    let written = std::fs::read_to_string(&file).unwrap();
    assert_eq!(out["text"], written);
    let v: serde_json::Value = serde_json::from_str(&written).unwrap();
    assert_eq!(v["format"], THEME_FILE_FORMAT);
    assert_eq!(v["version"], THEME_FILE_VERSION);
    assert_eq!(v["base"], "dark");
    assert_eq!(
        v["colors"],
        json!({ "ink": "#f0e8d8", "lapis": "#8fb8ff", "warn": "#ffa080" })
    );

    // Something else, then the file back.
    s.run(
        "theme.start_from",
        &json!({ "base": "high_contrast_light" }),
    )
    .unwrap();
    s.run("view.theme", &json!({ "value": "light" })).unwrap();
    s.run("theme.import", &json!({ "path": file.to_string_lossy() }))
        .unwrap();
    assert_eq!(s.custom_theme, made);
    assert_eq!(
        s.appearance.theme,
        Theme::Custom,
        "an import is used at once"
    );

    drop(s);
    let s = launch(&t.db());
    assert_eq!(s.custom_theme, made, "after a restart");
    assert_eq!(s.appearance.theme, Theme::Custom);

    // On another computer: a fresh User database imports the same file to the same theme.
    let other = Temp::new("round-trip-other");
    let mut o = launch(&other.db());
    o.run("theme.import", &json!({ "text": written })).unwrap();
    assert_eq!(o.custom_theme, made);
}

#[test]
fn a_file_that_is_not_a_theme_is_refused_and_changes_nothing() {
    let t = Temp::new("refused");
    let mut s = launch(&t.db());
    s.run(
        "theme.set_color",
        &json!({ "role": "ink", "color": "#123456" }),
    )
    .unwrap();
    let before = s.custom_theme.clone();
    for (text, says) in [
        ("{", "not a theme file"),
        (
            r#"{"format":"openbible-highlight-palette","version":1,"base":"light"}"#,
            "openbible-highlight-palette",
        ),
        (
            r#"{"format":"openbible-theme","version":9,"base":"light"}"#,
            "version 9",
        ),
        (
            r#"{"format":"openbible-theme","version":1,"base":"sepia"}"#,
            "sepia",
        ),
        (
            r##"{"format":"openbible-theme","version":1,"base":"light","colors":{"page":"#ffffff"}}"##,
            "page",
        ),
        (
            r##"{"format":"openbible-theme","version":1,"base":"light","colors":{"ink":"#12"}}"##,
            "#12",
        ),
    ] {
        let err = s
            .run("theme.import", &json!({ "text": text }))
            .unwrap_err()
            .to_string();
        assert!(err.contains(says), "{text}: {err}");
        assert_eq!(s.custom_theme, before, "{text}");
    }
    // A saved theme that cannot be read (from a later version, say) launches as the default.
    s.user
        .as_ref()
        .unwrap()
        .set_preference(CUSTOM_THEME_PREF, r#"{"format":"x"}"#)
        .unwrap();
    drop(s);
    assert_eq!(launch(&t.db()).custom_theme, CustomTheme::default());
}

#[test]
fn every_colour_role_has_an_id_and_a_name() {
    let mut ids: Vec<&str> = ThemeRole::ALL.iter().map(|r| r.1).collect();
    for (role, id, label) in ThemeRole::ALL {
        assert_eq!(ThemeRole::from_id(id), Some(role));
        assert_eq!(role.id(), id);
        assert_eq!(role.label(), label);
    }
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), ThemeRole::ALL.len());
}

#[test]
fn the_theme_editor_opens_from_the_view_menu_and_settings() {
    let mut s = Session::new(JobMode::Inline);
    s.run("view.theme_editor", &json!({})).unwrap();
    assert!(
        s.workspace
            .take_effects()
            .contains(&workspace::Effect::ShowThemeEditor)
    );
    let setting = crate::settings::setting_by_id("theme_editor").unwrap();
    assert!(matches!(
        setting.kind,
        SettingKind::Action {
            command: "view.theme_editor",
            ..
        }
    ));
    assert_eq!(
        crate::settings::settings_matching(&s, "high contrast")
            .iter()
            .map(|s| s.id)
            .collect::<Vec<_>>(),
        ["theme", "theme_editor"]
    );
}
