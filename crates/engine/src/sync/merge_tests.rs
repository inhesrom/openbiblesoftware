//! The merge rules (ADR 0022 rule 3, plan S28b-1), one test per rule, with the arrival orders
//! that matter: last writer wins and keeps the op's clock; every clock merged is observed (one
//! a day ahead is not, its row merged all the same); edit beats delete in every order, and a
//! delete covering the newest edit deletes it; fields an op does not carry are left as they
//! are; Reading done-marks merge per Reading; the Preferred order is one value; two default
//! Notebooks keep the older; two Layouts of one name both stay; a file needing another
//! device's row waits for it; a parent delete meeting live children is skipped.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_store::annotation::new_id;
use obs_store::{
    Annotation, Change, Content, Hlc, Notebook, PlanReadingRow, ReadingPlanRow, Row, RowKey,
    SyncedFavorite, UserStore,
};

use super::folder::{MemSyncFolder, SyncFolder};
use super::merge::{MergeReport, Outcome, merge_folder};
use super::ops::{Op, encode_op_file, op_file_path, ops_dir, write_pending_ops};

pub(super) struct Dir(pub(super) PathBuf);

impl Dir {
    pub(super) fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("obs-merge-{name}-{}", std::process::id()));
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

/// A Synced device: its User database, recording ops, its default Notebook in its ops (as
/// joining with a snapshot would put it, plan S28c).
struct Device {
    user: UserStore,
    id: String,
}

fn device(dir: &Dir, name: &str) -> Device {
    let (user, _) = UserStore::open(&dir.0.join(format!("{name}.db"))).unwrap();
    user.start_recording_ops().unwrap();
    let default = user.notebooks().unwrap().remove(0);
    assert!(default.is_default);
    user.apply_changes(&[Change::update(
        Row::Notebook(default.clone()),
        Row::Notebook(Notebook {
            modified_at: default.modified_at + 1,
            ..default
        }),
    )])
    .unwrap();
    let id = user.device_id().unwrap();
    Device { user, id }
}

impl Device {
    /// Write its pending ops to `folder`, then merge every other device's there.
    fn sync(&self, folder: &dyn SyncFolder) -> MergeReport {
        write_pending_ops(&self.user, folder).unwrap();
        merge_folder(&self.user, folder).unwrap()
    }

    /// Sync until nothing more is written or merged anywhere (the merge itself can record
    /// edits, such as a rename for a name clash, which go out at the next sync).
    fn settle(devices: &[&Device], folder: &dyn SyncFolder) {
        for _ in 0..20 {
            let mut busy = false;
            for d in devices {
                busy |= !write_pending_ops(&d.user, folder).unwrap().is_empty();
                busy |= !merge_folder(&d.user, folder).unwrap().merged.is_empty();
            }
            if !busy {
                return;
            }
        }
        panic!("the devices did not settle");
    }

    /// Write its pending ops only.
    fn push(&self, folder: &dyn SyncFolder) {
        write_pending_ops(&self.user, folder).unwrap();
    }

    fn row(&self, key: &RowKey) -> Option<Row> {
        self.user.row_state(key).unwrap().row
    }

    fn clock(&self, key: &RowKey) -> Option<Hlc> {
        self.user.row_state(key).unwrap().clock
    }

