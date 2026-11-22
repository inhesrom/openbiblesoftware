//! The merge rules (ADR 0022 rule 3, plan S28b-1), one test per rule, with the arrival orders
//! that matter: last writer wins and keeps the op's clock; every clock merged is observed (one
//! a day ahead is not, its row merged all the same); edit beats delete in every order, and a
//! delete covering the newest edit deletes it; fields an op does not carry are left as they
//! are; Reading done-marks merge per Reading; a Collection renamed on one device and edited on
//! another keeps both; the Preferred order is one value; two default Notebooks keep the older;
//! two Layouts of one name both stay; a file needing another device's row waits for it.
//! S28b-2: a Note's text edited on two devices keeps both (D2), also when the losing text
//! meets a delete; an older app's op keeps `conflict_of`; a deleted parent comes back for a
//! child added meanwhile, in either order and when the child's maker moved or deleted it,
//! for Notebooks, Reading plans and palettes (D3), and a child of one deleted before schema
//! 16 is held; two list items of one name both stay (D6); a damaged whole list value holds
//! the later items on every device (D7); an op this build rejects is held, and its file and
//! the next merge. From the S28b-2 review: an empty Resource tags value saved where there was
//! none supersedes no other device's tags; an edit or a delete that never saw this device's
//! list item of its name keeps it; a parent's tombstone row is the same in every order; a
//! child of a held parent is held too; a kept parent brings back no row deleted here too; of a
//! row's held writes only the newest is kept.

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
pub(super) fn view(from: &MemSyncFolder, files: &[(&str, u64)]) -> MemSyncFolder {
    let to = MemSyncFolder::new();
    for (device, seq) in files {
        let path = op_file_path(device, *seq);
        to.put(&path, &from.get(&path).unwrap());
    }
    to
}

