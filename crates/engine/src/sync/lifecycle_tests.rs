//! The lifecycle rules (ADR 0022 rules 5–8, plan S28c), one test per rule: two devices on one
//! disk converge end to end through `sync_now`, and a third joins late from their snapshots;
//! a late joiner from snapshots alone holds what merging needs (deleted parents' rows, list
//! bases, Sync conflicts' copies, held ops; what a snapshot never holds is
//! `obs-store`'s `tests/snapshot_db.rs`); a device's op files wait for
//! its first snapshot; trimming deletes only own files a snapshot covers, old enough and
//! merged by every device seen lately, and a device silent longer catches up from the
//! snapshot; a newer snapshot or folder is left alone; a copied database takes a new id; a
//! restore gives a new id, waits for the user's choice on a syncing device (not on another),
//! catches up, or replaces what is in the folder so a third device converges to the restored
//! data; and a text written under the old id still survives a remote edit that never saw it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use obs_store::annotation::new_id;
use obs_store::{Annotation, Change, Content, Notebook, Row, RowKey, UserStore};

use super::folder::{FsSyncFolder, MemSyncFolder, SyncFolder};
use super::lifecycle::{
    RestoreChoice, SyncOptions, SyncReport, SyncState, needs_restore_choice, restore_choice,
    sync_now,
};
use super::merge_tests::Dir;
use super::ops::{OpsStop, op_file_path, op_file_seq, ops_dir};
use super::snapshot::{SnapshotPolicy, newest_snapshot, snapshots_dir, write_snapshot};
use super::{DEVICES_DIR, SYNC_JSON};

/// A device's User database in `dir`, named `name`.
fn open(dir: &Dir, name: &str) -> UserStore {
    UserStore::open(&dir.0.join(name).join("user.db"))
        .unwrap()
        .0
}

fn options() -> SyncOptions {
    SyncOptions::new("macos")
}

/// Sync `user` now, expecting it to sync.
fn sync(user: &UserStore, folder: &dyn SyncFolder) -> SyncReport {
    sync_with(user, folder, &options())
}

fn sync_with(user: &UserStore, folder: &dyn SyncFolder, options: &SyncOptions) -> SyncReport {
    match sync_now(user, folder, options).unwrap() {
        SyncState::Synced(report) => *report,
        other => panic!("{other:?}"),
    }
}

/// Sync every device until nothing more is written or merged.
fn settle(devices: &[&UserStore], folder: &dyn SyncFolder) {
    for _ in 0..20 {
        let mut busy = false;
        for user in devices {
            let report = sync(user, folder);
            busy |= !report.written.is_empty()
                || !report.merge.merged.is_empty()
                || !report.merge.snapshots.is_empty();
        }
        if !busy {
            return;
        }
    }
    panic!("the devices did not settle");
}

