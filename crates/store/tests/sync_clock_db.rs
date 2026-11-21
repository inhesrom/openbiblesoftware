//! Sync's clocks and outbox in the User database (ADR 0022 rules 2–3, S28a): migration 13 → 14
//! loses nothing, from a database or a Backup; every database has a device id, a damaged one
//! replaced; the clock never hands out one at or below a clock the database holds or a merge
//! observed, after a reopen, a restore or a wall clock stepping back, taking in no clock that
//! does not parse or is more than a day ahead (whose rows stay); and the outbox keeps ops only
//! while this device records them, giving a reserved op file the same ops when it is retried
//! and marking only the next reserved file written.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use obs_store::annotation::new_id;
use obs_store::{Change, Hlc, Notebook, Row, SYNCED_TABLES, USER_SCHEMA_VERSION, UserStore};
use rusqlite::Connection;
use rusqlite::types::Value;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("obs-clock-{name}-{}", std::process::id()));
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
        obs_store::clock::fix(None);
    }
}

type Tables = BTreeMap<String, Vec<String>>;

fn strings(c: &Connection, sql: &str) -> Vec<String> {
    c.prepare(sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Every row of every table, each as its `column=value` pairs, rows sorted.
fn everything(c: &Connection) -> Tables {
    let tables = strings(
        c,
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    );
    tables
        .into_iter()
        .map(|t| {
            let columns = strings(c, &format!("SELECT name FROM pragma_table_info('{t}')"));
            let list = columns
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            let mut rows: Vec<String> = c
                .prepare(&format!("SELECT {list} FROM \"{t}\""))
                .unwrap()
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
            rows.sort();
            (t, rows)
        })
        .collect()
}

/// Every clock the database holds.
fn clocks(c: &Connection) -> Vec<String> {
    SYNCED_TABLES
        .iter()
        .chain(&["tombstone", "item_clock", "op_outbox"])
        .flat_map(|t| {
            strings(
                c,
                &format!("SELECT clock FROM {t} WHERE clock IS NOT NULL AND clock <> ''"),
            )
        })
        .collect()
}

fn notebook(name: &str) -> Notebook {
    Notebook {
        id: new_id(),
        name: name.into(),
        is_default: false,
        show_in_text: true,
        created_at: 1,
        modified_at: 1,
    }
}

/// Make a Notebook and return the clock it was stamped with.
fn add_notebook(db: &UserStore, name: &str) -> Hlc {
    let nb = notebook(name);
    db.apply_changes(&[Change::insert(Row::Notebook(nb.clone()))])
        .unwrap();
    let c = Connection::open(db.path()).unwrap();
    let clock: String = c
        .query_row("SELECT clock FROM notebooks WHERE id = ?1", [&nb.id], |r| {
            r.get(0)
        })
        .unwrap();
    Hlc::parse(&clock).unwrap()
}

/// A v13 database with a row of most kinds of Synced data, a tombstone and a list
/// preference, written as schema 13 writes them (no clocks, the tombstone's empty).
fn v13_fixture(path: &Path) {
    let (v13, _) = UserStore::open_to(path, 13).unwrap();
    assert_eq!(v13.schema_version().unwrap(), 13);
    v13.save_layout("Study", r#"{"v":1}"#).unwrap();
    v13.save_preferred_order(&["kjv".into(), "web".into()])
        .unwrap();
    v13.add_favorite("", "Jn 3:16", "{}").unwrap();
    let gone = v13.add_favorite("", "Gen 1:1", "{}").unwrap();
    assert!(v13.remove_favorite(gone).unwrap());
    v13.set_preference(
        "library.collections",
        r#"[{"name":"Greek","rules":[],"resources":["sblgnt"]}]"#,
    )
    .unwrap();
    v13.set_preference("view", r#"{"theme":"dark"}"#).unwrap();
    v13.apply_changes(&[Change::insert(Row::Notebook(notebook("Study")))])
        .unwrap();
}

#[test]
fn migration_14_keeps_every_row_of_a_v13_database_and_copies_it_first() {
    let dir = Dir::new("v13");
    v13_fixture(&dir.db());
    let before = everything(&Connection::open(dir.db()).unwrap());
    assert_eq!(
        strings(
            &Connection::open(dir.db()).unwrap(),
            "SELECT clock FROM tombstone"
        ),
        [""]
    );

    let (db, report) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(report.found_version, 13);
    assert_eq!(USER_SCHEMA_VERSION, 14);
    assert_eq!(db.schema_version().unwrap(), 14);
    let copy = report.migration_backup.unwrap();
    assert!(copy.to_string_lossy().contains(".v13-"));
    assert_eq!(everything(&Connection::open(&copy).unwrap()), before);
    drop(db);

    let after = everything(&Connection::open(dir.db()).unwrap());
    for (table, rows) in &before {
        assert_eq!(&after[table], rows, "{table}");
    }
    for table in ["item_clock", "op_outbox"] {
        assert_eq!(after[table], Vec::<String>::new(), "{table}");
    }
}

#[test]
fn a_v13_backup_restores_into_14_losing_nothing() {
    let dir = Dir::new("v13-backup");
    let old = dir.0.join("old").join("user.db");
    v13_fixture(&old);
    let backup = dir.0.join("v13.obsbackup");
    {
        let (v13, _) = UserStore::open_to(&old, 13).unwrap();
        v13.back_up(&backup).unwrap();
    }
    let held = everything(&Connection::open(&backup).unwrap());
    assert_eq!(UserStore::backup_contents(&backup).unwrap().version, 13);

    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    db.restore(&backup).unwrap();
    assert_eq!(db.schema_version().unwrap(), 14);
    let restored = everything(&Connection::open(db.path()).unwrap());
    for (table, rows) in &held {
        assert_eq!(&restored[table], rows, "{table}");
    }
    // Restored, it writes clocks.
    let clock = add_notebook(&db, "After");
    assert_eq!(clock.device_id, db.device_id().unwrap());
}

#[test]
fn a_backup_carries_the_item_clocks_and_the_outbox() {
    let dir = Dir::new("carry");
    let first = dir.0.join("first").join("user.db");
    let (a, _) = UserStore::open(&first).unwrap();
    a.start_recording_ops().unwrap();
    a.set_preference(
        "library.collections",
        r#"[{"name":"Greek","rules":[],"resources":["sblgnt"]}]"#,
    )
    .unwrap();
    add_notebook(&a, "Study");
    let backup = dir.0.join("a.obsbackup");
    a.back_up(&backup).unwrap();
    let held = everything(&Connection::open(&backup).unwrap());
    assert_eq!(held["item_clock"].len(), 1);
    assert_eq!(held["op_outbox"].len(), 2);

    let (mut b, _) = UserStore::open(&dir.db()).unwrap();
    b.restore(&backup).unwrap();
    let restored = everything(&Connection::open(b.path()).unwrap());
    for table in [
        "item_clock",
        "op_outbox",
        "tombstone",
        "applied_up_to",
        "device",
    ] {
        assert_eq!(restored[table], held[table], "{table}");
    }
}

#[test]
fn every_database_has_a_device_id_from_open_and_from_open_backup() {
    let dir = Dir::new("device");
    // A v13 database and a Backup of it without the device row (only migration 12 → 13
    // makes it).
    let backup = dir.0.join("no-device.obsbackup");
    {
        let (v13, _) = UserStore::open_to(&dir.db(), 13).unwrap();
        v13.execute_unchecked("DELETE FROM device").unwrap();
        v13.back_up(&backup).unwrap();
    }
    assert_eq!(
        strings(
            &Connection::open(&backup).unwrap(),
            "SELECT device_id FROM device"
        ),
        Vec::<String>::new()
    );
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let id = db.device_id().unwrap();
    assert!(obs_store::annotation::is_valid_id(&id));
    // Opened again, it keeps it.
    drop(db);
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(db.device_id().unwrap(), id);

    let (from_backup, _) = UserStore::open_backup(&backup).unwrap();
    let other = from_backup.device_id().unwrap();
    assert!(obs_store::annotation::is_valid_id(&other));
    assert_ne!(other, id);
}

#[test]
fn a_damaged_device_id_is_replaced_at_open_as_a_missing_one_is() {
    let dir = Dir::new("bad-device");
    obs_store::clock::fix(Some(1_800_000_000_000));
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let first = add_notebook(&db, "A");
    db.execute_unchecked("UPDATE device SET device_id = 'not-a-device'")
        .unwrap();
    assert!(matches!(
        db.device_id(),
        Err(obs_store::StoreError::Corrupt(_))
    ));
    let backup = dir.0.join("bad-device.obsbackup");
    db.back_up(&backup).unwrap();
    drop(db);

    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let id = db.device_id().unwrap();
    assert!(obs_store::annotation::is_valid_id(&id));
    assert_ne!(id, first.device_id);
    // Its clocks name the new id and still pass every clock held.
    let next = add_notebook(&db, "B");
    assert_eq!(next.device_id, id);
    assert_eq!(next.tick(), (1_800_000_000_000, first.counter + 1));

    let (from_backup, _) = UserStore::open_backup(&backup).unwrap();
    let other = from_backup.device_id().unwrap();
    assert!(obs_store::annotation::is_valid_id(&other));
    assert_eq!(
        strings(
            &Connection::open(&backup).unwrap(),
            "SELECT device_id FROM device"
        ),
        ["not-a-device"],
        "the Backup itself is left as it is"
    );
}

#[test]
fn the_clock_passes_every_clock_held_after_a_reopen_even_when_the_wall_clock_steps_back() {
    let dir = Dir::new("reopen");
    obs_store::clock::fix(Some(1_800_000_000_000));
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let first = add_notebook(&db, "A");
    let second = add_notebook(&db, "B");
    assert_eq!(
        first.tick(),
        (1_800_000_000_000, 1),
        "after the default Notebook's"
    );
    assert_eq!(second.tick(), (1_800_000_000_000, 2));
    drop(db);
    // A day earlier: the wall clock stepped back while the app was closed.
    obs_store::clock::fix(Some(1_799_913_600_000));
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let held = clocks(&Connection::open(dir.db()).unwrap());
    let next = add_notebook(&db, "C");
    assert!(
        held.iter().all(|c| next.encode().unwrap() > *c),
        "{next} passes {held:?}"
    );
    assert_eq!(next.tick(), (1_800_000_000_000, 3));
    // A clock from another device in the database (as a merge would write), a day ahead of
    // this one's wall clock (the most a clock taken in may be), is passed too.
    let other = Hlc {
        wall_ms: 1_800_000_000_000,
        counter: 9,
        device_id: new_id(),
    };
    db.execute_unchecked(&format!(
        "INSERT INTO tombstone (row_table, row_id, clock) VALUES ('notebook', 'x', '{other}')"
    ))
    .unwrap();
    drop(db);
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let after = add_notebook(&db, "D");
    assert_eq!(after.tick(), (1_800_000_000_000, 10));
    assert_ne!(after.device_id, other.device_id);
}

#[test]
fn the_seed_takes_in_no_damaged_clock_nor_one_more_than_a_day_ahead_and_keeps_their_rows() {
    let dir = Dir::new("seed-bounds");
    let now = 1_800_000_000_000_u64;
    obs_store::clock::fix(Some(i64::try_from(now).unwrap()));
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let first = add_notebook(&db, "A");
    let clock = |wall_ms, counter| Hlc {
        wall_ms,
        counter,
        device_id: new_id(),
    };
    let day = obs_store::hlc::MAX_FORWARD_SKEW_MS;
    // An hour ahead, a day ahead (the bound, still taken in), a day and a millisecond ahead,
    // and text that is no clock but sorts above every clock.
    let hour = clock(now + 3_600_000, 4);
    let bound = clock(now + day, 2);
    let beyond = clock(now + day + 1, 0);
    for statement in [
        format!(
            "INSERT INTO tombstone (row_table, row_id, clock) VALUES
                 ('notebook', 'hour', '{hour}'), ('notebook', 'beyond', '{beyond}')"
        ),
        "UPDATE notebooks SET clock = 'zz-not-a-clock' WHERE name = 'A'".into(),
        format!(
            "INSERT INTO item_clock (row_table, row_id, clock) VALUES ('collection', 'x', '{bound}')"
        ),
    ] {
        db.execute_unchecked(&statement).unwrap();
    }
    let held = everything(&Connection::open(dir.db()).unwrap());
    drop(db);

    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let next = add_notebook(&db, "B");
    assert_eq!(next.tick(), (now + day, 3), "passes the bound's clock only");
    assert!(next > first);
    // The rows holding them are as they were.
    let after = everything(&Connection::open(dir.db()).unwrap());
    for table in ["tombstone", "item_clock", "op_outbox"] {
        assert_eq!(after[table], held[table], "{table}");
    }
    assert!(
        after["notebooks"]
            .iter()
            .any(|r| r.contains("zz-not-a-clock") && r.contains("\"A\""))
    );
    // Two days on, the wall clock passes them all.
    drop(db);
    obs_store::clock::fix(Some(i64::try_from(now + 2 * day).unwrap()));
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(add_notebook(&db, "C").tick(), (now + 2 * day, 0));
    // Back at `now`, C's clock is more than a day ahead; the largest taken in is B's, in
    // notebooks under its damaged text.
    drop(db);
    obs_store::clock::fix(Some(i64::try_from(now).unwrap()));
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(add_notebook(&db, "D").tick(), (now + day, 4));
}

#[test]
fn a_clock_merged_in_is_observed_so_the_next_clock_passes_it_unless_more_than_a_day_ahead() {
    let dir = Dir::new("observe");
    let now = 1_800_000_000_000_u64;
    obs_store::clock::fix(Some(i64::try_from(now).unwrap()));
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let first = add_notebook(&db, "A");
    let remote = |wall_ms, counter| Hlc {
        wall_ms,
        counter,
        device_id: new_id(),
    };
    // Within the session, with nothing written to the database.
    assert!(db.observe_clock(&remote(now + 3_600_000, 5)));
    assert_eq!(add_notebook(&db, "B").tick(), (now + 3_600_000, 6));
    // An older clock leaves it where it is; one more than a day ahead is not taken in.
    assert!(db.observe_clock(&first));
    assert!(!db.observe_clock(&remote(now + obs_store::hlc::MAX_FORWARD_SKEW_MS + 1, 0)));
    assert_eq!(add_notebook(&db, "C").tick(), (now + 3_600_000, 7));
}

#[test]
fn the_clock_passes_every_clock_held_after_a_restore_but_not_a_backups_a_year_ahead() {
    let dir = Dir::new("restore");
    // A Backup made on a computer whose clock was a year ahead.
    obs_store::clock::fix(Some(1_830_000_000_000));
    let ahead = dir.0.join("ahead").join("user.db");
    let (a, _) = UserStore::open(&ahead).unwrap();
    add_notebook(&a, "Ahead");
    let ahead_backup = dir.0.join("ahead.obsbackup");
    a.back_up(&ahead_backup).unwrap();
    // A Backup whose clocks are all older than this device's.
    obs_store::clock::fix(Some(1_700_000_000_000));
    let behind = dir.0.join("behind").join("user.db");
    let (b, _) = UserStore::open(&behind).unwrap();
    add_notebook(&b, "Behind");
    let behind_backup = dir.0.join("behind.obsbackup");
    b.back_up(&behind_backup).unwrap();

    obs_store::clock::fix(Some(1_800_000_000_000));
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    let mine = add_notebook(&db, "Mine");
    db.restore(&ahead_backup).unwrap();
    let held = everything(&Connection::open(db.path()).unwrap());
    let next = add_notebook(&db, "After ahead");
    // Its clocks are more than a day ahead: not taken in, and its rows kept as they are.
    assert_eq!(next.tick(), (1_800_000_000_000, mine.counter + 1));
    let after = everything(&Connection::open(db.path()).unwrap());
    assert!(
        held["notebooks"]
            .iter()
            .all(|row| after["notebooks"].contains(row))
    );
    assert!(after["notebooks"].iter().any(|r| r.contains("\"Ahead\"")));

    db.restore(&behind_backup).unwrap();
    let held = clocks(&Connection::open(db.path()).unwrap());
    let next = add_notebook(&db, "After behind");
    assert!(
        held.iter().all(|c| next.encode().unwrap() > *c),
        "{next} passes {held:?}"
    );
    // It never goes below a clock it handed out before, either.
    assert!(next.tick() > mine.tick());
    assert_eq!(next.tick(), (1_800_000_000_000, mine.counter + 2));
}

#[test]
fn the_outbox_keeps_ops_only_while_recording_and_a_reserved_file_comes_back_the_same() {
    let dir = Dir::new("outbox");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    add_notebook(&db, "Before");
    assert_eq!(
        strings(
            &Connection::open(dir.db()).unwrap(),
            "SELECT clock FROM op_outbox"
        ),
        Vec::<String>::new(),
        "not recording: no ops kept, the clocks still stamped"
    );
    db.start_recording_ops().unwrap();
    db.start_recording_ops().unwrap();
    add_notebook(&db, "One");
    let first = db.reserve_op_files().unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].seq, 1);
    // The write stopped half way: more edits arrive, then the next try.
    add_notebook(&db, "Two");
    let retried = db.reserve_op_files().unwrap();
    assert_eq!(retried.len(), 2);
    assert_eq!(
        retried[0], first[0],
        "file 1 comes back with exactly its ops"
    );
    assert_eq!(retried[1].seq, 2);
    assert_eq!(retried[1].ops.len(), 1);
    assert_eq!(
        db.reserve_op_files().unwrap(),
        retried,
        "nothing new: the same files"
    );
    // Only the next reserved file is marked written: no number is skipped.
    for (seq, why) in [(2, "after one not written"), (7, "not reserved")] {
        assert!(
            matches!(
                db.op_file_written(seq),
                Err(obs_store::StoreError::Rejected(_))
            ),
            "{seq}: {why}"
        );
    }
    db.op_file_written(1).unwrap();
    db.op_file_written(2).unwrap();
    assert!(
        matches!(
            db.op_file_written(2),
            Err(obs_store::StoreError::Rejected(_))
        ),
        "written already"
    );
    assert_eq!(db.reserve_op_files().unwrap(), Vec::new());
    // Op files count on from the last written, across a reopen.
    drop(db);
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    add_notebook(&db, "Three");
    let next = db.reserve_op_files().unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].seq, 3);
    let own: i64 = Connection::open(dir.db())
        .unwrap()
        .query_row(
            "SELECT seq FROM applied_up_to WHERE device_id = ?1",
            [db.device_id().unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(own, 2, "the last of its own op files written");
}
