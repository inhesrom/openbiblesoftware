//! Snapshots (ADR 0022 rules 4–5, S28c): a snapshot is a Backup holding only Synced data and
//! what merging it needs, so nothing that stays on the device leaves it: no History, Last
//! session, recent entries, device-only preferences, remembered Resource names, outbox or
//! former ids; it reads back as a Backup and lists what the merge needs; it is written only
//! with an empty outbox; a join stamps every row with no clock yet.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use obs_store::annotation::new_id;
use obs_store::{Change, Notebook, Row, RowKey, SNAPSHOT_EMPTIED, SYNCED_TABLES, UserStore};
use rusqlite::Connection;
use rusqlite::types::Value;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("obs-snapshot-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

/// Every table of the SQLite file `bytes`, with its rows as text.
fn tables(dir: &Dir, bytes: &[u8]) -> Vec<(String, Vec<String>)> {
    let path = dir.0.join("snapshot.obsbackup");
    std::fs::write(&path, bytes).unwrap();
    let c = Connection::open(&path).unwrap();
    let names: Vec<String> = c
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    names
        .into_iter()
        .map(|t| {
            let mut stmt = c.prepare(&format!("SELECT * FROM \"{t}\"")).unwrap();
            let n = stmt.column_count();
            let rows: Vec<String> = stmt
                .query_map([], |r| {
                    Ok((0..n)
                        .map(|i| format!("{:?}", r.get::<_, Value>(i).unwrap()))
                        .collect::<Vec<_>>()
                        .join("|"))
                })
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            (t, rows)
        })
        .collect()
}

#[test]
fn a_snapshot_never_holds_what_stays_on_the_device() {
    let dir = Dir::new("local");
    let (user, _) = UserStore::open(&dir.0.join("user.db")).unwrap();
    user.record_visit("John 3:16", "{\"v\":\"Jn 3:16\"}")
        .unwrap();
    user.save_last_session("{\"tabs\":[]}").unwrap();
    user.save_resource_names(&[("kjv".into(), "KJV".into(), "King James".into())])
        .unwrap();
    for (key, value) in [
        ("search.history", "[\"love\"]"),
        ("search.recent.bible", "[\"grace\"]"),
        ("command-box.recent", "[\"Rom 8\"]"),
        ("view", "{\"theme\":\"dark\"}"),
        ("reading.visual_filters", "[]"),
        ("a key nobody listed", "1"),
        ("search.word_forms", "true"),
    ] {
        user.set_preference(key, value).unwrap();
    }
    user.join_sync().unwrap();
    user.apply_changes(&[Change::insert(Row::Notebook(notebook("Romans")))])
        .unwrap();
    // A snapshot of this outbox would cover ops no op file has yet; the engine writes them
    // first (`write_pending_ops`), as here.
    assert!(user.has_pending_ops().unwrap());
    for file in user.reserve_op_files().unwrap() {
        user.op_file_written(file.seq).unwrap();
    }
    user.execute_unchecked(&format!(
        "INSERT INTO former_device (device_id) VALUES ('{}')",
        new_id()
    ))
    .unwrap();
    let bytes = user.snapshot_bytes().unwrap();
    let tables = tables(&dir, &bytes);
    let rows = |table: &str| &tables.iter().find(|(t, _)| t == table).unwrap().1;
    for table in SNAPSHOT_EMPTIED {
        assert!(
            rows(table).is_empty(),
            "`{table}` stays on the device: {:?}",
            rows(table)
        );
    }
    let preferences = rows("preference");
    assert!(
        preferences.iter().all(|p| p.contains("search.word_forms")),
        "only Synced data: {preferences:?}"
    );
    for local in [
        "love",
        "grace",
        "Rom 8",
        "dark",
        "King James",
        "Jn 3:16",
        "tabs",
    ] {
        for (table, rows) in &tables {
            assert!(
                !rows.iter().any(|r| r.contains(local)),
                "`{local}` left the device in `{table}`"
            );
        }
    }
    assert!(rows("notebooks").iter().any(|r| r.contains("Romans")));
    let device = user.device_id().unwrap();
    assert!(rows("device").iter().any(|r| r.contains(&device)));
    assert!(rows("applied_up_to").iter().any(|r| r.contains(&device)));

    // It reads back as a Backup, with what the merge needs.
    let (snapshot, version) = user.open_snapshot(&bytes).unwrap();
    assert_eq!(version, obs_store::USER_SCHEMA_VERSION);
    let contents = snapshot.snapshot_contents().unwrap();
    assert_eq!(contents.device, device);
    assert_eq!(contents.applied.get(&device), Some(&1));
    // A join stamps every row (a Preferred order never set has none to stamp).
    let unclocked: Vec<_> = contents
        .rows
        .iter()
        .filter(|r| r.clock.is_none() && !matches!(r.row.key(), RowKey::PreferredOrder))
        .collect();
    assert!(unclocked.is_empty(), "{unclocked:?}");
    assert!(
        contents
            .rows
            .iter()
            .any(|r| matches!(&r.row, Row::Notebook(n) if n.name == "Romans"))
    );
    let first_child = contents
        .rows
        .iter()
        .position(|r| matches!(r.row.key(), RowKey::Annotation(_) | RowKey::Reading(..)));
    let last_parent = contents
        .rows
        .iter()
        .rposition(|r| matches!(r.row.key(), RowKey::Notebook(_) | RowKey::HighlightStyle(_)));
    assert!(
        first_child.is_none_or(|c| last_parent.is_none_or(|p| p < c)),
        "parents first"
    );
}

#[test]
fn a_join_gives_every_row_without_a_clock_one_and_starts_recording() {
    let dir = Dir::new("join");
    let (user, _) = UserStore::open(&dir.0.join("user.db")).unwrap();
    // The built-in Highlight styles and palette were made by a migration, with no clock.
    let c = Connection::open(dir.0.join("user.db")).unwrap();
    let unclocked = |c: &Connection| -> i64 {
        SYNCED_TABLES
            .iter()
            .filter(|t| **t != "preference")
            .map(|t| {
                c.query_row(
                    &format!("SELECT COUNT(*) FROM {t} WHERE clock IS NULL"),
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap()
            })
            .sum()
    };
    assert!(unclocked(&c) > 0);
    assert!(!user.records_ops().unwrap());
    user.join_sync().unwrap();
    assert!(user.records_ops().unwrap());
    assert_eq!(unclocked(&c), 0);
    // Joining again changes nothing.
    let dump = user.synced_dump().unwrap();
    user.join_sync().unwrap();
    assert_eq!(user.synced_dump().unwrap(), dump);
}