fn converged(devices: &[&UserStore]) {
    let first = devices[0].synced_dump().unwrap();
    for user in &devices[1..] {
        assert_eq!(user.synced_dump().unwrap(), first);
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

fn note(notebook: &str, body: &str) -> Annotation {
    Annotation {
        id: new_id(),
        notebook_id: notebook.into(),
        content: Content::Note { body: body.into() },
        anchor: None,
        created_at: 1,
        modified_at: 1,
        conflict_of: None,
    }
}

fn with_text(a: &Annotation, body: &str) -> Annotation {
    Annotation {
        content: Content::Note { body: body.into() },
        modified_at: a.modified_at + 1,
        ..a.clone()
    }
}

fn add(user: &UserStore, row: Row) {
    user.apply_changes(&[Change::insert(row)]).unwrap();
}

fn default_notebook(user: &UserStore) -> String {
    user.notebooks().unwrap().remove(0).id
}

fn names(user: &UserStore) -> Vec<String> {
    let mut names: Vec<String> = user
        .notebooks()
        .unwrap()
        .into_iter()
        .map(|n| n.name)
        .collect();
    names.sort();
    names
}

fn texts(user: &UserStore) -> Vec<String> {
    let mut texts: Vec<String> = user
        .annotations(None)
        .unwrap()
        .into_iter()
        .map(|a| a.body().to_string())
        .collect();
    texts.sort();
    texts
}

#[test]
fn two_devices_on_one_disk_converge_end_to_end_and_a_third_joins_late_from_snapshots() {
    let dir = Dir::new("end-to-end");
    let folder = FsSyncFolder::new(dir.0.join("cloud"));
    let (a, b) = (open(&dir, "a"), open(&dir, "b"));
    // Made before either syncs: only their join snapshots bring these to the other.
    add(&a, Row::Notebook(notebook("Romans")));
    add(&b, Row::Notebook(notebook("Greek")));
    let report = sync(&a, &folder);
    assert!(report.joined && report.snapshot == Some(0), "{report:?}");
    assert!(a.records_ops().unwrap());
    sync(&b, &folder);
    let nb = a.notebooks().unwrap().remove(0).id;
    add(&a, Row::Annotation(note(&nb, "by A")));
    add(&b, Row::Notebook(notebook("Hebrew")));
    settle(&[&a, &b], &folder);
    converged(&[&a, &b]);
    for name in ["Romans", "Greek", "Hebrew"] {
        assert!(names(&a).contains(&name.to_string()), "{name}");
    }
    assert!(folder.root().join(SYNC_JSON).is_file());
    for user in [&a, &b] {
        let id = user.device_id().unwrap();
        assert!(newest_snapshot(&folder, &id).unwrap().is_some());
        assert!(
            folder
                .read(&super::device_json_path(&id))
                .is_ok_and(|j| String::from_utf8_lossy(&j).contains("\"synced_at\""))
        );
    }

    // C joins late: it merges A's and B's newest snapshots, then the op files after them.
    let c = open(&dir, "c");
    add(&c, Row::Notebook(notebook("Psalms")));
    let report = sync(&c, &folder);
    assert_eq!(report.merge.snapshots.len(), 2, "{report:?}");
    settle(&[&a, &b, &c], &folder);
    converged(&[&a, &b, &c]);
    assert!(names(&a).contains(&"Psalms".to_string()));
    assert_eq!(texts(&c), ["by A"]);
}

#[test]
fn a_late_joiner_from_snapshots_holds_what_the_others_hold_deleted_parents_bases_copies_and_held_ops()
 {
    let dir = Dir::new("snapshot-carries");
    let folder = MemSyncFolder::new();
    let (a, b) = (open(&dir, "a"), open(&dir, "b"));
    sync(&a, &folder);
    sync(&b, &folder);
    // A deleted parent's row (D3), a list's base (D7), a Sync conflict's copy (D2).
    let gone = notebook("Gone");
    add(&a, Row::Notebook(gone.clone()));
    a.apply_changes(&[Change::delete(Row::Notebook(gone.clone()))])
        .unwrap();
    a.set_preference("library.collections", "not a list")
        .unwrap();
    a.set_preference(
        "library.collections",
        r#"[{"name":"Greek","ids":[]},{"name":"Hebrew","ids":[]}]"#,
    )
    .unwrap();
    a.set_preference(
        "library.collections",
        r#"[{"name":"Greek","ids":["sblgnt"]},{"name":"Hebrew","ids":[]}]"#,
    )
    .unwrap();
    let nb = default_notebook(&a);
    let n = note(&nb, "first");
    add(&a, Row::Annotation(n.clone()));
    settle(&[&a, &b], &folder);
    b.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "by B")),
    )])
    .unwrap();
    a.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "by A")),
    )])
    .unwrap();
    sync(&a, &folder);
    sync(&b, &folder);
    // A held op: a synced preference a later app added, from a device both have met.
    let later = new_id();
    let unknown = super::ops::Op {
        device: later.clone(),
        clock: obs_store::Hlc {
            wall_ms: u64::try_from(obs_store::clock::now_ms()).unwrap(),
            counter: 0,
            device_id: later.clone(),
        },
        before: None,
        after: Some(Row::Preference(obs_store::SyncedPreference {
            key: "study.later_app_setting".into(),
            value: "1".into(),
        })),
        before_clock: None,
        absent: Vec::new(),
    };
    folder.put(
        &op_file_path(&later, 1),
        &super::ops::encode_op_file(&[unknown]).unwrap(),
    );
    for user in [&a, &b] {
        user.execute_unchecked(&format!(
            "INSERT INTO applied_up_to (device_id, seq) VALUES ('{later}', 0)"
        ))
        .unwrap();
    }
    settle(&[&a, &b], &folder);
    converged(&[&a, &b]);
    assert_eq!(a.sync_conflicts().unwrap().len(), 1);
    assert_eq!(a.sync_held().unwrap().len(), 1);
    // Snapshots of both, then C from them alone: every op file they cover is gone.
    for user in [&a, &b] {
        write_snapshot(user, &folder).unwrap();
        let id = user.device_id().unwrap();
        for name in folder.list(&ops_dir(&id)).unwrap() {
            folder.remove(&format!("{}/{name}", ops_dir(&id))).unwrap();
        }
    }
    let c = open(&dir, "c");
    let report = sync(&c, &folder);
    assert_eq!(report.merge.snapshots.len(), 2, "{report:?}");
    assert!(report.merge.stopped.is_empty(), "{report:?}");
    settle(&[&a, &b, &c], &folder);
    // C holds everything A and B do, whichever device's snapshot gave it what.
    let dump = |u: &UserStore| {
        u.synced_dump()
            .unwrap()
            .lines()
            .filter(|l| !l.contains("My Notebook"))
            .map(String::from)
            .collect::<Vec<_>>()
    };
    let (da, dc) = (dump(&a), dump(&c));
    for line in &da {
        assert!(dc.contains(line), "C lacks {line}");
    }
    assert_eq!(c.sync_conflicts().unwrap().len(), 1);
    assert_eq!(
        c.sync_held().unwrap().len(),
        1,
        "the held op came in a snapshot"
    );
    let tombstone = c
        .row_state(&RowKey::Notebook(gone.id.clone()))
        .unwrap()
        .tombstone;
    assert!(tombstone.is_some());
    assert!(
        dc.iter().any(|l| l.contains("Gone")),
        "the tombstone keeps its row"
    );
    converged(&[&a, &b, &c]);
}

#[test]
fn op_files_of_a_device_wait_for_its_first_snapshot() {
    let dir = Dir::new("snapshot-first");
    let folder = MemSyncFolder::new();
    let (a, b) = (open(&dir, "a"), open(&dir, "b"));
    add(&a, Row::Notebook(notebook("Before joining")));
    sync(&a, &folder);
    add(&a, Row::Notebook(notebook("After joining")));
    sync(&a, &folder);
    // B's cloud has brought A's op file but not yet its snapshot.
    let a_id = a.device_id().unwrap();
    let partial = MemSyncFolder::new();
    for path in folder.paths() {
        if !path.contains("/snapshots/") {
            partial.put(&path, &folder.get(&path).unwrap());
        }
    }
    let report = sync(&b, &partial);
    assert_eq!(
        report.merge.stopped,
        [(a_id.clone(), 0, OpsStop::NoSnapshot)]
    );
    assert!(!names(&b).contains(&"After joining".to_string()));
    for path in folder.paths() {
        partial.put(&path, &folder.get(&path).unwrap());
    }
    let report = sync(&b, &partial);
    assert_eq!(report.merge.snapshots, [(a_id, 0)]);
    assert!(names(&b).contains(&"Before joining".to_string()));
    assert!(names(&b).contains(&"After joining".to_string()));
}

