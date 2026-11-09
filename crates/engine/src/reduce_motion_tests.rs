//! Reduce motion (#225): the in-app toggle, View → Reduce motion and its Setting, kept in the
//! User database like the rest of the View preferences.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::json;

use crate::*;

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn session(root: &std::path::Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    std::fs::create_dir_all(root).unwrap();
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    s
}

#[test]
fn reduce_motion_is_off_by_default_toggles_and_is_kept_across_launches() {
    let root = std::env::temp_dir().join(format!("obs-reduce-motion-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _t = Temp(root.clone());
    let mut s = session(&root);
    assert!(!s.appearance.reduce_motion);
    assert_eq!(s.setting_is_default("reduce_motion"), Some(true));

    // Without `on` it toggles; with it, it sets.
    let out = s.run("view.reduce_motion", &json!({})).unwrap();
    assert_eq!(out["on"], true);
    assert!(s.appearance.reduce_motion);
    s.run("view.reduce_motion", &json!({ "on": true })).unwrap();
    assert!(s.appearance.reduce_motion);
    assert_eq!(s.setting_is_default("reduce_motion"), Some(false));
    drop(s);

    let mut s = session(&root);
    assert!(s.appearance.reduce_motion, "kept in the User database");
    s.run("settings.reset", &json!({ "setting": "reduce_motion" }))
        .unwrap();
    assert!(!s.appearance.reduce_motion);
    drop(s);
    assert!(!session(&root).appearance.reduce_motion);
}

#[test]
fn reduce_motion_is_in_the_view_menu_and_its_own_settings_section() {
    assert!(
        MENUS
            .iter()
            .find(|m| m.title == "View")
            .unwrap()
            .commands()
            .contains(&"view.reduce_motion")
    );
    let setting = SETTINGS.iter().find(|s| s.id == "reduce_motion").unwrap();
    assert_eq!(setting.section, SettingsSection::Motion);
    assert_eq!(setting.section.title(), "Motion");
    let s = Session::new(JobMode::Inline);
    for q in ["reduce motion", "animation", "accessibility"] {
        assert!(
            settings_matching(&s, q)
                .iter()
                .any(|f| f.id == "reduce_motion"),
            "{q:?}"
        );
    }
}
