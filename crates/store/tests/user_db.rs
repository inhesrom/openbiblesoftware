#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::path::{Path, PathBuf};

use obs_store::{ROLLING_BACKUPS, USER_SCHEMA_VERSION, UserStore};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-user-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn backups(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir.join("backups"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn a_new_database_is_at_the_current_version() {
    let dir = tmp("new");
    let (db, report) = UserStore::open(&dir.join("user.db")).unwrap();
    assert_eq!(report.found_version, 0);
    assert!(report.migration_backup.is_none());
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    db.set_preference("text.size", "18").unwrap();
    db.set_preference("text.size", "19").unwrap();
    assert_eq!(db.preference("text.size").unwrap().as_deref(), Some("19"));
    assert_eq!(db.preference("nope").unwrap(), None);
    assert_eq!(db.last_session().unwrap(), None);
    db.save_last_session("{}").unwrap();
    db.save_last_session("{\"x\":1}").unwrap();
    assert_eq!(db.last_session().unwrap().as_deref(), Some("{\"x\":1}"));
}

#[test]
fn a_migration_upgrades_a_v1_database_and_leaves_a_backup() {
    let dir = tmp("migrate");
    let path = dir.join("user.db");
    {
        let (v1, _) = UserStore::open_to(&path, 1).unwrap();
        assert_eq!(v1.schema_version().unwrap(), 1);
        v1.set_preference("theme", "dark").unwrap();
        v1.save_last_session("{\"v\":1}").unwrap();
    }
    let (db, report) = UserStore::open(&path).unwrap();
    assert_eq!(report.found_version, 1);
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    let backup = report.migration_backup.unwrap();
    assert!(backup.is_file(), "{}", backup.display());
    assert!(backup.to_string_lossy().contains(".v1-"));
    // The data survived, and the new table works.
    assert_eq!(db.preference("theme").unwrap().as_deref(), Some("dark"));
    assert_eq!(db.last_session().unwrap().as_deref(), Some("{\"v\":1}"));
    db.save_layout("Study", "{}").unwrap();
    // The backup is the v1 database as it was.
    let (old, _) = UserStore::open_to(&backup, 1).unwrap();
    assert_eq!(old.schema_version().unwrap(), 1);
    assert_eq!(old.preference("theme").unwrap().as_deref(), Some("dark"));
}

#[test]
fn six_launches_leave_exactly_five_rolling_backups() {
    let dir = tmp("rolling");
    let path = dir.join("user.db");
    for i in 0..6 {
        let (db, report) = UserStore::open(&path).unwrap();
        db.set_preference("launch", &i.to_string()).unwrap();
        assert!(report.launch_backup.unwrap().is_file());
    }
    let b = backups(&dir);
    assert_eq!(b.len(), ROLLING_BACKUPS, "{b:?}");
    // The oldest was dropped; the newest is the sixth launch's.
    assert_eq!(b.first().map(String::as_str), Some("user-000002.db"));
    assert_eq!(b.last().map(String::as_str), Some("user-000006.db"));
    // Backups are complete databases, taken at launch (before that launch's write).
    let copy = dir.join("backups").join("user-000006.db");
    let (copy, _) = UserStore::open_to(&copy, USER_SCHEMA_VERSION).unwrap();
    assert_eq!(copy.preference("launch").unwrap().as_deref(), Some("4"));
}

#[test]
fn layouts_round_trip_rename_and_delete() {
    let dir = tmp("layouts");
    let (db, _) = UserStore::open(&dir.join("user.db")).unwrap();
    db.save_layout("Study desk", "{\"a\":1}").unwrap();
    db.save_layout("Reading", "{\"b\":2}").unwrap();
    assert_eq!(db.layouts().unwrap(), ["Reading", "Study desk"]);
    assert_eq!(
        db.layout("study DESK").unwrap().as_deref(),
        Some("{\"a\":1}")
    );
    // Saving under the same name replaces it.
    db.save_layout("Reading", "{\"b\":3}").unwrap();
    assert_eq!(db.layout("Reading").unwrap().as_deref(), Some("{\"b\":3}"));
    assert!(db.rename_layout("Reading", "Morning").unwrap());
    assert_eq!(db.layouts().unwrap(), ["Morning", "Study desk"]);
    // Renaming onto a taken name fails and changes nothing.
    assert!(db.rename_layout("Morning", "Study desk").is_err());
    assert!(!db.rename_layout("Nope", "X").unwrap());
    assert!(db.delete_layout("Morning").unwrap());
    assert!(!db.delete_layout("Morning").unwrap());
    assert_eq!(db.layouts().unwrap(), ["Study desk"]);
}

#[test]
fn a_database_from_a_newer_build_is_refused() {
    let dir = tmp("newer");
    let path = dir.join("user.db");
    drop(UserStore::open(&path).unwrap());
    // user_version lives at byte 60 of the SQLite header, big-endian. Closing the last
    // connection checkpointed the WAL into the main file.
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[60..64].copy_from_slice(&(USER_SCHEMA_VERSION + 1).to_be_bytes());
    std::fs::write(&path, bytes).unwrap();
    let err = UserStore::open(&path).unwrap_err();
    assert!(err.to_string().contains("version"), "{err}");
}