/// Options trimming at a test age and writing a snapshot every two files.
fn trimming(trim_after_ms: i64) -> SyncOptions {
    SyncOptions {
        platform: "macos".into(),
        policy: SnapshotPolicy {
            after_files: 2,
            after_ms: i64::MAX,
            trim_after_ms,
        },
    }
}

#[test]
fn trimming_never_removes_ops_a_known_device_has_not_applied() {
    let dir = Dir::new("trim");
    let folder = MemSyncFolder::new();
    let (a, b) = (open(&dir, "a"), open(&dir, "b"));
    let day = 24 * 60 * 60 * 1000;
    let t0 = obs_store::clock::now_ms();
    obs_store::clock::fix(Some(t0));
    let opts = trimming(90 * day);
    let keep_all = trimming(i64::MAX);
    sync_with(&a, &folder, &opts);
    sync_with(&b, &folder, &keep_all);
    let a_id = a.device_id().unwrap();
    let files = || -> Vec<u64> {
        let mut seqs: Vec<u64> = folder
            .list(&ops_dir(&a_id))
            .unwrap()
            .iter()
            .filter_map(|n| op_file_seq(n))
            .collect();
        seqs.sort_unstable();
        seqs
    };
    for name in ["One", "Two"] {
        add(&a, Row::Notebook(notebook(name)));
        sync_with(&a, &folder, &opts);
    }
    sync_with(&b, &folder, &keep_all);
    for name in ["Three", "Four"] {
        add(&a, Row::Notebook(notebook(name)));
        sync_with(&a, &folder, &opts);
    }
    assert_eq!(files(), [1, 2, 3, 4]);
    assert_eq!(newest_snapshot(&folder, &a_id).unwrap().unwrap().0, 4);
    // 89 days on, B syncs seeing none of A's later files (its cloud has not brought them):
    // it is seen lately, having merged A's files up to 2.
    obs_store::clock::fix(Some(t0 + 89 * day));
    let partial = MemSyncFolder::new();
    for path in folder.paths() {
        let later = [3, 4].iter().any(|n| path == op_file_path(&a_id, *n));
        if !later && !path.contains("/snapshots/") {
            partial.put(&path, &folder.get(&path).unwrap());
        }
    }
    sync_with(&b, &partial, &keep_all);
    let b_json = super::device_json_path(&b.device_id().unwrap());
    folder.put(&b_json, &partial.get(&b_json).unwrap());
    // 91 days after A wrote them, every file is old enough and covered by A's snapshot, but
    // B has merged only 1 and 2.
    obs_store::clock::fix(Some(t0 + 91 * day));
    let report = sync_with(&a, &folder, &opts);
    assert_eq!(report.trimmed, [1, 2], "{report:?}");
    assert_eq!(files(), [3, 4]);
    // B, silent for more than 90 days, no longer holds them back.
    obs_store::clock::fix(Some(t0 + 180 * day));
    add(&a, Row::Notebook(notebook("Five")));
    let report = sync_with(&a, &folder, &opts);
    assert_eq!(report.trimmed, [3, 4], "{report:?}");
    assert_eq!(files(), [5]);
    // It catches up from A's snapshot, then A's op files after it.
    let report = sync_with(&b, &folder, &keep_all);
    assert_eq!(report.merge.snapshots, [(a_id.clone(), 4)], "{report:?}");
    assert!(report.merge.stopped.is_empty(), "{report:?}");
    obs_store::clock::fix(None);
    settle(&[&a, &b], &folder);
    converged(&[&a, &b]);
    for name in ["Three", "Four", "Five"] {
        assert!(names(&b).contains(&name.to_string()), "{name}");
    }
}

#[test]
fn a_newer_snapshot_or_folder_is_left_alone_and_asks_for_an_update() {
    let dir = Dir::new("newer");
    let folder = MemSyncFolder::new();
    let (a, b) = (open(&dir, "a"), open(&dir, "b"));
    sync(&a, &folder);
    let a_id = a.device_id().unwrap();
    // A's snapshot as a later app's: its schema is one this build does not read.
    let (seq, name) = newest_snapshot(&folder, &a_id).unwrap().unwrap();
    let path = format!("{}/{name}", snapshots_dir(&a_id));
    let mut bytes = folder.get(&path).unwrap();
    let later = obs_store::USER_SCHEMA_VERSION + 1;
    bytes[60..64].copy_from_slice(&later.to_be_bytes());
    folder.remove(&path).unwrap();
    let hash = crate::starter::sha256_hex(&bytes);
    let newer = format!(
        "{}/{}",
        snapshots_dir(&a_id),
        super::snapshot::snapshot_file_name(seq, &hash)
    );
    folder.put(&newer, &bytes);
    let report = sync(&b, &folder);
    assert_eq!(report.merge.needs_newer_app(), std::slice::from_ref(&a_id));
    assert_eq!(folder.get(&newer).unwrap(), bytes, "left alone");
    assert!(
        !report.written.is_empty() || b.records_ops().unwrap(),
        "B still syncs its own"
    );
    // A changed snapshot (its hash not its name's) is skipped and read again later.
    folder.put(&path, b"not a snapshot");
    folder.remove(&newer).unwrap();
    let report = sync(&b, &folder);
    assert!(
        matches!(
            &report.merge.stopped[..],
            [(_, _, OpsStop::SnapshotNotWhole(_))]
        ),
        "{report:?}"
    );
    // A folder set up by a newer app: nothing is read or written.
    let other = MemSyncFolder::new();
    other.put(SYNC_JSON, b"{\"format\":2}\n");
    assert_eq!(
        sync_now(&a, &other, &options()).unwrap(),
        SyncState::FolderNewer { format: 2 }
    );
    assert_eq!(other.paths(), [SYNC_JSON.to_string()]);
}

