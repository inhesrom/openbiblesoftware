#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]
//! Favorites and History in the User database (#155).

use std::path::PathBuf;

use obs_store::{HISTORY_DAYS, HISTORY_LIMIT, USER_SCHEMA_VERSION, UserStore};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-favorites-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("user.db")
}

fn now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}

#[test]
fn favorites_survive_a_reopen_in_their_folders() {
    let path = tmp("folders");
    {
        let (db, _) = UserStore::open(&path).unwrap();
        let a = db
            .add_favorite("", "Romans 8:28 · BSB", "{\"a\":1}")
            .unwrap();
        db.add_favorite("Sermon", "G26 · Word Study", "{\"b\":2}")
            .unwrap();
        // Folder names ignore case: the folder keeps the spelling it was made with.
        let c = db
            .add_favorite("sermon", "John 3:16 · BSB", "{\"c\":3}")
            .unwrap();
        db.add_favorite("Advent", "Isaiah 9:6 · BSB", "{\"d\":4}")
            .unwrap();
        assert!(db.move_favorite(a, "Advent").unwrap());
        assert!(db.remove_favorite(c).unwrap());
        assert!(!db.remove_favorite(c).unwrap());
        assert!(!db.move_favorite(c, "").unwrap());
    }
    let (db, _) = UserStore::open(&path).unwrap();
    let rows: Vec<(String, String, String)> = db
        .favorites()
        .unwrap()
        .into_iter()
        .map(|f| (f.folder, f.label, f.location))
        .collect();
    assert_eq!(
        rows,
        [
            (
                "Advent".into(),
                "Isaiah 9:6 · BSB".into(),
                "{\"d\":4}".into()
            ),
            (
                "Advent".into(),
                "Romans 8:28 · BSB".into(),
                "{\"a\":1}".into()
            ),
            (
                "Sermon".into(),
                "G26 · Word Study".into(),
                "{\"b\":2}".into()
            ),
        ]
    );
}

#[test]
fn history_is_newest_first_with_its_local_day_and_can_be_cleared() {
    let (db, _) = UserStore::open(&tmp("history")).unwrap();
    let t = now();
    db.record_visit_at("Genesis 1:1 · BSB", "1", t - 86_400)
        .unwrap();
    db.record_visit_at("Romans 8:28 · BSB", "2", t).unwrap();
    db.record_visit_at("John 3:16 · BSB", "3", t).unwrap();
    let h = db.history().unwrap();
    let labels: Vec<&str> = h.iter().map(|v| v.label.as_str()).collect();
    assert_eq!(
        labels,
        ["John 3:16 · BSB", "Romans 8:28 · BSB", "Genesis 1:1 · BSB"]
    );
    assert_eq!(h[0].days_ago, 0);
    assert_eq!(h[2].days_ago, 1);
    assert_eq!(h[0].day.len(), "2026-11-05".len());
    assert_eq!(h[0].time.len(), "14:05".len());
    assert_ne!(h[0].day, h[2].day);
    db.clear_history().unwrap();
    assert!(db.history().unwrap().is_empty());
}

#[test]
fn history_is_capped_by_age_and_by_count() {
    let (db, _) = UserStore::open(&tmp("cap")).unwrap();
    let t = now();
    db.record_visit_at("old", "0", t - (HISTORY_DAYS + 1) * 86_400)
        .unwrap();
    db.record_visit_at("kept", "1", t - (HISTORY_DAYS - 1) * 86_400)
        .unwrap();
    db.record_visit_at("new", "2", t).unwrap();
    let labels: Vec<String> = db.history().unwrap().into_iter().map(|v| v.label).collect();
    assert_eq!(labels, ["new", "kept"]);

    for i in 0..HISTORY_LIMIT + 3 {
        db.record_visit_at(&format!("v{i}"), "x", t + i).unwrap();
    }
    let h = db.history().unwrap();
    assert_eq!(h.len(), usize::try_from(HISTORY_LIMIT).unwrap());
    assert_eq!(h[0].label, format!("v{}", HISTORY_LIMIT + 2));
}

#[test]
fn a_v6_database_gains_empty_favorites_and_history() {
    let path = tmp("migrate");
    {
        let (v6, _) = UserStore::open_to(&path, 6).unwrap();
        v6.save_layout("Study", "{}").unwrap();
    }
    let (db, report) = UserStore::open(&path).unwrap();
    assert_eq!(report.found_version, 6);
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    assert_eq!(db.layouts().unwrap(), ["Study"]);
    assert!(db.favorites().unwrap().is_empty());
    assert!(db.history().unwrap().is_empty());
}
