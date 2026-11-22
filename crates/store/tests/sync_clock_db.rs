//! Sync's clocks and outbox in the User database (ADR 0022 rules 2–3, S28a): migration 13 → 14
//! loses nothing, from a database or a Backup; migration 15's item positions and what each
//! tombstone covers round-trip through a Backup, and a v14 Backup restores with both unknown
//! (an item at its index, a tombstone covering up to its own clock); migration 16's Sync
//! conflict copies, deleted parents' rows, held ops and list bases round-trip through a
//! Backup, and a v15 Backup restores with none of them; every database has a device id, a damaged one
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
use obs_store::{
    Annotation, Change, Content, HeldOp, Hlc, Notebook, Row, RowKey, SYNCED_TABLES,
    SyncedPreference, USER_SCHEMA_VERSION, UserStore,
};
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
            // `tombstone.covers` (schema 15), `tombstone.row` with its `row_covers` and
            // `row_clock`, and `annotations.conflict_of` (16) are `NULL` on every row written
            // before them; the migration tests below check that on their own.
            let columns = strings(
                c,
                &format!(
                    "SELECT name FROM pragma_table_info('{t}')
                     WHERE name NOT IN ('covers', 'row', 'row_covers', 'row_clock', 'conflict_of')"
                ),
            );
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
fn migrations_14_to_16_keep_every_row_of_a_v13_database_and_copy_it_first() {
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
    assert_eq!(USER_SCHEMA_VERSION, 16);
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    let copy = report.migration_backup.unwrap();
    assert!(copy.to_string_lossy().contains(".v13-"));
    assert_eq!(everything(&Connection::open(&copy).unwrap()), before);
    drop(db);

    let after = everything(&Connection::open(dir.db()).unwrap());
    for (table, rows) in &before {
        assert_eq!(&after[table], rows, "{table}");
    }
    for table in ["item_clock", "op_outbox", "sync_held"] {
        assert_eq!(after[table], Vec::<String>::new(), "{table}");
    }
    // A tombstone from before schema 15 covers up to its own clock (`NULL`).
    assert_eq!(
        strings(
            &Connection::open(dir.db()).unwrap(),
            "SELECT coalesce(covers, 'NULL') FROM tombstone"
        ),
        ["NULL"]
    );
}

