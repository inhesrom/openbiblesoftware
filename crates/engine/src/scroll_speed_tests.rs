//! Scroll speed (#293): one multiplier on every mouse-wheel, trackpad and arrow-key scroll
//! step, set in Settings → Motion or with `view.scroll_speed {value}` (also from the Command
//! box), kept in the User database with the View preferences and never in a Layout.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::*;

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-scroll-speed-{name}-{}", std::process::id()));
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

fn speed(s: &Session) -> f32 {
    s.appearance.scroll_speed
}

#[test]
fn scroll_speed_is_1x_by_default_is_set_by_its_command_and_kept_across_launches() {
    let t = Temp::new("kept");
    let mut s = launch(&t.db());
    assert_eq!(speed(&s), 1.0);
    assert_eq!(s.setting_is_default("scroll_speed"), Some(true));

    let out = s
        .run("view.scroll_speed", &json!({ "value": 2.5 }))
        .unwrap();
    assert_eq!(out["value"], 2.5);
    assert_eq!(speed(&s), 2.5);
    assert_eq!(s.setting_is_default("scroll_speed"), Some(false));
    // Kept to a hundredth.
    s.run("view.scroll_speed", &json!({ "value": 0.333 }))
        .unwrap();
    assert_eq!(speed(&s), 0.33);
    s.run("view.scroll_speed", &json!({ "value": 0.3 }))
        .unwrap();
    drop(s);

    let mut s = launch(&t.db());
    assert_eq!(speed(&s), 0.3, "kept in the User database");
    let saved: serde_json::Value = serde_json::from_str(
        &s.user
            .as_ref()
            .unwrap()
            .preference("view")
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved["scroll_speed"], json!(0.3), "{saved}");

    // Settings → Motion → Reset puts 1× back, saved too.
    s.run("settings.reset", &json!({ "setting": "scroll_speed" }))
        .unwrap();
    assert_eq!(speed(&s), 1.0);
    drop(s);
    assert_eq!(speed(&launch(&t.db())), 1.0);
}

#[test]
fn scroll_speed_outside_a_quarter_to_four_times_is_refused() {
    let mut s = Session::new(JobMode::Inline);
    for value in [json!(0.2), json!(4.5), json!(0), json!(-1)] {
        let e = s
            .run("view.scroll_speed", &json!({ "value": value }))
            .unwrap_err();
        assert!(e.to_string().contains("0.25 to 4"), "{value}: {e}");
    }
    assert!(
        s.run("view.scroll_speed", &json!({ "value": "fast" }))
            .is_err()
    );
    assert!(s.run("view.scroll_speed", &json!({})).is_err());
    assert_eq!(speed(&s), 1.0);
    for edge in [0.25, 4.0] {
        s.run("view.scroll_speed", &json!({ "value": edge }))
            .unwrap();
        assert_eq!(f64::from(speed(&s)), edge);
    }
    // From a command line, `value=1.5` is a number.
    let spec = *s.commands().get("view.scroll_speed").unwrap();
    let params = spec.params_from_pairs(&[("value", "1.5")]).unwrap();
    s.run("view.scroll_speed", &params).unwrap();
    assert_eq!(speed(&s), 1.5);
    assert!(spec.params_from_pairs(&[("value", "fast")]).is_err());
}

#[test]
fn a_saved_scroll_speed_out_of_range_loads_within_it() {
    let t = Temp::new("range");
    {
        let s = launch(&t.db());
        let user = s.user.as_ref().unwrap();
        user.set_preference("view", r#"{"scroll_speed": 12}"#)
            .unwrap();
    }
    assert_eq!(speed(&launch(&t.db())), 4.0);
    {
        let s = launch(&t.db());
        let user = s.user.as_ref().unwrap();
        user.set_preference("view", r#"{"scroll_speed": 0.01, "theme": "dark"}"#)
            .unwrap();
    }
    let s = launch(&t.db());
    assert_eq!(speed(&s), 0.25);
    assert_eq!(s.appearance.theme, Theme::Dark, "the rest still loads");
}

/// Acceptance: the value is not part of a Layout. Saving a Layout does not write it, and
/// restoring one leaves the Scroll speed as it is.
#[test]
fn scroll_speed_is_not_part_of_a_layout() {
    let t = Temp::new("layout");
    let mut s = launch(&t.db());
    s.run("view.scroll_speed", &json!({ "value": 2.0 }))
        .unwrap();
    s.run("layout.save", &json!({ "name": "Study" })).unwrap();
    let saved = s.user.as_ref().unwrap().layout("Study").unwrap().unwrap();
    assert!(!saved.contains("scroll_speed"), "{saved}");
    s.run("view.scroll_speed", &json!({ "value": 0.5 }))
        .unwrap();
    s.run("layout.restore", &json!({ "name": "Study" }))
        .unwrap();
    assert_eq!(speed(&s), 0.5);
}

#[test]
fn scroll_speed_is_in_settings_motion_and_found_by_plain_words() {
    let setting = setting_by_id("scroll_speed").unwrap();
    assert_eq!(setting.section, SettingsSection::Motion);
    assert_eq!(setting.label, "Scroll speed");
    assert!(matches!(setting.kind, SettingKind::ScrollSpeed));
    // Beside Reduce motion.
    let motion: Vec<&str> = (SETTINGS.iter())
        .filter(|s| s.section == SettingsSection::Motion)
        .map(|s| s.id)
        .collect();
    assert_eq!(motion, ["reduce_motion", "scroll_speed"]);
    let s = Session::new(JobMode::Inline);
    for q in [
        "scroll",
        "wheel",
        "speed",
        "trackpad",
        "Scroll speed",
        "mouse wheel",
    ] {
        assert!(
            settings_matching(&s, q)
                .iter()
                .any(|f| f.id == "scroll_speed"),
            "{q:?}"
        );
    }
}

#[test]
fn the_command_box_sets_the_scroll_speed() {
    let t = Temp::new("box");
    let mut s = launch(&t.db());
    for (text, hundredths, label) in [
        ("scroll speed 2", 200, "Scroll speed 2×"),
        ("Scroll Speed 1.5x", 150, "Scroll speed 1.5×"),
        ("scroll speed 0.25×", 25, "Scroll speed 0.25×"),
        ("scroll speed 75%", 75, "Scroll speed 0.75×"),
    ] {
        let top = s.command_box_suggestions(text).remove(0);
        assert_eq!(
            top.action,
            CommandBoxAction::ScrollSpeed(hundredths),
            "{text}"
        );
        assert_eq!(top.label, label);
    }
    for text in ["scroll speed 10", "scroll speed fast", "scroll speed"] {
        assert!(
            (s.command_box_suggestions(text).iter())
                .all(|g| !matches!(g.action, CommandBoxAction::ScrollSpeed(_))),
            "{text}"
        );
    }
    let out = s
        .run("command_box.run", &json!({ "text": "scroll speed 3" }))
        .unwrap();
    assert_eq!(out["kind"], "scroll_speed");
    assert_eq!(speed(&s), 3.0);
}
