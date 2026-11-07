//! The default reading Text size and the readable range a saved one is kept in (#241).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::appearance::{TEXT_SIZE_DEFAULT, TEXT_SIZE_MAX, TEXT_SIZE_MIN};
use crate::*;

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-text-size-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn db(&self) -> PathBuf {
        self.0.join("user.db")
    }

    /// A User database whose View preferences are `view`, as an older build saved them.
    fn with_view(name: &str, view: &Value) -> Self {
        let t = Self::new(name);
        let (store, _) = obs_store::UserStore::open(&t.db()).unwrap();
        store.set_preference("view", &view.to_string()).unwrap();
        t
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

fn messages(s: &mut Session) -> Vec<String> {
    s.workspace
        .take_effects()
        .into_iter()
        .filter_map(|e| match e {
            Effect::Message(m) => Some(m),
            _ => None,
        })
        .collect()
}

/// A fresh install reads at 20 points, shared by Bibles and every other Resource, and the
/// readable range brackets it.
#[test]
fn the_default_reading_size_is_twenty_points_inside_the_readable_range() {
    assert_eq!(TEXT_SIZE_DEFAULT, 20.0);
    assert_eq!(obs_layout::TextSettings::default().size, TEXT_SIZE_DEFAULT);
    assert_eq!(Session::new(JobMode::Inline).text.size, TEXT_SIZE_DEFAULT);
    // Inside the range, whose minimum is readable: an older build's bottom step (10) is not.
    const {
        assert!(TEXT_SIZE_MIN <= TEXT_SIZE_DEFAULT && TEXT_SIZE_DEFAULT <= TEXT_SIZE_MAX);
        assert!(TEXT_SIZE_MIN >= 12.0);
    }
}

/// A User database with size 9 loads at the default, with a notice shown once: the
/// reset is saved, so the next launch is quiet.
#[test]
fn a_tiny_saved_text_size_loads_at_the_default_with_a_one_time_notice() {
    let t = Temp::with_view("tiny", &json!({ "theme": "dark", "text_size": 9.0 }));
    let mut s = launch(&t.db());
    assert_eq!(s.text.size, TEXT_SIZE_DEFAULT);
    assert_eq!(
        s.appearance.theme,
        Theme::Dark,
        "the rest of the record is kept"
    );
    let notices = messages(&mut s);
    assert_eq!(
        notices,
        [format!(
            "Text size 9 pt was too small to read; it is back to the default {TEXT_SIZE_DEFAULT} pt"
        )]
    );
    drop(s);

    let mut s = launch(&t.db());
    assert_eq!(s.text.size, TEXT_SIZE_DEFAULT);
    assert_eq!(messages(&mut s), Vec::<String>::new(), "the notice is once");
}

/// Every size below the readable minimum (an older build let Smaller text reach 10) is reset;
/// the minimum itself and larger sizes are the user's and are kept; a huge one is clamped.
#[test]
fn saved_text_sizes_are_kept_in_the_readable_range() {
    for (saved, loaded, notice) in [
        (1.0, TEXT_SIZE_DEFAULT, true),
        (10.0, TEXT_SIZE_DEFAULT, true),
        (TEXT_SIZE_MIN - 0.5, TEXT_SIZE_DEFAULT, true),
        (TEXT_SIZE_MIN, TEXT_SIZE_MIN, false),
        (24.0, 24.0, false),
        (400.0, TEXT_SIZE_MAX, false),
    ] {
        let t = Temp::with_view(
            &format!("range-{saved}"),
            &json!({ "text_size": saved, "text_size_default": TEXT_SIZE_DEFAULT }),
        );
        let mut s = launch(&t.db());
        assert_eq!(s.text.size, loaded, "saved {saved}");
        assert_eq!(!messages(&mut s).is_empty(), notice, "saved {saved}");
    }
}

/// Smaller text stops at the readable minimum, so a size the user picks is never one the
/// next launch resets.
#[test]
fn smaller_text_stops_at_the_readable_minimum() {
    let t = Temp::new("smaller");
    let mut s = launch(&t.db());
    for _ in 0..60 {
        s.run("view.text_smaller", &json!({})).unwrap();
    }
    assert_eq!(s.text.size, TEXT_SIZE_MIN);
    drop(s);
    let mut s = launch(&t.db());
    assert_eq!(s.text.size, TEXT_SIZE_MIN);
    assert_eq!(messages(&mut s), Vec::<String>::new());
}

/// Any View change saved the whole record, so most older User databases hold the old default
/// (17) without anyone choosing it: it follows the new default. A size saved with the default
/// it was chosen under is the user's.
#[test]
fn a_size_saved_at_the_old_default_follows_the_new_default() {
    let t = Temp::with_view(
        "old-default",
        &json!({ "theme": "dark", "text_size": 17.0 }),
    );
    let mut s = launch(&t.db());
    assert_eq!(s.text.size, TEXT_SIZE_DEFAULT);
    assert_eq!(messages(&mut s), Vec::<String>::new());

    let t = Temp::with_view("old-larger", &json!({ "text_size": 18.0 }));
    assert_eq!(launch(&t.db()).text.size, 18.0);

    let t = Temp::new("chosen-17");
    let mut s = launch(&t.db());
    for _ in 0..3 {
        s.run("view.text_smaller", &json!({})).unwrap();
    }
    assert_eq!(s.text.size, 17.0);
    drop(s);
    assert_eq!(
        launch(&t.db()).text.size,
        17.0,
        "chosen under the new default"
    );
}

/// Ctrl+0 (Reset text size) and the Settings Tab's Reset both go to the new default.
#[test]
fn reset_text_size_and_the_settings_reset_use_the_default() {
    let t = Temp::new("reset");
    let mut s = launch(&t.db());
    s.run("view.text_larger", &json!({})).unwrap();
    s.run("view.text_reset", &json!({})).unwrap();
    assert_eq!(s.text.size, TEXT_SIZE_DEFAULT);
    s.run("view.text_smaller", &json!({})).unwrap();
    assert_eq!(s.setting_is_default("text_size"), Some(false));
    s.run("settings.reset", &json!({ "setting": "text_size" }))
        .unwrap();
    assert_eq!(s.text.size, TEXT_SIZE_DEFAULT);
    assert_eq!(s.setting_is_default("text_size"), Some(true));
}