#[test]
fn a_copied_database_takes_a_new_id_and_catches_up() {
    let dir = Dir::new("copied");
    let folder = MemSyncFolder::new();
    let a = open(&dir, "a");
    add(&a, Row::Notebook(notebook("Romans")));
    sync(&a, &folder);
    let a_id = a.device_id().unwrap();
    // The database copied to another computer, then A goes on syncing.
    let copy = a.copy_in_memory().unwrap();
    add(&a, Row::Notebook(notebook("On A")));
    sync(&a, &folder);
    add(&copy, Row::Notebook(notebook("On the copy")));
    let report = sync(&copy, &folder);
    let new = report.new_device_id.clone().unwrap();
    assert!(new != a_id && report.joined, "{report:?}");
    assert_eq!(copy.own_device_ids().unwrap(), [new.clone(), a_id.clone()]);
    // The copy's edit made before it took its id reaches A through its join snapshot.
    settle(&[&a, &copy], &folder);
    converged(&[&a, &copy]);
    assert!(names(&a).contains(&"On the copy".to_string()));
    assert!(names(&copy).contains(&"On A".to_string()));
    let mut ids: Vec<String> = folder.list(DEVICES_DIR).unwrap();
    ids.sort();
    let mut expected = vec![a_id, new];
    expected.sort();
    assert_eq!(ids, expected);
}

#[test]
fn a_copy_that_writes_first_leaves_the_original_to_take_a_new_id() {
    let dir = Dir::new("copied-first");
    let folder = MemSyncFolder::new();
    let a = open(&dir, "a");
    sync(&a, &folder);
    let a_id = a.device_id().unwrap();
    let copy = a.copy_in_memory().unwrap();
    add(&copy, Row::Notebook(notebook("On the copy")));
    assert!(
        sync(&copy, &folder).new_device_id.is_none(),
        "it wrote first"
    );
    add(&a, Row::Notebook(notebook("On A")));
    let report = sync(&a, &folder);
    assert!(report.new_device_id.is_some(), "{report:?}");
    assert!(a.device_id().unwrap() != a_id);
    settle(&[&a, &copy], &folder);
    converged(&[&a, &copy]);
    assert!(names(&copy).contains(&"On A".to_string()));
}

/// Back up `user` to a file in `dir`.
fn back_up(dir: &Dir, user: &UserStore, name: &str) -> std::path::PathBuf {
    let path = dir.0.join(format!("{name}.obsbackup"));
    user.back_up(&path).unwrap();
    path
}

#[test]
fn a_restore_gives_a_new_id_and_on_a_syncing_device_waits_for_the_users_choice() {
    let dir = Dir::new("restore-wait");
    let folder = MemSyncFolder::new();
    let mut a = open(&dir, "a");
    let backup = back_up(&dir, &a, "before-sync");
    // Not syncing: a new id, no choice.
    let first = a.device_id().unwrap();
    a.restore(&backup).unwrap();
    assert!(a.device_id().unwrap() != first);
    assert!(!needs_restore_choice(&a).unwrap());
    sync(&a, &folder);
    let backup = back_up(&dir, &a, "syncing");
    add(&a, Row::Notebook(notebook("After the Backup")));
    sync(&a, &folder);
    a.restore(&backup).unwrap();
    assert!(needs_restore_choice(&a).unwrap());
    assert!(!a.records_ops().unwrap());
    assert_eq!(
        sync_now(&a, &folder, &options()).unwrap(),
        SyncState::NeedsRestoreChoice
    );
    // The choice survives a restart.
    let path = a.path().to_path_buf();
    drop(a);
    let (a, _) = UserStore::open(&path).unwrap();
    assert!(needs_restore_choice(&a).unwrap());
    let SyncState::Synced(report) =
        restore_choice(&a, &folder, RestoreChoice::CatchUp, &options()).unwrap()
    else {
        panic!()
    };
    assert!(report.joined);
    assert!(!needs_restore_choice(&a).unwrap());
    // Catching up brings back what the Backup did not have.
    assert!(names(&a).contains(&"After the Backup".to_string()));
    assert!(restore_choice(&a, &folder, RestoreChoice::CatchUp, &options()).is_err());
}

