//! Property tests of the merge (ADR 0022 rule 3, plan S28b-1): two or three Synced devices
//! over one in-memory Sync folder make random edits of Notebooks, Notes and Highlights,
//! Favorites, Layouts (names clash), the Preferred order, Reading done-marks, a synced
//! preference and Collections (items changed, added, removed and moved), deleting rows too,
//! undo their own last change (a row deleted comes back with its old id), and sync at random
//! moments. Each device's wall clock is its own: the case's clock moves on by random steps,
//! and a device's runs from it by an offset of its own, sometimes more than
//! [`MAX_FORWARD_SKEW_MS`] ahead (its clocks are then not observed). Once every device has
//! synced until nothing more moves:
//!
//! - **convergence:** every device holds the same Synced data, clocks and tombstones;
//! - **no user-made row disappears** (ADR 0006), and edit beats delete: a row is there exactly
//!   when its newest write is later than everything a delete of it covered, holding that
//!   write, on every device; no row is there that no op wrote (or the devices began with);
//! - each tombstone holds the newest delete clock and the newest version covered of the op
//!   stream; each device has one default Notebook, the oldest (`created_at`, id) of those the
//!   devices began with.
//!
//! The generator is a seeded SplitMix64 (no property-test crate); under the case's fixed clock
//! (`obs_store::clock::fix`) and seeded ids (`obs_store::annotation::seed_ids`) a seed makes
//! the same case on every run. The CI run takes [`CI_CASES`] cases;
//! `cargo test -p obs-engine sync -- --include-ignored` runs [`FULL_CASES`] too. A failure
//! names its seed; `OBS_MERGE_SEED=<seed>` reruns that case first.
//!
//! Not generated, for plan S28b-2: deleting a Notebook or Reading plan that may hold rows
//! (the parent rule, D3; an undo that would is skipped), Note-text Sync conflicts (D2: here
//! the later edit wins), two list items of one name (D6), and a list preference's whole value
//! (D7).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use obs_scripture::{Book, Passage, Verse};
use obs_store::annotation::{inverse, new_id, seed_ids};
use obs_store::clock::fix;
use obs_store::hlc::MAX_FORWARD_SKEW_MS;
use obs_store::{
    Anchor, Annotation, Change, Content, Hlc, Notebook, PlanReadingRow, ReadingPlanRow, Row,
    RowKey, StoreError, Tombstone, UserStore,
};

use super::folder::MemSyncFolder;
use super::merge::{Outcome, merge_folder};
use super::merge_tests::Dir;
use super::ops::{Op, read_ops_after, write_pending_ops};

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
const STYLES: &[&str] = &["yellow", "green", "blue", "pink", "red_underline"];

/// One case's devices and the folder they share.
struct World {
    devices: Vec<UserStore>,
    folder: MemSyncFolder,
    /// Notebooks the generator may delete: never given an Annotation (deleting a parent with
    /// rows is plan S28b-2's).
    deletable: BTreeSet<String>,
    /// For unique texts.
    n: u64,
    /// The case's wall clock (Unix ms); each device's runs from it by its offset.
    now: i64,
    /// How far each device's wall clock runs from `now`.
    offsets: Vec<i64>,
    /// Each device's last change (the Changes its edit applied), for its undo.
    last: Vec<Vec<Change>>,
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
}

impl Stats {
    fn add(&mut self, report: &super::merge::MergeReport) {
        for (outcome, n) in &report.outcomes {
            *self.outcomes.entry(*outcome).or_default() += n;
        }
        self.retried += report.retried;
        self.not_observed += report.not_observed;
    }
}