    fn apply(&self, changes: &[Change]) {
        self.user.apply_changes(changes).unwrap();
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

fn favorite(label: &str) -> SyncedFavorite {
    SyncedFavorite {
        uid: new_id(),
        folder: String::new(),
        label: label.into(),
        location: "{}".into(),
        position: 0,
        added_at: 1,
    }
}

fn relabel(f: &SyncedFavorite, label: &str) -> SyncedFavorite {
    SyncedFavorite {
        label: label.into(),
        ..f.clone()
    }
}

/// A copy of `from` holding only the op files named, as a device sees the folder when the
/// others are still arriving.
fn view(from: &MemSyncFolder, files: &[(&str, u64)]) -> MemSyncFolder {
    let to = MemSyncFolder::new();
    for (device, seq) in files {
        let path = op_file_path(device, *seq);
        to.put(&path, &from.get(&path).unwrap());
    }
    to
}

/// Every op file of `device` in `folder`, by number.
fn files_of(folder: &MemSyncFolder, device: &str) -> Vec<u64> {
    let mut seqs: Vec<u64> = folder
        .list(&ops_dir(device))
        .unwrap()
        .iter()
        .filter_map(|n| super::ops::op_file_seq(n))
        .collect();
    seqs.sort_unstable();
    seqs
}

#[test]
fn last_writer_wins_per_row_and_the_merged_row_keeps_the_ops_clock() {
    let dir = Dir::new("lww");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let f = favorite("John 3:16");
    let key = Row::Favorite(f.clone()).key();
    a.apply(&[Change::insert(Row::Favorite(f.clone()))]);
    a.sync(&cloud);
    b.sync(&cloud);
    assert_eq!(b.row(&key), Some(Row::Favorite(f.clone())));
    assert_eq!(
        b.clock(&key),
        a.clock(&key),
        "the op's clock, not a new one"
    );

    // Both edit it before seeing the other's edit: B's is later.
    a.apply(&[Change::update(
        Row::Favorite(f.clone()),
        Row::Favorite(relabel(&f, "by A")),
    )]);
    b.apply(&[Change::update(
        Row::Favorite(f.clone()),
        Row::Favorite(relabel(&f, "by B")),
    )]);
    let b_clock = b.clock(&key).unwrap();
    assert!(a.clock(&key).unwrap() < b_clock);
    a.push(&cloud);
    let report = b.sync(&cloud);
    assert_eq!(report.outcomes.get(&Outcome::Superseded), Some(&1));
    a.sync(&cloud);
    for d in [&a, &b] {
        assert_eq!(d.row(&key), Some(Row::Favorite(relabel(&f, "by B"))));
        assert_eq!(d.clock(&key), Some(b_clock.clone()));
    }
    assert_eq!(a.user.synced_dump().unwrap(), b.user.synced_dump().unwrap());
}

#[test]
fn every_merged_clock_is_observed_so_a_later_edit_here_wins() {
    let dir = Dir::new("observe");
    let cloud = MemSyncFolder::new();
    let b = device(&dir, "b");
    // Another device whose clock runs an hour ahead.
    let ahead = new_id();
    let f = favorite("Gen 1:1");
    let now = u64::try_from(obs_store::clock::now_ms()).unwrap();
    let op = Op {
        device: ahead.clone(),
        clock: Hlc {
            wall_ms: now + 3_600_000,
            counter: 0,
            device_id: ahead.clone(),
        },
        before: None,
        after: Some(Row::Favorite(f.clone())),
        before_clock: None,
        absent: Vec::new(),
    };
    cloud.put(
        &op_file_path(&ahead, 1),
        &encode_op_file(std::slice::from_ref(&op)).unwrap(),
    );
    let report = b.sync(&cloud);
    assert_eq!(report.not_observed, 0);
    let key = Row::Favorite(f.clone()).key();
    b.apply(&[Change::update(
        Row::Favorite(f.clone()),
        Row::Favorite(relabel(&f, "edited after")),
    )]);
    assert!(
        b.clock(&key).unwrap() > op.clock,
        "an edit after a merge is later than what it merged"
    );
}

#[test]
fn a_clock_more_than_a_day_ahead_is_not_observed_but_its_row_merges() {
    let dir = Dir::new("skew");
    let cloud = MemSyncFolder::new();
    let (b, c) = (device(&dir, "b"), device(&dir, "c"));
    // B's Layout "Study" is older than the far device's of that name, so B renames that one.
    b.user.save_layout("Study", r#"{"by":"b"}"#).unwrap();
    b.sync(&cloud);
    let ahead = new_id();
    let f = favorite("Ps 23");
    let layout = obs_store::SyncedLayout {
        id: new_id(),
        name: "Study".into(),
        snapshot: r#"{"by":"far"}"#.into(),
        saved_at: 1,
    };
    let now = u64::try_from(obs_store::clock::now_ms()).unwrap();
    let far = |counter| Hlc {
        wall_ms: now + 2 * obs_store::hlc::MAX_FORWARD_SKEW_MS,
        counter,
        device_id: ahead.clone(),
    };
    let op = |counter, row| Op {
        device: ahead.clone(),
        clock: far(counter),
        before: None,
        after: Some(row),
        before_clock: None,
        absent: Vec::new(),
    };
    cloud.put(
        &op_file_path(&ahead, 1),
        &encode_op_file(&[
            op(0, Row::Favorite(f.clone())),
            op(1, Row::Layout(layout.clone())),
        ])
        .unwrap(),
    );
    let report = b.sync(&cloud);
    assert_eq!(report.not_observed, 2);
    let key = Row::Favorite(f.clone()).key();
    assert_eq!(
        b.row(&key),
        Some(Row::Favorite(f.clone())),
        "kept (ADR 0006)"
    );
    assert_eq!(b.clock(&key), Some(far(0)));
    // This device's own clock did not jump two days ahead.
    let nb = notebook("After");
    b.apply(&[Change::insert(Row::Notebook(nb.clone()))]);
    assert!(b.clock(&Row::Notebook(nb).key()).unwrap().wall_ms < far(0).wall_ms);

    // B edits the far row, and its rename of the far Layout for the clash is its own edit
    // too: each is stamped just after the far clock it replaces (the device's clock still
    // does not follow), so C, which merged the far ops, takes both.
    b.apply(&[Change::update(
        Row::Favorite(f.clone()),
        Row::Favorite(relabel(&f, "edited on B")),
    )]);
    let edited = b.clock(&key).unwrap();
    assert!(edited > far(0) && edited.device_id == b.id, "{edited}");
    let layout_key = Row::Layout(layout.clone()).key();
    let renamed = b.clock(&layout_key).unwrap();
    assert!(renamed > far(1) && renamed.device_id == b.id, "{renamed}");
    let nb = notebook("After the edit");
    b.apply(&[Change::insert(Row::Notebook(nb.clone()))]);
    assert!(b.clock(&Row::Notebook(nb).key()).unwrap().wall_ms < far(0).wall_ms);
    Device::settle(&[&b, &c], &cloud);
    for d in [&b, &c] {
        assert_eq!(d.row(&key), Some(Row::Favorite(relabel(&f, "edited on B"))));
        assert_eq!(d.clock(&key), Some(edited.clone()));
        assert_eq!(d.user.layouts().unwrap(), ["Study", "Study (2)"]);
        assert_eq!(d.clock(&layout_key), Some(renamed.clone()));
    }
    assert_eq!(b.user.synced_dump().unwrap(), c.user.synced_dump().unwrap());
}

#[test]
fn edit_beats_delete_in_every_arrival_order() {
    let dir = Dir::new("edit-beats-delete");
    let cloud = MemSyncFolder::new();
    let (phone, laptop) = (device(&dir, "phone"), device(&dir, "laptop"));
    let f = favorite("Rom 8");
    let key = Row::Favorite(f.clone()).key();
    phone.apply(&[Change::insert(Row::Favorite(f.clone()))]);
    phone.sync(&cloud);
    laptop.sync(&cloud);
    // The phone edits it offline; the laptop, later, deletes it without having seen that.
    phone.apply(&[Change::update(
        Row::Favorite(f.clone()),
        Row::Favorite(relabel(&f, "edited on the phone")),
    )]);
    laptop.apply(&[Change::delete(Row::Favorite(f.clone()))]);
    assert!(
        phone.clock(&key)
            < laptop
                .user
                .row_state(&key)
                .unwrap()
                .tombstone
                .unwrap()
                .clock
    );
    phone.push(&cloud);
    laptop.push(&cloud);
    let edited = Some(Row::Favorite(relabel(&f, "edited on the phone")));

    // A third device in either order, from nothing.
    let order = [
        [(phone.id.as_str(), 1), (laptop.id.as_str(), 1)],
        [(laptop.id.as_str(), 1), (phone.id.as_str(), 1)],
    ];
    for (i, files) in order.iter().enumerate() {
        let third = device(&dir, &format!("third-{i}"));
        let mut seen = Vec::new();
        for (dev, _) in files {
            for seq in files_of(&cloud, dev) {
                seen.push((*dev, seq));
            }
            let report = merge_folder(&third.user, &view(&cloud, &seen)).unwrap();
            assert!(report.waiting.is_empty(), "{report:?}");
        }
        assert_eq!(third.row(&key), edited, "order {i}");
    }
    // Both ends: the phone meets the delete, the laptop the edit.
    phone.sync(&cloud);
    let report = laptop.sync(&cloud);
    assert!(
        report.outcomes.contains_key(&Outcome::Written),
        "{report:?}"
    );
    assert_eq!(phone.row(&key), edited);
    assert_eq!(laptop.row(&key), edited);
    assert_eq!(phone.clock(&key), laptop.clock(&key));
}

#[test]
fn a_delete_that_saw_the_newest_edit_deletes_it_in_every_arrival_order() {
    // A: V0 → VA; B saw VA: VA → VB, then deleted VB. C gets A's file and B's in either order.
    let dir = Dir::new("delete-covers");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let f = favorite("V0");
    let key = Row::Favorite(f.clone()).key();
    a.apply(&[Change::insert(Row::Favorite(f.clone()))]);
    a.sync(&cloud);
    b.sync(&cloud);
    let va = relabel(&f, "VA");
    a.apply(&[Change::update(
        Row::Favorite(f.clone()),
        Row::Favorite(va.clone()),
    )]);
    a.sync(&cloud);
    b.sync(&cloud);
    let vb = relabel(&f, "VB");
    b.apply(&[Change::update(
        Row::Favorite(va.clone()),
        Row::Favorite(vb.clone()),
    )]);
    b.apply(&[Change::delete(Row::Favorite(vb))]);
    b.push(&cloud);
    let a_files: Vec<(&str, u64)> = files_of(&cloud, &a.id)
        .into_iter()
        .map(|s| (a.id.as_str(), s))
        .collect();
    let b_files: Vec<(&str, u64)> = files_of(&cloud, &b.id)
        .into_iter()
        .map(|s| (b.id.as_str(), s))
        .collect();
    for (i, first_a) in [true, false].into_iter().enumerate() {
        let c = device(&dir, &format!("c-{i}"));
        let (first, second) = if first_a {
            (&a_files, &b_files)
        } else {
            (&b_files, &a_files)
        };
        merge_folder(&c.user, &view(&cloud, first)).unwrap();
        let all: Vec<(&str, u64)> = first.iter().chain(second.iter()).copied().collect();
        merge_folder(&c.user, &view(&cloud, &all)).unwrap();
        assert_eq!(c.row(&key), None, "first A: {first_a}");
        assert!(c.user.row_state(&key).unwrap().tombstone.is_some());
    }
    a.sync(&cloud);
    assert_eq!(a.row(&key), None);
}

#[test]
fn a_delete_records_the_clock_of_the_version_it_covers() {
    let dir = Dir::new("covers");
    let a = device(&dir, "a");
    let f = favorite("Jn 1");
    let key = Row::Favorite(f.clone()).key();
    a.apply(&[Change::insert(Row::Favorite(f.clone()))]);
    let version = a.clock(&key).unwrap();
    a.apply(&[Change::delete(Row::Favorite(f))]);
    let tombstone = a.user.row_state(&key).unwrap().tombstone.unwrap();
    assert_eq!(tombstone.covers, Some(version.clone()));
    assert!(tombstone.clock.unwrap() > version);
    let files = a.user.reserve_op_files().unwrap();
    let last = files.last().unwrap().ops.last().unwrap();
    assert_eq!(last.before_clock, Some(version), "the op carries it too");
}

/// One op file of `device` holding `lines` (op JSON values) as they are.
fn raw_op_file(lines: &[serde_json::Value]) -> Vec<u8> {
    let mut body = Vec::new();
    for line in lines {
        body.extend_from_slice(&serde_json::to_vec(line).unwrap());
        body.push(b'\n');
    }
    let trailer = serde_json::json!({
        "format": 1,
        "ops": lines.len(),
        "bytes": body.len(),
        "sha256": crate::starter::sha256_hex(&body),
    });
    body.extend_from_slice(&serde_json::to_vec(&trailer).unwrap());
    body.push(b'\n');
    body
}

#[test]
fn fields_an_op_does_not_carry_are_left_as_they_are() {
    let dir = Dir::new("absent");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let nb = Notebook {
        show_in_text: false,
        ..notebook("Greek")
    };
    a.apply(&[Change::insert(Row::Notebook(nb.clone()))]);
    a.sync(&cloud);
    b.sync(&cloud);
    // An older app, which does not know Show in text, renames the Notebook.
    let older = new_id();
    let renamed = Notebook {
        name: "Greek words".into(),
        ..nb.clone()
    };
    let mut line = serde_json::to_value(Op {
        device: older.clone(),
        clock: Hlc {
            wall_ms: u64::try_from(obs_store::clock::now_ms()).unwrap(),
            counter: 0,
            device_id: older.clone(),
        },
        before: Some(Row::Notebook(nb.clone())),
        after: Some(Row::Notebook(renamed.clone())),
        before_clock: None,
        absent: Vec::new(),
    })
    .unwrap();
    for side in ["before", "after"] {
        line[side].as_object_mut().unwrap().remove("show_in_text");
    }
    cloud.put(&op_file_path(&older, 1), &raw_op_file(&[line]));
    b.sync(&cloud);
    let key = Row::Notebook(nb.clone()).key();
    assert_eq!(
        b.row(&key),
        Some(Row::Notebook(Notebook {
            show_in_text: false,
            ..renamed
        })),
        "renamed, and still hidden in text"
    );
}

#[test]
fn reading_done_marks_merge_per_reading() {
    let dir = Dir::new("readings");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let plan = ReadingPlanRow {
        id: 0,
        name: "NT in 90 days".into(),
        template: "nt90".into(),
        bible_id: "bsb".into(),
        start_day: "2026-01-01".into(),
        weekdays: 0x7f,
        created_at: 0,
    };
    let readings: Vec<PlanReadingRow> = (0..3)
        .map(|i| PlanReadingRow {
            position: i,
            passages: format!("Matt {}", i + 1),
            day: format!("2026-01-0{}", i + 1),
            read_at: None,
        })
        .collect();
    let plan_a = a.user.add_reading_plan(&plan, &readings).unwrap();
    a.sync(&cloud);
    b.sync(&cloud);
    let plan_b = b.user.reading_plans().unwrap()[0].id;
    // Each marks another Reading read before seeing the other's.
    assert!(a.user.set_reading_read(plan_a, 0, true).unwrap());
    assert!(b.user.set_reading_read(plan_b, 2, true).unwrap());
    Device::settle(&[&a, &b], &cloud);
    for (d, id) in [(&a, plan_a), (&b, plan_b)] {
        let read: Vec<bool> = d
            .user
            .plan_readings(id)
            .unwrap()
            .iter()
            .map(|r| r.read_at.is_some())
            .collect();
        assert_eq!(read, [true, false, true]);
    }
    assert_eq!(a.user.synced_dump().unwrap(), b.user.synced_dump().unwrap());
}

#[test]
fn the_preferred_order_is_one_value_and_the_last_writer_wins() {
    let dir = Dir::new("preferred-order");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    a.user
        .save_preferred_order(&ids(&["kjv", "web", "bsb"]))
        .unwrap();
    b.user.save_preferred_order(&ids(&["bsb", "lsv"])).unwrap();
    Device::settle(&[&a, &b], &cloud);
    for d in [&a, &b] {
        assert_eq!(
            d.user.preferred_order().unwrap(),
            ids(&["bsb", "lsv"]),
            "whole, not mixed"
        );
        assert_eq!(
            d.clock(&RowKey::PreferredOrder),
            b.clock(&RowKey::PreferredOrder)
        );
    }
    assert_eq!(a.user.synced_dump().unwrap(), b.user.synced_dump().unwrap());
}

#[test]
fn two_default_notebooks_keep_the_older_and_the_newer_is_renamed_after_its_device() {
    let dir = Dir::new("defaults");
    let cloud = MemSyncFolder::new();
    let a = device(&dir, "a");
    let b = device(&dir, "b");
    b.user
        .execute_unchecked("UPDATE device SET name = 'Ian''s iPhone'")
        .unwrap();
    let older = a.user.notebooks().unwrap().remove(0);
    let newer = b.user.notebooks().unwrap().remove(0);
    assert!((older.created_at, &older.id) < (newer.created_at, &newer.id));
    assert_eq!(older.name, newer.name);
    Device::settle(&[&a, &b], &cloud);
    for d in [&a, &b] {
        let notebooks = d.user.notebooks().unwrap();
        assert_eq!(notebooks.len(), 2);
        assert_eq!(notebooks[0].id, older.id);
        assert!(notebooks[0].is_default);
        assert_eq!(notebooks[1].id, newer.id);
        assert!(!notebooks[1].is_default);
        assert_eq!(notebooks[1].name, "Ian's iPhone");
    }
    assert_eq!(a.user.synced_dump().unwrap(), b.user.synced_dump().unwrap());

    // A device with no name renames its own default "My Notebook (2)".
    let c = device(&dir, "c");
    Device::settle(&[&a, &b, &c], &cloud);
    let mine = c.user.notebooks().unwrap();
    assert!(
        mine.iter()
            .any(|n| n.name == "My Notebook (2)" && !n.is_default),
        "{mine:?}"
    );
}

#[test]
fn two_layouts_of_one_name_both_stay_and_the_later_is_renamed() {
    let dir = Dir::new("layouts");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    a.user.save_layout("Study", r#"{"by":"a"}"#).unwrap();
    b.user.save_layout("Study", r#"{"by":"b"}"#).unwrap();
    Device::settle(&[&a, &b], &cloud);
    for d in [&a, &b] {
        assert_eq!(d.user.layouts().unwrap(), ["Study", "Study (2)"]);
        assert_eq!(
            d.user.layout("Study").unwrap().as_deref(),
            Some(r#"{"by":"a"}"#)
        );
        assert_eq!(
            d.user.layout("Study (2)").unwrap().as_deref(),
            Some(r#"{"by":"b"}"#)
        );
    }
    assert_eq!(a.user.synced_dump().unwrap(), b.user.synced_dump().unwrap());
}

#[test]
fn a_layout_renamed_for_a_clash_its_owner_never_saw_still_converges() {
    // A saves L1 "Study", B saves L2 "Study" (the later id). C meets both and renames L2.
    // A renames L1 "Notes"; D, having that, saves L1 again; B reads D's file before A's, so
    // A's "Study" is older than what B holds and B never meets a clash itself: only C's
    // recorded rename tells it.
    let dir = Dir::new("layout-abcd");
    let cloud = MemSyncFolder::new();
    let (a, b, c, d) = (
        device(&dir, "a"),
        device(&dir, "b"),
        device(&dir, "c"),
        device(&dir, "d"),
    );
    a.user.save_layout("Study", r#"{"l":1}"#).unwrap();
    a.push(&cloud);
    b.user.save_layout("Study", r#"{"l":2}"#).unwrap();
    b.push(&cloud);
    c.sync(&cloud);
    assert_eq!(c.user.layouts().unwrap(), ["Study", "Study (2)"]);
    c.push(&cloud);
    assert!(a.user.rename_layout("Study", "Notes").unwrap());
    a.push(&cloud);
    let a_files: Vec<(&str, u64)> = files_of(&cloud, &a.id)
        .into_iter()
        .map(|s| (a.id.as_str(), s))
        .collect();
    merge_folder(&d.user, &view(&cloud, &a_files)).unwrap();
    d.user
        .save_layout("Notes", r#"{"l":1,"again":true}"#)
        .unwrap();
    d.push(&cloud);
    let d_files: Vec<(&str, u64)> = files_of(&cloud, &d.id)
        .into_iter()
        .map(|s| (d.id.as_str(), s))
        .collect();
    merge_folder(&b.user, &view(&cloud, &d_files)).unwrap();
    assert_eq!(b.user.layouts().unwrap(), ["Notes", "Study"]);
    Device::settle(&[&a, &b, &c, &d], &cloud);
    let dumps: Vec<String> = [&a, &b, &c, &d]
        .iter()
        .map(|x| x.user.synced_dump().unwrap())
        .collect();
    for (x, dump) in [&a, &b, &c, &d].iter().zip(&dumps) {
        assert_eq!(x.user.layouts().unwrap(), ["Notes", "Study (2)"]);
        assert_eq!(dump, &dumps[0]);
    }
}

#[test]
fn a_file_needing_another_devices_notebook_waits_and_merges_after_it() {
    let dir = Dir::new("waits");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let nb = notebook("Romans");
    a.apply(&[Change::insert(Row::Notebook(nb.clone()))]);
    a.sync(&cloud);
    b.sync(&cloud);
    let note = Annotation {
        id: new_id(),
        notebook_id: nb.id.clone(),
        content: Content::Note {
            body: "Grace".into(),
        },
        anchor: None,
        created_at: 1,
        modified_at: 1,
    };
    b.apply(&[Change::insert(Row::Annotation(note.clone()))]);
    b.push(&cloud);
    // C reads B's file while A's (with the Notebook) is still arriving.
    let c = device(&dir, "c");
    let b_files: Vec<(&str, u64)> = files_of(&cloud, &b.id)
        .into_iter()
        .map(|s| (b.id.as_str(), s))
        .collect();
    let report = merge_folder(&c.user, &view(&cloud, &b_files)).unwrap();
    assert_eq!(report.waiting.len(), 1, "{report:?}");
    assert!(report.waiting[0].2.contains("needs Notebook"), "{report:?}");
    let key = Row::Annotation(note.clone()).key();
    assert_eq!(c.row(&key), None);
    // Once A's file is there, both merge in one sync: A's first, then B's again.
    let report = c.sync(&cloud);
    assert!(report.waiting.is_empty(), "{report:?}");
    assert_eq!(c.row(&key), Some(Row::Annotation(note)));
    b.sync(&cloud);
    assert_eq!(c.user.synced_dump().unwrap(), b.user.synced_dump().unwrap());
}

#[test]
fn a_parent_delete_meeting_live_children_is_skipped_so_nothing_is_lost() {
    let dir = Dir::new("children");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let nb = notebook("Romans");
    a.apply(&[Change::insert(Row::Notebook(nb.clone()))]);
    a.sync(&cloud);
    b.sync(&cloud);
    // A deletes the empty Notebook; B, not having seen that, adds a Note to it.
    a.apply(&[Change::delete(Row::Notebook(nb.clone()))]);
    let note = Annotation {
        id: new_id(),
        notebook_id: nb.id.clone(),
        content: Content::Note {
            body: "Mine".into(),
        },
        anchor: None,
        created_at: 1,
        modified_at: 1,
    };
    b.apply(&[Change::insert(Row::Annotation(note.clone()))]);
    a.push(&cloud);
    let report = b.sync(&cloud);
    assert_eq!(
        report.outcomes.get(&Outcome::ChildrenKept),
        Some(&1),
        "{report:?}"
    );
    assert!(b.row(&Row::Notebook(nb).key()).is_some());
    assert!(b.row(&Row::Annotation(note).key()).is_some());
}

#[test]
fn an_own_op_file_or_one_out_of_order_is_not_merged() {
    let dir = Dir::new("refuse");
    let a = device(&dir, "a");
    let own = a.id.clone();
    assert!(
        a.user
            .merge_op_file(&own, 1, |_| Ok::<(), obs_store::StoreError>(()))
            .is_err()
    );
    let other = new_id();
    assert!(
        a.user
            .merge_op_file(&other, 2, |_| Ok::<(), obs_store::StoreError>(()))
            .is_err()
    );
    a.user
        .merge_op_file(&other, 1, |_| Ok::<(), obs_store::StoreError>(()))
        .unwrap();
    assert_eq!(a.user.applied_up_to(&other).unwrap(), 1);
    // A merge that fails notes nothing.
    let failed = a.user.merge_op_file(&other, 2, |_| {
        Err::<(), _>(obs_store::StoreError::Rejected("no".into()))
    });
    assert!(failed.is_err());
    assert_eq!(a.user.applied_up_to(&other).unwrap(), 1);
}