#[test]
fn a_restored_database_catches_up_with_the_others_by_default() {
    let dir = Dir::new("restore-catch-up");
    let folder = MemSyncFolder::new();
    let (mut a, b) = (open(&dir, "a"), open(&dir, "b"));
    settle(&[&a, &b], &folder);
    let nb = default_notebook(&a);
    let kept = note(&nb, "kept");
    add(&a, Row::Annotation(kept.clone()));
    settle(&[&a, &b], &folder);
    let backup = back_up(&dir, &a, "a");
    // After the Backup: B edits the Note, A deletes a Notebook and adds one.
    b.apply_changes(&[Change::update(
        Row::Annotation(kept.clone()),
        Row::Annotation(with_text(&kept, "edited on B")),
    )])
    .unwrap();
    add(&a, Row::Notebook(notebook("Made on A")));
    settle(&[&a, &b], &folder);
    a.restore(&backup).unwrap();
    assert!(!names(&a).contains(&"Made on A".to_string()));
    restore_choice(&a, &folder, RestoreChoice::CatchUp, &options()).unwrap();
    settle(&[&a, &b], &folder);
    converged(&[&a, &b]);
    assert!(
        names(&a).contains(&"Made on A".to_string()),
        "A's own later edit"
    );
    assert_eq!(texts(&a), ["edited on B"]);
}

#[test]
fn a_restored_database_with_replace_makes_a_third_device_converge_to_the_restored_state() {
    let dir = Dir::new("restore-replace");
    let folder = MemSyncFolder::new();
    let (mut a, b) = (open(&dir, "a"), open(&dir, "b"));
    settle(&[&a, &b], &folder);
    let nb = default_notebook(&a);
    let kept = note(&nb, "as backed up");
    let romans = notebook("Romans");
    add(&a, Row::Annotation(kept.clone()));
    add(&a, Row::Notebook(romans.clone()));
    let deleted = notebook("Deleted before the Backup");
    add(&a, Row::Notebook(deleted.clone()));
    settle(&[&a, &b], &folder);
    a.apply_changes(&[Change::delete(Row::Notebook(deleted.clone()))])
        .unwrap();
    settle(&[&a, &b], &folder);
    let backup = back_up(&dir, &a, "a");
    // After the Backup: A edits the Note and deletes Romans; B brings the deleted Notebook
    // back (an undo) and makes a Notebook of its own.
    a.apply_changes(&[Change::update(
        Row::Annotation(kept.clone()),
        Row::Annotation(with_text(&kept, "edited on A after")),
    )])
    .unwrap();
    a.apply_changes(&[Change::delete(Row::Notebook(romans.clone()))])
        .unwrap();
    settle(&[&a, &b], &folder);
    b.apply_changes(&[Change::insert(Row::Notebook(deleted.clone()))])
        .unwrap();
    let made_on_b = notebook("Made on B after");
    add(&b, Row::Notebook(made_on_b.clone()));
    settle(&[&a, &b], &folder);
    assert!(names(&b).contains(&"Deleted before the Backup".to_string()));

    a.restore(&backup).unwrap();
    let report = restore_choice(&a, &folder, RestoreChoice::Replace, &options()).unwrap();
    assert!(matches!(report, SyncState::Synced(_)));
    settle(&[&a, &b], &folder);
    let c = open(&dir, "c");
    settle(&[&a, &b, &c], &folder);
    converged(&[&a, &b, &c]);
    for user in [&a, &b, &c] {
        let names = names(user);
        assert!(names.contains(&"Romans".to_string()), "back, as backed up");
        assert!(
            !names.contains(&"Deleted before the Backup".to_string()),
            "deleted, as backed up"
        );
        assert!(
            names.contains(&"Made on B after".to_string()),
            "never known to the Backup: kept"
        );
        assert_eq!(
            texts(user),
            ["as backed up"],
            "A's later text replaced on purpose"
        );
    }
}

#[test]
fn a_replace_keeps_another_devices_own_unseen_text_as_a_sync_conflict() {
    let dir = Dir::new("restore-replace-copy");
    let folder = MemSyncFolder::new();
    let (mut a, b) = (open(&dir, "a"), open(&dir, "b"));
    settle(&[&a, &b], &folder);
    let nb = default_notebook(&a);
    let n = note(&nb, "as backed up");
    add(&a, Row::Annotation(n.clone()));
    settle(&[&a, &b], &folder);
    let backup = back_up(&dir, &a, "a");
    b.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "B's own later text")),
    )])
    .unwrap();
    settle(&[&a, &b], &folder);
    a.restore(&backup).unwrap();
    restore_choice(&a, &folder, RestoreChoice::Replace, &options()).unwrap();
    settle(&[&a, &b], &folder);
    converged(&[&a, &b]);
    let in_place = a.annotation(&n.id).unwrap().unwrap();
    assert_eq!(in_place.body(), "as backed up");
    let copies = a.sync_conflicts().unwrap();
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(copies[0].body(), "B's own later text");
}

#[test]
fn a_text_written_under_the_old_id_survives_a_remote_edit_that_never_saw_it() {
    let dir = Dir::new("restore-own");
    let folder = MemSyncFolder::new();
    let (mut a, b) = (open(&dir, "a"), open(&dir, "b"));
    settle(&[&a, &b], &folder);
    let nb = default_notebook(&a);
    let n = note(&nb, "first");
    add(&a, Row::Annotation(n.clone()));
    settle(&[&a, &b], &folder);
    // A writes T and syncs; B, not having synced, edits to V later; A is backed up and then
    // restored (a new id) before it meets B's edit.
    a.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "T on A")),
    )])
    .unwrap();
    sync(&a, &folder);
    let backup = back_up(&dir, &a, "a");
    b.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "V on B")),
    )])
    .unwrap();
    // B's later V stays in place; A, whose own T lost, would make the copy.
    let b_report = sync(&b, &folder);
    assert_eq!(b_report.merge.recorded.conflicts, 0, "{b_report:?}");
    a.restore(&backup).unwrap();
    restore_choice(&a, &folder, RestoreChoice::CatchUp, &options()).unwrap();
    settle(&[&a, &b], &folder);
    converged(&[&a, &b]);
    let texts = texts(&a);
    assert!(texts.contains(&"T on A".to_string()), "{texts:?}");
    assert!(texts.contains(&"V on B".to_string()), "{texts:?}");
}

