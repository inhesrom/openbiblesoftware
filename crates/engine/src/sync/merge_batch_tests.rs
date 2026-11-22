//! Merges in batches (plan S29c, `docs/dev/sync.md` "Merges in batches"): a merge stopped
//! between two batches goes on where it stood and ends as the unsplit merge does; a cursor
//! whose snapshot its device replaced is dropped and the newer one merges from its start,
//! losing nothing; a Backup taken while a merge stood between batches catches up from its
//! cursor, and Replace merges that file again from its first op; a file that might wait
//! merges whole and waits as before; a snapshot is not written while a merge is partly done.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use obs_store::annotation::new_id;
use obs_store::clock::fix;
use obs_store::{Annotation, Change, Content, Notebook, Row, StoreError, UserStore};

use super::SyncError;
use super::folder::MemSyncFolder;
use super::lifecycle::{RestoreChoice, SyncOptions, SyncState, restore_choice, sync_now};
use super::merge::{MergeBatch, merge_folder_with};
use super::merge_tests::{Dir, files_of, view};
use super::snapshot::{SnapshotPolicy, newest_snapshot};

/// The wall clock the tests run at (Unix ms), so two merges record the same clocks.
const T0: i64 = 1_800_000_000_000;

fn open(dir: &Dir, name: &str) -> UserStore {
    UserStore::open(&dir.0.join(name).join("user.db"))
        .unwrap()
        .0
}

/// The app's options, merging in batches of `batch`.
fn options(batch: MergeBatch) -> SyncOptions {
    SyncOptions {
        batch,
        ..SyncOptions::new("macos")
    }
}

/// Batches of three ops, the merge stopping after `stop` of them.
fn stopping(stop: usize) -> MergeBatch {
    MergeBatch {
        stop_after: Some(stop),
        ..MergeBatch::of_ops(3)
    }
}

fn sync(user: &UserStore, folder: &MemSyncFolder, options: &SyncOptions) -> super::SyncReport {
    match sync_now(user, folder, options).unwrap() {
        SyncState::Synced(report) => *report,
        other => panic!("{other:?}"),
    }
}

/// Sync until a sync stops between two batches; how many batches it committed first is
/// `stop`.
fn sync_stopped(user: &UserStore, folder: &MemSyncFolder, stop: usize) {
    match sync_now(user, folder, &options(stopping(stop))) {
        Err(SyncError::Store(StoreError::Cancelled)) => {}
        other => panic!("the sync did not stop: {other:?}"),
    }
}