/// A device copied from `template` with an id and default Notebook of its own, recording
/// ops, its default Notebook in its ops (as joining with a snapshot would put it, plan S28c).
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
    user.start_recording_ops().unwrap();
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

    fn sync(&mut self, d: usize) {
        write_pending_ops(&self.devices[d], &self.folder).unwrap();
        let report = merge_folder(&self.devices[d], &self.folder).unwrap();
        assert!(report.stopped.is_empty(), "{report:?}");
        self.stats.add(&report);
        // What the merge recorded is not this device's change to undo.
        self.devices[d].take_applied_changes();
    }

    /// Sync every device until nothing more is written or merged.
    fn settle(&mut self) {
        for _ in 0..30 {
            let mut busy = false;
            for d in 0..self.devices.len() {
                self.now += 1;
                self.as_device(d);
                let user = &self.devices[d];
                busy |= !write_pending_ops(user, &self.folder).unwrap().is_empty();
                let report = merge_folder(user, &self.folder).unwrap();
                assert!(report.waiting.is_empty(), "{report:?}");
                busy |= !report.merged.is_empty();
                self.stats.add(&report);
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
    /// comes back with its old id. Skipped when a row has changed since (`Conflict`), when it
    /// would delete a Notebook or Reading plan that may hold rows (plan S28b-2's, D3), or when
    /// it would give a Layout a name another Layout has here now.
    fn undo(&mut self, d: usize) {
        let changes = std::mem::take(&mut self.last[d]);
        let user = &self.devices[d];
        let undo = inverse(&changes);
        for change in &undo {
            match (&change.before, &change.after) {
                (Some(Row::ReadingPlan(_)), None) => return,
                (Some(Row::Notebook(nb)), None) if !self.deletable.contains(&nb.id) => return,
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
            Err(StoreError::Conflict(_)) => {}
            Err(e) => panic!("undo on device {d}: {e}"),
        }
    }

    /// One random change on device `d`, planned from what it holds.
    fn change(&mut self, d: usize, rng: &mut Rng) {
        let user = &self.devices[d];
        match rng.below(16) {
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
            2 => {
                let notebooks: Vec<Notebook> = user
                    .notebooks()
                    .unwrap()
                    .into_iter()
                    .filter(|n| self.deletable.contains(&n.id))
                    .collect();
                if let Some(nb) = rng.pick(&notebooks) {
                    user.apply_changes(&[Change::delete(Row::Notebook(nb.clone()))])
                        .unwrap();
                }
            }
            3 | 4 => {
                let holders = self.holders(d);
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
                    }
                } else {
                    let verse = Verse::new(Book::from_index(rng.below(66)).unwrap(), 1, 1);
                    Annotation {
                        id: new_id(),
                        notebook_id: nb,
                        content: Content::Highlight {
                            style: (*rng.pick(STYLES).unwrap()).into(),
                        },
                        anchor: Some(Anchor::Passage {
                            passage: Passage::new(verse, verse).unwrap(),
                            origin: None,
                        }),
                        created_at: 7,
                        modified_at: 7,
                    }
                };
                user.apply_changes(&[Change::insert(Row::Annotation(a))])
                    .unwrap();
            }
            5 | 6 => {
                let holders = self.holders(d);
                let text = self.text("edited");
                let user = &self.devices[d];
                let all = user.annotations(None).unwrap();
                let Some(a) = rng.pick(&all) else {
                    return;
                };
                let mut after = a.clone();
                match (&a.content, rng.below(2)) {
                    (_, 0) => after.notebook_id = rng.pick(&holders).unwrap().clone(),
                    (Content::Note { .. }, _) => after.content = Content::Note { body: text },
                    (Content::Highlight { .. }, _) => {
                        after.content = Content::Highlight {
                            style: (*rng.pick(STYLES).unwrap()).into(),
                        }
                    }
                }
                after.modified_at += 1;
                user.apply_changes(&[Change::update(
                    Row::Annotation(a.clone()),
                    Row::Annotation(after),
                )])
                .unwrap();
            }
            7 => {
                let all = user.annotations(None).unwrap();
                if let Some(a) = rng.pick(&all) {
                    user.apply_changes(&[Change::delete(Row::Annotation(a.clone()))])
                        .unwrap();
                }
            }
            8 => {
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
            9 | 10 => {
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
            11 => {
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
            12 => {
                let plans = user.reading_plans().unwrap();
                match rng.pick(&plans) {
                    Some(plan) if rng.below(4) != 0 => {
                        let position = u32::try_from(rng.below(3)).unwrap();
                        user.set_reading_read(plan.id, position, rng.below(2) == 0)
                            .unwrap();
                    }
                    _ => {
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
            13 => {
                let value = if rng.below(2) == 0 { "true" } else { "false" };
                user.set_preference("search.word_forms", value).unwrap();
            }
            _ => {
                let n = self.n;
                self.n += 1;
                let user = &self.devices[d];
                let mut items: Vec<serde_json::Value> = user
                    .preference("library.collections")
                    .unwrap()
                    .map(|v| serde_json::from_str(&v).unwrap())
                    .unwrap_or_default();
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
                        let name = rng.pick(NAMES).unwrap();
                        // A name another device also adds is plan S28b-2's (D6): this one's
                        // own names are its device's.
                        let name = format!("{name} {d}");
                        if items.iter().all(|it| it["name"] != name.as_str()) {
                            items.push(serde_json::json!({ "name": name, "n": n }));
                        }
                    }
                }
                user.set_preference(
                    "library.collections",
                    &serde_json::to_string(&items).unwrap(),
                )
                .unwrap();
            }
        }
    }

    /// The Notebooks here an Annotation may go in: those never deleted (the defaults).
    fn holders(&self, d: usize) -> Vec<String> {
        self.devices[d]
            .notebooks()
            .unwrap()
            .into_iter()
            .filter(|n| !self.deletable.contains(&n.id))
            .map(|n| n.id)
            .collect()
    }
}

/// Every op every device wrote.
fn every_op(folder: &MemSyncFolder, devices: &[UserStore]) -> Vec<Op> {
    let mut ops = Vec::new();
    for user in devices {
        let read = read_ops_after(folder, &user.device_id().unwrap(), 0).unwrap();
        assert!(read.stopped.is_none());
        for (_, file) in read.files {
            ops.extend(file);
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
    let mut world = World {
        devices,
        folder: MemSyncFolder::new(),
        deletable: BTreeSet::new(),
        n: 0,
        now,
        offsets,
        last: vec![Vec::new(); n],
        stats: std::mem::take(stats),
    };
    for _ in 0..(10 + rng.below(30)) {
        let d = rng.below(n);
        world.now += rng.step();
        world.as_device(d);
        if rng.below(4) == 0 {
            world.sync(d);
        } else {
            world.edit(d, &mut rng);
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
    *stats = std::mem::take(&mut world.stats);

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

    // The op stream: each row's newest write, and for each row deleted the newest delete
    // clock and the newest version a delete covered.
    let mut newest: BTreeMap<String, (RowKey, Hlc, Row)> = BTreeMap::new();
    let mut deleted: BTreeMap<String, Tombstone> = BTreeMap::new();
    for op in every_op(&world.folder, &world.devices) {
        let key = op.change().key().unwrap();
        let name = row_name(&key);
        match &op.after {
            Some(after) => {
                if newest.get(&name).is_none_or(|(_, c, _)| *c < op.clock) {
                    newest.insert(name, (key, op.clock.clone(), after.clone()));
                }
            }
            None => {
                let t = deleted.entry(name).or_insert(Tombstone {
                    clock: None,
                    covers: None,
                });
                t.clock = t.clock.clone().max(Some(op.clock.clone()));
                t.covers = t.covers.clone().max(op.before_clock.clone());
            }
        }
    }
    // Deletes of rows never written in an op (made before the devices recorded) are left
    // out: none are generated.
    for name in deleted.keys() {
        assert!(newest.contains_key(name), "{name} deleted, never written");
    }
    // Each row is there exactly when its newest write is later than what its deletes cover,
    // holding that write; its tombstone is the op stream's.
    for (name, (key, clock, after)) in &newest {
        let tombstone = deleted.get(name);
        let is_covered = tombstone.is_some_and(|t| t.covers.as_ref() >= Some(clock));
        for (i, user) in world.devices.iter().enumerate() {
            let state = user.row_state(key).unwrap();
            assert_eq!(state.tombstone.as_ref(), tombstone, "{name} on device {i}");
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
    // No row is there that no op wrote and the devices did not begin with; one default
    // Notebook, the oldest the devices began with.
    let oldest = defaults.iter().min().map(|(_, id)| id.clone());
    for (i, user) in world.devices.iter().enumerate() {
        for key in user.synced_keys().unwrap() {
            let name = row_name(&key);
            assert!(
                newest.contains_key(&name) || template_rows.contains(&name),
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
         more than a day ahead not observed {}",
        stats.outcomes,
        stats.retried,
        stats.renamed_layouts,
        stats.demoted_defaults,
        stats.moved,
        stats.undone,
        stats.not_observed
    );
    for outcome in [
        Outcome::Written,
        Outcome::Superseded,
        Outcome::Covered,
        Outcome::EditKept,
        Outcome::DeleteNoted,
    ] {
        assert!(
            stats.outcomes.get(&outcome).copied().unwrap_or(0) > 0,
            "no case reached {outcome:?}"
        );
    }
    for (what, n) in [
        ("a file retried after waiting", stats.retried),
        ("a Layout renamed for a clash", stats.renamed_layouts),
        ("a default Notebook demoted", stats.demoted_defaults),
        ("a Collection item moved", stats.moved),
        ("an undo", stats.undone),
        ("a clock more than a day ahead", stats.not_observed),
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
