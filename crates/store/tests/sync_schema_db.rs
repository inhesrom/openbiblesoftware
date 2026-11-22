//! Migration 12 → 13 (ADR 0022): what Sync needs is added to a User database with no row lost
//! or changed, a copy of the old file is taken first, every synced table has a clock, Layouts
//! keep their names and gain ids, and Backups still round-trip.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use obs_store::annotation::{is_valid_id, new_id};
use obs_store::{
    Change, DEVICE_PREFERENCES, Notebook, PlanReadingRow, ReadingPlanRow, Row, SYNCED_PREFERENCES,
    SYNCED_TABLES, USER_SCHEMA_VERSION, UserStore, is_synced_preference,
};
use rusqlite::Connection;
use rusqlite::types::Value;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("obs-sync13-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn db(&self) -> PathBuf {
        self.0.join("user.db")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A v12 User database holding a row in the tables the migration rebuilds or gives ids
/// (`layout`, `favorite`, `reading_plan`) and in most others the user fills; Annotations, Anchors
/// and Highlight styles only gain a `clock` column, so they are left out. The rows are written
/// as schema 12 wrote them (today's `save_layout`, `add_favorite` and `add_reading_plan` write
/// columns it did not have).
fn v12_fixture(path: &Path) {
    let (v12, report) = UserStore::open_to(path, 12).unwrap();
    assert_eq!(report.found_version, 0);
    assert_eq!(v12.schema_version().unwrap(), 12);
    v12.set_preference("view", r#"{"theme":"dark"}"#).unwrap();
    v12.set_preference("highlights.palette", "standard")
        .unwrap();
    v12.set_preference("guide-arrangement/passage", "{}")
        .unwrap();
    v12.save_last_session(r#"{"tabs":[]}"#).unwrap();
    v12.save_preferred_order(&["kjv".to_string(), "web".to_string()])
        .unwrap();
    v12.record_visit("Jn 3:16 · KJV", "{}").unwrap();
    let study = Notebook {
        id: new_id(),
        name: "Study".into(),
        is_default: false,
        show_in_text: true,
        created_at: 5,
        modified_at: 5,
    };
    v12.apply_changes(&[Change::insert(Row::Notebook(study))])
        .unwrap();
    for row in [
        "INSERT INTO layout (name, snapshot, saved_at) VALUES
             ('Study desk', '{\"a\":1}', 100), ('reading', '{\"b\":2}', 200)",
        "INSERT INTO favorite (folder, label, location, position, added_at) VALUES
             ('', 'Gen 1:1', '{\"g\":1}', 1, 10), ('Gospels', 'Jn 1:1', '{\"j\":1}', 2, 20)",
        "INSERT INTO reading_plan (name, template, bible_id, start_day, weekdays, created_at)
             VALUES ('Gospel', 'nt90', 'kjv', '2026-01-01', 127, 30)",
        "INSERT INTO plan_reading (plan_id, position, passages, day, read_at) VALUES
             (1, 0, 'Mt 1', '2026-01-01', 40), (1, 1, 'Mt 2', '2026-01-02', NULL)",
    ] {
        v12.execute_unchecked(row).unwrap();
    }
}

/// Every row of every table, by table: each row as its `column=value` pairs, rows sorted.
type Tables = BTreeMap<String, Vec<String>>;

fn tables(c: &Connection) -> Vec<String> {
    c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn columns(c: &Connection, table: &str) -> Vec<String> {
    c.prepare("SELECT name FROM pragma_table_info(?1)")
        .unwrap()
        .query_map([table], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn rows(c: &Connection, table: &str, columns: &[String]) -> Vec<String> {
    let list = columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = c
        .prepare(&format!("SELECT {list} FROM \"{table}\""))
        .unwrap();
    let mut out: Vec<String> = stmt
        .query_map([], |r| {
            (0..columns.len())
                .map(|i| {
                    r.get::<_, Value>(i)
                        .map(|v| format!("{}={v:?}", columns[i]))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(|v| v.join(" "))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    out.sort();
    out
}

fn everything(c: &Connection) -> Tables {
    tables(c)
        .into_iter()
        .map(|t| {
            let cols = columns(c, &t);
            let r = rows(c, &t, &cols);
            (t, r)
        })
        .collect()
}

fn uuid_v7(id: &str) -> bool {
    is_valid_id(id) && &id[14..15] == "7"
}

fn strings(c: &Connection, sql: &str) -> Vec<String> {
    c.prepare(sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn migration_13_keeps_every_row_of_a_v12_database_and_copies_it_first() {
    let dir = Dir::new("v12");
    v12_fixture(&dir.db());
    let before = everything(&Connection::open(dir.db()).unwrap());

    let (db, report) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(report.found_version, 12);
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    assert_eq!(USER_SCHEMA_VERSION, 19);
    // The copy taken before migrating is the v12 database as it was.
    let copy = report.migration_backup.unwrap();
    assert!(
        copy.to_string_lossy().contains(".v12-"),
        "{}",
        copy.display()
    );
    assert_eq!(everything(&Connection::open(&copy).unwrap()), before);
    drop(db);

    let c = Connection::open(dir.db()).unwrap();
    // Every row of every v12 table is there, each column as it was.
    for (table, old_rows) in &before {
        let old_columns = columns(&Connection::open(&copy).unwrap(), table);
        assert_eq!(&rows(&c, table, &old_columns), old_rows, "{table}");
    }
    // Nothing has a clock yet, nothing is deleted, and no other device's ops are applied.
    for table in SYNCED_TABLES {
        let clocks = strings(
            &c,
            &format!("SELECT coalesce(clock, 'none') FROM {table} WHERE clock IS NOT NULL"),
        );
        assert!(clocks.is_empty(), "{table}: {clocks:?}");
    }
    assert!(strings(&c, "SELECT row_id FROM tombstone").is_empty());
    assert!(strings(&c, "SELECT device_id FROM applied_up_to").is_empty());
    // This device has an id; the user has not named it.
    let device: Vec<(String, String)> = c
        .prepare("SELECT device_id, name FROM device")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(device.len(), 1);
    assert!(uuid_v7(&device[0].0), "{device:?}");
    assert_eq!(device[0].1, "");
    // Favorites and Reading plans keep their numbers and gain ids of their own.
    for table in ["favorite", "reading_plan"] {
        let uids = strings(&c, &format!("SELECT uid FROM {table} ORDER BY id"));
        assert!(
            !uids.is_empty() && uids.iter().all(|u| uuid_v7(u)),
            "{uids:?}"
        );
        let distinct: std::collections::BTreeSet<&String> = uids.iter().collect();
        assert_eq!(distinct.len(), uids.len(), "{table}: {uids:?}");
    }
    // A second launch migrates nothing and copies nothing.
    drop(c);
    let (_db, report) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(report.found_version, USER_SCHEMA_VERSION);
    assert!(report.migration_backup.is_none());
}

#[test]
fn migration_13_gives_every_synced_table_a_clock() {
    let dir = Dir::new("clocks");
    drop(UserStore::open(&dir.db()).unwrap());
    let c = Connection::open(dir.db()).unwrap();
    let all = tables(&c);
    for table in SYNCED_TABLES {
        assert!(all.iter().any(|t| t == table), "no table `{table}`");
        assert!(
            columns(&c, table).iter().any(|c| c == "clock"),
            "`{table}` has no clock"
        );
    }
    // Every other table is one that stays on the device, or rides on a synced row.
    let riding = ["anchors"];
    let device = [
        "last_session",
        "history",
        "resource_names",
        "tombstone",
        "device",
        "applied_up_to",
        "item_clock",
        "op_outbox",
        "sync_held",
        "former_device",
        "merge_cursor",
    ];
    for table in &all {
        let known = SYNCED_TABLES.contains(&table.as_str())
            || riding.contains(&table.as_str())
            || device.contains(&table.as_str())
            || table.starts_with("notes_fts")
            || table.starts_with("sqlite_");
        assert!(
            known,
            "`{table}`: list it in SYNCED_TABLES or as a table that stays on the device"
        );
    }
    for table in riding.iter().chain(&device) {
        assert!(
            !columns(&c, table).iter().any(|c| c == "clock")
                || ["tombstone", "item_clock", "op_outbox", "sync_held"].contains(table),
            "`{table}` is not synced but has a clock"
        );
    }
}

#[test]
fn migration_13_keeps_each_layouts_name_and_gives_it_an_id() {
    let dir = Dir::new("layouts");
    v12_fixture(&dir.db());
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(db.layouts().unwrap(), ["reading", "Study desk"]);
    assert_eq!(
        db.layout("study DESK").unwrap().as_deref(),
        Some(r#"{"a":1}"#)
    );
    let ids = |db: &UserStore| -> BTreeMap<String, String> {
        let c = Connection::open(db.path()).unwrap();
        c.prepare("SELECT name, id FROM layout")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    let migrated = ids(&db);
    assert_eq!(migrated.len(), 2);
    assert!(migrated.values().all(|id| uuid_v7(id)), "{migrated:?}");
    assert_ne!(migrated["reading"], migrated["Study desk"]);

    // Saving over a Layout (its name in another case) and renaming it keep its id; a new
    // Layout gets one of its own.
    db.save_layout("STUDY DESK", r#"{"a":2}"#).unwrap();
    assert!(db.rename_layout("reading", "Morning").unwrap());
    db.save_layout("Evening", "{}").unwrap();
    let now = ids(&db);
    assert_eq!(now["STUDY DESK"], migrated["Study desk"]);
    assert_eq!(now["Morning"], migrated["reading"]);
    assert!(uuid_v7(&now["Evening"]));
    assert!(!migrated.values().any(|id| *id == now["Evening"]));
    // A name is still taken whatever its case.
    assert!(db.rename_layout("Morning", "evening").is_err());
}

#[test]
fn rows_added_after_migration_13_get_ids() {
    let dir = Dir::new("new-rows");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let a = db.add_favorite("", "Gen 1:1", "{}").unwrap();
    let b = db.add_favorite("", "Gen 1:2", "{}").unwrap();
    let plan = ReadingPlanRow {
        id: 0,
        name: "Gospel".into(),
        template: "nt90".into(),
        bible_id: "kjv".into(),
        start_day: "2026-01-01".into(),
        weekdays: 127,
        created_at: 0,
    };
    let reading = PlanReadingRow {
        position: 0,
        passages: "Mt 1".into(),
        day: "2026-01-01".into(),
        read_at: None,
    };
    db.add_reading_plan(&plan, std::slice::from_ref(&reading))
        .unwrap();
    db.add_reading_plan(&plan, &[reading]).unwrap();
    let c = Connection::open(db.path()).unwrap();
    for table in ["favorite", "reading_plan"] {
        let uids = strings(&c, &format!("SELECT uid FROM {table} ORDER BY id"));
        assert_eq!(uids.len(), 2, "{table}");
        assert!(uids.iter().all(|u| uuid_v7(u)), "{uids:?}");
        assert_ne!(uids[0], uids[1]);
    }
    assert_eq!(
        db.favorites()
            .unwrap()
            .iter()
            .map(|f| f.id)
            .collect::<Vec<_>>(),
        [a, b]
    );
}

#[test]
fn backups_round_trip_with_the_sync_columns_and_an_older_one_gains_them() {
    let dir = Dir::new("backups");
    // A v13 Backup restores whole: Layout ids and Favorite ids come back. (Which device id a
    // restored database has is Sync's to decide, ADR 0022 rule 7.)
    let first = dir.0.join("first").join("user.db");
    let (a, _) = UserStore::open(&first).unwrap();
    a.save_layout("Study", "{}").unwrap();
    a.add_favorite("", "Gen 1:1", "{}").unwrap();
    let backup = dir.0.join("a.obsbackup");
    a.back_up(&backup).unwrap();
    let held = everything(&Connection::open(&backup).unwrap());
    let contents = UserStore::backup_contents(&backup).unwrap();
    assert_eq!(contents.version, USER_SCHEMA_VERSION);
    assert_eq!(contents.layouts, ["Study"]);

    let (mut b, _) = UserStore::open(&dir.db()).unwrap();
    b.restore(&backup).unwrap();
    drop(b);
    let restored = everything(&Connection::open(dir.db()).unwrap());
    for table in ["layout", "favorite", "preference", "notebooks"] {
        assert_eq!(restored[table], held[table], "{table}");
    }

    // A v12 Backup is migrated as it is restored: its Layouts keep their names and gain ids.
    let old = dir.0.join("old").join("user.db");
    v12_fixture(&old);
    let old_backup = dir.0.join("v12.obsbackup");
    {
        let (v12, _) = UserStore::open_to(&old, 12).unwrap();
        v12.back_up(&old_backup).unwrap();
    }
    assert_eq!(UserStore::backup_contents(&old_backup).unwrap().version, 12);
    let (mut b, _) = UserStore::open(&dir.db()).unwrap();
    b.restore(&old_backup).unwrap();
    assert_eq!(b.schema_version().unwrap(), USER_SCHEMA_VERSION);
    assert_eq!(b.layouts().unwrap(), ["reading", "Study desk"]);
    let c = Connection::open(b.path()).unwrap();
    let ids = strings(&c, "SELECT id FROM layout");
    assert!(
        ids.len() == 2 && ids.iter().all(|id| uuid_v7(id)),
        "{ids:?}"
    );
    assert_eq!(strings(&c, "SELECT device_id FROM device").len(), 1);
}

/// Every preference key the engine keeps (its `*_PREF`, `*_PREF_PREFIX` and `*_KEY`
/// constants) is listed once: as Synced data or as staying on the device (ADR 0022 rule 4).
#[test]
fn every_preference_the_engine_keeps_is_listed_as_synced_or_staying_on_the_device() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../engine/src");
    let mut keys = Vec::new();
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        for line in std::fs::read_to_string(&path).unwrap().lines() {
            let Some((name, rest)) = line
                .trim_start()
                .trim_start_matches("pub ")
                .trim_start_matches("pub(crate) ")
                .strip_prefix("const ")
                .and_then(|l| l.split_once(": &str = \""))
            else {
                continue;
            };
            if name.ends_with("_PREF") || name.ends_with("_PREF_PREFIX") || name.ends_with("_KEY") {
                keys.push(rest.trim_end_matches("\";").to_string());
            }
        }
    }
    assert!(keys.len() > 20, "{keys:?}");
    let all: Vec<&str> = SYNCED_PREFERENCES
        .iter()
        .chain(DEVICE_PREFERENCES)
        .copied()
        .collect();
    for key in &keys {
        let n = all.iter().filter(|e| **e == key.as_str()).count();
        assert_eq!(
            n, 1,
            "`{key}`: list it once, in SYNCED_PREFERENCES or DEVICE_PREFERENCES"
        );
    }
    for entry in &all {
        assert!(
            keys.iter().any(|k| k == entry),
            "`{entry}` is listed but the engine keeps no such preference"
        );
    }
    assert!(is_synced_preference("guide-arrangement/word_study"));
    assert!(!is_synced_preference("search.recent.notes"));
}