fn settle(devices: &[&UserStore], folder: &MemSyncFolder) {
    for _ in 0..20 {
        let mut busy = false;
        for user in devices {
            let report = sync(user, folder, &options(MergeBatch::of_ops(3)));
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

fn add(user: &UserStore, rows: Vec<Row>) {
    let changes: Vec<Change> = rows.into_iter().map(Change::insert).collect();
    user.apply_changes(&changes).unwrap();
}

/// `notes` Notes in a new Notebook on `user`.
fn many_notes(user: &UserStore, name: &str, notes: usize) -> (Notebook, Vec<Annotation>) {
    let nb = notebook(name);
    let made: Vec<Annotation> = (0..notes)
        .map(|i| note(&nb.id, &format!("{name} {i}")))
        .collect();
    let mut rows = vec![Row::Notebook(nb.clone())];
    rows.extend(made.iter().cloned().map(Row::Annotation));
    add(user, rows);
    (nb, made)
}

fn note_texts(user: &UserStore) -> Vec<String> {
    let mut texts: Vec<String> = (user.annotations(None).unwrap().into_iter())
        .map(|a| a.body().to_string())
        .collect();
    texts.sort();
    texts
}

#[test]
fn a_merge_stopped_between_batches_goes_on_where_it_stood_and_ends_as_the_unsplit_merge() {
    let dir = Dir::new("batches-stopped");
    let folder = MemSyncFolder::new();
    fix(Some(T0));
    let (a, b) = (open(&dir, "a"), open(&dir, "b"));
    let batch = options(MergeBatch::of_ops(3));
    settle(&[&a, &b], &folder);
    // B's own Note, and a Notebook of A's with a Note in it, on both.
    let shared = note(&b.notebooks().unwrap()[0].id, "first");
    add(&b, vec![Row::Annotation(shared.clone())]);
    let (doomed, doomed_notes) = many_notes(&a, "Doomed", 2);
    settle(&[&a, &b], &folder);
    // Then, before either syncs: B edits its Note and adds one to A's Notebook; A edits the
    // Note having seen only "first" (a Sync conflict's copy on B, D2), deletes its Notebook
    // with its Notes (kept on B for B's Note, D3), and makes 40 Notes, editing and deleting
    // some.
    fix(Some(T0 + 1_000));
    b.apply_changes(&[Change::update(
        Row::Annotation(shared.clone()),
        Row::Annotation(with_text(&shared, "B's text")),
    )])
    .unwrap();
    add(&b, vec![Row::Annotation(note(&doomed.id, "B's in Doomed"))]);
    fix(Some(T0 + 2_000));
    a.apply_changes(&[Change::update(
        Row::Annotation(shared.clone()),
        Row::Annotation(with_text(&shared, "A's text")),
    )])
    .unwrap();
    let mut gone: Vec<Change> = (doomed_notes.iter().cloned())
        .map(|n| Change::delete(Row::Annotation(n)))
        .collect();
    gone.push(Change::delete(Row::Notebook(doomed.clone())));
    a.apply_changes(&gone).unwrap();
    let (_, made) = many_notes(&a, "Study", 40);
    for n in made.iter().take(5) {
        a.apply_changes(&[Change::update(
            Row::Annotation(n.clone()),
            Row::Annotation(with_text(n, "edited")),
        )])
        .unwrap();
    }
    a.apply_changes(&[Change::delete(Row::Annotation(made[39].clone()))])
        .unwrap();
    sync(&a, &folder, &batch);

    // B merges A's file twice over: whole (a copy of it), and stopped every two batches of
    // three ops, going on each time where it stood.
    fix(Some(T0 + 3_000));
    let whole = b.copy_in_memory().unwrap();
    let report = merge_folder_with(&whole, &folder, &MergeBatch::whole()).unwrap();
    assert_eq!(report.recorded.conflicts, 1);
    assert_eq!(
        report.outcomes.get(&super::merge::Outcome::ParentKept),
        Some(&1)
    );
    let mut stops = 0;
    let mut cursors = Vec::new();
    loop {
        match merge_folder_with(&b, &folder, &stopping(2)) {
            Ok(report) => {
                assert_eq!(report.split_waits, 0);
                break;
            }
            Err(SyncError::Store(StoreError::Cancelled)) => {
                stops += 1;
                cursors.extend(b.merge_cursors().unwrap());
            }
            Err(e) => panic!("{e}"),
        }
        assert!(stops < 100, "the merge never ended");
    }
    assert!(stops >= 5, "stopped {stops} times");
    // It stood inside A's file, and went on from there each time.
    assert!(
        cursors
            .windows(2)
            .any(|w| w[0].seq == w[1].seq && w[1].ops > w[0].ops)
    );
    assert!(b.merge_cursors().unwrap().is_empty());
    assert_eq!(b.synced_dump().unwrap(), whole.synced_dump().unwrap());
    assert_eq!(b.sync_conflicts().unwrap(), whole.sync_conflicts().unwrap());
    assert_eq!(note_texts(&b), note_texts(&whole));
    assert!(note_texts(&b).contains(&"B's text".to_string()));
    fix(None);
}

#[test]
fn a_cursor_whose_snapshot_was_replaced_is_dropped_and_the_newer_merges_from_its_start() {
    let dir = Dir::new("batches-replaced");
    let folder = MemSyncFolder::new();
    // A snapshot after every op file, so A's notes are in one.
    let often = SyncOptions {
        policy: SnapshotPolicy {
            after_files: 1,
            after_ms: i64::MAX,
            trim_after_ms: i64::MAX,
        },
        ..options(MergeBatch::of_ops(3))
    };
    let a = open(&dir, "a");
    sync(&a, &folder, &often);
    many_notes(&a, "Study", 30);
    sync(&a, &folder, &often);
    let a_id = a.device_id().unwrap();
    let (first, name) = newest_snapshot(&folder, &a_id).unwrap().unwrap();
    assert_eq!(first, 1);
    // C joins and stops inside A's snapshot.
    let c = open(&dir, "c");
    sync_stopped(&c, &folder, 3);
    let cursor = c.merge_cursors().unwrap().remove(0);
    assert_eq!(cursor.snapshot.as_deref(), Some(name.as_str()));
    assert!(!c.annotations(None).unwrap().is_empty());
    // A writes on, and a newer snapshot replaces that one.
    many_notes(&a, "More", 10);
    let study = a.annotations(None).unwrap().remove(0);
    a.apply_changes(&[Change::update(
        Row::Annotation(study.clone()),
        Row::Annotation(with_text(&study, "edited after")),
    )])
    .unwrap();
    sync(&a, &folder, &often);
    let (newer, _) = newest_snapshot(&folder, &a_id).unwrap().unwrap();
    assert_eq!(newer, 2);
    let report = sync(&c, &folder, &often);
    assert_eq!(report.merge.cursors_dropped, 1);
    assert_eq!(report.merge.resumed, 0);
    assert!(report.merge.snapshots.contains(&(a_id, 2)));
    assert!(c.merge_cursors().unwrap().is_empty());
    settle(&[&a, &c], &folder);
    converged(&[&a, &c]);
    assert_eq!(note_texts(&c).len(), 40);
    assert!(note_texts(&c).contains(&"edited after".to_string()));
}

#[test]
fn a_backup_taken_between_batches_catches_up_from_its_cursor_and_replace_merges_the_file_again() {
    let dir = Dir::new("batches-restore");
    let folder = MemSyncFolder::new();
    let batch = options(MergeBatch::of_ops(3));
    let (a, mut b) = (open(&dir, "a"), open(&dir, "b"));
    settle(&[&a, &b], &folder);
    many_notes(&a, "Study", 30);
    sync(&a, &folder, &batch);
    sync_stopped(&b, &folder, 2);
    let cursor = b.merge_cursors().unwrap();
    assert_eq!(cursor.len(), 1);
    assert_eq!(cursor[0].snapshot, None);
    let backup = dir.0.join("b.obsbackup");
    b.back_up(&backup).unwrap();
    settle(&[&a, &b], &folder);
    assert!(b.merge_cursors().unwrap().is_empty());

    // Catch up: the restored data's merge goes on from the Backup's cursor, and the
    // snapshot of its new id waits for it.
    b.restore(&backup).unwrap();
    assert_eq!(b.merge_cursors().unwrap(), cursor);
    let SyncState::Synced(report) =
        restore_choice(&b, &folder, RestoreChoice::CatchUp, &batch).unwrap()
    else {
        panic!()
    };
    assert_eq!(report.merge.resumed, 1);
    assert!(report.snapshot.is_some(), "written once the merge is done");
    assert!(b.merge_cursors().unwrap().is_empty());
    settle(&[&a, &b], &folder);
    converged(&[&a, &b]);
    assert_eq!(note_texts(&b).len(), 30);

    // Replace: the restored data replaces every version; the cursor goes, and the file merges
    // again from its first op on top of it (here its rows come through the snapshot of the id
    // the first restore gave, which covers it).
    b.restore(&backup).unwrap();
    assert_eq!(b.merge_cursors().unwrap(), cursor);
    let SyncState::Synced(report) =
        restore_choice(&b, &folder, RestoreChoice::Replace, &batch).unwrap()
    else {
        panic!()
    };
    // Replace emptied the cursor before the sync: none to finish or drop.
    assert_eq!(report.merge.resumed, 0);
    assert_eq!(report.merge.cursors_dropped, 0);
    assert!(b.merge_cursors().unwrap().is_empty());
    settle(&[&a, &b], &folder);
    converged(&[&a, &b]);
    assert_eq!(note_texts(&b).len(), 30);
}

#[test]
fn a_file_that_might_wait_merges_whole_and_waits_as_before() {
    let dir = Dir::new("batches-whole");
    let folder = MemSyncFolder::new();
    let batch = options(MergeBatch::of_ops(1));
    let (a, b) = (open(&dir, "a"), open(&dir, "b"));
    settle(&[&a, &b], &folder);
    // A's Notebook, and B's Notes in it.
    let nb = notebook("A's");
    add(&a, vec![Row::Notebook(nb.clone())]);
    sync(&a, &folder, &batch);
    sync(&b, &folder, &batch);
    add(
        &b,
        (0..5)
            .map(|i| Row::Annotation(note(&nb.id, &format!("B's {i}"))))
            .collect(),
    );
    sync(&b, &folder, &batch);
    // C sees B's files but not A's: B's Notes need A's Notebook.
    let c = open(&dir, "c");
    let (a_id, b_id) = (a.device_id().unwrap(), b.device_id().unwrap());
    let b_files: Vec<(&str, u64)> = (files_of(&folder, &b_id).into_iter())
        .map(|n| (b_id.as_str(), n))
        .collect();
    let partial = view(&folder, &b_files);
    let report = sync(&c, &partial, &batch);
    assert!(report.merge.whole >= 1, "{:?}", report.merge);
    assert_eq!(report.merge.waiting.len(), 1, "{:?}", report.merge);
    assert!(
        c.merge_cursors().unwrap().is_empty(),
        "nothing of it committed"
    );
    assert!(note_texts(&c).is_empty());
    // With A's file there it merges, after A's.
    let report = sync(&c, &folder, &batch);
    assert!(report.merge.waiting.is_empty());
    assert!(report.merge.merged.iter().any(|(d, _)| *d == a_id));
    assert_eq!(note_texts(&c).len(), 5);
    settle(&[&a, &b, &c], &folder);
    converged(&[&a, &b, &c]);
}

#[test]
fn no_snapshot_is_written_while_a_merge_is_partly_done() {
    let dir = Dir::new("batches-no-snapshot");
    let folder = MemSyncFolder::new();
    let a = open(&dir, "a");
    let batch = options(MergeBatch::of_ops(3));
    sync(&a, &folder, &batch);
    many_notes(&a, "Study", 20);
    sync(&a, &folder, &batch);
    let c = open(&dir, "c");
    sync(&c, &folder, &batch);
    many_notes(&a, "More", 20);
    sync(&a, &folder, &batch);
    sync_stopped(&c, &folder, 2);
    assert!(c.has_partial_merge().unwrap());
    let due = super::snapshot::snapshot_due(&c, &folder, &SnapshotPolicy::default(), T0).unwrap();
    assert!(!due);
    assert_eq!(
        super::snapshot::write_snapshot_unless_edited(&c, &folder).unwrap(),
        None
    );
}

/// How long a 10,000-Note merge takes in one transaction (as before S29c) and in the app's
/// batches, for a new device joining (10,000 Notes added in one op file) and then catching up
/// with all of them edited (one op file of 10,000 updates): the cost of the batches' commits.
/// `cargo test -p obs-engine --lib measure_merge_batches -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn measure_merge_batches_against_one_transaction() {
    let dir = Dir::new("batches-measure");
    let folder = MemSyncFolder::new();
    let app = options(MergeBatch::default());
    let l = open(&dir, "library");
    sync(&l, &folder, &app);
    let (_, notes) = many_notes(&l, "Study", 10_000);
    sync(&l, &folder, &app);
    let n = open(&dir, "new");
    n.join_sync().unwrap();
    let whole = n.copy_in_memory().unwrap();
    let ms = |at: std::time::Instant| at.elapsed().as_secs_f64() * 1000.0;
    // Both merges record at one clock, so they record the same.
    let clock = obs_store::clock::now_ms() + 60_000;
    fix(Some(clock));
    let at = std::time::Instant::now();
    let one = merge_folder_with(&whole, &folder, &MergeBatch::whole()).unwrap();
    let join_whole = ms(at);
    let at = std::time::Instant::now();
    let split = merge_folder_with(&n, &folder, &MergeBatch::default()).unwrap();
    let join_split = ms(at);
    fix(None);
    assert_eq!(n.synced_dump().unwrap(), whole.synced_dump().unwrap());
    let changes: Vec<Change> = (notes.iter())
        .map(|a| {
            Change::update(
                Row::Annotation(a.clone()),
                Row::Annotation(with_text(a, &format!("{} (edited)", a.body()))),
            )
        })
        .collect();
    l.apply_changes(&changes).unwrap();
    sync(&l, &folder, &app);
    fix(Some(clock + 60_000));
    let at = std::time::Instant::now();
    let one_edit = merge_folder_with(&whole, &folder, &MergeBatch::whole()).unwrap();
    let edit_whole = ms(at);
    let at = std::time::Instant::now();
    let split_edit = merge_folder_with(&n, &folder, &MergeBatch::default()).unwrap();
    let edit_split = ms(at);
    fix(None);
    assert_eq!(n.synced_dump().unwrap(), whole.synced_dump().unwrap());
    eprintln!(
        "10,000 Notes joined: one transaction {join_whole:.0} ms ({} batch), in batches \
         {join_split:.0} ms ({} batches, {} merged whole); then all edited: one transaction \
         {edit_whole:.0} ms ({} batch), in batches {edit_split:.0} ms ({} batches, {} merged \
         whole)",
        one.batches,
        split.batches,
        split.whole,
        one_edit.batches,
        split_edit.batches,
        split_edit.whole,
    );
}