#[test]
fn a_former_ids_text_merged_after_an_edit_that_never_saw_it_is_kept_as_a_copy() {
    let dir = Dir::new("restore-former-late");
    let folder = MemSyncFolder::new();
    let (mut a, b) = (open(&dir, "a"), open(&dir, "b"));
    settle(&[&a, &b], &folder);
    let nb = default_notebook(&a);
    let n = note(&nb, "first");
    add(&a, Row::Annotation(n.clone()));
    settle(&[&a, &b], &folder);
    // A is backed up, then writes T; B, never seeing T, writes V later. A is restored from the
    // Backup (it never had T) and catches up seeing only B's files first: V is in place before
    // A's own T arrives, from its old id, and loses to it.
    let backup = back_up(&dir, &a, "a");
    let a_id = a.device_id().unwrap();
    a.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "T on A")),
    )])
    .unwrap();
    sync(&a, &folder);
    b.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "V on B")),
    )])
    .unwrap();
    super::ops::write_pending_ops(&b, &folder).unwrap();
    a.restore(&backup).unwrap();
    let only_b = MemSyncFolder::new();
    for path in folder.paths() {
        if !path.contains(&format!("{a_id}/ops/")) {
            only_b.put(&path, &folder.get(&path).unwrap());
        }
    }
    restore_choice(&a, &only_b, RestoreChoice::CatchUp, &options()).unwrap();
    assert_eq!(a.annotation(&n.id).unwrap().unwrap().body(), "V on B");
    let report = sync(&a, &folder);
    assert_eq!(report.merge.recorded.conflicts, 1, "{report:?}");
    settle(&[&a, &b], &folder);
    converged(&[&a, &b]);
    let texts = texts(&a);
    assert!(texts.contains(&"T on A".to_string()), "{texts:?}");
    assert!(texts.contains(&"V on B".to_string()), "{texts:?}");
}

#[test]
fn a_former_ids_files_merge_op_by_op_when_a_snapshot_of_a_device_met_after_the_backup_covers_them()
{
    let dir = Dir::new("restore-former-snapshot");
    let folder = MemSyncFolder::new();
    let mut a = open(&dir, "a");
    sync(&a, &folder);
    let nb = default_notebook(&a);
    let n = note(&nb, "first");
    add(&a, Row::Annotation(n.clone()));
    sync(&a, &folder);
    // A is backed up before C joins: the Backup has never met C.
    let backup = back_up(&dir, &a, "a");
    let c = open(&dir, "c");
    sync(&c, &folder);
    // A writes T; C, never seeing T, writes V later, then merges T (V stays: T is not C's
    // own, so C makes no copy) and writes a snapshot covering A's file of T.
    a.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "T on A")),
    )])
    .unwrap();
    sync(&a, &folder);
    c.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "V on C")),
    )])
    .unwrap();
    let c_report = sync(&c, &folder);
    assert_eq!(c_report.merge.recorded.conflicts, 0, "{c_report:?}");
    write_snapshot(&c, &folder).unwrap();
    // A, restored, catches up: it meets C through C's snapshot, which covers A's old id's
    // file of T. That file is A's own history and merges op by op, so T is kept as a copy.
    a.restore(&backup).unwrap();
    restore_choice(&a, &folder, RestoreChoice::CatchUp, &options()).unwrap();
    settle(&[&a, &c], &folder);
    converged(&[&a, &c]);
    let texts = texts(&a);
    assert!(texts.contains(&"T on A".to_string()), "{texts:?}");
    assert!(texts.contains(&"V on C".to_string()), "{texts:?}");
}

