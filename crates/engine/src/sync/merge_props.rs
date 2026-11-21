//! Property tests of the merge (ADR 0022 rule 3, plan S28b-1): two or three Synced devices
//! over one in-memory Sync folder make random edits of Notebooks, Notes and Highlights,
//! Favorites, Layouts (names clash), the Preferred order, Reading done-marks, a synced
//! preference and Collections, deleting rows too, and sync at random moments. Once every
//! device has synced until nothing more moves:
//!
//! - **convergence:** every device holds the same Synced data, clocks and tombstones;
//! - **no user-made row disappears** (ADR 0006), and edit beats delete: a row is there exactly
//!   when its newest write is later than everything a delete of it covered, holding that
//!   write, on every device.
//!
//! The generator is a seeded SplitMix64 (no property-test crate). The CI run takes
//! [`CI_CASES`] cases; `cargo test -p obs-engine sync -- --include-ignored` runs
//! [`FULL_CASES`] too. A failure names its seed; `OBS_MERGE_SEED=<seed>` reruns that case first.
//!
//! Not generated, for plan S28b-2: deleting a Notebook or Reading plan that may hold rows
//! (the parent rule, D3), Note-text Sync conflicts (D2: here the later edit wins), list items
//! moved or two of one name (D6), and a list preference's whole value (D7).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, BTreeSet};

use obs_scripture::{Book, Passage, Verse};
use obs_store::annotation::new_id;
use obs_store::{
    Anchor, Annotation, Change, Content, Hlc, Notebook, PlanReadingRow, ReadingPlanRow, Row,
    RowKey, UserStore,
};

use super::folder::MemSyncFolder;
use super::merge::{Outcome, merge_folder};
use super::merge_tests::Dir;
use super::ops::{Op, read_ops_after, write_pending_ops};

/// Cases each CI run checks.
const CI_CASES: u64 = 1_000;
/// Cases the ignored long run checks.
const FULL_CASES: u64 = 10_000;

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
    /// What the merges did, over every case of a run.
    stats: Stats,
}

/// What the merges of a run did, to show the cases reach every rule.
#[derive(Default)]
struct Stats {
    outcomes: BTreeMap<super::merge::Outcome, usize>,
    retried: usize,
    renamed_layouts: usize,
    demoted_defaults: usize,
}

impl Stats {
    fn add(&mut self, report: &super::merge::MergeReport) {
        for (outcome, n) in &report.outcomes {
            *self.outcomes.entry(*outcome).or_default() += n;
        }
        self.retried += report.retried;
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
    user
}

impl World {
    fn text(&mut self, what: &str) -> String {
        self.n += 1;
        format!("{what} {}", self.n)
    }

    fn sync(&mut self, d: usize) {
        write_pending_ops(&self.devices[d], &self.folder).unwrap();
        let report = merge_folder(&self.devices[d], &self.folder).unwrap();
        assert!(report.stopped.is_empty(), "{report:?}");
        self.stats.add(&report);
    }

    /// Sync every device until nothing more is written or merged.
    fn settle(&mut self) {
        for _ in 0..30 {
            let mut busy = false;
            for user in &self.devices {
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

    /// One random edit on device `d`, planned from what it holds.
    fn edit(&mut self, d: usize, rng: &mut Rng) {
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
                match rng.below(3) {
                    0 if !items.is_empty() => {
                        items.remove(i);
                    }
                    1 if !items.is_empty() => items[i]["n"] = n.into(),
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

/// One case: its devices edit and sync at random, then settle; then the invariants.
fn check_case(template: &UserStore, seed: u64, stats: &mut Stats) {
    let mut rng = Rng(seed);
    let n = 2 + rng.below(2);
    let devices = (0..n).map(|_| device(template, &mut rng)).collect();
    let mut world = World {
        devices,
        folder: MemSyncFolder::new(),
        deletable: BTreeSet::new(),
        n: 0,
        stats: std::mem::take(stats),
    };
    for _ in 0..(10 + rng.below(30)) {
        let d = rng.below(n);
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

    // Each row is there exactly when its newest write is later than what its deletes cover,
    // holding that write.
    let mut newest: BTreeMap<String, (RowKey, Hlc, Row)> = BTreeMap::new();
    let mut covered: BTreeMap<String, Option<Hlc>> = BTreeMap::new();
    for op in every_op(&world.folder, &world.devices) {
        let key = op.change().key().unwrap();
        let (kind, id) = key.tombstone();
        let name = format!("{kind}/{id}");
        match &op.after {
            Some(after) => {
                if newest.get(&name).is_none_or(|(_, c, _)| *c < op.clock) {
                    newest.insert(name, (key, op.clock.clone(), after.clone()));
                }
            }
            None => {
                let c = covered.entry(name).or_insert(None);
                *c = (*c).clone().max(op.before_clock.clone());
            }
        }
    }
    for (name, (key, clock, after)) in &newest {
        let is_covered = covered
            .get(name)
            .is_some_and(|covers| covers.as_ref() >= Some(clock));
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
    // Deletes of rows never written in an op (made before the devices recorded) are left
    // out: none are generated.
    for name in covered.keys() {
        assert!(newest.contains_key(name), "{name} deleted, never written");
    }
}

fn run(cases: u64, name: &str) {
    let dir = Dir::new(name);
    let (template, _) = UserStore::open(&dir.0.join("template.db")).unwrap();
    let base: u64 = std::env::var("OBS_MERGE_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5EED_0022);
    let mut stats = Stats::default();
    for i in 0..cases {
        let seed = base.wrapping_add(i);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            check_case(&template, seed, &mut stats)
        }));
        if let Err(e) = result {
            let why = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            panic!("case {i} failed: OBS_MERGE_SEED={seed} reruns it\n{why}");
        }
    }
    eprintln!(
        "{cases} cases: ops merged {:?}; files retried after waiting {}; Layouts renamed for a \
         clash {}; default Notebooks demoted {}",
        stats.outcomes, stats.retried, stats.renamed_layouts, stats.demoted_defaults
    );
    for outcome in [
        Outcome::Written,
        Outcome::Superseded,
        Outcome::Covered,
        Outcome::EditKept,
    ] {
        assert!(
            stats.outcomes.get(&outcome).copied().unwrap_or(0) > 0,
            "no case reached {outcome:?}"
        );
    }
}

#[test]
fn random_edits_on_several_devices_converge_and_lose_no_row() {
    run(CI_CASES, "props");
}

#[test]
#[ignore = "10k cases, several minutes: cargo test -p obs-engine sync -- --include-ignored"]
fn random_edits_on_several_devices_converge_and_lose_no_row_10k_cases() {
    run(FULL_CASES, "props-10k");
}
