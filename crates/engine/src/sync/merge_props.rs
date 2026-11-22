//! Property tests of the merge (ADR 0022 rule 3, plans S28b-1 and S28b-2): two or three
//! Synced devices over one in-memory Sync folder make random edits of Notebooks (deleted with
//! their Annotations, or those moved to the default), Notes (their text edited on several
//! devices at once) and Highlights, Highlight styles and palettes (deleted with Highlights on
//! them, moved off), Favorites, Layouts (names clash), the Preferred order, Reading plans
//! (deleted with their Readings) and their done-marks, a synced preference and Collections
//! (items changed, added under one name on two devices, removed and moved, and the whole value
//! written damaged and written again in shape) with the Resource tags saved beside them as
//! `Session::save_collections` saves them, deleting rows too, undo their own last change (a row
//! deleted comes back with its old id), and sync at random moments, sometimes seeing only part
//! of the folder (some devices' files, or only their first few). Two sequences from the S28b-2
//! review run now and then: Q1 (a Note's text edited on two devices, then deleted by a third
//! device before the first meets the second's edit) and D7's (a list damaged and mended, an item
//! added after the mend by a device whose clock runs ahead, a third device meeting that item
//! first). Each device's wall
//! clock is its own: the case's clock moves on by random steps, and a device's runs from it by
//! an offset of its own, sometimes more than [`MAX_FORWARD_SKEW_MS`] ahead (its clocks are then
//! not observed). Once every device has synced until nothing more moves:
//!
//! - **convergence:** every device holds the same Synced data, clocks and tombstones;
//! - **no user-made row disappears** (ADR 0006), and edit beats delete: a row is there exactly
//!   when its newest write is later than everything a delete of it covered, holding that
//!   write, on every device; no row is there that no op wrote (or the devices began with); a
//!   Collection follows its list's base, the newest whole-value write (D7): an item written
//!   after it as above, one before it as the base has it unless a later delete covering the
//!   base removed it, none while the base is damaged;
//! - **every Note text survives** (D2, ADR 0006): each text an op wrote is a live Note's (in
//!   place, or a Sync conflict's copy), unless an op that had it deleted its Note or wrote
//!   other text over it (an op that only moved it does not count); each copy, as first
//!   written, holds a text an op wrote to the Note its `conflict_of` names, and its id is
//!   `conflict_id` of that Note and that op's clock;
//! - **every Collection added survives** (D6, ADR 0006): each JSON an add op wrote is a live
//!   Collection's under some name, unless an op that had it replaced or deleted it, or the
//!   Collections' base supersedes it (written at or after it, or damaged);
//! - each tombstone holds the newest delete clock and the newest version covered of the op
//!   stream; each device has one default Notebook, the oldest (`created_at`, id) of those the
//!   devices began with; an op is held only while its list's value is damaged.
//!
//! The generator is a seeded SplitMix64 (no property-test crate); under the case's fixed clock
//! (`obs_store::clock::fix`) and seeded ids (`obs_store::annotation::seed_ids`) a seed makes
//! the same case on every run. The CI run takes [`CI_CASES`] cases;
//! `cargo test -p obs-engine sync -- --include-ignored` runs [`FULL_CASES`] too. A failure
//! names its seed; `OBS_MERGE_SEED=<seed>` reruns that case first, and `OBS_MERGE_TRACE=<text>`
//! prints every op of each case whose line holds that text. Not generated: deleting a
//! list preference's whole value (an undo of the first whole write of it), and ops a later app
//! wrote (`merge_tests` holds one).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use obs_scripture::{Book, Passage, Verse};
use obs_store::annotation::{inverse, new_id, seed_ids};
use obs_store::clock::fix;
use obs_store::hlc::MAX_FORWARD_SKEW_MS;
use obs_store::{
    Anchor, Annotation, Change, Content, HighlightLook, HighlightPalette, HighlightStyle, Hlc,
    Notebook, PlanReadingRow, ReadingPlanRow, Rgb, Row, RowKey, StoreError, StyleAppearance,
    Tombstone, UserStore,
};

use super::folder::{FolderError, MemSyncFolder, SyncFolder};
use super::lifecycle::{RestoreChoice, SyncOptions, SyncState, restore_choice, sync_now};
use super::merge::{Outcome, merge_folder};
use super::merge_tests::{Dir, files_of, push_ops, view};
use super::ops::{Op, OpFileRead, decode_op_file, write_pending_ops};
use super::snapshot::{SnapshotFile, SnapshotPolicy, snapshot_file, snapshot_ops};

/// Cases each CI run checks.
const CI_CASES: u64 = 1_000;
/// Cases the ignored long run checks.
const FULL_CASES: u64 = 10_000;
/// The wall clock (Unix ms, 2027) the template is made at; each case's starts an hour on.
const TEMPLATE_MS: i64 = 1_800_000_000_000;
/// `MAX_FORWARD_SKEW_MS` as a wall-clock offset.
const SKEW_MS: i64 = MAX_FORWARD_SKEW_MS as i64;

/// SplitMix64: small, seeded, and the same on every platform.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n.max(1)).unwrap()).unwrap()
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }

    /// Milliseconds below `n`.
    fn ms(&mut self, n: usize) -> i64 {
        i64::try_from(self.below(n)).unwrap()
    }

    /// How far a device's wall clock runs from the case's: mostly not at all, sometimes
    /// seconds either way or minutes behind, sometimes more than [`MAX_FORWARD_SKEW_MS`] ahead.
    fn offset(&mut self) -> i64 {
        match self.below(6) {
            0..=2 => 0,
            3 => self.ms(20_000) - 10_000,
            4 => -self.ms(600_000),
            _ => SKEW_MS + 1 + self.ms(MAX_FORWARD_SKEW_MS as usize),
        }
    }

    /// The step the case's clock moves on by before an action: often none or one millisecond
    /// (clock ties, counters), else up to seconds.
    fn step(&mut self) -> i64 {
        match self.below(4) {
            0 => 0,
            1 => 1,
            2 => self.ms(50),
            _ => self.ms(5_000),
        }
    }
}