/// The merge property test's seed 3146456, shrunk: A's Collection "Greek" (n 1) is in a
/// Backup; A then deletes it and C joins, its snapshot holding that tombstone; A adds another
/// "Greek" (n 9) and restores the Backup, catching up. C's snapshot brings the delete with no
/// `before`, covering the Backup's "Greek": it had seen it, so it deletes it (D6's unseen
/// delete keeps only a version it does not cover), and A's old id's "Greek" (n 9) is in place
/// on every device. Before the fix the delete counted as unseen, A recorded "Greek" (n 1)
/// again at a new clock, and that beat the old id's later "Greek" (n 9), which was lost.
#[test]
fn a_snapshots_delete_covering_this_devices_own_item_deletes_it() {
    let dir = Dir::new("restore-snapshot-delete-item");
    let folder = MemSyncFolder::new();
    let mut a = open(&dir, "a");
    let key = "library.collections";
    a.set_preference(key, r#"[{"name":"Greek","n":1}]"#)
        .unwrap();
    sync(&a, &folder);
    let backup = back_up(&dir, &a, "a");
    a.set_preference(key, "[]").unwrap();
    sync(&a, &folder);
    let c = open(&dir, "c");
    sync(&c, &folder);
    write_snapshot(&c, &folder).unwrap();
    a.set_preference(key, r#"[{"name":"Greek","n":9}]"#)
        .unwrap();
    sync(&a, &folder);
    a.restore(&backup).unwrap();
    restore_choice(&a, &folder, RestoreChoice::CatchUp, &options()).unwrap();
    settle(&[&a, &c], &folder);
    converged(&[&a, &c]);
    for user in [&a, &c] {
        assert_eq!(
            user.preference(key).unwrap().as_deref(),
            Some(r#"[{"name":"Greek","n":9}]"#)
        );
    }
}

/// The merge property test's seed 5007792, shrunk: A writes T over a Note and is backed up
/// (the Backup has never met E); D, never seeing T, writes V later; E joins and its snapshot
/// holds V; A meets V, keeps T as a Sync conflict's copy, and edits the copy to T2; then A
/// restores the Backup and catches up. Its old id's files merge before E's snapshot, so the
/// copy and its edit are here as A's own when V arrives again, and T2 stays. Before the fix
/// E's snapshot went first: A made the copy again (T, at a new clock), and that beat the old
/// id's T2, which was lost.
#[test]
fn a_conflict_copy_edited_before_a_restore_keeps_its_edit_when_catching_up() {
    let dir = Dir::new("restore-copy-edited");
    let folder = MemSyncFolder::new();
    let (mut a, d) = (open(&dir, "a"), open(&dir, "d"));
    let nb = default_notebook(&a);
    let n = note(&nb, "first");
    add(&a, Row::Annotation(n.clone()));
    settle(&[&a, &d], &folder);
    let t = with_text(&n, "T on A");
    a.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(t.clone()),
    )])
    .unwrap();
    let backup = back_up(&dir, &a, "a");
    d.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "V on D")),
    )])
    .unwrap();
    sync(&d, &folder);
    let e = open(&dir, "e");
    sync(&e, &folder);
    write_snapshot(&e, &folder).unwrap();
    let report = sync(&a, &folder);
    assert_eq!(report.merge.recorded.conflicts, 1, "{report:?}");
    let copy = a
        .annotations(None)
        .unwrap()
        .into_iter()
        .find(|c| c.conflict_of.is_some())
        .unwrap();
    a.apply_changes(&[Change::update(
        Row::Annotation(copy.clone()),
        Row::Annotation(with_text(&copy, "T2 on A")),
    )])
    .unwrap();
    sync(&a, &folder);
    a.restore(&backup).unwrap();
    restore_choice(&a, &folder, RestoreChoice::CatchUp, &options()).unwrap();
    settle(&[&a, &d, &e], &folder);
    converged(&[&a, &d, &e]);
    assert_eq!(texts(&a), ["T2 on A", "V on D"]);
}

/// The merge property test's seed 7744, shrunk: P's database is copied to A, which takes a
/// new id (P's stays P's and a former one of A's). A writes T over a Note and is backed up; P,
/// never seeing T, writes V; A meets V, keeps T as a Sync conflict's copy and edits it to T2;
/// A restores the Backup and catches up. Its former ids' files merge the last given up first
/// (`former_device.seq`), A's before P's, so the copy and T2 are here as A's own when V
/// arrives, and T2 stays. Before the fix P's files went first (by id), A made the copy again
/// at a new clock, and that beat T2. `p_id_sorts_last`: P's id sorts after A's new one (a
/// clock running behind when A took its id), which must not change the order.
fn a_copys_edited_conflict_copy_survives_a_restore(name: &str, p_id_sorts_last: bool) {
    let dir = Dir::new(name);
    let folder = MemSyncFolder::new();
    let p = open(&dir, "p");
    if p_id_sorts_last {
        p.execute_unchecked("UPDATE device SET device_id = 'ffffffff-ffff-7fff-bfff-ffffffffffff'")
            .unwrap();
    }
    let nb = default_notebook(&p);
    let n = note(&nb, "first");
    add(&p, Row::Annotation(n.clone()));
    sync(&p, &folder);
    let p_id = p.device_id().unwrap();
    let mut a = p.copy_in_memory().unwrap();
    add(&p, Row::Notebook(notebook("On P")));
    sync(&p, &folder);
    let report = sync(&a, &folder);
    let a_id = report.new_device_id.clone().unwrap();
    assert_eq!(a.own_device_ids().unwrap(), [a_id.clone(), p_id.clone()]);
    assert_eq!(a_id > p_id, !p_id_sorts_last);
    settle(&[&p, &a], &folder);
    a.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "T on A")),
    )])
    .unwrap();
    let backup = a.copy_in_memory().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    p.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(with_text(&n, "V on P")),
    )])
    .unwrap();
    sync(&p, &folder);
    let report = sync(&a, &folder);
    assert_eq!(report.merge.recorded.conflicts, 1, "{report:?}");
    let copy = a
        .annotations(None)
        .unwrap()
        .into_iter()
        .find(|c| c.conflict_of.is_some())
        .unwrap();
    a.apply_changes(&[Change::update(
        Row::Annotation(copy.clone()),
        Row::Annotation(with_text(&copy, "T2 on A")),
    )])
    .unwrap();
    sync(&a, &folder);
    a.restore_from(&backup).unwrap();
    restore_choice(&a, &folder, RestoreChoice::CatchUp, &options()).unwrap();
    assert_eq!(a.own_device_ids().unwrap()[1..], [a_id, p_id]);
    settle(&[&p, &a], &folder);
    converged(&[&p, &a]);
    assert_eq!(texts(&a), ["T2 on A", "V on P"]);
}

#[test]
fn a_copys_edited_conflict_copy_survives_a_restore_catching_up() {
    a_copys_edited_conflict_copy_survives_a_restore("copy-restore-copy-edited", false);
}