#[test]
fn a_v13_backup_restores_into_16_losing_nothing() {
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
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
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

/// `(row_id, position)` of every item clock and `(row_id, covers)` of every tombstone, with
/// `NULL` as "NULL".
fn positions_and_covers(c: &Connection) -> (Vec<String>, Vec<String>) {
    (
        strings(
            c,
            "SELECT row_id || '=' || coalesce(position, 'NULL') FROM item_clock ORDER BY row_id",
        ),
        strings(
            c,
            "SELECT row_id || '=' || coalesce(covers, 'NULL') FROM tombstone ORDER BY row_id",
        ),
    )
}

/// Two Collections, then a Favorite added and removed.
fn list_and_delete(db: &UserStore) {
    db.set_preference(
        "library.collections",
        r#"[{"name":"Greek","rules":[]},{"name":"Hebrew","rules":[]}]"#,
    )
    .unwrap();
    let gone = db.add_favorite("", "Gen 1:1", "{}").unwrap();
    assert!(db.remove_favorite(gone).unwrap());
}

#[test]
fn item_positions_and_what_tombstones_cover_round_trip_through_a_backup() {
    let dir = Dir::new("v15-backup");
    let first = dir.0.join("first").join("user.db");
    let (a, _) = UserStore::open(&first).unwrap();
    list_and_delete(&a);
    let held = positions_and_covers(&Connection::open(&first).unwrap());
    assert_eq!(held.0, ["Greek=0", "Hebrew=1"]);
    assert_eq!(held.1.len(), 1);
    assert!(
        Hlc::parse(held.1[0].split('=').nth(1).unwrap()).is_some(),
        "{held:?}"
    );
    let backup = dir.0.join("a.obsbackup");
    a.back_up(&backup).unwrap();

    let (mut b, _) = UserStore::open(&dir.db()).unwrap();
    b.restore(&backup).unwrap();
    assert_eq!(
        positions_and_covers(&Connection::open(b.path()).unwrap()),
        held
    );
}

#[test]
fn a_v14_backup_restores_with_item_positions_at_their_index_and_tombstones_covering_their_clock() {
    let dir = Dir::new("v14-backup");
    let old = dir.0.join("old").join("user.db");
    let (v14, _) = UserStore::open_to(&old, 14).unwrap();
    list_and_delete(&v14);
    let backup = dir.0.join("v14.obsbackup");
    v14.back_up(&backup).unwrap();
    drop(v14);
    assert_eq!(UserStore::backup_contents(&backup).unwrap().version, 14);

    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    db.restore(&backup).unwrap();
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    let (positions, covers) = positions_and_covers(&Connection::open(db.path()).unwrap());
    assert_eq!(positions, ["Greek=NULL", "Hebrew=NULL"]);
    assert_eq!(covers.len(), 1);
    assert!(covers[0].ends_with("=NULL"), "{covers:?}");
    // NULL: an item is at its index, a tombstone covers up to its own clock.
    for (i, name) in ["Greek", "Hebrew"].into_iter().enumerate() {
        let row = db
            .row_state(&RowKey::Collection(name.into()))
            .unwrap()
            .row
            .unwrap();
        assert!(
            matches!(&row, Row::Collection(item) if item.position == u32::try_from(i).unwrap()),
            "{row:?}"
        );
    }
    let uid = covers[0].split('=').next().unwrap();
    let tombstone = db
        .row_state(&RowKey::Favorite(uid.into()))
        .unwrap()
        .tombstone
        .unwrap();
    assert!(tombstone.clock.is_some());
    assert_eq!(tombstone.covers, tombstone.clock);
}

/// A Sync conflict's copy, a deleted Notebook (its tombstone keeping its row), a held op and
/// a list written whole (its base).
fn schema_16_rows(db: &UserStore) -> String {
    let nb = notebook("Gone");
    db.apply_changes(&[Change::insert(Row::Notebook(nb.clone()))])
        .unwrap();
    db.apply_changes(&[Change::delete(Row::Notebook(nb))])
        .unwrap();
    let default = db.notebooks().unwrap().remove(0);
    let note = Annotation {
        id: new_id(),
        notebook_id: default.id,
        content: Content::Note {
            body: "lost text".into(),
        },
        anchor: None,
        created_at: 1,
        modified_at: 1,
        conflict_of: Some(new_id()),
    };
    db.apply_changes(&[Change::insert(Row::Annotation(note.clone()))])
        .unwrap();
    db.set_preference("library.collections", "not a list")
        .unwrap();
    let clock = add_notebook(db, "Clock");
    db.merge_held(|tx| {
        tx.hold(&HeldOp {
            clock,
            change: Change::insert(Row::Preference(SyncedPreference {
                key: "later.app.key".into(),
                value: "1".into(),
            })),
            before_clock: None,
            absent: Vec::new(),
            why: "a later app's".into(),
        })
    })
    .unwrap();
    note.id
}

/// `annotations.conflict_of`, `tombstone.row` (with the covers and clock of its delete),
/// `sync_held` and the list bases, as text.
fn schema_16_columns(c: &Connection) -> Vec<String> {
    [
        "SELECT 'conflict_of ' || id || '=' || coalesce(conflict_of, 'NULL') FROM annotations",
        "SELECT 'row ' || row_id || '=' || coalesce(row || ' ' || row_covers || ' ' || row_clock, 'NULL')
         FROM tombstone",
        "SELECT 'held ' || row_table || '/' || row_id || ' ' || clock || ' ' || change FROM sync_held",
        "SELECT 'base ' || row_id || ' ' || clock FROM item_clock WHERE row_table = 'preference'",
    ]
    .iter()
    .flat_map(|q| strings(c, q))
    .collect()
}

#[test]
fn migration_16_columns_round_trip_through_a_backup() {
    let dir = Dir::new("v16-backup");
    let first = dir.0.join("first").join("user.db");
    let (a, _) = UserStore::open(&first).unwrap();
    let copy = schema_16_rows(&a);
    let held = schema_16_columns(&Connection::open(&first).unwrap());
    assert!(
        held.iter()
            .any(|r| r.starts_with(&format!("conflict_of {copy}=")) && !r.ends_with("NULL")),
        "{held:?}"
    );
    assert!(
        held.iter()
            .any(|r| r.starts_with("row ") && r.contains(r#""name":"Gone""#)),
        "{held:?}"
    );
    assert!(
        held.iter()
            .any(|r| r.starts_with("held preference/later.app.key")),
        "{held:?}"
    );
    assert!(
        held.iter()
            .any(|r| r.starts_with("base library.collections")),
        "{held:?}"
    );
    let backup = dir.0.join("a.obsbackup");
    a.back_up(&backup).unwrap();

    let (mut b, _) = UserStore::open(&dir.db()).unwrap();
    b.restore(&backup).unwrap();
    assert_eq!(
        schema_16_columns(&Connection::open(b.path()).unwrap()),
        held
    );
    assert_eq!(b.sync_conflicts().unwrap().len(), 1);
    assert_eq!(b.sync_held().unwrap().len(), 1);
}

#[test]
fn a_v15_backup_restores_with_no_conflicts_no_kept_rows_and_nothing_held() {
    let dir = Dir::new("v15-old-backup");
    let old = dir.0.join("old").join("user.db");
    let (v15, _) = UserStore::open_to(&old, 15).unwrap();
    let nb = notebook("Gone");
    v15.apply_changes(&[Change::insert(Row::Notebook(nb.clone()))])
        .unwrap();
    v15.apply_changes(&[Change::delete(Row::Notebook(nb))])
        .unwrap();
    list_and_delete(&v15);
    let backup = dir.0.join("v15.obsbackup");
    v15.back_up(&backup).unwrap();
    drop(v15);
    assert_eq!(UserStore::backup_contents(&backup).unwrap().version, 15);
    let held = everything(&Connection::open(&backup).unwrap());

    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    db.restore(&backup).unwrap();
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    let restored = everything(&Connection::open(db.path()).unwrap());
    for (table, rows) in &held {
        assert_eq!(&restored[table], rows, "{table}");
    }
    let columns = schema_16_columns(&Connection::open(db.path()).unwrap());
    assert!(
        columns.iter().all(|r| r.ends_with("=NULL")),
        "no copies, no kept rows, nothing held, no bases: {columns:?}"
    );
    assert!(db.sync_held().unwrap().is_empty());
    assert!(db.sync_conflicts().unwrap().is_empty());
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