/// Every op file of `device` in `folder`, by number.
pub(super) fn files_of(folder: &MemSyncFolder, device: &str) -> Vec<u64> {
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
fn a_collection_renamed_on_one_device_and_edited_on_another_keeps_both() {
    let dir = Dir::new("collection-rename");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let key = "library.collections";
    a.user
        .set_preference(
            key,
            r#"[{"name":"Greek","ids":["sblgnt"]},{"name":"Study","ids":[]}]"#,
        )
        .unwrap();
    a.sync(&cloud);
    b.sync(&cloud);
    // A renames "Greek" (a delete of the old name and an insert of the new); B, not having
    // seen that, edits "Greek".
    a.user
        .set_preference(
            key,
            r#"[{"name":"Greek NT","ids":["sblgnt"]},{"name":"Study","ids":[]}]"#,
        )
        .unwrap();
    b.user
        .set_preference(
            key,
            r#"[{"name":"Greek","ids":["sblgnt","lxx"]},{"name":"Study","ids":[]}]"#,
        )
        .unwrap();
    Device::settle(&[&a, &b], &cloud);
    // Edit beats delete: B's "Greek" stays beside A's "Greek NT", in the same order on both
    // (by position, then name), with the same clock.
    let both = r#"[{"name":"Greek","ids":["sblgnt","lxx"]},{"name":"Greek NT","ids":["sblgnt"]},{"name":"Study","ids":[]}]"#;
    let clock = a.clock(&RowKey::Preference(key.into()));
    assert!(clock.is_some());
    for d in [&a, &b] {
        assert_eq!(d.user.preference(key).unwrap().as_deref(), Some(both));
        assert_eq!(d.clock(&RowKey::Preference(key.into())), clock);
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
        conflict_of: None,
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

fn note_in(notebook: &str, body: &str) -> Annotation {
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

fn annotation_of(d: &Device, id: &str) -> Option<Annotation> {
    d.user.annotation(id).unwrap()
}

/// Every device holds the same Synced data.
fn converged(devices: &[&Device]) {
    let first = devices[0].user.synced_dump().unwrap();
    for d in &devices[1..] {
        assert_eq!(d.user.synced_dump().unwrap(), first);
    }
}

#[test]
fn a_note_edited_on_two_devices_keeps_both_texts_the_loser_as_a_conflict_copy() {
    let dir = Dir::new("conflict");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let default = a.user.notebooks().unwrap().remove(0).id;
    let note = Annotation {
        anchor: Some(obs_store::Anchor::Passage {
            passage: obs_scripture::Passage::verse(obs_scripture::Verse::new(
                obs_scripture::Book::Romans,
                8,
                28,
            )),
            origin: None,
        }),
        ..note_in(&default, "V0")
    };
    a.apply(&[Change::insert(Row::Annotation(note.clone()))]);
    Device::settle(&[&a, &b], &cloud);
    // Both edit its text before seeing the other's: B's is later.
    a.apply(&[Change::update(
        Row::Annotation(note.clone()),
        Row::Annotation(with_text(&note, "by A")),
    )]);
    b.apply(&[Change::update(
        Row::Annotation(note.clone()),
        Row::Annotation(with_text(&note, "by B")),
    )]);
    let lost = a.clock(&Row::Annotation(note.clone()).key()).unwrap();
    b.push(&cloud);
    let report = a.sync(&cloud);
    assert_eq!(report.recorded.conflicts, 1, "{report:?}");
    // B, whose edit won, makes none.
    let report = b.sync(&cloud);
    assert_eq!(report.recorded.conflicts, 0, "{report:?}");
    Device::settle(&[&a, &b], &cloud);
    for d in [&a, &b] {
        assert_eq!(annotation_of(d, &note.id).unwrap().body(), "by B");
        let copies = d.user.sync_conflicts().unwrap();
        assert_eq!(copies.len(), 1, "{copies:?}");
        let copy = &copies[0];
        assert_eq!(copy.body(), "by A");
        assert_eq!(copy.conflict_of.as_deref(), Some(note.id.as_str()));
        assert_eq!(copy.anchor, note.anchor, "the same Anchor");
        assert_eq!(copy.notebook_id, note.notebook_id, "the same Notebook");
        assert_eq!(copy.id, super::merge::conflict_id(&note.id, &lost));
    }
    converged(&[&a, &b]);
    // An edit that had seen the text makes no copy.
    let now = annotation_of(&a, &note.id).unwrap();
    a.apply(&[Change::update(
        Row::Annotation(now.clone()),
        Row::Annotation(with_text(&now, "by A again")),
    )]);
    Device::settle(&[&a, &b], &cloud);
    assert_eq!(b.user.sync_conflicts().unwrap().len(), 1);
}

#[test]
fn an_older_apps_op_without_conflict_of_keeps_it_here() {
    let dir = Dir::new("conflict-absent");
    let cloud = MemSyncFolder::new();
    let b = device(&dir, "b");
    let default = b.user.notebooks().unwrap().remove(0).id;
    let copy = Annotation {
        conflict_of: Some(new_id()),
        ..note_in(&default, "a copy")
    };
    b.apply(&[Change::insert(Row::Annotation(copy.clone()))]);
    let older = new_id();
    let edited = with_text(&copy, "edited by an older app");
    let mut line = serde_json::to_value(Op {
        device: older.clone(),
        clock: Hlc {
            wall_ms: u64::try_from(obs_store::clock::now_ms()).unwrap() + 1_000,
            counter: 0,
            device_id: older.clone(),
        },
        before: Some(Row::Annotation(copy.clone())),
        after: Some(Row::Annotation(edited.clone())),
        before_clock: None,
        absent: Vec::new(),
    })
    .unwrap();
    // This build always writes it, null or not.
    assert!(line["after"].get("conflict_of").is_some());
    for side in ["before", "after"] {
        line[side].as_object_mut().unwrap().remove("conflict_of");
    }
    cloud.put(&op_file_path(&older, 1), &raw_op_file(&[line]));
    b.sync(&cloud);
    assert_eq!(annotation_of(&b, &copy.id), Some(edited), "still a copy");
}

/// D edits a Note to T; G edits it to V' (later, never seeing T); X, having V', deletes the
/// Note with its Notebook. D gets X's delete before G's edit, or G's edit first: T survives
/// as a Sync conflict's copy either way.
#[test]
fn a_delete_that_never_saw_this_devices_note_text_keeps_it_as_a_conflict_copy() {
    for delete_first in [true, false] {
        let dir = Dir::new(&format!("conflict-delete-{delete_first}"));
        let cloud = MemSyncFolder::new();
        let (d, g, x) = (device(&dir, "d"), device(&dir, "g"), device(&dir, "x"));
        let nb = notebook("Romans");
        let note = note_in(&nb.id, "V0");
        d.apply(&[
            Change::insert(Row::Notebook(nb.clone())),
            Change::insert(Row::Annotation(note.clone())),
        ]);
        Device::settle(&[&d, &g, &x], &cloud);
        d.apply(&[Change::update(
            Row::Annotation(note.clone()),
            Row::Annotation(with_text(&note, "T")),
        )]);
        let v1 = with_text(&note, "V'");
        g.apply(&[Change::update(
            Row::Annotation(note.clone()),
            Row::Annotation(v1.clone()),
        )]);
        g.push(&cloud);
        x.sync(&cloud);
        x.apply(&[
            Change::delete(Row::Annotation(v1.clone())),
            Change::delete(Row::Notebook(nb.clone())),
        ]);
        x.push(&cloud);
        fn only<'a>(cloud: &MemSyncFolder, dev: &'a Device) -> Vec<(&'a str, u64)> {
            files_of(cloud, &dev.id)
                .into_iter()
                .map(|s| (dev.id.as_str(), s))
                .collect()
        }
        let (first, second) = if delete_first { (&x, &g) } else { (&g, &x) };
        merge_folder(&d.user, &view(&cloud, &only(&cloud, first))).unwrap();
        let all: Vec<(&str, u64)> = only(&cloud, first)
            .into_iter()
            .chain(only(&cloud, second))
            .collect();
        let report = merge_folder(&d.user, &view(&cloud, &all)).unwrap();
        assert!(report.waiting.is_empty(), "{report:?}");
        Device::settle(&[&d, &g, &x], &cloud);
        for dev in [&d, &g, &x] {
            let copies = dev.user.sync_conflicts().unwrap();
            assert_eq!(copies.len(), 1, "delete first: {delete_first}: {copies:?}");
            assert_eq!(copies[0].body(), "T");
            assert_eq!(copies[0].conflict_of.as_deref(), Some(note.id.as_str()));
        }
        converged(&[&d, &g, &x]);
    }
}

/// A deletes Notebook N with its Notes; B, not having seen that, adds a Note to it. Every
/// device ends with N, A's Notes and B's: B keeps N when A's delete meets its Note, and A
/// (and a third device) restore N from its tombstone when B's Note arrives, recording it, so
/// nothing waits for good.
#[test]
fn a_deleted_notebook_comes_back_with_its_notes_when_another_device_added_to_it() {
    let dir = Dir::new("parent");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let nb = notebook("Romans");
    let old = note_in(&nb.id, "old note");
    a.apply(&[
        Change::insert(Row::Notebook(nb.clone())),
        Change::insert(Row::Annotation(old.clone())),
    ]);
    Device::settle(&[&a, &b], &cloud);
    a.apply(&[
        Change::delete(Row::Annotation(old.clone())),
        Change::delete(Row::Notebook(nb.clone())),
    ]);
    let added = note_in(&nb.id, "added on B");
    b.apply(&[Change::insert(Row::Annotation(added.clone()))]);
    a.push(&cloud);
    b.push(&cloud);
    // A meets B's Note at its deleted Notebook: restored, not waiting.
    let report = a.sync(&cloud);
    assert!(report.waiting.is_empty(), "{report:?}");
    assert_eq!(report.recorded.parents_restored, 1, "{report:?}");
    // B meets A's delete with its Note there: kept, with A's Note deleted with it.
    let report = b.sync(&cloud);
    assert_eq!(
        report.outcomes.get(&Outcome::ParentKept),
        Some(&1),
        "{report:?}"
    );
    assert_eq!(report.recorded.children_restored, 1, "{report:?}");
    let c = device(&dir, "c");
    Device::settle(&[&a, &b, &c], &cloud);
    for d in [&a, &b, &c] {
        assert!(d.row(&Row::Notebook(nb.clone()).key()).is_some());
        assert!(annotation_of(d, &added.id).is_some());
        assert!(
            annotation_of(d, &old.id).is_some(),
            "deleted with it, back with it"
        );
    }
    converged(&[&a, &b, &c]);
}

/// A deletes Notebook N with its Note; B, not having seen that, adds a Note to N and then
/// deletes A's Note itself. B keeps N for its Note when A's delete arrives, but does not
/// bring back the Note its own user deleted (the S28b-2 review's P2-4).
#[test]
fn a_kept_parent_does_not_bring_back_a_row_this_devices_user_deleted_too() {
    let dir = Dir::new("parent-own-delete");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let nb = notebook("Romans");
    let old = note_in(&nb.id, "old note");
    a.apply(&[
        Change::insert(Row::Notebook(nb.clone())),
        Change::insert(Row::Annotation(old.clone())),
    ]);
    Device::settle(&[&a, &b], &cloud);
    a.apply(&[
        Change::delete(Row::Annotation(old.clone())),
        Change::delete(Row::Notebook(nb.clone())),
    ]);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let added = note_in(&nb.id, "added on B");
    b.apply(&[Change::insert(Row::Annotation(added.clone()))]);
    b.apply(&[Change::delete(Row::Annotation(old.clone()))]);
    a.push(&cloud);
    let report = b.sync(&cloud);
    assert_eq!(
        report.outcomes.get(&Outcome::ParentKept),
        Some(&1),
        "{report:?}"
    );
    assert_eq!(report.recorded.children_restored, 0, "{report:?}");
    Device::settle(&[&a, &b], &cloud);
    for d in [&a, &b] {
        assert!(d.row(&Row::Notebook(nb.clone()).key()).is_some());
        assert!(annotation_of(d, &added.id).is_some());
        assert_eq!(annotation_of(d, &old.id), None, "deleted on both");
    }
    converged(&[&a, &b]);
}

/// B adds a Note to N, then moves it (or deletes it) before it meets A's delete of N: B has
/// no child of N then and deletes it, but A restored N for B's Note and recorded that, so
/// every device ends the same.
#[test]
fn a_parent_restored_for_a_child_its_maker_then_moved_or_deleted_still_converges() {
    for delete in [false, true] {
        let dir = Dir::new(&format!("parent-moved-{delete}"));
        let cloud = MemSyncFolder::new();
        let (a, b) = (device(&dir, "a"), device(&dir, "b"));
        let default = b.user.notebooks().unwrap().remove(0).id;
        let nb = notebook("Romans");
        a.apply(&[Change::insert(Row::Notebook(nb.clone()))]);
        Device::settle(&[&a, &b], &cloud);
        a.apply(&[Change::delete(Row::Notebook(nb.clone()))]);
        let added = note_in(&nb.id, "added on B");
        b.apply(&[Change::insert(Row::Annotation(added.clone()))]);
        b.push(&cloud);
        if delete {
            b.apply(&[Change::delete(Row::Annotation(added.clone()))]);
        } else {
            b.apply(&[Change::update(
                Row::Annotation(added.clone()),
                Row::Annotation(Annotation {
                    notebook_id: default.clone(),
                    ..added.clone()
                }),
            )]);
        }
        b.push(&cloud);
        a.push(&cloud);
        // A reads B's file with the Note in N before the one moving or deleting it.
        let mut seen: Vec<(&str, u64)> = files_of(&cloud, &b.id)
            .into_iter()
            .map(|s| (b.id.as_str(), s))
            .collect();
        seen.pop();
        let report = merge_folder(&a.user, &view(&cloud, &seen)).unwrap();
        assert!(report.waiting.is_empty(), "{report:?}");
        assert_eq!(report.recorded.parents_restored, 1, "{report:?}");
        Device::settle(&[&a, &b], &cloud);
        for d in [&a, &b] {
            assert!(
                d.row(&Row::Notebook(nb.clone()).key()).is_some(),
                "delete: {delete}"
            );
        }
        converged(&[&a, &b]);
    }
}

/// The same rule for a Reading plan's Readings and a palette's styles.
#[test]
fn a_deleted_reading_plan_or_palette_comes_back_for_a_reading_or_style_added_meanwhile() {
    let dir = Dir::new("parent-plan");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let plan = ReadingPlanRow {
        id: 0,
        name: "NT".into(),
        template: "nt90".into(),
        bible_id: "bsb".into(),
        start_day: "2026-01-01".into(),
        weekdays: 0x7f,
        created_at: 0,
    };
    let readings: Vec<PlanReadingRow> = (0..2)
        .map(|i| PlanReadingRow {
            position: i,
            passages: format!("Matt {}", i + 1),
            day: format!("2026-01-0{}", i + 1),
            read_at: None,
        })
        .collect();
    let plan_a = a.user.add_reading_plan(&plan, &readings).unwrap();
    let palette = obs_store::HighlightPalette {
        id: new_id(),
        name: "Mine".into(),
        position: 1,
        created_at: 1,
        modified_at: 1,
    };
    a.apply(&[Change::insert(Row::HighlightPalette(palette.clone()))]);
    Device::settle(&[&a, &b], &cloud);
    // A deletes both; B marks a Reading read and adds a style to the palette.
    assert!(a.user.remove_reading_plan(plan_a).unwrap());
    a.apply(&[Change::delete(Row::HighlightPalette(palette.clone()))]);
    let plan_b = b.user.reading_plans().unwrap()[0].id;
    assert!(b.user.set_reading_read(plan_b, 1, true).unwrap());
    let style = obs_store::HighlightStyle {
        id: new_id(),
        palette_id: palette.id.clone(),
        position: 0,
        name: "Promise".into(),
        appearance: obs_store::StyleAppearance::new(
            obs_store::HighlightLook::Underline,
            obs_store::Rgb::new(1, 2, 3),
        ),
        created_at: 1,
        modified_at: 1,
    };
    b.apply(&[Change::insert(Row::HighlightStyle(style.clone()))]);
    Device::settle(&[&a, &b], &cloud);
    for d in [&a, &b] {
        let plans = d.user.reading_plans().unwrap();
        assert_eq!(plans.len(), 1);
        let read: Vec<bool> = d
            .user
            .plan_readings(plans[0].id)
            .unwrap()
            .iter()
            .map(|r| r.read_at.is_some())
            .collect();
        assert_eq!(read, [false, true], "both Readings back, B's mark kept");
        assert!(
            d.row(&Row::HighlightPalette(palette.clone()).key())
                .is_some()
        );
        assert!(d.row(&Row::HighlightStyle(style.clone()).key()).is_some());
    }
    converged(&[&a, &b]);
}

/// Three devices delete Notebook N, each its own version: A covers v5 at 10, B v7 at 8, C v7
/// at 9 (as wall clocks). A device meeting them in either order keeps C's row in N's
/// tombstone (the newest version covered, then the later delete), each compared with the
/// delete whose row is kept, not with the tombstone's newest of all (the S28b-2 review's
/// P2-2: A, B, C kept B's row and C, B, A kept C's).
#[test]
fn a_parents_tombstone_keeps_the_same_row_in_every_order_of_its_deletes() {
    let dir = Dir::new("tombstone-row");
    let now = u64::try_from(obs_store::clock::now_ms()).unwrap();
    let hlc = |ms: u64, device: &str| Hlc {
        wall_ms: now + ms,
        counter: 0,
        device_id: device.into(),
    };
    let nb = notebook("N");
    let writer = new_id();
    let cloud = MemSyncFolder::new();
    let mut files = Vec::new();
    for (name, covers, at) in [("A", 5, 10), ("B", 7, 8), ("C", 7, 9)] {
        let device = new_id();
        let delete = Op {
            device: device.clone(),
            clock: hlc(at, &device),
            before: Some(Row::Notebook(Notebook {
                name: format!("as {name} had it"),
                ..nb.clone()
            })),
            after: None,
            before_clock: Some(hlc(covers, &writer)),
            absent: Vec::new(),
        };
        cloud.put(
            &op_file_path(&device, 1),
            &encode_op_file(std::slice::from_ref(&delete)).unwrap(),
        );
        files.push(device);
    }
    let mut dumps = Vec::new();
    for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2]] {
        let d = device(&dir, &format!("d{}{}{}", order[0], order[1], order[2]));
        for i in order {
            merge_folder(&d.user, &view(&cloud, &[(files[i].as_str(), 1)])).unwrap();
        }
        let dump = d.user.synced_dump().unwrap();
        let tombstones = dump.split("[tombstone]").nth(1).unwrap();
        assert!(
            tombstones.contains("as C had it"),
            "{order:?}: {tombstones}"
        );
        dumps.push(tombstones.split("[item_clock]").next().unwrap().to_string());
    }
    assert_eq!(dumps[0], dumps[1]);
    assert_eq!(dumps[0], dumps[2]);
}

/// A child arriving at a parent deleted before schema 16 (its tombstone keeps no row) is
/// held, not waited for, and applies once another device brings the parent back.
#[test]
fn a_child_of_a_parent_deleted_before_schema_16_is_held_until_the_parent_comes_back() {
    let dir = Dir::new("parent-pre16");
    let cloud = MemSyncFolder::new();
    let (a, b, c) = (device(&dir, "a"), device(&dir, "b"), device(&dir, "c"));
    let nb = notebook("Romans");
    b.apply(&[Change::insert(Row::Notebook(nb.clone()))]);
    Device::settle(&[&a, &b, &c], &cloud);
    a.apply(&[Change::delete(Row::Notebook(nb.clone()))]);
    // As a tombstone from before schema 16 is.
    a.user
        .execute_unchecked("UPDATE tombstone SET row = NULL, row_covers = NULL, row_clock = NULL")
        .unwrap();
    let added = note_in(&nb.id, "added on B");
    b.apply(&[Change::insert(Row::Annotation(added.clone()))]);
    b.push(&cloud);
    let report = a.sync(&cloud);
    assert!(report.waiting.is_empty(), "{report:?}");
    assert_eq!(report.outcomes.get(&Outcome::Held), Some(&1), "{report:?}");
    let held = a.user.sync_held().unwrap();
    assert_eq!(held.len(), 1);
    assert!(held[0].why.contains("held until"), "{held:?}");
    // B meets A's delete with its Note there: it keeps N and records it, which brings N
    // back on A, and A's held Note applies.
    Device::settle(&[&a, &b, &c], &cloud);
    assert!(a.user.sync_held().unwrap().is_empty());
    assert!(annotation_of(&a, &added.id).is_some());
    converged(&[&b, &c]);
    // The one difference: A's own tombstone, from before schema 16, still keeps no row.
    a.user
        .execute_unchecked("UPDATE tombstone SET row = NULL, row_covers = NULL, row_clock = NULL")
        .unwrap();
    b.user
        .execute_unchecked("UPDATE tombstone SET row = NULL, row_covers = NULL, row_clock = NULL")
        .unwrap();
    converged(&[&a, &b]);
}

/// A child whose parent's own op is held here (a style held under a palette deleted before
/// schema 16) is held too, not waited for, so its device's stream goes on; both apply, the
/// parent first, once another device brings the palette back (the S28b-2 review's P2-1).
#[test]
fn a_child_of_a_held_parent_is_held_too_and_applies_after_it() {
    let dir = Dir::new("parent-held");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let nb = notebook("Romans");
    let palette = obs_store::HighlightPalette {
        id: new_id(),
        name: "Mine".into(),
        position: 1,
        created_at: 1,
        modified_at: 1,
    };
    b.apply(&[
        Change::insert(Row::Notebook(nb.clone())),
        Change::insert(Row::HighlightPalette(palette.clone())),
    ]);
    Device::settle(&[&a, &b], &cloud);
    a.apply(&[Change::delete(Row::HighlightPalette(palette.clone()))]);
    // As a tombstone from before schema 16 is.
    a.user
        .execute_unchecked("UPDATE tombstone SET row = NULL, row_covers = NULL, row_clock = NULL")
        .unwrap();
    let style = obs_store::HighlightStyle {
        id: new_id(),
        palette_id: palette.id.clone(),
        position: 0,
        name: "Promise".into(),
        appearance: obs_store::StyleAppearance::new(
            obs_store::HighlightLook::Underline,
            obs_store::Rgb::new(1, 2, 3),
        ),
        created_at: 1,
        modified_at: 1,
    };
    let verse = obs_scripture::Verse::new(obs_scripture::Book::Romans, 8, 28);
    let highlight = Annotation {
        id: new_id(),
        notebook_id: nb.id.clone(),
        content: Content::Highlight {
            style: style.id.clone(),
        },
        anchor: Some(obs_store::Anchor::Passage {
            passage: obs_scripture::Passage::verse(verse),
            origin: None,
        }),
        created_at: 1,
        modified_at: 1,
        conflict_of: None,
    };
    b.apply(&[Change::insert(Row::HighlightStyle(style.clone()))]);
    b.apply(&[Change::insert(Row::Annotation(highlight.clone()))]);
    b.push(&cloud);
    let report = a.sync(&cloud);
    assert!(report.waiting.is_empty(), "{report:?}");
    assert_eq!(report.outcomes.get(&Outcome::Held), Some(&2), "{report:?}");
    // B's next op file merges on A.
    let later = note_in(&nb.id, "later on B");
    b.apply(&[Change::insert(Row::Annotation(later.clone()))]);
    b.push(&cloud);
    let report = a.sync(&cloud);
    assert!(report.waiting.is_empty(), "{report:?}");
    assert!(annotation_of(&a, &later.id).is_some());
    // B meets A's delete with its style there: it keeps the palette and records it, which
    // brings it back on A, where the held style and then its Highlight apply.
    Device::settle(&[&a, &b], &cloud);
    assert!(a.user.sync_held().unwrap().is_empty());
    assert!(a.row(&Row::HighlightStyle(style.clone()).key()).is_some());
    assert_eq!(annotation_of(&a, &highlight.id), Some(highlight));
    a.user
        .execute_unchecked("UPDATE tombstone SET row = NULL, row_covers = NULL, row_clock = NULL")
        .unwrap();
    b.user
        .execute_unchecked("UPDATE tombstone SET row = NULL, row_covers = NULL, row_clock = NULL")
        .unwrap();
    converged(&[&a, &b]);
}

#[test]
fn two_collections_of_one_name_added_on_two_devices_both_stay_the_loser_renamed() {
    let dir = Dir::new("items-one-name");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let key = "library.collections";
    a.user
        .set_preference(key, r#"[{"name":"Greek","ids":["sblgnt"]}]"#)
        .unwrap();
    b.user
        .set_preference(key, r#"[{"name":"Greek","ids":["lxx"]}]"#)
        .unwrap();
    a.push(&cloud);
    let report = b.sync(&cloud);
    // A's add is the earlier: B keeps its own, and A, whose own lost, renames it.
    assert_eq!(report.recorded.items_renamed, 0, "{report:?}");
    let report = a.sync(&cloud);
    assert_eq!(report.recorded.items_renamed, 1, "{report:?}");
    Device::settle(&[&a, &b], &cloud);
    let both = r#"[{"name":"Greek","ids":["lxx"]},{"name":"Greek (2)","ids":["sblgnt"]}]"#;
    for d in [&a, &b] {
        assert_eq!(d.user.preference(key).unwrap().as_deref(), Some(both));
    }
    converged(&[&a, &b]);
}

/// G adds "Greek" at g0, D adds its own "Greek" at d1 after g0, then G edits its "Greek" at g2
/// after d1, neither having synced: G's edit replaces D's item having seen only G's own, so D
/// records its item again as "Greek (2)" and nothing is lost (the S28b-2 review's P1-2).
#[test]
fn an_edit_of_an_item_that_never_saw_this_devices_item_of_its_name_keeps_both() {
    let dir = Dir::new("items-one-name-edit");
    let cloud = MemSyncFolder::new();
    let (d, g) = (device(&dir, "d"), device(&dir, "g"));
    let key = "library.collections";
    let pause = || std::thread::sleep(std::time::Duration::from_millis(2));
    g.user
        .set_preference(key, r#"[{"name":"Greek","ids":["lxx"]}]"#)
        .unwrap();
    pause();
    d.user
        .set_preference(key, r#"[{"name":"Greek","ids":["sblgnt"]}]"#)
        .unwrap();
    pause();
    g.user
        .set_preference(key, r#"[{"name":"Greek","ids":["lxx","wlc"]}]"#)
        .unwrap();
    g.push(&cloud);
    let report = d.sync(&cloud);
    assert_eq!(report.recorded.items_renamed, 1, "{report:?}");
    let report = g.sync(&cloud);
    assert_eq!(report.recorded.items_renamed, 0, "{report:?}");
    Device::settle(&[&d, &g], &cloud);
    let both = r#"[{"name":"Greek","ids":["lxx","wlc"]},{"name":"Greek (2)","ids":["sblgnt"]}]"#;
    for dev in [&d, &g] {
        assert_eq!(dev.user.preference(key).unwrap().as_deref(), Some(both));
    }
    converged(&[&d, &g]);
    // An edit that saw D's item (its `before` is D's version) replaces it: no third copy.
    let seen =
        r#"[{"name":"Greek","ids":["lxx","wlc"]},{"name":"Greek (2)","ids":["sblgnt","na28"]}]"#;
    g.user.set_preference(key, seen).unwrap();
    Device::settle(&[&d, &g], &cloud);
    for dev in [&d, &g] {
        assert_eq!(dev.user.preference(key).unwrap().as_deref(), Some(seen));
    }
    converged(&[&d, &g]);
}

/// X adds Collection "Read"; Y adds its own "Read" later; Z, having Y's, deletes "Read". X
/// meets Z's delete before Y's add (or after it): the delete never saw X's item, so X's comes
/// back under its own name, edit beats delete (or, Y's add having renamed it first, as "Read
/// (2)"); nothing is lost on any device (the S28b-2 review's D6 survival invariant, seed
/// 1592592031).
#[test]
fn a_delete_that_never_saw_this_devices_list_item_of_its_name_keeps_it() {
    for delete_first in [true, false] {
        let dir = Dir::new(&format!("item-unseen-delete-{delete_first}"));
        let cloud = MemSyncFolder::new();
        let (x, y, z) = (device(&dir, "x"), device(&dir, "y"), device(&dir, "z"));
        Device::settle(&[&x, &y, &z], &cloud);
        let key = "library.collections";
        x.user
            .set_preference(key, r#"[{"name":"Read","ids":["kjv"]}]"#)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        y.user
            .set_preference(key, r#"[{"name":"Read","ids":["bsb"]}]"#)
            .unwrap();
        y.push(&cloud);
        z.sync(&cloud);
        z.user.set_preference(key, "[]").unwrap();
        z.push(&cloud);
        // Y meets Z's delete of its item before X's: else X's re-record (which never saw Y's
        // item) would make Y keep its item as "Read (2)", a copy no one needed (safe, as D2's
        // three-device chain is).
        y.sync(&cloud);
        let only = |dev: &Device| -> Vec<(String, u64)> {
            files_of(&cloud, &dev.id)
                .into_iter()
                .map(|s| (dev.id.clone(), s))
                .collect()
        };
        let (first, second) = if delete_first { (&z, &y) } else { (&y, &z) };
        let mut seen = only(first);
        let files = |seen: &[(String, u64)]| -> MemSyncFolder {
            let files: Vec<(&str, u64)> = seen.iter().map(|(d, n)| (d.as_str(), *n)).collect();
            view(&cloud, &files)
        };
        write_pending_ops(&x.user, &cloud).unwrap();
        let report = merge_folder(&x.user, &files(&seen)).unwrap();
        if delete_first {
            assert_eq!(report.recorded.items_kept, 1, "{report:?}");
        }
        seen.extend(only(second));
        merge_folder(&x.user, &files(&seen)).unwrap();
        Device::settle(&[&x, &y, &z], &cloud);
        let kept = if delete_first {
            r#"[{"name":"Read","ids":["kjv"]}]"#
        } else {
            r#"[{"name":"Read (2)","ids":["kjv"]}]"#
        };
        for dev in [&x, &y, &z] {
            assert_eq!(
                dev.user.preference(key).unwrap().as_deref(),
                Some(kept),
                "delete first: {delete_first}"
            );
        }
        converged(&[&x, &y, &z]);
    }
}

/// B tags a Resource; A, which has no tags and has not met B's, saves its Collections, which
/// writes the Resource tags too (`Session::save_collections`), `"{}"`: that changes nothing,
/// so it is no whole value (a base, D7) superseding B's tag, and every device keeps the tag.
#[test]
fn a_collections_save_on_a_device_with_no_tags_keeps_another_devices_earlier_tags() {
    let dir = Dir::new("no-tags");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let tags = "library.tags";
    let tagged = r#"{"kjv":["English"]}"#;
    b.user.set_preference(tags, tagged).unwrap();
    b.push(&cloud);
    std::thread::sleep(std::time::Duration::from_millis(2));
    a.user
        .set_preference("library.collections", r#"[{"name":"Greek","ids":[]}]"#)
        .unwrap();
    a.user.set_preference(tags, "{}").unwrap();
    assert_eq!(a.user.preference(tags).unwrap(), None, "no value: no tags");
    Device::settle(&[&a, &b], &cloud);
    for d in [&a, &b] {
        assert_eq!(d.user.preference(tags).unwrap().as_deref(), Some(tagged));
    }
    converged(&[&a, &b]);
}

/// A writes a value of the Collections that is not a list (damaged, D7) at w; B adds an item
/// at x after w. Both hold the damaged value with B's item held, whatever order they meet;
/// a value in shape written after that (before x) gets B's item on top, on both.
#[test]
fn a_damaged_whole_list_value_holds_later_items_on_every_device_until_it_is_in_shape() {
    let dir = Dir::new("whole-value");
    let cloud = MemSyncFolder::new();
    let (a, b) = (device(&dir, "a"), device(&dir, "b"));
    let key = "library.collections";
    a.user
        .set_preference(key, r#"[{"name":"Greek","ids":[]}]"#)
        .unwrap();
    Device::settle(&[&a, &b], &cloud);
    a.user.set_preference(key, "not a list").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    b.user
        .set_preference(
            key,
            r#"[{"name":"Greek","ids":[]},{"name":"Hebrew","ids":["wlc"]}]"#,
        )
        .unwrap();
    a.push(&cloud);
    b.push(&cloud);
    Device::settle(&[&a, &b], &cloud);
    for d in [&a, &b] {
        assert_eq!(
            d.user.preference(key).unwrap().as_deref(),
            Some("not a list")
        );
        let held = d.user.sync_held().unwrap();
        assert_eq!(held.len(), 1, "{held:?}");
        assert_eq!(
            held[0].change.after.as_ref().unwrap().key(),
            RowKey::Collection("Hebrew".into())
        );
    }
    converged(&[&a, &b]);
    // A value in shape written over it, at a clock before B's item: B's item goes on top.
    let fix = r#"[{"name":"Fixed","ids":[]}]"#;
    let b_item = b.user.sync_held().unwrap()[0].clock.clone();
    let older = new_id();
    let fixed = Op {
        device: older.clone(),
        clock: Hlc {
            wall_ms: b_item.wall_ms - 1,
            counter: 0,
            device_id: older.clone(),
        },
        before: Some(Row::Preference(obs_store::SyncedPreference {
            key: key.into(),
            value: "not a list".into(),
        })),
        after: Some(Row::Preference(obs_store::SyncedPreference {
            key: key.into(),
            value: fix.into(),
        })),
        before_clock: None,
        absent: Vec::new(),
    };
    cloud.put(
        &op_file_path(&older, 1),
        &encode_op_file(std::slice::from_ref(&fixed)).unwrap(),
    );
    Device::settle(&[&a, &b], &cloud);
    let on_top = r#"[{"name":"Fixed","ids":[]},{"name":"Hebrew","ids":["wlc"]}]"#;
    for d in [&a, &b] {
        assert_eq!(d.user.preference(key).unwrap().as_deref(), Some(on_top));
        assert!(d.user.sync_held().unwrap().is_empty());
    }
    converged(&[&a, &b]);
}

/// P writes a damaged value at w; R, having it, writes one in shape at f; Q adds an item at x
/// after f before meeting either, and meets R's value before P's: its item goes on top of
/// R's value, as on every other device.
#[test]
fn an_item_written_after_a_whole_value_goes_on_top_of_it_in_any_arrival_order() {
    let dir = Dir::new("whole-value-later");
    let cloud = MemSyncFolder::new();
    let (p, q, r) = (device(&dir, "p"), device(&dir, "q"), device(&dir, "r"));
    let key = "library.collections";
    p.user
        .set_preference(key, r#"[{"name":"A","ids":[]}]"#)
        .unwrap();
    Device::settle(&[&p, &q, &r], &cloud);
    p.user.set_preference(key, "not a list").unwrap();
    p.push(&cloud);
    r.sync(&cloud);
    let fixed = r#"[{"name":"Fixed","ids":[]}]"#;
    r.user.set_preference(key, fixed).unwrap();
    r.push(&cloud);
    std::thread::sleep(std::time::Duration::from_millis(2));
    q.user
        .set_preference(key, r#"[{"name":"A","ids":[]},{"name":"Q","ids":["q"]}]"#)
        .unwrap();
    q.push(&cloud);
    let r_files: Vec<(&str, u64)> = files_of(&cloud, &r.id)
        .into_iter()
        .map(|s| (r.id.as_str(), s))
        .collect();
    merge_folder(&q.user, &view(&cloud, &r_files)).unwrap();
    let on_top = r#"[{"name":"Fixed","ids":[]},{"name":"Q","ids":["q"]}]"#;
    assert_eq!(q.user.preference(key).unwrap().as_deref(), Some(on_top));
    Device::settle(&[&p, &q, &r], &cloud);
    for d in [&p, &q, &r] {
        assert_eq!(d.user.preference(key).unwrap().as_deref(), Some(on_top));
        assert!(d.user.sync_held().unwrap().is_empty());
    }
    converged(&[&p, &q, &r]);
}

#[test]
fn an_op_this_build_rejects_is_held_and_the_rest_of_its_file_merges() {
    let dir = Dir::new("held");
    let cloud = MemSyncFolder::new();
    let b = device(&dir, "b");
    let later = new_id();
    let (f1, f2) = (favorite("Gen 1:1"), favorite("Ex 3:14"));
    let now = u64::try_from(obs_store::clock::now_ms()).unwrap();
    let op = |counter, row| Op {
        device: later.clone(),
        clock: Hlc {
            wall_ms: now,
            counter,
            device_id: later.clone(),
        },
        before: None,
        after: Some(row),
        before_clock: None,
        absent: Vec::new(),
    };
    // A synced preference a later app added, between two good ops.
    let unknown = Row::Preference(obs_store::SyncedPreference {
        key: "study.later_app_setting".into(),
        value: "1".into(),
    });
    cloud.put(
        &op_file_path(&later, 1),
        &encode_op_file(&[
            op(0, Row::Favorite(f1.clone())),
            op(1, unknown.clone()),
            op(2, Row::Favorite(f2.clone())),
        ])
        .unwrap(),
    );
    let report = b.sync(&cloud);
    assert!(report.waiting.is_empty(), "{report:?}");
    assert_eq!(report.merged, [(later.clone(), 1)]);
    assert_eq!(report.outcomes.get(&Outcome::Held), Some(&1), "{report:?}");
    assert!(b.row(&Row::Favorite(f1).key()).is_some());
    assert!(b.row(&Row::Favorite(f2).key()).is_some());
    let held = b.user.sync_held().unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].change.after, Some(unknown));
    assert!(held[0].why.contains("not Synced data"), "{held:?}");
    // The stream goes on: the device's next file merges too.
    cloud.put(
        &op_file_path(&later, 2),
        &encode_op_file(&[op(3, Row::Favorite(favorite("Lev 19:2")))]).unwrap(),
    );
    let report = b.sync(&cloud);
    assert_eq!(report.merged, [(later, 2)]);
    assert_eq!(
        b.user.sync_held().unwrap().len(),
        1,
        "still held, tried again"
    );
}

/// A later app writing its own synced preference again and again: only its newest held
/// write is kept (it supersedes the older once it applies), in either order of holding; a
/// held delete is kept beside it (the S28b-2 review's P2-5).
#[test]
fn of_a_rows_held_writes_only_the_newest_is_kept() {
    let dir = Dir::new("held-newest");
    let later = new_id();
    let now = u64::try_from(obs_store::clock::now_ms()).unwrap();
    let key = "study.later_app_setting";
    let value = |v: &str| {
        Some(Row::Preference(obs_store::SyncedPreference {
            key: key.into(),
            value: v.into(),
        }))
    };
    let op = |counter, before, after| Op {
        device: later.clone(),
        clock: Hlc {
            wall_ms: now,
            counter,
            device_id: later.clone(),
        },
        before,
        after,
        before_clock: None,
        absent: Vec::new(),
    };
    let ops = [
        op(0, None, value("1")),
        op(1, value("1"), value("2")),
        op(2, value("2"), None),
        op(3, None, value("3")),
    ];
    let b = device(&dir, "b");
    let cloud = MemSyncFolder::new();
    cloud.put(&op_file_path(&later, 1), &encode_op_file(&ops).unwrap());
    let report = b.sync(&cloud);
    // The delete of a row not here is only noted in its tombstone.
    assert_eq!(report.outcomes.get(&Outcome::Held), Some(&3), "{report:?}");
    let held: Vec<(u16, bool)> = b
        .user
        .sync_held()
        .unwrap()
        .iter()
        .map(|h| (h.clock.counter, h.change.after.is_some()))
        .collect();
    assert_eq!(held, [(3, true)], "the newest write");
    // Held in the other order (as a device holds them that met another's files first): an
    // older write is not kept beside a newer one, and a held delete is kept.
    let c = device(&dir, "c");
    c.user
        .merge_held(|tx| {
            for op in ops.iter().rev() {
                tx.hold(&obs_store::HeldOp {
                    clock: op.clock.clone(),
                    change: op.change(),
                    before_clock: None,
                    absent: Vec::new(),
                    why: "held".into(),
                })?;
            }
            Ok::<(), obs_store::StoreError>(())
        })
        .unwrap();
    let held: Vec<u16> = c
        .user
        .sync_held()
        .unwrap()
        .iter()
        .map(|h| h.clock.counter)
        .collect();
    assert_eq!(held, [2, 3]);
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