#[test]
fn a_copys_edited_conflict_copy_survives_a_restore_when_the_originals_id_sorts_last() {
    a_copys_edited_conflict_copy_survives_a_restore("copy-restore-copy-edited-skew", true);
}

#[test]
fn a_replace_over_a_list_base_keeps_another_devices_collection_made_after_the_backup() {
    let dir = Dir::new("restore-replace-base");
    let folder = MemSyncFolder::new();
    let (mut a, b) = (open(&dir, "a"), open(&dir, "b"));
    settle(&[&a, &b], &folder);
    // The Collections get a base (D7): once written not in their shape, then in it.
    a.set_preference("library.collections", "not a list")
        .unwrap();
    a.set_preference("library.collections", r#"[{"name":"Greek","ids":[]}]"#)
        .unwrap();
    settle(&[&a, &b], &folder);
    let backup = back_up(&dir, &a, "a");
    // After the Backup, B adds a Collection of its own.
    b.set_preference(
        "library.collections",
        r#"[{"name":"Greek","ids":[]},{"name":"Made on B","ids":[]}]"#,
    )
    .unwrap();
    settle(&[&a, &b], &folder);
    a.restore(&backup).unwrap();
    restore_choice(&a, &folder, RestoreChoice::Replace, &options()).unwrap();
    settle(&[&a, &b], &folder);
    let c = open(&dir, "c");
    settle(&[&a, &b, &c], &folder);
    converged(&[&a, &b, &c]);
    for user in [&a, &b, &c] {
        let value = user.preference("library.collections").unwrap().unwrap();
        assert!(value.contains("\"Greek\""), "{value}");
        assert!(
            value.contains("\"Made on B\""),
            "never known to the Backup: {value}"
        );
    }
}

#[test]
fn a_database_copied_before_it_joins_takes_a_new_id_and_trims_none_of_the_others_files() {
    let dir = Dir::new("copied-before-joining");
    let folder = MemSyncFolder::new();
    let a = open(&dir, "a");
    add(&a, Row::Notebook(notebook("Romans")));
    // The data directory copied to a new computer before either syncs.
    let copy = a.copy_in_memory().unwrap();
    sync(&a, &folder);
    let a_id = a.device_id().unwrap();
    add(&a, Row::Notebook(notebook("On A")));
    sync(&a, &folder);
    write_snapshot(&a, &folder).unwrap();
    let a_files = folder.list(&ops_dir(&a_id)).unwrap();
    assert!(!a_files.is_empty());
    let a_snapshots = folder.list(&snapshots_dir(&a_id)).unwrap();
    add(&copy, Row::Notebook(notebook("On the copy")));
    // The copy trims as soon as it may: none of A's files are its own to trim.
    let report = sync_with(&copy, &folder, &trimming(0));
    let new = report.new_device_id.clone().unwrap();
    assert!(new != a_id && report.joined, "{report:?}");
    assert_eq!(copy.own_device_ids().unwrap(), [new.clone(), a_id.clone()]);
    assert_eq!(report.snapshot, Some(0), "its join snapshot: {report:?}");
    assert!(report.trimmed.is_empty(), "{report:?}");
    assert_eq!(folder.list(&ops_dir(&a_id)).unwrap(), a_files);
    assert_eq!(folder.list(&snapshots_dir(&a_id)).unwrap(), a_snapshots);
    settle(&[&a, &copy], &folder);
    converged(&[&a, &copy]);
    assert!(a.device_id().unwrap() == a_id, "A keeps its id");
    for user in [&a, &copy] {
        let names = names(user);
        for name in ["Romans", "On A", "On the copy"] {
            assert!(names.contains(&name.to_string()), "{name}: {names:?}");
        }
    }
    assert_eq!(folder.list(&ops_dir(&a_id)).unwrap(), a_files);
}

#[test]
fn an_own_snapshot_the_others_cannot_read_is_written_again_by_an_idle_device() {
    let dir = Dir::new("snapshot-rewrite");
    let folder = MemSyncFolder::new();
    let a = open(&dir, "a");
    add(&a, Row::Notebook(notebook("Romans")));
    sync(&a, &folder);
    let a_id = a.device_id().unwrap();
    // A's only snapshot damaged in the folder (its hash no longer its name's).
    let (_, name) = newest_snapshot(&folder, &a_id).unwrap().unwrap();
    let path = format!("{}/{name}", snapshots_dir(&a_id));
    let whole = folder.get(&path).unwrap();
    folder.put(&path, &whole[..whole.len() / 2]);
    // Written again with the same bytes, under the same name: the damaged file is replaced.
    let report = sync(&a, &folder);
    assert_eq!(report.snapshot, Some(0), "{report:?}");
    assert_eq!(folder.get(&path).unwrap(), whole);
    folder.put(&path, &whole[..whole.len() / 2]);
    let b = open(&dir, "b");
    let report = sync(&b, &folder);
    assert!(
        matches!(
            &report.merge.stopped[..],
            [(id, _, OpsStop::SnapshotNotWhole(_))] if *id == a_id
        ),
        "{report:?}"
    );
    // A writes no ops, but its next sync writes its snapshot again, and B meets it.
    let report = sync(&a, &folder);
    assert!(report.written.is_empty(), "{report:?}");
    assert_eq!(report.snapshot, Some(0), "{report:?}");
    let report = sync(&b, &folder);
    assert_eq!(report.merge.snapshots, [(a_id, 0)], "{report:?}");
    assert!(names(&b).contains(&"Romans".to_string()));
    settle(&[&a, &b], &folder);
    converged(&[&a, &b]);
}
