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
//! and marking only the next reserved file written. A second connection (the sync worker's,
//! plan S29b) shares the clock: two connections writing at once on two threads hand out one
//! increasing sequence, none twice, and a new id one takes is the other's next clock's; once
//! its writes are refused (a restore is coming) it commits nothing; and a write on the first
//! waits only for the second's transaction running, not for each one after it. Migration 19's
//! merge cursor (S29c) is committed per batch, carried by a Backup and emptied by snapshots and
//! Replace.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use obs_store::annotation::new_id;
use obs_store::{
    Annotation, BatchEnd, Change, Content, HeldOp, Hlc, MergeCursor, Notebook, Row, RowKey,
    SYNCED_TABLES, StoreError, SyncedPreference, USER_SCHEMA_VERSION, UserStore,
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
            // before them, and `device.restore_choice` (17) is 0; the migration tests below
            // check that on their own.
            let columns = strings(
                c,
                &format!(
                    "SELECT name FROM pragma_table_info('{t}')
                     WHERE name NOT IN ('covers', 'row', 'row_covers', 'row_clock', 'conflict_of',
                                        'restore_choice')"
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
fn migrations_after_13_keep_every_row_of_a_v13_database_and_copy_it_first() {
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
    assert_eq!(USER_SCHEMA_VERSION, 19);
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    let copy = report.migration_backup.unwrap();
    assert!(copy.to_string_lossy().contains(".v13-"));
    assert_eq!(everything(&Connection::open(&copy).unwrap()), before);
    drop(db);

    let after = everything(&Connection::open(dir.db()).unwrap());
    for (table, rows) in &before {
        assert_eq!(&after[table], rows, "{table}");
    }
    for table in [
        "item_clock",
        "op_outbox",
        "sync_held",
        "former_device",
        "merge_cursor",
    ] {
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

/// A restored database holds every table of the Backup as it was (`held`), but for what ADR
/// 0022 rule 7 changes (S28c): the device has a new id, the one it had before (`before`)
/// among its former ones but not the Backup's (another database's), and the outbox (ops under
/// the old id) is empty.
fn assert_restored_as_held(db: &UserStore, held: &Tables, before: &str) {
    let restored = everything(&Connection::open(db.path()).unwrap());
    for (table, rows) in held {
        if ["device", "op_outbox", "former_device"].contains(&table.as_str()) {
            continue;
        }
        assert_eq!(&restored[table], rows, "{table}");
    }
    assert_eq!(restored["op_outbox"], Vec::<String>::new());
    let ids = db.own_device_ids().unwrap();
    assert!(ids[0] != before, "a restore gives the device a new id");
    assert_eq!(ids[1..], [before.to_string()], "its former id");
    for row in held.get("device").into_iter().flatten() {
        assert!(
            !ids.iter().any(|id| row.contains(id.as_str())),
            "another database's Backup's id is not this one's own: {ids:?} {row}"
        );
    }
}

#[test]
fn a_v13_backup_restores_into_18_losing_nothing() {
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
    let before = db.device_id().unwrap();
    db.restore(&backup).unwrap();
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    assert_restored_as_held(&db, &held, &before);
    // Restored, it writes clocks.
    let clock = add_notebook(&db, "After");
    assert_eq!(clock.device_id, db.device_id().unwrap());
}

#[test]
fn a_backup_carries_the_item_clocks_and_the_outbox_which_a_restore_empties() {
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
    let b_id = b.device_id().unwrap();
    b.restore(&backup).unwrap();
    let restored = everything(&Connection::open(b.path()).unwrap());
    for table in ["item_clock", "tombstone", "applied_up_to"] {
        assert_eq!(restored[table], held[table], "{table}");
    }
    // ADR 0022 rule 7: a new id, the one B had before kept as its former one, but not the
    // Backup's (A's, another device's: its ops are not B's own); the outbox's ops were A's,
    // and what they changed is in the rows.
    assert_eq!(restored["op_outbox"], Vec::<String>::new());
    let ids = b.own_device_ids().unwrap();
    let a_id = a.device_id().unwrap();
    assert!(ids[0] != a_id && ids[0] != b_id);
    assert_eq!(ids[1..], [b_id]);
    assert!(!b.records_ops().unwrap(), "the new id has not joined");
    assert!(!b.restore_choice_pending().unwrap(), "B was not syncing");
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
    let before = db.device_id().unwrap();
    db.restore(&backup).unwrap();
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    assert_restored_as_held(&db, &held, &before);
    let columns = schema_16_columns(&Connection::open(db.path()).unwrap());
    assert!(
        columns.iter().all(|r| r.ends_with("=NULL")),
        "no copies, no kept rows, nothing held, no bases: {columns:?}"
    );
    assert!(db.sync_held().unwrap().is_empty());
    assert!(db.sync_conflicts().unwrap().is_empty());
}

#[test]
fn a_v16_backup_restores_into_18_with_its_id_a_former_one_and_no_choice_waiting() {
    let dir = Dir::new("v16-own-backup");
    let (v16, _) = UserStore::open_to(&dir.db(), 16).unwrap();
    assert_eq!(v16.schema_version().unwrap(), 16);
    add_notebook(&v16, "Study");
    list_and_delete(&v16);
    let backup = dir.0.join("v16.obsbackup");
    v16.back_up(&backup).unwrap();
    let id = v16.device_id().unwrap();
    drop(v16);
    assert_eq!(UserStore::backup_contents(&backup).unwrap().version, 16);
    let held = everything(&Connection::open(&backup).unwrap());

    // The same database, now at 18, restores its own v16 Backup.
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(db.device_id().unwrap(), id);
    db.restore(&backup).unwrap();
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    let restored = everything(&Connection::open(db.path()).unwrap());
    for (table, rows) in &held {
        if !["device", "op_outbox"].contains(&table.as_str()) {
            assert_eq!(&restored[table], rows, "{table}");
        }
    }
    assert!(db.device_id().unwrap() != id, "a new id");
    assert_eq!(former_ids(&db), [id], "the Backup's id, this device's own");
    let c = Connection::open(db.path()).unwrap();
    assert_eq!(
        strings(&c, "SELECT CAST(restore_choice AS TEXT) FROM device"),
        ["0"]
    );
    assert!(!db.restore_choice_pending().unwrap());
}

/// `former_device`'s ids with their `seq`, by `seq`.
fn former_seqs(path: &Path) -> Vec<String> {
    strings(
        &Connection::open(path).unwrap(),
        "SELECT device_id || ' ' || seq FROM former_device ORDER BY seq",
    )
}

/// Migration 18 numbers a v17 database's former ids in their id order, a restore of its own
/// v17 Backup gives them their order and the id it gave up the next number, a copy
/// detection the next one after that, and a Backup keeps the numbers (the merge merges the
/// former ids' files the last given up first).
#[test]
fn former_ids_keep_the_order_they_were_given_up_through_migration_18_restores_and_backups() {
    let dir = Dir::new("former-order");
    let (v17, _) = UserStore::open_to(&dir.db(), 17).unwrap();
    let (older, newer) = (new_id(), new_id());
    for id in [&newer, &older] {
        v17.execute_unchecked(&format!(
            "INSERT INTO former_device (device_id) VALUES ('{id}')"
        ))
        .unwrap();
    }
    let x = v17.device_id().unwrap();
    let backup = dir.0.join("v17.obsbackup");
    v17.back_up(&backup).unwrap();
    drop(v17);
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    assert_eq!(
        former_seqs(&dir.db()),
        [format!("{older} 1"), format!("{newer} 2")]
    );
    assert_eq!(
        db.own_device_ids().unwrap(),
        [x.clone(), newer.clone(), older.clone()]
    );
    db.restore(&backup).unwrap();
    let n = db.device_id().unwrap();
    assert_eq!(
        former_seqs(&dir.db()),
        [format!("{older} 1"), format!("{newer} 2"), format!("{x} 3")]
    );
    assert_eq!(
        db.own_device_ids().unwrap(),
        [n.clone(), x.clone(), newer.clone(), older.clone()]
    );
    let m = db.take_new_device_id().unwrap();
    assert_eq!(
        db.own_device_ids().unwrap(),
        [m, n.clone(), x, newer, older]
    );
    let again = dir.0.join("v18.obsbackup");
    db.back_up(&again).unwrap();
    assert_eq!(former_seqs(&again), former_seqs(&dir.db()));
    assert_eq!(former_seqs(&again).last(), Some(&format!("{n} 4")));
}

#[test]
fn a_name_held_for_a_damaged_list_is_not_free_for_a_clash_rename() {
    let dir = Dir::new("held-name");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    db.set_preference("library.collections", r#"[{"name":"Study","n":1}]"#)
        .unwrap();
    let item = |name: &str, n: u32| {
        Row::Collection(obs_store::SyncedListItem {
            name: name.into(),
            position: 0,
            json: format!(r#"{{"n":{n},"name":"{name}"}}"#),
        })
    };
    // "Study (2)" waits in `sync_held` (its list was damaged when it came).
    let clock = add_notebook(&db, "Clock");
    let renamed = db
        .merge_held(|tx| {
            tx.hold(&HeldOp {
                clock,
                change: Change::insert(item("Study (2)", 3)),
                before_clock: None,
                absent: Vec::new(),
                why: "the list was damaged".into(),
            })?;
            tx.renamed_for_clash(&item("Study", 1))
        })
        .unwrap();
    let Some(Row::Collection(renamed)) = renamed else {
        panic!("{renamed:?}");
    };
    assert_eq!(renamed.name, "Study (3)", "the held item keeps its name");
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

/// The ids `former_device` holds, sorted.
fn former_ids(db: &UserStore) -> Vec<String> {
    strings(
        &Connection::open(db.path()).unwrap(),
        "SELECT device_id FROM former_device ORDER BY device_id",
    )
}

#[test]
fn a_restore_on_a_syncing_device_brings_its_new_id_former_ids_empty_outbox_and_choice_together() {
    let dir = Dir::new("restore-together");
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    db.start_recording_ops().unwrap();
    add_notebook(&db, "In the Backup");
    let path = dir.0.join("a.obsbackup");
    db.back_up(&path).unwrap();
    let id = db.device_id().unwrap();
    let outbox = "SELECT CAST(COUNT(*) AS TEXT) FROM op_outbox";
    assert_eq!(
        strings(&Connection::open(db.path()).unwrap(), outbox),
        ["1"]
    );
    db.restore(&path).unwrap();
    let c = Connection::open(db.path()).unwrap();
    let new = strings(&c, "SELECT device_id FROM device");
    assert!(new != [id.clone()], "a new id");
    assert_eq!(
        strings(&c, "SELECT CAST(restore_choice AS TEXT) FROM device"),
        ["1"]
    );
    assert_eq!(
        former_ids(&db),
        [id],
        "the Backup's id is this device's own"
    );
    assert_eq!(strings(&c, outbox), ["0"]);
    assert!(db.restore_choice_pending().unwrap());
}

#[test]
fn a_restore_that_fails_giving_the_new_id_leaves_the_database_as_it_was() {
    let dir = Dir::new("restore-atomic");
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    db.start_recording_ops().unwrap();
    add_notebook(&db, "In the Backup");
    let path = dir.0.join("a.obsbackup");
    db.back_up(&path).unwrap();
    add_notebook(&db, "After the Backup");
    let id = db.device_id().unwrap();
    let before = everything(&Connection::open(dir.db()).unwrap());
    let (backup, _) = UserStore::open_backup(&path).unwrap();
    // A stop between the steps: giving the restored data its new id fails.
    backup
        .execute_unchecked(
            "CREATE TRIGGER stop_renew BEFORE UPDATE OF device_id ON device
             BEGIN SELECT RAISE(ABORT, 'stopped'); END",
        )
        .unwrap();
    assert!(db.restore_from(&backup).is_err());
    // Not the Backup's data under its old id with no choice waiting: nothing restored.
    assert_eq!(everything(&Connection::open(dir.db()).unwrap()), before);
    assert_eq!(db.device_id().unwrap(), id);
    assert!(!db.restore_choice_pending().unwrap());
    assert!(db.records_ops().unwrap());
}

#[test]
fn a_restore_keeps_the_backups_id_as_a_former_one_only_when_it_is_this_devices_own() {
    let dir = Dir::new("restore-former");
    let (mut a, _) = UserStore::open(&dir.db()).unwrap();
    let first = a.device_id().unwrap();
    let own_backup = dir.0.join("a.obsbackup");
    a.back_up(&own_backup).unwrap();
    // Its own Backup, twice: the Backup's id is the one it had, then one of its former ones.
    a.restore(&own_backup).unwrap();
    let second = a.device_id().unwrap();
    assert_eq!(former_ids(&a), std::slice::from_ref(&first));
    a.restore(&own_backup).unwrap();
    let mut expected = vec![first.clone(), second.clone()];
    expected.sort();
    assert_eq!(former_ids(&a), expected);
    // Another device's Backup (B, itself restored once, so with a former id of its own): B's
    // ids are not this device's own; A's own stay.
    let (mut b, _) = UserStore::open(&dir.0.join("b").join("user.db")).unwrap();
    let b_first = b.device_id().unwrap();
    let b_backup = dir.0.join("b.obsbackup");
    b.back_up(&b_backup).unwrap();
    b.restore(&b_backup).unwrap();
    assert_eq!(former_ids(&b), std::slice::from_ref(&b_first));
    let b_backup = dir.0.join("b-restored.obsbackup");
    b.back_up(&b_backup).unwrap();
    let third = a.device_id().unwrap();
    a.restore(&b_backup).unwrap();
    expected.push(third);
    expected.sort();
    assert_eq!(former_ids(&a), expected);
    let ids = a.own_device_ids().unwrap();
    assert!(!ids.contains(&b_first) && !ids.contains(&b.device_id().unwrap()));
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

/// The Notebook rows' clocks, oldest first.
fn notebook_clocks(path: &Path) -> Vec<String> {
    strings(
        &Connection::open(path).unwrap(),
        "SELECT clock FROM notebooks WHERE clock IS NOT NULL ORDER BY clock",
    )
}

#[test]
fn two_connections_on_two_threads_hand_out_one_sequence_of_clocks() {
    let dir = Dir::new("two-connections");
    // Both threads' wall clocks stand still at the same millisecond, so every clock is told
    // apart by its counter alone: two connections counting on their own would hand out the
    // same clocks.
    const AT: i64 = 1_800_000_000_000;
    obs_store::clock::fix(Some(AT));
    let (ui, _) = UserStore::open(&dir.db()).unwrap();
    ui.start_recording_ops().unwrap();
    let second = ui.second_connection().unwrap();
    let start = std::sync::Arc::new(std::sync::Barrier::new(2));
    let worker = {
        let start = std::sync::Arc::clone(&start);
        std::thread::spawn(move || {
            obs_store::clock::fix(Some(AT));
            let sync = second.open().unwrap();
            start.wait();
            let mut mine = Vec::new();
            for i in 0..200 {
                mine.push(add_notebook(&sync, &format!("Worker {i}")));
                if i == 100 {
                    // A merge takes in another device's clock 5 seconds on.
                    assert!(sync.observe_clock(&Hlc {
                        wall_ms: 1_800_000_005_000,
                        counter: 7,
                        device_id: new_id(),
                    }));
                }
            }
            obs_store::clock::fix(None);
            mine
        })
    };
    start.wait();
    let mine: Vec<Hlc> = (0..200)
        .map(|i| add_notebook(&ui, &format!("UI {i}")))
        .collect();
    let theirs = worker.join().unwrap();
    for (who, clocks) in [("the UI's", &mine), ("the worker's", &theirs)] {
        assert!(
            clocks.windows(2).all(|w| w[0] < w[1]),
            "{who} clocks increase"
        );
    }
    let mut all: Vec<Hlc> = mine.iter().chain(&theirs).cloned().collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 400, "no clock handed out twice");
    let held = notebook_clocks(&dir.db());
    let unique: std::collections::BTreeSet<&String> = held.iter().collect();
    assert_eq!(unique.len(), held.len(), "every row its own clock");
    // Every change of both connections is in the outbox, once.
    let outbox = strings(
        &Connection::open(dir.db()).unwrap(),
        "SELECT clock FROM op_outbox",
    );
    assert_eq!(outbox.len(), 400);
    // The UI's next clock passes the one the worker observed.
    let next = add_notebook(&ui, "After");
    assert!(next.tick() > (1_800_000_005_000, 7), "{next}");
}

#[test]
fn a_new_id_taken_on_the_second_connection_is_the_first_connections_next_clocks() {
    let dir = Dir::new("new-id-elsewhere");
    let (ui, _) = UserStore::open(&dir.db()).unwrap();
    ui.start_recording_ops().unwrap();
    let old = add_notebook(&ui, "Before").device_id;
    let second = ui.second_connection().unwrap();
    let new = std::thread::spawn(move || second.open().unwrap().take_new_device_id().unwrap())
        .join()
        .unwrap();
    assert_ne!(new, old);
    assert_eq!(add_notebook(&ui, "After").device_id, new);
    assert_eq!(ui.device_id().unwrap(), new);
}

#[test]
fn a_second_connection_is_refused_at_another_schema_and_where_there_is_no_database() {
    let dir = Dir::new("second-refused");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let second = db.second_connection().unwrap();
    drop(db);
    Connection::open(dir.db())
        .unwrap()
        .pragma_update(None, "user_version", USER_SCHEMA_VERSION + 1)
        .unwrap();
    assert!(matches!(
        second.open(),
        Err(obs_store::StoreError::Rejected(_))
    ));
    std::fs::remove_file(dir.db()).unwrap();
    assert!(second.open().is_err(), "nothing is made at its path");
    assert!(!dir.db().exists());
}

#[test]
fn a_second_connection_writes_nothing_once_its_writes_are_refused() {
    let dir = Dir::new("second-refuses");
    let (ui, _) = UserStore::open(&dir.db()).unwrap();
    let second = ui.second_connection().unwrap();
    let sync = second.open().unwrap();
    add_notebook(&sync, "Before");
    assert!(!second.writes_refused());
    second.clone().refuse_writes();
    assert!(second.writes_refused(), "its clones share the switch");
    let before = everything(&Connection::open(dir.db()).unwrap());
    let nb = notebook("After");
    assert!(
        sync.apply_changes(&[Change::insert(Row::Notebook(nb))])
            .is_err()
    );
    assert!(
        sync.set_device_name("Refused").is_err(),
        "one statement too"
    );
    assert!(sync.take_new_device_id().is_err());
    assert!(
        second.open().unwrap().start_recording_ops().is_err(),
        "a connection opened after it too"
    );
    assert_eq!(everything(&Connection::open(dir.db()).unwrap()), before);
    // The first connection writes as before.
    add_notebook(&ui, "From the UI");
}

#[test]
fn a_write_on_the_first_connection_waits_only_for_the_transaction_running_on_the_second() {
    let dir = Dir::new("write-turns");
    let (ui, _) = UserStore::open(&dir.db()).unwrap();
    let second = ui.second_connection().unwrap();
    let busy = std::time::Duration::from_secs(2);
    let worker = std::thread::spawn(move || {
        // A merge: write transactions one after another, with no gap between them.
        let sync = second.open().unwrap();
        let start = std::time::Instant::now();
        let mut n = 0;
        while start.elapsed() < busy {
            let batch: Vec<Change> = (0..50)
                .map(|_| Change::insert(Row::Notebook(notebook(&new_id()))))
                .collect();
            sync.apply_changes(&batch).unwrap();
            n += 1;
        }
        n
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    let mut longest = std::time::Duration::ZERO;
    for i in 0..20 {
        let at = std::time::Instant::now();
        add_notebook(&ui, &format!("UI {i}"));
        longest = longest.max(at.elapsed());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let merged = worker.join().unwrap();
    assert!(merged > 20, "the worker kept writing ({merged})");
    // Waiting its turn behind every transaction would take the worker's whole 2 seconds.
    assert!(
        longest < std::time::Duration::from_millis(500),
        "an edit waited {longest:?}"
    );
}

/// Migration 19's `merge_cursor` (S29c): a batch that is not its file's last commits where the
/// merge stands, one that does not start there is refused, and the last drops it with the
/// file noted applied; a Backup carries it, a snapshot holds it empty, Replace empties it, and
/// a v18 Backup restores with none.
#[test]
fn the_merge_cursor_is_committed_per_batch_carried_by_a_backup_and_emptied_by_snapshots_and_replace()
 {
    let dir = Dir::new("merge-cursor");
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    db.join_sync().unwrap();
    let other = new_id();
    db.execute_unchecked(&format!(
        "INSERT INTO applied_up_to (device_id, seq) VALUES ('{other}', 0)"
    ))
    .unwrap();
    let batch = |from: u64, end: BatchEnd| {
        db.merge_op_file_batch(&other, 1, from, |_| Ok::<_, StoreError>(((), end)))
    };
    batch(0, BatchEnd::At(2)).unwrap();
    let cursor = MergeCursor {
        device: other.clone(),
        seq: 1,
        snapshot: None,
        ops: 2,
        fingerprint: None,
    };
    assert_eq!(db.merge_cursors().unwrap(), std::slice::from_ref(&cursor));
    assert!(db.has_partial_merge().unwrap());
    assert!(
        batch(0, BatchEnd::Done).is_err(),
        "a batch from another op than where it stands"
    );
    assert_eq!(
        db.applied_up_to(&other).unwrap(),
        0,
        "the file is not applied yet"
    );
    batch(2, BatchEnd::At(4)).unwrap();

    // A Backup keeps it; a snapshot does not.
    let backup = dir.0.join("cursor.obsbackup");
    db.back_up(&backup).unwrap();
    let c = Connection::open(&backup).unwrap();
    assert_eq!(
        strings(
            &c,
            "SELECT device_id || ' ' || seq || ' ' || ops FROM merge_cursor"
        ),
        [format!("{other} 1 4")]
    );
    let (snapshot, _) = db.open_snapshot(&db.snapshot_bytes().unwrap()).unwrap();
    assert!(snapshot.merge_cursors().unwrap().is_empty());

    batch(4, BatchEnd::Done).unwrap();
    assert!(db.merge_cursors().unwrap().is_empty());
    assert_eq!(db.applied_up_to(&other).unwrap(), 1);

    // A restore brings the Backup's back (with its rows); Replace empties it.
    db.restore(&backup).unwrap();
    assert_eq!(
        db.merge_cursors().unwrap(),
        [MergeCursor {
            ops: 4,
            ..cursor.clone()
        }]
    );
    db.restamp_for_replace().unwrap();
    assert!(db.merge_cursors().unwrap().is_empty());

    // A v18 Backup has none, and restores whole.
    let old = Dir::new("merge-cursor-v18");
    let (v18, _) = UserStore::open_to(&old.db(), 18).unwrap();
    add_notebook(&v18, "Study");
    let v18_backup = old.0.join("v18.obsbackup");
    v18.back_up(&v18_backup).unwrap();
    drop(v18);
    assert_eq!(UserStore::backup_contents(&v18_backup).unwrap().version, 18);
    db.restore(&v18_backup).unwrap();
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    assert!(db.merge_cursors().unwrap().is_empty());
}

/// From S29c's review: merge cursors come back in the order their merges started, not by
/// device (P2-2), a cursor moved on keeping its place and one started again going last; a
/// snapshot's keeps its fingerprint (P2-1); `drop_merge_cursors` (Stop syncing, another folder
/// chosen) forgets them all.
#[test]
fn merge_cursors_come_back_in_the_order_their_merges_started_and_go_all_at_once() {
    let dir = Dir::new("merge-cursor-order");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    db.join_sync().unwrap();
    // UUIDv7s: `late`'s id sorts after `early`'s, and its merge starts first.
    let early = new_id();
    let late = new_id();
    assert!(late > early);
    let file = |from: u64, end: BatchEnd| {
        db.merge_op_file_batch(&late, 1, from, |_| Ok::<_, StoreError>(((), end)))
    };
    file(0, BatchEnd::At(2)).unwrap();
    let name = "3-abc.obsbackup";
    db.merge_snapshot_batch(&early, 3, name, &BTreeMap::new(), 0, |_| {
        Ok::<_, StoreError>(((), BatchEnd::At(5), "fingerprint 5".to_string()))
    })
    .unwrap();
    let order = |db: &UserStore| -> Vec<String> {
        (db.merge_cursors().unwrap().into_iter())
            .map(|c| c.device)
            .collect()
    };
    assert_eq!(order(&db), [late.clone(), early.clone()]);
    assert_eq!(
        db.merge_cursor(&early).unwrap().unwrap(),
        MergeCursor {
            device: early.clone(),
            seq: 3,
            snapshot: Some(name.into()),
            ops: 5,
            fingerprint: Some("fingerprint 5".into()),
        }
    );
    // Moved on, it keeps its place.
    file(2, BatchEnd::At(4)).unwrap();
    assert_eq!(order(&db), [late.clone(), early.clone()]);
    // Dropped and started again, it goes after the other.
    db.drop_merge_cursor(&late).unwrap();
    file(0, BatchEnd::At(1)).unwrap();
    assert_eq!(order(&db), [early.clone(), late.clone()]);
    db.drop_merge_cursors().unwrap();
    assert!(db.merge_cursors().unwrap().is_empty());
    assert!(!db.has_partial_merge().unwrap());
}