const NAMES: &[&str] = &["Study", "Romans", "Greek", "Read"];
const RESOURCES: &[&str] = &["kjv", "web", "bsb", "lsv", "sblgnt"];
/// The built-in style Highlights move to when theirs is deleted (never deleted itself).
const KEPT_STYLE: &str = "yellow";
const COLLECTIONS: &str = "library.collections";
const TAGS: &str = "library.tags";
/// Values of the Collections that are not a list this app reads (D7).
const DAMAGED: &[&str] = &["not a list", r#"[{"name":"Dup"},{"name":"Dup"}]"#];

/// The Sync folder of a case, keeping every op file and snapshot ever written to it
/// (trimming and newer snapshots delete them from the folder; the invariants read every one).
struct ArchivingFolder {
    folder: MemSyncFolder,
    archive: std::sync::Mutex<BTreeMap<String, Vec<u8>>>,
}

impl SyncFolder for ArchivingFolder {
    fn list(&self, dir: &str) -> Result<Vec<String>, FolderError> {
        self.folder.list(dir)
    }
    fn read(&self, path: &str) -> Result<Vec<u8>, FolderError> {
        self.folder.read(path)
    }
    fn write_new(&self, path: &str, bytes: &[u8]) -> Result<(), FolderError> {
        if path.contains("/ops/") || path.contains("/snapshots/") {
            self.archive
                .lock()
                .unwrap()
                .insert(path.to_string(), bytes.to_vec());
        }
        self.folder.write_new(path, bytes)
    }
    fn remove(&self, path: &str) -> Result<(), FolderError> {
        self.folder.remove(path)
    }
    fn ensure_available(&self, path: &str) -> Result<bool, FolderError> {
        self.folder.ensure_available(path)
    }
}

/// One case's devices and the folder they share.
struct World {
    devices: Vec<UserStore>,
    folder: ArchivingFolder,
    /// Notebooks the generator made (not a device's default): it may delete them.
    deletable: BTreeSet<String>,
    /// Highlight styles and palettes the generator made: it may delete them.
    made_styles: BTreeSet<String>,
    made_palettes: BTreeSet<String>,
    /// Parents (`row_name`) some device deleted, and the Notebooks among them whose child a
    /// device moved out or deleted while it still had the Notebook (D3's second case, if the
    /// Notebook comes back).
    deleted_parents: BTreeSet<String>,
    child_gone: BTreeSet<String>,
    /// For unique texts.
    n: u64,
    /// The case's wall clock (Unix ms); each device's runs from it by its offset.
    now: i64,
    /// How far each device's wall clock runs from `now`.
    offsets: Vec<i64>,
    /// Each device's last change (the Changes its edit applied), for its undo.
    last: Vec<Vec<Change>>,
    /// Each device's join snapshot not yet in the folder (its first push writes it before its
    /// op files); `None` before it joins or once written.
    joins: Vec<Option<SnapshotFile>>,
    /// Whether each device has joined (it records ops).
    joined: Vec<bool>,
    /// Each device's Backup (a copy of its database), to restore later.
    backups: Vec<Option<UserStore>>,
    /// Note texts and Collections (their JSON without the name) a device discarded on purpose
    /// by restoring a Backup without them and replacing what is in the folder: they need not
    /// survive.
    discarded_texts: BTreeSet<String>,
    discarded_items: BTreeSet<String>,
    /// How this case writes snapshots and trims (small, so they happen).
    options: SyncOptions,
    /// What the merges did, over every case of a run.
    stats: Stats,
}

/// What the merges of a run did, to show the cases reach every rule.
#[derive(Default)]
struct Stats {
    outcomes: BTreeMap<super::merge::Outcome, usize>,
    retried: usize,
    /// Clocks not observed for being more than a day ahead.
    not_observed: usize,
    renamed_layouts: usize,
    demoted_defaults: usize,
    /// Collection items moved, and undos applied.
    moved: usize,
    undone: usize,
    /// What the merges recorded (D2, D3, D6) and held ops applied.
    recorded: super::merge::Recorded,
    /// Whole values of the Collections written damaged, and in shape over a damaged one.
    damaged: usize,
    fixed: usize,
    /// Parents back after a device moved or deleted its child of them (D3's second case).
    maker_left: usize,
    /// The review's targeted Q1 and D7 sequences run whole ([`World::q1_step`],
    /// [`World::d7_step`]).
    q1_steps: usize,
    d7_steps: usize,
    /// D3's second case run whole ([`World::d3_step`]).
    d3_steps: usize,
    /// S28c: devices joined after the others had synced, snapshots merged (and of them, of a
    /// device met before: its files trimmed), own op files trimmed, Backups restored with
    /// each choice.
    late_joins: usize,
    snapshots_merged: usize,
    snapshots_after_trim: usize,
    trimmed: usize,
    catch_ups: usize,
    replaces: usize,
}

impl Stats {
    fn add(&mut self, report: &super::merge::MergeReport) {
        for (outcome, n) in &report.outcomes {
            *self.outcomes.entry(*outcome).or_default() += n;
        }
        self.retried += report.retried;
        self.not_observed += report.not_observed;
        self.recorded.conflicts += report.recorded.conflicts;
        self.recorded.parents_restored += report.recorded.parents_restored;
        self.recorded.children_restored += report.recorded.children_restored;
        self.recorded.items_renamed += report.recorded.items_renamed;
        self.recorded.items_kept += report.recorded.items_kept;
        self.recorded.unheld += report.recorded.unheld;
    }
}

/// A device copied from `template` with an id and default Notebook of its own (written as
/// its own edit before it records ops, so only its join snapshot brings it to the others).
fn device(template: &UserStore, rng: &mut Rng) -> UserStore {
    let user = template.copy_in_memory().unwrap();
    user.execute_unchecked(&format!("UPDATE device SET device_id = '{}'", new_id()))
        .unwrap();
    user.execute_unchecked(&format!(
        "UPDATE notebooks SET id = '{}', created_at = {} WHERE is_default = 1",
        new_id(),
        1_000 + rng.below(3)
    ))
    .unwrap();
    let default = user.notebooks().unwrap().remove(0);
    user.apply_changes(&[Change::update(
        Row::Notebook(default.clone()),
        Row::Notebook(Notebook {
            modified_at: default.modified_at + 1,
            ..default
        }),
    )])
    .unwrap();
    // Joining is not a change to undo.
    user.take_applied_changes();
    user
}

impl World {
    fn text(&mut self, what: &str) -> String {
        self.n += 1;
        format!("{what} {}", self.n)
    }

    /// Run this thread's clock as device `d`'s.
    fn as_device(&self, d: usize) {
        fix(Some(self.now + self.offsets[d]));
    }

    /// Device `d` joins now if it has not (plan S28c): it records ops from now on, and its
    /// snapshot of what it holds goes out with its first push.
    fn join(&mut self, d: usize) {
        if self.joined[d] {
            return;
        }
        let user = &self.devices[d];
        user.join_sync().unwrap();
        self.joins[d] = Some(snapshot_file(user).unwrap());
        self.joined[d] = true;
        if self.joined.iter().filter(|j| **j).count() > 1
            && !self.folder.archive.lock().unwrap().is_empty()
        {
            self.stats.late_joins += 1;
        }
    }

    /// Device `d` writes its join snapshot (if not yet) and its pending ops.
    fn push_device(&mut self, d: usize) -> Vec<u64> {
        self.join(d);
        let user = &self.devices[d];
        let written = match &self.joins[d] {
            Some(join) => push_ops(user, join, &self.folder),
            None => write_pending_ops(user, &self.folder).unwrap(),
        };
        self.joins[d] = None;
        written
    }

    /// Device `d` syncs as the app does (`sync_now`): it also writes its snapshots and trims
    /// its own op files, at this case's small policy.
    fn sync_now(&mut self, d: usize) {
        self.push_device(d);
        let met = self.met(d);
        let report = match sync_now(&self.devices[d], &self.folder, &self.options).unwrap() {
            SyncState::Synced(report) => report,
            other => panic!("device {d}: {other:?}"),
        };
        self.stats.trimmed += report.trimmed.len();
        self.merged(d, &report.merge, &met);
    }

    /// What device `d` has merged of each other device's (none: not met).
    fn met(&self, d: usize) -> BTreeMap<String, Option<u64>> {
        let user = &self.devices[d];
        self.devices
            .iter()
            .map(|e| e.device_id().unwrap())
            .chain(user.merged_devices().unwrap().into_keys())
            .map(|id| {
                let seq = user.merged_up_to(&id).unwrap();
                (id, seq)
            })
            .collect()
    }

    /// Device `d` takes a Backup of its database, or restores the one it took and chooses to
    /// catch up or replace (ADR 0022 rule 7). What a replace discards on purpose (the Note
    /// texts and Collections it holds that the Backup does not) need not survive.
    fn backup_or_restore(&mut self, d: usize, rng: &mut Rng) {
        if !self.joined[d] {
            return;
        }
        let Some(backup) = self.backups[d].take().filter(|_| rng.below(2) == 0) else {
            self.backups[d] = Some(self.devices[d].copy_in_memory().unwrap());
            return;
        };
        self.push_device(d);
        let choice = if rng.below(2) == 0 {
            RestoreChoice::CatchUp
        } else {
            RestoreChoice::Replace
        };
        if choice == RestoreChoice::Replace {
            let texts = |u: &UserStore| -> BTreeSet<String> {
                u.annotations(None)
                    .unwrap()
                    .iter()
                    .filter(|a| a.is_note())
                    .map(|a| a.body().to_string())
                    .collect()
            };
            let items = |u: &UserStore| -> BTreeSet<String> {
                u.preference(COLLECTIONS)
                    .unwrap()
                    .and_then(|v| collection_items(&v))
                    .unwrap_or_default()
                    .values()
                    .map(|j| without_name(j))
                    .collect()
            };
            let (now_texts, now_items) = (texts(&self.devices[d]), items(&self.devices[d]));
            let (then_texts, then_items) = (texts(&backup), items(&backup));
            self.discarded_texts
                .extend(now_texts.difference(&then_texts).cloned());
            self.discarded_items
                .extend(now_items.difference(&then_items).cloned());
        }
        self.devices[d].restore_from(&backup).unwrap();
        let met = self.met(d);
        let report =
            match restore_choice(&self.devices[d], &self.folder, choice, &self.options).unwrap() {
                SyncState::Synced(report) => report,
                other => panic!("device {d}: {other:?}"),
            };
        match choice {
            RestoreChoice::CatchUp => self.stats.catch_ups += 1,
            RestoreChoice::Replace => self.stats.replaces += 1,
        }
        self.last[d].clear();
        self.joins[d] = None;
        self.stats.trimmed += report.trimmed.len();
        self.merged(d, &report.merge, &met);
    }

    /// Device `d` writes its ops and merges: often everything in the folder, else what a
    /// partly synced folder shows it (each other device's files, or only its first few), and
    /// sometimes as the app does, trimming too.
    fn sync(&mut self, d: usize, rng: &mut Rng) {
        if rng.below(4) == 0 {
            self.sync_now(d);
            return;
        }
        if rng.below(3) == 0 {
            let mut seen = Vec::new();
            for e in 0..self.devices.len() {
                if e != d {
                    let files = self.files(e);
                    let k = match rng.below(3) {
                        0 => files.len(),
                        _ => rng.below(files.len() + 1),
                    };
                    seen.extend(files.into_iter().take(k));
                }
            }
            self.sync_seeing(d, &seen);
        } else {
            self.sync_all(d);
        }
    }

    /// Device `d` writes its ops and merges every other device's.
    fn sync_all(&mut self, d: usize) {
        self.push_device(d);
        let met = self.met(d);
        let report = merge_folder(&self.devices[d], &self.folder).unwrap();
        self.merged(d, &report, &met);
    }

    /// Device `d` writes its ops and merges only the op files `seen` (device id, number), as
    /// a folder its cloud has not brought the rest of shows them (every snapshot is there).
    fn sync_seeing(&mut self, d: usize, seen: &[(String, u64)]) {
        self.push_device(d);
        let met = self.met(d);
        let files: Vec<(&str, u64)> = seen.iter().map(|(id, n)| (id.as_str(), *n)).collect();
        let report = merge_folder(&self.devices[d], &view(&self.folder.folder, &files)).unwrap();
        self.merged(d, &report, &met);
    }

    /// Device `d` merged (`report`), having merged `met` of each device before.
    fn merged(
        &mut self,
        d: usize,
        report: &super::merge::MergeReport,
        met: &BTreeMap<String, Option<u64>>,
    ) {
        assert!(report.stopped.is_empty(), "{report:?}");
        self.stats.add(report);
        self.stats.snapshots_merged += report.snapshots.len();
        self.stats.snapshots_after_trim += report
            .snapshots
            .iter()
            .filter(|(id, _)| met.get(id).is_some_and(Option::is_some))
            .count();
        // What the merge recorded is not this device's change to undo.
        self.devices[d].take_applied_changes();
    }

    /// Device `e`'s op files in the folder, in order.
    fn files(&self, e: usize) -> Vec<(String, u64)> {
        let id = self.devices[e].device_id().unwrap();
        files_of(&self.folder.folder, &id)
            .into_iter()
            .map(|n| (id.clone(), n))
            .collect()
    }

    /// Device `d` writes its pending ops to the folder.
    fn push(&mut self, d: usize) {
        self.push_device(d);
    }

    /// Device `d`'s last change: what it applied since the last call.
    fn done(&mut self, d: usize) {
        let changes = self.devices[d].take_applied_changes();
        if !changes.is_empty() {
            self.last[d] = changes;
        }
    }

    /// Sync every device until nothing more is written or merged.
    fn settle(&mut self) {
        for _ in 0..30 {
            let mut busy = false;
            for d in 0..self.devices.len() {
                self.now += 1;
                self.as_device(d);
                busy |= !self.push_device(d).is_empty();
                let met = self.met(d);
                let report = merge_folder(&self.devices[d], &self.folder).unwrap();
                assert!(report.waiting.is_empty(), "{report:?}");
                busy |= !report.merged.is_empty() || !report.snapshots.is_empty();
                self.merged(d, &report, &met);
            }
            if !busy {
                return;
            }
        }
        panic!("the devices did not settle");
    }

    /// One random edit on device `d`, or an undo of its last.
    fn edit(&mut self, d: usize, rng: &mut Rng) {
        if rng.below(9) == 0 {
            self.undo(d);
            return;
        }
        self.change(d, rng);
        let changes = self.devices[d].take_applied_changes();
        if !changes.is_empty() {
            self.last[d] = changes;
        }
    }

    /// Undo device `d`'s last change, as the app's undo does (`inverse`): a row it deleted
    /// comes back with its old id. Skipped when a row has changed since (`Conflict`) or the
    /// undo is refused (a Highlight style gone), when it would delete a parent that holds rows
    /// here or bring back a row whose parent is gone, delete a list preference's whole value,
    /// or give a Layout a name another Layout has here now.
    fn undo(&mut self, d: usize) {
        let changes = std::mem::take(&mut self.last[d]);
        let user = &self.devices[d];
        let undo = inverse(&changes);
        let here = |key: RowKey| user.row_state(&key).unwrap().row.is_some();
        for change in &undo {
            match (&change.before, &change.after) {
                (Some(Row::Notebook(nb)), None)
                    if !user.annotations(Some(&nb.id)).unwrap().is_empty() =>
                {
                    return;
                }
                (Some(Row::HighlightPalette(p)), None)
                    if user
                        .highlight_styles()
                        .unwrap()
                        .iter()
                        .any(|s| s.palette_id == p.id) =>
                {
                    return;
                }
                (Some(Row::Preference(_)), None) => return,
                (_, Some(Row::Annotation(a))) if !here(RowKey::Notebook(a.notebook_id.clone())) => {
                    // Unless this undo brings the Notebook back too.
                    let back = undo.iter().any(|c| {
                        matches!(&c.after, Some(Row::Notebook(n)) if n.id == a.notebook_id)
                    });
                    if !back {
                        return;
                    }
                }
                (_, Some(Row::HighlightStyle(s)))
                    if !here(RowKey::HighlightPalette(s.palette_id.clone()))
                        && !undo.iter().any(|c| {
                            matches!(&c.after, Some(Row::HighlightPalette(p)) if p.id == s.palette_id)
                        }) =>
                {
                    return;
                }
                (_, Some(Row::Layout(layout))) => {
                    let here = user.row_state(&Row::Layout(layout.clone()).key()).unwrap();
                    let own = matches!(&here.row, Some(Row::Layout(l))
                        if l.name.eq_ignore_ascii_case(&layout.name));
                    let taken = user
                        .layouts()
                        .unwrap()
                        .iter()
                        .any(|n| n.eq_ignore_ascii_case(&layout.name));
                    if taken && !own {
                        return;
                    }
                }
                _ => {}
            }
        }
        match user.apply_changes(&undo) {
            Ok(()) => {
                self.stats.undone += 1;
                // Undoing the undo is a redo.
                self.last[d] = user.take_applied_changes();
            }
            Err(StoreError::Conflict(_) | StoreError::Rejected(_)) => {}
            Err(e) => panic!("undo on device {d}: {e}"),
        }
    }

    /// Note that device `d` moves its Annotation `a` out of its Notebook or deletes it: when
    /// another device has deleted that Notebook and this one has not met the delete, and the
    /// Notebook comes back, that is D3's second case (the child's maker left it).
    fn child_leaves(&mut self, d: usize, a: &Annotation) {
        let name = row_name(&RowKey::Notebook(a.notebook_id.clone()));
        let made_here = self.devices[d]
            .row_state(&RowKey::Annotation(a.id.clone()))
            .unwrap()
            .clock
            .is_some_and(|c| c.device_id == self.devices[d].device_id().unwrap());
        if made_here && self.deleted_parents.contains(&name) {
            self.child_gone.insert(a.notebook_id.clone());
        }
    }

    /// Delete Notebook `nb` on device `d` with its Annotations, or moving them to the
    /// default (`move_them`), as the app does (`Session::delete_notebook`).
    fn delete_notebook(&mut self, d: usize, nb: &Notebook, move_them: bool) {
        let user = &self.devices[d];
        let default = user.notebooks().unwrap().remove(0);
        let mut changes: Vec<Change> = user
            .annotations(Some(&nb.id))
            .unwrap()
            .into_iter()
            .map(|a| {
                if move_them {
                    let moved = Annotation {
                        notebook_id: default.id.clone(),
                        ..a.clone()
                    };
                    Change::update(Row::Annotation(a), Row::Annotation(moved))
                } else {
                    Change::delete(Row::Annotation(a))
                }
            })
            .collect();
        changes.push(Change::delete(Row::Notebook(nb.clone())));
        user.apply_changes(&changes).unwrap();
        self.deleted_parents
            .insert(row_name(&RowKey::Notebook(nb.id.clone())));
    }

    /// Note `note`'s text edited on device `d`, to a new text.
    fn edit_text(&mut self, d: usize, note: &Annotation, what: &str) {
        let body = self.text(what);
        let after = Annotation {
            content: Content::Note { body },
            modified_at: note.modified_at + 1,
            ..note.clone()
        };
        self.devices[d]
            .apply_changes(&[Change::update(
                Row::Annotation(note.clone()),
                Row::Annotation(after),
            )])
            .unwrap();
        self.done(d);
    }

    /// The S28b-2 review's Q1 sequence (a Sync conflict's copy made for a delete): D edits
    /// Note N's text, G edits it later, neither syncing; G pushes; X syncs, deletes N (or N's
    /// Notebook with its Annotations) and pushes; D syncs seeing only X's files (or only G's),
    /// then everything. The roles are random devices; three are needed.
    fn q1_step(&mut self, rng: &mut Rng) {
        if self.devices.len() < 3 {
            return;
        }
        let d = rng.below(3);
        let g = (d + 1 + rng.below(2)) % 3;
        let x = 3 - d - g;
        let has = |e: usize, id: &str| self.devices[e].annotation(id).unwrap().is_some();
        let notes: Vec<Annotation> = self.devices[d]
            .annotations(None)
            .unwrap()
            .into_iter()
            .filter(|a| a.is_note() && has(g, &a.id) && has(x, &a.id))
            .collect();
        let Some(note) = rng.pick(&notes).cloned() else {
            return;
        };
        self.as_device(d);
        self.edit_text(d, &note, "q1 by D");
        self.now += 1 + rng.ms(50);
        self.as_device(g);
        let theirs = self.devices[g].annotation(&note.id).unwrap().unwrap();
        self.edit_text(g, &theirs, "q1 by G");
        self.push(g);
        self.now += 1;
        self.as_device(x);
        self.sync_all(x);
        if let Some(a) = self.devices[x].annotation(&note.id).unwrap() {
            let nb = self.devices[x].notebook(&a.notebook_id).unwrap();
            match nb {
                Some(nb) if self.deletable.contains(&nb.id) && rng.below(2) == 0 => {
                    self.delete_notebook(x, &nb, false);
                }
                _ => {
                    self.child_leaves(x, &a);
                    self.devices[x]
                        .apply_changes(&[Change::delete(Row::Annotation(a))])
                        .unwrap();
                }
            }
            self.done(x);
        }
        self.push(x);
        self.now += 1;
        self.as_device(d);
        let first = if rng.below(4) == 0 { g } else { x };
        let seen = self.files(first);
        self.sync_seeing(d, &seen);
        self.sync_all(d);
        self.stats.q1_steps += 1;
    }

    /// D3's second case (a parent restored for a child its maker then moved or deleted): X
    /// deletes a Notebook both it and M have, with its Annotations, and pushes; M, not having
    /// met the delete, adds a Note to it, then moves the Note to its default Notebook or
    /// deletes it, and syncs. The Notebook comes back on every device (the others restored it
    /// for the Note), so M must take it back too.
    fn d3_step(&mut self, rng: &mut Rng) {
        let n = self.devices.len();
        let x = rng.below(n);
        let m = (x + 1 + rng.below(n - 1)) % n;
        let shared: Vec<Notebook> = self.devices[x]
            .notebooks()
            .unwrap()
            .into_iter()
            .filter(|nb| {
                self.deletable.contains(&nb.id)
                    && self.devices[m].notebook(&nb.id).unwrap().is_some()
            })
            .collect();
        let Some(nb) = rng.pick(&shared).cloned() else {
            return;
        };
        self.as_device(x);
        self.delete_notebook(x, &nb, false);
        self.done(x);
        self.push(x);
        self.now += 1;
        self.as_device(m);
        let body = self.text("d3 by M");
        let a = Annotation {
            id: new_id(),
            notebook_id: nb.id.clone(),
            content: Content::Note { body },
            anchor: None,
            created_at: 7,
            modified_at: 7,
            conflict_of: None,
        };
        self.devices[m]
            .apply_changes(&[Change::insert(Row::Annotation(a.clone()))])
            .unwrap();
        self.done(m);
        self.push(m);
        self.child_leaves(m, &a);
        let default = self.devices[m].notebooks().unwrap().remove(0);
        let leave = if rng.below(2) == 0 {
            Change::update(
                Row::Annotation(a.clone()),
                Row::Annotation(Annotation {
                    notebook_id: default.id,
                    modified_at: 8,
                    ..a
                }),
            )
        } else {
            Change::delete(Row::Annotation(a))
        };
        self.devices[m].apply_changes(&[leave]).unwrap();
        self.done(m);
        self.sync_all(m);
        self.stats.d3_steps += 1;
    }

    /// The S28b-2 review's D7 sequence (held items put back on a merged base in shape): P
    /// writes the Collections damaged, then in shape again (its base p1); Q, its clock run
    /// ahead of P's, adds an item at q1 after p1 without syncing; R syncs seeing Q's files
    /// first, then P's. Three devices are needed.
    fn d7_step(&mut self, rng: &mut Rng) {
        if self.devices.len() < 3 {
            return;
        }
        let p = rng.below(3);
        let q = (p + 1 + rng.below(2)) % 3;
        let r = 3 - p - q;
        let in_shape = |e: usize| {
            self.devices[e]
                .preference(COLLECTIONS)
                .unwrap()
                .is_none_or(|v| collection_items(&v).is_some())
        };
        if !in_shape(p) || !in_shape(q) {
            return;
        }
        self.as_device(p);
        let user = &self.devices[p];
        let mended = user.preference(COLLECTIONS).unwrap();
        let damaged = rng.pick(DAMAGED).unwrap();
        user.set_preference(COLLECTIONS, damaged).unwrap();
        self.stats.damaged += 1;
        self.now += 1;
        self.as_device(p);
        let mut items: Vec<serde_json::Value> = mended
            .as_deref()
            .map(|v| serde_json::from_str(v).unwrap())
            .unwrap_or_default();
        let n = self.n;
        self.n += 1;
        items.push(serde_json::json!({ "name": format!("Mended {n}"), "n": n }));
        let user = &self.devices[p];
        user.set_preference(COLLECTIONS, &serde_json::to_string(&items).unwrap())
            .unwrap();
        self.stats.fixed += 1;
        self.done(p);
        self.push(p);
        // Q's clock runs ahead of P's, so its item is later than P's base.
        let ahead = self.offsets[p].max(self.offsets[q]) + 1 + rng.ms(1_000);
        fix(Some(self.now + ahead));
        let user = &self.devices[q];
        let value = user.preference(COLLECTIONS).unwrap();
        let mut items: Vec<serde_json::Value> = value
            .as_deref()
            .map(|v| serde_json::from_str(v).unwrap())
            .unwrap_or_default();
        let name = rng.pick(NAMES).unwrap();
        if items.iter().any(|it| it["name"] == *name) {
            return;
        }
        let n = self.n;
        self.n += 1;
        items.push(serde_json::json!({ "name": name, "n": n }));
        user.set_preference(COLLECTIONS, &serde_json::to_string(&items).unwrap())
            .unwrap();
        self.done(q);
        self.push(q);
        self.now += 1;
        self.as_device(r);
        let mut seen = self.files(q);
        self.sync_seeing(r, &seen);
        seen.extend(self.files(p));
        self.sync_seeing(r, &seen);
        self.stats.d7_steps += 1;
    }

    /// One random change on device `d`, planned from what it holds.
    fn change(&mut self, d: usize, rng: &mut Rng) {
        // A device whose Collections are damaged often writes them in shape again.
        let damaged = self.devices[d]
            .preference(COLLECTIONS)
            .unwrap()
            .is_some_and(|v| collection_items(&v).is_none());
        if damaged && rng.below(2) == 0 {
            self.collections(d, rng);
            return;
        }
        let user = &self.devices[d];
        match rng.below(19) {
            0 => {
                let nb = Notebook {
                    id: new_id(),
                    name: (*rng.pick(NAMES).unwrap()).into(),
                    is_default: false,
                    show_in_text: true,
                    created_at: 5,
                    modified_at: 5,
                };
                self.deletable.insert(nb.id.clone());
                user.apply_changes(&[Change::insert(Row::Notebook(nb))])
                    .unwrap();
            }
            1 => {
                let notebooks = user.notebooks().unwrap();
                if let Some(nb) = rng.pick(&notebooks) {
                    let after = if rng.below(2) == 0 {
                        Notebook {
                            name: (*rng.pick(NAMES).unwrap()).into(),
                            ..nb.clone()
                        }
                    } else {
                        Notebook {
                            show_in_text: !nb.show_in_text,
                            ..nb.clone()
                        }
                    };
                    if after != *nb {
                        user.apply_changes(&[Change::update(
                            Row::Notebook(nb.clone()),
                            Row::Notebook(after),
                        )])
                        .unwrap();
                    }
                }
            }
            // Delete a Notebook with its Annotations, or moving them to the default, as the
            // app does (`Session::delete_notebook`).
            2 => {
                let notebooks: Vec<Notebook> = user
                    .notebooks()
                    .unwrap()
                    .into_iter()
                    .filter(|n| !n.is_default && self.deletable.contains(&n.id))
                    .collect();
                let Some(nb) = rng.pick(&notebooks).cloned() else {
                    return;
                };
                self.delete_notebook(d, &nb, rng.below(2) == 0);
            }
            3 | 4 => {
                let holders = self.holders(d);
                let styles = self.style_ids(d);
                let Some(nb) = rng.pick(&holders).cloned() else {
                    return;
                };
                let body = self.text("note");
                let user = &self.devices[d];
                let a = if rng.below(2) == 0 {
                    Annotation {
                        id: new_id(),
                        notebook_id: nb,
                        content: Content::Note { body },
                        anchor: None,
                        created_at: 7,
                        modified_at: 7,
                        conflict_of: None,
                    }
                } else {
                    let verse = Verse::new(Book::from_index(rng.below(66)).unwrap(), 1, 1);
                    Annotation {
                        id: new_id(),
                        notebook_id: nb,
                        content: Content::Highlight {
                            style: rng.pick(&styles).unwrap().clone(),
                        },
                        anchor: Some(Anchor::Passage {
                            passage: Passage::new(verse, verse).unwrap(),
                            origin: None,
                        }),
                        created_at: 7,
                        modified_at: 7,
                        conflict_of: None,
                    }
                };
                user.apply_changes(&[Change::insert(Row::Annotation(a))])
                    .unwrap();
            }
            // Edit an Annotation: most often a Note's text (on several devices at once, D2),
            // else move it to another Notebook or restyle a Highlight.
            5..=7 => {
                let holders = self.holders(d);
                let styles = self.style_ids(d);
                let text = self.text("edited");
                let all = self.devices[d].annotations(None).unwrap();
                let Some(a) = rng.pick(&all).cloned() else {
                    return;
                };
                let mut after = a.clone();
                match (&a.content, rng.below(4)) {
                    (_, 0) => after.notebook_id = rng.pick(&holders).unwrap().clone(),
                    (Content::Note { .. }, _) => after.content = Content::Note { body: text },
                    (Content::Highlight { .. }, _) => {
                        after.content = Content::Highlight {
                            style: rng.pick(&styles).unwrap().clone(),
                        }
                    }
                }
                if after.notebook_id != a.notebook_id {
                    self.child_leaves(d, &a);
                }
                after.modified_at += 1;
                self.devices[d]
                    .apply_changes(&[Change::update(
                        Row::Annotation(a.clone()),
                        Row::Annotation(after),
                    )])
                    .unwrap();
            }
            8 => {
                let all = user.annotations(None).unwrap();
                if let Some(a) = rng.pick(&all).cloned() {
                    self.child_leaves(d, &a);
                    self.devices[d]
                        .apply_changes(&[Change::delete(Row::Annotation(a))])
                        .unwrap();
                }
            }
            9 => {
                let label = self.text("favorite");
                let user = &self.devices[d];
                let favorites = user.favorites().unwrap();
                match (rng.below(3), rng.pick(&favorites)) {
                    (0, Some(f)) => {
                        user.remove_favorite(f.id).unwrap();
                    }
                    (1, Some(f)) => {
                        user.move_favorite(f.id, rng.pick(NAMES).unwrap()).unwrap();
                    }
                    _ => {
                        user.add_favorite("", &label, "{}").unwrap();
                    }
                }
            }
            10 | 11 => {
                let snapshot = format!(r#"{{"s":"{}"}}"#, self.text("layout"));
                let user = &self.devices[d];
                let names = user.layouts().unwrap();
                match (rng.below(3), rng.pick(&names)) {
                    (0, Some(name)) => {
                        user.delete_layout(name).unwrap();
                    }
                    (1, Some(name)) => {
                        let to = rng.pick(NAMES).unwrap();
                        if !names.iter().any(|n| n.eq_ignore_ascii_case(to)) {
                            user.rename_layout(name, to).unwrap();
                        }
                    }
                    _ => user
                        .save_layout(rng.pick(NAMES).unwrap(), &snapshot)
                        .unwrap(),
                }
            }
            12 => {
                let mut ids: Vec<String> = RESOURCES
                    .iter()
                    .filter(|_| rng.below(2) == 0)
                    .map(|s| s.to_string())
                    .collect();
                if ids.len() > 1 {
                    let i = rng.below(ids.len());
                    ids.swap(0, i);
                }
                user.save_preferred_order(&ids).unwrap();
            }
            13 => {
                let plans = user.reading_plans().unwrap();
                match (rng.pick(&plans), rng.below(5)) {
                    // Delete a plan with its Readings.
                    (Some(plan), 0) => {
                        user.remove_reading_plan(plan.id).unwrap();
                        let changes = user.take_applied_changes();
                        for c in &changes {
                            if let Some(Row::ReadingPlan(p)) = &c.before {
                                self.deleted_parents
                                    .insert(row_name(&RowKey::ReadingPlan(p.uid.clone())));
                            }
                        }
                        self.last[d] = changes;
                    }
                    (Some(plan), _) => {
                        let position = u32::try_from(rng.below(3)).unwrap();
                        user.set_reading_read(plan.id, position, rng.below(2) == 0)
                            .unwrap();
                    }
                    (None, _) => {
                        let plan = ReadingPlanRow {
                            id: 0,
                            name: (*rng.pick(NAMES).unwrap()).into(),
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
                        user.add_reading_plan(&plan, &readings).unwrap();
                    }
                }
            }
            14 => {
                let value = if rng.below(2) == 0 { "true" } else { "false" };
                user.set_preference("search.word_forms", value).unwrap();
            }
            15 | 16 => self.collections(d, rng),
            _ => self.styles(d, rng),
        }
    }

    /// Change the Collections on device `d`: an item changed, added (under a name another
    /// device may add too, D6), removed or moved; or the whole value written damaged, or in
    /// shape again over a damaged one (D7). The Resource tags are saved with them, as
    /// `Session::save_collections` does (`"{}"` on a device that has none), sometimes with a
    /// Resource tagged or untagged.
    fn collections(&mut self, d: usize, rng: &mut Rng) {
        let n = self.n;
        self.n += 1;
        let user = &self.devices[d];
        let value = user.preference(COLLECTIONS).unwrap();
        let in_shape = value
            .as_deref()
            .is_none_or(|v| collection_items(v).is_some());
        if value.is_some() && in_shape && rng.below(4) == 0 {
            let damaged = rng.pick(DAMAGED).unwrap();
            user.set_preference(COLLECTIONS, damaged).unwrap();
            self.stats.damaged += 1;
            return;
        }
        let mut items: Vec<serde_json::Value> = value
            .as_deref()
            .and_then(|v| serde_json::from_str(v).ok())
            .unwrap_or_default();
        // A damaged value written in shape again: one item of each name.
        let mut names = BTreeSet::new();
        items.retain(|it| names.insert(it["name"].to_string()));
        let i = rng.below(items.len().max(1));
        match rng.below(4) {
            0 if !items.is_empty() => {
                items.remove(i);
            }
            1 if !items.is_empty() => items[i]["n"] = n.into(),
            // Move item i to j: the whole list written in its new order.
            2 if items.len() > 1 => {
                let item = items.remove(i);
                let j = rng.below(items.len() + 1);
                items.insert(j, item);
                self.stats.moved += 1;
            }
            _ => {
                // A name another device may add too, with other JSON (D6).
                let name = rng.pick(NAMES).unwrap();
                if items.iter().all(|it| it["name"] != *name) {
                    items.push(serde_json::json!({ "name": name, "n": n }));
                }
            }
        }
        let new = serde_json::to_string(&items).unwrap();
        if value.as_deref() != Some(new.as_str()) {
            user.set_preference(COLLECTIONS, &new).unwrap();
            if !in_shape {
                self.stats.fixed += 1;
            }
        }
        let mut tags: BTreeMap<String, Vec<String>> = user
            .preference(TAGS)
            .unwrap()
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        if rng.below(3) == 0 {
            let resource = (*rng.pick(RESOURCES).unwrap()).to_string();
            if tags.remove(&resource).is_none() {
                tags.insert(resource, vec![format!("tag {n}")]);
            }
        }
        user.set_preference(TAGS, &serde_json::to_string(&tags).unwrap())
            .unwrap();
    }

    /// Change device `d`'s Highlight styles and palettes: make a palette or a style, or delete
    /// one the generator made, moving the Highlights on it to [`KEPT_STYLE`] (as
    /// `Session::delete_highlight_style` and `delete_highlight_palette` do).
    fn styles(&mut self, d: usize, rng: &mut Rng) {
        let user = &self.devices[d];
        let palettes = user.highlight_palettes().unwrap();
        let styles = user.highlight_styles().unwrap();
        let move_off = |gone: &[String]| -> Vec<Change> {
            user.annotations(None)
                .unwrap()
                .into_iter()
                .filter(
                    |a| matches!(&a.content, Content::Highlight { style } if gone.contains(style)),
                )
                .map(|a| {
                    let moved = Annotation {
                        content: Content::Highlight {
                            style: KEPT_STYLE.into(),
                        },
                        ..a.clone()
                    };
                    Change::update(Row::Annotation(a), Row::Annotation(moved))
                })
                .collect()
        };
        match rng.below(4) {
            0 => {
                let palette = HighlightPalette {
                    id: new_id(),
                    name: (*rng.pick(NAMES).unwrap()).into(),
                    position: 1 + i64::try_from(rng.below(5)).unwrap(),
                    created_at: 9,
                    modified_at: 9,
                };
                self.made_palettes.insert(palette.id.clone());
                user.apply_changes(&[Change::insert(Row::HighlightPalette(palette))])
                    .unwrap();
            }
            1 => {
                let Some(palette) = rng.pick(&palettes) else {
                    return;
                };
                let style = HighlightStyle {
                    id: new_id(),
                    palette_id: palette.id.clone(),
                    position: i64::try_from(rng.below(9)).unwrap(),
                    name: (*rng.pick(NAMES).unwrap()).into(),
                    appearance: StyleAppearance::new(HighlightLook::Underline, Rgb::new(9, 9, 9)),
                    created_at: 9,
                    modified_at: 9,
                };
                self.made_styles.insert(style.id.clone());
                user.apply_changes(&[Change::insert(Row::HighlightStyle(style))])
                    .unwrap();
            }
            2 => {
                let made: Vec<&HighlightStyle> = styles
                    .iter()
                    .filter(|s| self.made_styles.contains(&s.id))
                    .collect();
                let Some(style) = rng.pick(&made).map(|s| (*s).clone()) else {
                    return;
                };
                let mut changes = move_off(std::slice::from_ref(&style.id));
                changes.push(Change::delete(Row::HighlightStyle(style.clone())));
                user.apply_changes(&changes).unwrap();
                self.deleted_parents
                    .insert(row_name(&RowKey::HighlightStyle(style.id)));
            }
            _ => {
                let made: Vec<&HighlightPalette> = palettes
                    .iter()
                    .filter(|p| self.made_palettes.contains(&p.id))
                    .collect();
                let Some(palette) = rng.pick(&made).map(|p| (*p).clone()) else {
                    return;
                };
                let theirs: Vec<HighlightStyle> = styles
                    .iter()
                    .filter(|s| s.palette_id == palette.id)
                    .cloned()
                    .collect();
                let ids: Vec<String> = theirs.iter().map(|s| s.id.clone()).collect();
                let mut changes = move_off(&ids);
                changes.extend(
                    theirs
                        .into_iter()
                        .map(|s| Change::delete(Row::HighlightStyle(s))),
                );
                changes.push(Change::delete(Row::HighlightPalette(palette.clone())));
                user.apply_changes(&changes).unwrap();
                self.deleted_parents
                    .insert(row_name(&RowKey::HighlightPalette(palette.id)));
            }
        }
    }

    /// The Highlight styles here.
    fn style_ids(&self, d: usize) -> Vec<String> {
        self.devices[d]
            .highlight_styles()
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect()
    }

    /// The Notebooks here an Annotation may go in.
    fn holders(&self, d: usize) -> Vec<String> {
        self.devices[d]
            .notebooks()
            .unwrap()
            .into_iter()
            .map(|n| n.id)
            .collect()
    }
}

/// Every op every device wrote, with the row it is about and whether it is a snapshot's
/// (`true`): each op file's ops, then each
/// snapshot's rows, tombstones and held ops as the ops they merge as (`snapshot_ops`), from
/// `archive` (every op file and snapshot ever written; `reader` opens the snapshots).
fn every_op(archive: &BTreeMap<String, Vec<u8>>, reader: &UserStore) -> Vec<(RowKey, Op, bool)> {
    let mut ops = Vec::new();
    for (path, bytes) in archive {
        if path.contains("/ops/") {
            let OpFileRead::Whole(file) = decode_op_file(bytes) else {
                panic!("{path} is not whole");
            };
            ops.extend(
                file.into_iter()
                    .map(|op| (op.change().key().unwrap(), op, false)),
            );
        }
    }
    for (path, bytes) in archive {
        if path.contains("/snapshots/") {
            let (snapshot, _) = reader.open_snapshot(bytes).unwrap();
            let contents = snapshot.snapshot_contents().unwrap();
            ops.extend(
                snapshot_ops(&contents)
                    .into_iter()
                    .map(|s| (s.key, s.op, true)),
            );
        }
    }
    ops
}

/// The row `row` as compared with another of its kind: whether a Notebook is the default is
/// settled on each device (two default Notebooks), and a list item's position is only its
/// order in the stored value.
fn comparable(row: &Row) -> Row {
    let mut row = row.clone();
    match &mut row {
        Row::Notebook(nb) => nb.is_default = false,
        Row::Collection(item) => item.position = 0,
        _ => {}
    }
    row
}

/// The devices' template: a new User database, made under a fixed clock with seeded ids so
/// every run starts from the same one; and the name (`kind/id`) of every row it holds.
fn template(dir: &Dir) -> (UserStore, BTreeSet<String>) {
    fix(Some(TEMPLATE_MS));
    // The first snapshot read in a process makes its reference schema once (taking an id),
    // before any seeded id is taken, so every run of a seed takes the same ids.
    let (warm, _) = UserStore::open(&dir.0.join("warm.db")).unwrap();
    warm.open_snapshot(&warm.snapshot_bytes().unwrap()).unwrap();
    seed_ids(Some(0));
    let (template, _) = UserStore::open(&dir.0.join("template.db")).unwrap();
    let names = template
        .synced_keys()
        .unwrap()
        .iter()
        .map(row_name)
        .collect();
    (template, names)
}

/// A row's name in the invariants: its tombstone's table and id.
fn row_name(key: &RowKey) -> String {
    let (kind, id) = key.tombstone();
    format!("{kind}/{id}")
}

/// Whether `value` of the Collections is a list this app reads one item at a time: a JSON
/// list of objects with a name each, no name twice (the generator writes them with
/// `serde_json`, as the engine does). Its items by name, if it is.
fn collection_items(value: &str) -> Option<BTreeMap<String, String>> {
    let items: Vec<serde_json::Value> = serde_json::from_str(value).ok()?;
    let mut by_name = BTreeMap::new();
    for item in items {
        let name = item.get("name")?.as_str()?.to_string();
        if by_name
            .insert(name, serde_json::to_string(&item).ok()?)
            .is_some()
        {
            return None;
        }
    }
    Some(by_name)
}

/// A list item's JSON without its `name` (D6 re-adds an item under another name).
fn without_name(json: &str) -> String {
    let mut item: serde_json::Value = serde_json::from_str(json).unwrap();
    if let Some(fields) = item.as_object_mut() {
        fields.remove("name");
    }
    item.to_string()
}

/// What a Collection is expected to be once settled, when its list has a base (D7): the
/// row's JSON and clock, or `None` for not there.
fn expected_item(
    name: &str,
    newest: Option<&(RowKey, Hlc, Row)>,
    tombstone: Option<&Tombstone>,
    base: &Hlc,
    base_items: &BTreeMap<String, String>,
) -> Option<(String, Hlc)> {
    let covers = tombstone.and_then(|t| t.covers.clone());
    if let Some((_, clock, Row::Collection(item))) = newest
        && clock > base
    {
        return (Some(clock) > covers.as_ref()).then(|| (item.json.clone(), clock.clone()));
    }
    let removed =
        tombstone.is_some_and(|t| t.clock.as_ref() > Some(base) && covers.as_ref() >= Some(base));
    match base_items.get(name) {
        Some(json) if !removed => Some((json.clone(), base.clone())),
        _ => None,
    }
}

/// One case: its devices edit and sync at random, then settle; then the invariants. Returns
/// the Synced data every device holds (`synced_dump`).
fn check_case(
    template: &UserStore,
    template_rows: &BTreeSet<String>,
    seed: u64,
    stats: &mut Stats,
) -> String {
    let mut rng = Rng(seed);
    seed_ids(Some(rng.next()));
    let n = 2 + rng.below(2);
    let offsets: Vec<i64> = (0..n).map(|_| rng.offset()).collect();
    let now = TEMPLATE_MS + 3_600_000 + rng.ms(1_000_000);
    let mut defaults = Vec::new();
    let devices = (0..n)
        .map(|d| {
            fix(Some(now + offsets[d]));
            let user = device(template, &mut rng);
            let default = user.notebooks().unwrap().remove(0);
            defaults.push((default.created_at, default.id));
            user
        })
        .collect();
    // Snapshots every few own op files; own op files trimmed once a few seconds old.
    let options = SyncOptions {
        platform: "test".into(),
        policy: SnapshotPolicy {
            after_files: 1 + u64::try_from(rng.below(3)).unwrap(),
            after_ms: 1_000 + rng.ms(20_000),
            trim_after_ms: 1_000 + rng.ms(30_000),
        },
    };
    let mut world = World {
        devices,
        folder: ArchivingFolder {
            folder: MemSyncFolder::new(),
            archive: Default::default(),
        },
        deletable: BTreeSet::new(),
        made_styles: BTreeSet::new(),
        made_palettes: BTreeSet::new(),
        deleted_parents: BTreeSet::new(),
        child_gone: BTreeSet::new(),
        n: 0,
        now,
        offsets,
        last: vec![Vec::new(); n],
        joins: vec![None; n],
        joined: vec![false; n],
        backups: (0..n).map(|_| None).collect(),
        discarded_texts: BTreeSet::new(),
        discarded_items: BTreeSet::new(),
        options,
        stats: std::mem::take(stats),
    };
    // Most devices join at once; the others join late, at their first sync, with what they
    // made before (only their snapshot brings it to the others).
    for d in 0..n {
        if d == 0 || rng.below(3) != 0 {
            world.as_device(d);
            world.join(d);
        }
    }
    for _ in 0..(10 + rng.below(40)) {
        let d = rng.below(n);
        world.now += rng.step();
        world.as_device(d);
        match rng.below(60) {
            0 | 1 => world.q1_step(&mut rng),
            2 | 3 => world.d7_step(&mut rng),
            7 => world.d3_step(&mut rng),
            5 | 6 => world.backup_or_restore(d, &mut rng),
            n if n % 4 == 0 => world.sync(d, &mut rng),
            _ => world.edit(d, &mut rng),
        }
    }
    world.settle();
    let user = &world.devices[0];
    world.stats.renamed_layouts += user
        .layouts()
        .unwrap()
        .iter()
        .filter(|n| n.ends_with(" (2)") || n.ends_with(" (3)"))
        .count();
    world.stats.demoted_defaults += user
        .notebooks()
        .unwrap()
        .iter()
        .filter(|n| !n.is_default && !world.deletable.contains(&n.id))
        .count();
    world.stats.maker_left += world
        .child_gone
        .iter()
        .filter(|id| user.notebook(id).unwrap().is_some())
        .count();
    *stats = std::mem::take(&mut world.stats);

    let ops = every_op(&world.folder.archive.lock().unwrap(), &world.devices[0]);
    if let Ok(filter) = std::env::var("OBS_MERGE_TRACE") {
        for (_, op, _) in &ops {
            let line = serde_json::to_string(&op).unwrap();
            if line.contains(&filter) {
                eprintln!("{line}");
            }
        }
    }
    // Convergence.
    let dumps: Vec<String> = world
        .devices
        .iter()
        .map(|u| u.synced_dump().unwrap())
        .collect();
    for (i, dump) in dumps.iter().enumerate().skip(1) {
        if dump != &dumps[0] {
            let (a, b): (BTreeSet<&str>, BTreeSet<&str>) =
                (dumps[0].lines().collect(), dump.lines().collect());
            panic!(
                "device {i} differs from device 0:\n only on 0: {:#?}\n only on {i}: {:#?}",
                a.difference(&b).collect::<Vec<_>>(),
                b.difference(&a).collect::<Vec<_>>()
            );
        }
    }

    // The op stream: each row's newest write, for each row deleted the newest delete clock
    // and the newest version a delete covered, the Collections' base (their newest whole
    // value), and every Note text written, and seen by a device that replaced or deleted it.
    let mut newest: BTreeMap<String, (RowKey, Hlc, Row)> = BTreeMap::new();
    let mut deleted: BTreeMap<String, (RowKey, Tombstone)> = BTreeMap::new();
    let mut base: Option<(Hlc, String)> = None;
    let mut written_texts = BTreeSet::new();
    let mut seen_texts = BTreeSet::new();
    let mut annotation_ids = BTreeSet::new();
    // Each Note's texts as ops wrote them, with their clocks; each Sync conflict's copy's
    // first write (its clock and the row).
    let mut note_writes: BTreeMap<String, Vec<(String, Hlc)>> = BTreeMap::new();
    let mut first_copies: BTreeMap<String, (Hlc, Annotation)> = BTreeMap::new();
    // Each Collection's JSON (without its name) an add op wrote, with its clock, and those an
    // op that had them replaced or deleted (D6).
    let mut added_items: Vec<(String, Hlc)> = Vec::new();
    let mut replaced_items = BTreeSet::new();
    let collections = RowKey::Preference(COLLECTIONS.into());
    // The Collections of every whole value written (their items had no op of their own).
    let mut whole_items = BTreeSet::new();
    // Rows a device made and deleted before it joined: only its snapshot's tombstone has them.
    let mut deleted_before_joining = BTreeSet::new();
    let mut deleted_in_files = BTreeSet::new();
    for (key, op, from_snapshot) in ops {
        let name = row_name(&key);
        if let Some(Row::Annotation(a)) = &op.after {
            annotation_ids.insert(a.id.clone());
            if a.is_note() {
                written_texts.insert(a.body().to_string());
                note_writes
                    .entry(a.id.clone())
                    .or_default()
                    .push((a.body().to_string(), op.clock.clone()));
            }
            if a.conflict_of.is_some()
                && first_copies
                    .get(&a.id)
                    .is_none_or(|(clock, _)| op.clock < *clock)
            {
                first_copies.insert(a.id.clone(), (op.clock.clone(), a.clone()));
            }
        }
        // A text is seen by an op that deleted its Note or wrote other text over it (not by
        // one that only moved it).
        if let Some(Row::Annotation(a)) = &op.before
            && a.is_note()
        {
            let replaced = match &op.after {
                Some(Row::Annotation(after)) => after.body() != a.body(),
                _ => true,
            };
            if replaced {
                seen_texts.insert(a.body().to_string());
            }
        }
        if let (None, Some(Row::Collection(item))) = (&op.before, &op.after) {
            added_items.push((without_name(&item.json), op.clock.clone()));
        }
        if let Some(Row::Collection(before)) = &op.before {
            let replaced = match &op.after {
                Some(Row::Collection(after)) => {
                    without_name(&after.json) != without_name(&before.json)
                }
                _ => true,
            };
            if replaced {
                replaced_items.insert(without_name(&before.json));
            }
        }
        if key == collections {
            let Some(Row::Preference(p)) = &op.after else {
                panic!("the whole Collections deleted: not generated");
            };
            if base.as_ref().is_none_or(|(c, _)| *c < op.clock) {
                base = Some((op.clock.clone(), p.value.clone()));
            }
            for item in collection_items(&p.value).unwrap_or_default().keys() {
                whole_items.insert(row_name(&RowKey::Collection(item.clone())));
            }
            continue;
        }
        match &op.after {
            Some(after) => {
                if newest.get(&name).is_none_or(|(_, c, _)| *c < op.clock) {
                    newest.insert(name, (key, op.clock.clone(), after.clone()));
                }
            }
            None => {
                if from_snapshot {
                    deleted_before_joining.insert(name.clone());
                } else {
                    deleted_in_files.insert(name.clone());
                }
                let (_, t) = deleted.entry(name).or_insert((
                    key,
                    Tombstone {
                        clock: None,
                        covers: None,
                    },
                ));
                t.clock = t.clock.clone().max(Some(op.clock.clone()));
                t.covers = t.covers.clone().max(op.before_clock.clone());
            }
        }
    }
    let base_items: Option<(Hlc, Option<BTreeMap<String, String>>)> = base
        .as_ref()
        .map(|(clock, value)| (clock.clone(), collection_items(value)));
    // A row deleted is one an op or snapshot wrote, or a Collection of a whole value, or one
    // a device made and deleted before it joined (only its snapshot's tombstone has it).
    for name in deleted.keys() {
        let before_joining =
            deleted_before_joining.contains(name) && !deleted_in_files.contains(name);
        assert!(
            newest.contains_key(name) || whole_items.contains(name) || before_joining,
            "{name} deleted, never written"
        );
    }
    // Each tombstone is the op stream's.
    for (name, (key, tombstone)) in &deleted {
        for (i, user) in world.devices.iter().enumerate() {
            let state = user.row_state(key).unwrap();
            assert_eq!(
                state.tombstone.as_ref(),
                Some(tombstone),
                "{name} on device {i}"
            );
        }
    }
    // Each row is there exactly when its newest write is later than what its deletes cover,
    // holding that write; a Collection, when its list has a base, as the base and the item
    // ops after it say.
    let mut item_names: BTreeSet<String> = newest
        .iter()
        .filter(|(_, (k, _, _))| matches!(k, RowKey::Collection(_)))
        .map(|(_, (k, _, _))| match k {
            RowKey::Collection(n) => n.clone(),
            _ => unreachable!(),
        })
        .collect();
    if let Some((_, Some(items))) = &base_items {
        item_names.extend(items.keys().cloned());
    }
    for (name, (key, clock, after)) in &newest {
        if matches!(key, RowKey::Collection(_)) && base_items.is_some() {
            continue;
        }
        let tombstone = deleted.get(name).map(|(_, t)| t);
        let is_covered = tombstone.is_some_and(|t| t.covers.as_ref() >= Some(clock));
        for (i, user) in world.devices.iter().enumerate() {
            let state = user.row_state(key).unwrap();
            if is_covered {
                assert_eq!(state.row, None, "{name} on device {i}: a delete covers it");
            } else {
                let row = state
                    .row
                    .unwrap_or_else(|| panic!("{name} disappeared on device {i}"));
                assert_eq!(comparable(&row), comparable(after), "{name} on device {i}");
                assert_eq!(state.clock.as_ref(), Some(clock), "{name} on device {i}");
            }
        }
    }
    if let Some((base_clock, items)) = &base_items {
        for (i, user) in world.devices.iter().enumerate() {
            let value = user.preference(COLLECTIONS).unwrap().unwrap();
            let Some(items) = items else {
                assert_eq!(
                    Some(&value),
                    base.as_ref().map(|(_, v)| v),
                    "device {i}: damaged base"
                );
                for held in user.sync_held().unwrap() {
                    assert!(held.clock > *base_clock, "device {i}: {held:?} superseded");
                }
                continue;
            };
            assert!(
                user.sync_held().unwrap().is_empty(),
                "device {i}: held with the base in shape"
            );
            for item in &item_names {
                let key = RowKey::Collection(item.clone());
                let name = row_name(&key);
                let expected = expected_item(
                    item,
                    newest.get(&name),
                    deleted.get(&name).map(|(_, t)| t),
                    base_clock,
                    items,
                );
                let state = user.row_state(&key).unwrap();
                let got = state.row.map(|r| match r {
                    Row::Collection(c) => (c.json, state.clock.clone().unwrap()),
                    other => panic!("{other:?}"),
                });
                assert_eq!(got, expected, "{name} on device {i} (base {base_clock})");
            }
        }
    } else {
        for (i, user) in world.devices.iter().enumerate() {
            assert!(
                user.sync_held().unwrap().is_empty(),
                "device {i}: held, no base"
            );
        }
    }
    // Every Note text an op wrote is a live Note's, unless a device that had it replaced or
    // deleted it; a Sync conflict's copy names a Note an op wrote.
    let live = world.devices[0].annotations(None).unwrap();
    let live_texts: BTreeSet<String> = live
        .iter()
        .filter(|a| a.is_note())
        .map(|a| a.body().to_string())
        .collect();
    for text in &written_texts {
        assert!(
            live_texts.contains(text)
                || seen_texts.contains(text)
                || world.discarded_texts.contains(text),
            "the Note text “{text}” was lost"
        );
    }
    for copy in live.iter().filter(|a| a.conflict_of.is_some()) {
        let of = copy.conflict_of.as_ref().unwrap();
        assert!(annotation_ids.contains(of), "{copy:?} is a copy of no Note");
        assert!(first_copies.contains_key(&copy.id), "{copy:?} no op wrote");
    }
    // Each copy, as first written, holds a text an op wrote to its Note, and its id is the
    // one that text's clock gives (`conflict_id`).
    for (id, (_, copy)) in &first_copies {
        let of = copy.conflict_of.as_ref().unwrap();
        let from = note_writes
            .get(of)
            .into_iter()
            .flatten()
            .any(|(text, clock)| {
                text == copy.body() && super::merge::conflict_id(of, clock) == *id
            });
        assert!(
            from,
            "{copy:?} holds no text an op wrote to {of} at the clock of its id"
        );
    }
    // Every Collection an add op wrote is live under some name (D6: a device whose own item
    // lost re-adds it as "Name (2)"), unless an op that had it replaced or deleted it, or the
    // Collections' base supersedes it (written at or after it, or damaged).
    let live_items: BTreeSet<String> = world.devices[0]
        .preference(COLLECTIONS)
        .unwrap()
        .and_then(|v| collection_items(&v))
        .unwrap_or_default()
        .values()
        .map(|json| without_name(json))
        .collect();
    for (json, clock) in &added_items {
        let superseded = base_items
            .as_ref()
            .is_some_and(|(base, items)| items.is_none() || clock <= base);
        assert!(
            live_items.contains(json)
                || replaced_items.contains(json)
                || superseded
                || world.discarded_items.contains(json),
            "the Collection {json} added at {clock} was lost"
        );
    }
    // No row is there that no op wrote and the devices did not begin with (or, a
    // Collection, that its base holds); one default Notebook, the oldest the devices began
    // with.
    let oldest = defaults.iter().min().map(|(_, id)| id.clone());
    for (i, user) in world.devices.iter().enumerate() {
        for key in user.synced_keys().unwrap() {
            let name = row_name(&key);
            let in_base = match (&key, &base_items) {
                (RowKey::Collection(n), Some((_, Some(items)))) => items.contains_key(n),
                (k, Some(_)) => *k == collections,
                _ => false,
            };
            assert!(
                newest.contains_key(&name) || template_rows.contains(&name) || in_base,
                "{name} on device {i}, which no op wrote"
            );
        }
        let defaults: Vec<String> = user
            .notebooks()
            .unwrap()
            .into_iter()
            .filter(|nb| nb.is_default)
            .map(|nb| nb.id)
            .collect();
        assert_eq!(
            Some(defaults),
            oldest.clone().map(|id| vec![id]),
            "device {i}"
        );
    }
    dumps.into_iter().next().unwrap()
}

/// The first case seed: `OBS_MERGE_SEED`, or a fixed one.
fn base_seed() -> u64 {
    std::env::var("OBS_MERGE_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5EED_0022)
}

fn run(cases: u64, name: &str) {
    let dir = Dir::new(name);
    let (template, template_rows) = template(&dir);
    let base = base_seed();
    let mut stats = Stats::default();
    for i in 0..cases {
        let seed = base.wrapping_add(i);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            check_case(&template, &template_rows, seed, &mut stats)
        }));
        if let Err(e) = result {
            fix(None);
            seed_ids(None);
            let why = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            panic!("case {i} failed: OBS_MERGE_SEED={seed} reruns it\n{why}");
        }
    }
    fix(None);
    seed_ids(None);
    eprintln!(
        "{cases} cases: ops merged {:?}; files retried after waiting {}; Layouts renamed for a \
         clash {}; default Notebooks demoted {}; Collection items moved {}; undos {}; clocks \
         more than a day ahead not observed {}; recorded {:?}; Collections written damaged {}, \
         in shape again {}; parents back after their child's maker left them {}; the review's \
         Q1 sequence {}, D7 sequence {}, D3 sequence {}; late joins {}, snapshots merged {} ({} for files \
         trimmed), own op files trimmed {}, restores catching up {}, replacing {}",
        stats.outcomes,
        stats.retried,
        stats.renamed_layouts,
        stats.demoted_defaults,
        stats.moved,
        stats.undone,
        stats.not_observed,
        stats.recorded,
        stats.damaged,
        stats.fixed,
        stats.maker_left,
        stats.q1_steps,
        stats.d7_steps,
        stats.d3_steps,
        stats.late_joins,
        stats.snapshots_merged,
        stats.snapshots_after_trim,
        stats.trimmed,
        stats.catch_ups,
        stats.replaces,
    );
    for outcome in [
        Outcome::Written,
        Outcome::Superseded,
        Outcome::Covered,
        Outcome::EditKept,
        Outcome::DeleteNoted,
        Outcome::ParentKept,
        Outcome::Held,
    ] {
        assert!(
            stats.outcomes.get(&outcome).copied().unwrap_or(0) > 0,
            "no case reached {outcome:?}"
        );
    }
    let r = stats.recorded;
    for (what, n) in [
        ("a file retried after waiting", stats.retried),
        ("a Layout renamed for a clash", stats.renamed_layouts),
        ("a default Notebook demoted", stats.demoted_defaults),
        ("a Collection item moved", stats.moved),
        ("an undo", stats.undone),
        ("a clock more than a day ahead", stats.not_observed),
        ("a Sync conflict's copy", r.conflicts),
        ("a deleted parent restored for a child", r.parents_restored),
        (
            "a row deleted with its parent restored",
            r.children_restored,
        ),
        ("a list item renamed for a clash", r.items_renamed),
        ("a held op applied", r.unheld),
        ("a Collections value written damaged", stats.damaged),
        ("a Collections value in shape again", stats.fixed),
        (
            "a parent back after its child's maker left it",
            stats.maker_left,
        ),
        ("the review's Q1 sequence", stats.q1_steps),
        ("the review's D7 sequence", stats.d7_steps),
        (
            "D3's sequence of a child's maker leaving it",
            stats.d3_steps,
        ),
        ("a device joining late", stats.late_joins),
        ("a snapshot merged", stats.snapshots_merged),
        (
            "a snapshot merged for op files trimmed",
            stats.snapshots_after_trim,
        ),
        ("an own op file trimmed", stats.trimmed),
        ("a restore that catches up", stats.catch_ups),
        ("a restore that replaces", stats.replaces),
    ] {
        assert!(n > 0, "no case reached {what}");
    }
}

#[test]
fn random_edits_on_several_devices_converge_and_lose_no_row() {
    run(CI_CASES, "props");
}

#[test]
#[ignore = "10k cases, about 25 s: cargo test -p obs-engine sync -- --include-ignored"]
fn random_edits_on_several_devices_converge_and_lose_no_row_10k_cases() {
    run(FULL_CASES, "props-10k");
}

#[test]
fn a_seed_makes_the_same_case_on_every_run() {
    let base = base_seed();
    for i in 0..20 {
        let seed = base.wrapping_add(i);
        let dumps: Vec<String> = (0..2)
            .map(|run| {
                let dir = Dir::new(&format!("props-seed-{i}-{run}"));
                let (template, template_rows) = template(&dir);
                check_case(&template, &template_rows, seed, &mut Stats::default())
            })
            .collect();
        assert_eq!(dumps[0], dumps[1], "OBS_MERGE_SEED={seed}");
    }
    fix(None);
    seed_ids(None);
}
