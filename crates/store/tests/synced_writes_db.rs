//! Every write of Synced data goes through `apply_changes` (ADR 0022, S27b): each of the
//! store's writes emits `Change`s, those Changes are the whole write (applying them inverted
//! puts every synced table back), a delete leaves a tombstone, and a list preference is
//! written item by item unless its value is not in shape, when it is kept whole.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeSet;
use std::path::PathBuf;

use obs_scripture::{Book, Passage, Verse};
use obs_store::annotation::{inverse, new_id};
use obs_store::{
    Anchor, Annotation, Change, Content, HighlightLook, HighlightPalette, HighlightStyle, Notebook,
    PlanReadingRow, ReadingPlanRow, Rgb, Row, RowKey, STANDARD_PALETTE_ID, SYNCED_TABLES,
    StoreError, StyleAppearance, SyncedPreference, UserStore,
};
use rusqlite::Connection;
use rusqlite::types::Value;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("obs-synced-{name}-{}", std::process::id()));
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
    }
}

/// The `row` tag of a Row. The match has no wildcard, so a new variant fails to compile
/// here until [`EVERY_ROW`] and a case of the table below name it.
fn kind(row: &Row) -> &'static str {
    match row {
        Row::Notebook(_) => "notebook",
        Row::Annotation(_) => "annotation",
        Row::HighlightStyle(_) => "highlight_style",
        Row::HighlightPalette(_) => "highlight_palette",
        Row::Favorite(_) => "favorite",
        Row::ReadingPlan(_) => "reading_plan",
        Row::Reading(_) => "reading",
        Row::Layout(_) => "layout",
        Row::PreferredOrder(_) => "preferred_order",
        Row::Preference(_) => "preference",
        Row::Collection(_) => "collection",
        Row::CopyPreset(_) => "copy_preset",
        Row::SavedSearch(_) => "saved_search",
        Row::GuideSection(_) => "guide_section",
        Row::ResourceTags(_) => "resource_tags",
    }
}

const EVERY_ROW: &[&str] = &[
    "notebook",
    "annotation",
    "highlight_style",
    "highlight_palette",
    "favorite",
    "reading_plan",
    "reading",
    "layout",
    "preferred_order",
    "preference",
    "collection",
    "copy_preset",
    "saved_search",
    "guide_section",
    "resource_tags",
];

/// Every row of every synced table, and of `anchors` and `notes_fts`, as text, without the
/// clocks (an undo is a change of its own, with a later clock).
fn synced_dump(db: &std::path::Path) -> String {
    let c = Connection::open(db).unwrap();
    let mut out = String::new();
    for table in SYNCED_TABLES
        .iter()
        .copied()
        .chain(["anchors", "notes_fts"])
    {
        out.push_str(&format!("[{table}]\n"));
        let columns: Vec<String> = c
            .prepare("SELECT name FROM pragma_table_info(?1) WHERE name <> 'clock'")
            .unwrap()
            .query_map([table], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let columns = if columns.is_empty() {
            "*".to_string()
        } else {
            columns.join(", ")
        };
        let mut stmt = c
            .prepare(&format!("SELECT {columns} FROM {table}"))
            .unwrap();
        let n = stmt.column_count();
        let mut rows: Vec<String> = stmt
            .query_map([], |r| {
                (0..n)
                    .map(|i| r.get::<_, Value>(i).map(|v| format!("{v:?}")))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|v| v.join(" | "))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        rows.sort();
        for row in rows {
            out.push_str(&row);
            out.push('\n');
        }
    }
    out
}

fn tombstones(db: &std::path::Path) -> Vec<(String, String, String)> {
    let c = Connection::open(db).unwrap();
    let mut stmt = c
        .prepare("SELECT row_table, row_id, clock FROM tombstone ORDER BY row_table, row_id")
        .unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn plan() -> ReadingPlanRow {
    ReadingPlanRow {
        id: 0,
        name: "Gospels".into(),
        template: "custom".into(),
        bible_id: "kjv".into(),
        start_day: "2026-01-05".into(),
        weekdays: 0b001_1111,
        created_at: 0,
    }
}

fn readings() -> Vec<PlanReadingRow> {
    ["Matt 1", "Matt 2"]
        .iter()
        .enumerate()
        .map(|(i, p)| PlanReadingRow {
            position: i as u32,
            passages: (*p).into(),
            day: format!("2026-01-0{}", 5 + i),
            read_at: None,
        })
        .collect()
}

fn note(notebook_id: &str, body: &str) -> Annotation {
    Annotation {
        id: new_id(),
        notebook_id: notebook_id.into(),
        content: Content::Note { body: body.into() },
        anchor: Some(Anchor::Passage {
            passage: Passage::verse(Verse::new(Book::John, 3, 16)),
            origin: None,
        }),
        created_at: 20,
        modified_at: 20,
    }
}

fn first_plan(db: &UserStore) -> i64 {
    db.reading_plans().unwrap()[0].id
}

fn first_favorite(db: &UserStore) -> i64 {
    db.favorites().unwrap()[0].id
}

/// Fills a database with one of everything a case below edits.
fn fill(db: &UserStore) {
    db.save_layout("Study", r#"{"v":1}"#).unwrap();
    db.save_preferred_order(&["kjv".into(), "web".into()])
        .unwrap();
    db.add_favorite("", "Jn 3:16", "{}").unwrap();
    db.add_reading_plan(&plan(), &readings()).unwrap();
    db.set_preference("highlights.palette", "standard").unwrap();
    db.set_preference(
        "library.collections",
        r#"[{"name":"Greek","rules":[],"resources":["sblgnt"]}]"#,
    )
    .unwrap();
    db.set_preference("library.tags", r#"{"kjv":["older"]}"#)
        .unwrap();
    db.set_preference("copy.presets", r#"[{"name":"Plain","options":{}}]"#)
        .unwrap();
    db.set_preference("search.saved", r#"[{"name":"Love","query":"love"}]"#)
        .unwrap();
    db.set_preference(
        "guide-arrangement/passage_guide",
        r#"{"sections":[{"id":"commentaries","collapsed":false,"hidden":false},{"id":"cross_references","collapsed":false,"hidden":false}]}"#,
    )
    .unwrap();
}

type Write = fn(&UserStore);

/// Each write of Synced data the store makes, with the kinds of row its Changes are about.
const WRITES: &[(&str, Write, &[&str])] = &[
    (
        "make a Notebook",
        |db| {
            db.apply_changes(&[Change::insert(Row::Notebook(Notebook {
                id: new_id(),
                name: "Study".into(),
                is_default: false,
                show_in_text: true,
                created_at: 1,
                modified_at: 1,
            }))])
            .unwrap()
        },
        &["notebook"],
    ),
    (
        "write a Note",
        |db| {
            let nb = db.notebooks().unwrap().remove(0);
            db.apply_changes(&[Change::insert(Row::Annotation(note(
                &nb.id,
                "God so loved",
            )))])
            .unwrap()
        },
        &["annotation"],
    ),
    (
        "make a Highlight palette with a style",
        |db| {
            let palette = HighlightPalette {
                id: new_id(),
                name: "Study".into(),
                position: 1,
                created_at: 1,
                modified_at: 1,
            };
            let style = HighlightStyle {
                id: new_id(),
                palette_id: palette.id.clone(),
                position: 0,
                name: "Promises".into(),
                appearance: StyleAppearance::new(HighlightLook::Box, Rgb::new(1, 2, 3)),
                created_at: 1,
                modified_at: 1,
            };
            db.apply_changes(&[
                Change::insert(Row::HighlightPalette(palette)),
                Change::insert(Row::HighlightStyle(style)),
            ])
            .unwrap()
        },
        &["highlight_palette", "highlight_style"],
    ),
    (
        "add a Favorite",
        |db| {
            db.add_favorite("Gospels", "Jn 1:1", "{}").unwrap();
        },
        &["favorite"],
    ),
    (
        "move a Favorite",
        |db| {
            assert!(db.move_favorite(first_favorite(db), "Later").unwrap());
        },
        &["favorite"],
    ),
    (
        "remove a Favorite",
        |db| {
            assert!(db.remove_favorite(first_favorite(db)).unwrap());
        },
        &["favorite"],
    ),
    (
        "add a Reading plan",
        |db| {
            db.add_reading_plan(&plan(), &readings()).unwrap();
        },
        &["reading_plan", "reading"],
    ),
    (
        "mark a Reading read",
        |db| {
            assert!(db.set_reading_read(first_plan(db), 1, true).unwrap());
        },
        &["reading"],
    ),
    (
        "move Readings",
        |db| {
            db.move_readings(first_plan(db), &[(0, "2026-02-01".into())])
                .unwrap()
        },
        &["reading"],
    ),
    (
        "remove a Reading plan",
        |db| {
            assert!(db.remove_reading_plan(first_plan(db)).unwrap());
        },
        &["reading", "reading_plan"],
    ),
    (
        "save a new Layout",
        |db| db.save_layout("Morning", "{}").unwrap(),
        &["layout"],
    ),
    (
        "save over a Layout",
        |db| db.save_layout("STUDY", r#"{"v":2}"#).unwrap(),
        &["layout"],
    ),
    (
        "rename a Layout",
        |db| {
            assert!(db.rename_layout("Study", "Evening").unwrap());
        },
        &["layout"],
    ),
    (
        "delete a Layout",
        |db| {
            assert!(db.delete_layout("Study").unwrap());
        },
        &["layout"],
    ),
    (
        "replace the Preferred order",
        |db| {
            db.save_preferred_order(&["web".into(), "kjv".into(), "web".into()])
                .unwrap()
        },
        &["preferred_order"],
    ),
    (
        "choose the active Highlight palette",
        |db| db.set_preference("highlights.palette", "mine").unwrap(),
        &["preference"],
    ),
    (
        "turn on Word forms",
        |db| db.set_preference("search.word_forms", "true").unwrap(),
        &["preference"],
    ),
    (
        "add a Collection and edit another",
        |db| {
            db.set_preference(
                "library.collections",
                r#"[{"name":"Greek","rules":[],"resources":["sblgnt","lxx"]},{"name":"Old","rules":[],"resources":[]}]"#,
            )
            .unwrap()
        },
        &["collection"],
    ),
    (
        "tag a Resource",
        |db| {
            db.set_preference("library.tags", r#"{"kjv":["older"],"web":["modern"]}"#)
                .unwrap()
        },
        &["resource_tags"],
    ),
    (
        "delete a Copy preset",
        |db| db.set_preference("copy.presets", "[]").unwrap(),
        &["copy_preset"],
    ),
    (
        "save a search",
        |db| {
            db.set_preference(
                "search.saved",
                r#"[{"name":"Faith","query":"faith"},{"name":"Love","query":"love"}]"#,
            )
            .unwrap()
        },
        &["saved_search"],
    ),
    (
        "move a Guide section",
        |db| {
            db.set_preference(
                "guide-arrangement/passage_guide",
                r#"{"sections":[{"id":"cross_references","collapsed":false,"hidden":false},{"id":"commentaries","collapsed":true,"hidden":false}]}"#,
            )
            .unwrap()
        },
        &["guide_section"],
    ),
];

#[test]
fn every_write_of_synced_data_emits_changes_and_they_are_the_whole_write() {
    let mut covered = BTreeSet::new();
    for (i, (what, write, kinds)) in WRITES.iter().enumerate() {
        let dir = Dir::new(&format!("write-{i}"));
        let (db, _) = UserStore::open(&dir.db()).unwrap();
        fill(&db);
        db.take_applied_changes();
        let before = synced_dump(&dir.db());
        write(&db);
        let changes = db.take_applied_changes();
        assert!(!changes.is_empty(), "{what}: no Change");
        let mut got: Vec<&str> = changes
            .iter()
            .flat_map(|c| c.before.iter().chain(c.after.iter()).map(kind))
            .collect();
        got.sort_unstable();
        got.dedup();
        let mut want = kinds.to_vec();
        want.sort_unstable();
        assert_eq!(got, want, "{what}: kinds of row");
        covered.extend(got);
        assert_ne!(
            synced_dump(&dir.db()),
            before,
            "{what}: nothing was written"
        );
        // The Changes are all the write did: undone, every synced table is as it was.
        db.apply_changes(&inverse(&changes)).unwrap();
        assert_eq!(synced_dump(&dir.db()), before, "{what}: undone");
    }
    let every: BTreeSet<&str> = EVERY_ROW.iter().copied().collect();
    assert_eq!(covered, every, "every kind of Row has a write above");
}

/// The clock stored for the row `key` names: its row's `clock`, its item's `item_clock` or,
/// once deleted, its tombstone's.
fn stored_clock(c: &Connection, key: &RowKey, deleted: bool) -> Option<String> {
    let (kind, id) = key.tombstone();
    let one = |sql: &str, params: &[&dyn rusqlite::ToSql]| -> Option<String> {
        c.query_row(sql, params, |r| r.get::<_, Option<String>>(0))
            .ok()
            .flatten()
    };
    if deleted {
        return one(
            "SELECT clock FROM tombstone WHERE row_table = ?1 AND row_id = ?2",
            &[&kind, &id],
        );
    }
    let by = |table: &str, column: &str, id: &str| {
        one(
            &format!("SELECT clock FROM {table} WHERE {column} = ?1"),
            &[&id],
        )
    };
    match key {
        RowKey::Notebook(id) => by("notebooks", "id", id),
        RowKey::Annotation(id) => by("annotations", "id", id),
        RowKey::HighlightStyle(id) => by("highlight_styles", "id", id),
        RowKey::HighlightPalette(id) => by("highlight_palettes", "id", id),
        RowKey::Favorite(uid) => by("favorite", "uid", uid),
        RowKey::ReadingPlan(uid) => by("reading_plan", "uid", uid),
        RowKey::Layout(id) => by("layout", "id", id),
        RowKey::Preference(key) => by("preference", "key", key),
        RowKey::Reading(plan, position) => one(
            "SELECT r.clock FROM plan_reading r JOIN reading_plan p ON p.id = r.plan_id
             WHERE p.uid = ?1 AND r.position = ?2",
            &[plan, position],
        ),
        RowKey::PreferredOrder => {
            let clocks: BTreeSet<Option<String>> = c
                .prepare("SELECT clock FROM preferred_order")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert!(
                clocks.len() <= 1,
                "one clock for the whole order: {clocks:?}"
            );
            clocks.into_iter().next().flatten()
        }
        RowKey::Collection(_)
        | RowKey::CopyPreset(_)
        | RowKey::SavedSearch(_)
        | RowKey::GuideSection(..)
        | RowKey::ResourceTags(_) => one(
            "SELECT clock FROM item_clock WHERE row_table = ?1 AND row_id = ?2",
            &[&kind, &id],
        ),
    }
}

/// The preference a list item is kept in.
fn list_key(key: &RowKey) -> Option<String> {
    Some(match key {
        RowKey::Collection(_) => "library.collections".into(),
        RowKey::CopyPreset(_) => "copy.presets".into(),
        RowKey::SavedSearch(_) => "search.saved".into(),
        RowKey::ResourceTags(_) => "library.tags".into(),
        RowKey::GuideSection(guide, _) => format!("guide-arrangement/{guide}"),
        _ => return None,
    })
}

#[test]
fn every_write_of_synced_data_stamps_its_rows_with_clocks_and_records_its_ops() {
    for (i, (what, write, _)) in WRITES.iter().enumerate() {
        let dir = Dir::new(&format!("clock-{i}"));
        let (db, _) = UserStore::open(&dir.db()).unwrap();
        let device = db.device_id().unwrap();
        fill(&db);
        assert!(!db.records_ops().unwrap());
        assert_eq!(db.reserve_op_files().unwrap(), Vec::new(), "{what}");
        db.start_recording_ops().unwrap();
        assert!(db.records_ops().unwrap());
        db.take_applied_changes();
        write(&db);
        let changes = db.take_applied_changes();
        // Undone, the inverse is a change of its own, with later clocks.
        db.apply_changes(&inverse(&changes)).unwrap();
        let undo = db.take_applied_changes();
        let files = db.reserve_op_files().unwrap();
        assert_eq!(files.len(), 1, "{what}");
        assert_eq!(files[0].seq, 1, "{what}");
        let ops = &files[0].ops;
        let recorded: Vec<Change> = ops.iter().map(|o| o.change.clone()).collect();
        assert_eq!(
            recorded,
            [changes, undo].concat(),
            "{what}: the outbox is the stream"
        );
        for pair in ops.windows(2) {
            assert!(pair[0].clock < pair[1].clock, "{what}: clocks count on");
        }
        assert!(ops.iter().all(|o| o.clock.device_id == device), "{what}");
        // The row (item, tombstone) each Change wrote holds its clock; the last wins.
        let c = Connection::open(dir.db()).unwrap();
        let mut last = std::collections::HashMap::new();
        for op in ops {
            last.insert(op.change.key().unwrap(), op);
        }
        for (key, op) in &last {
            let deleted = op.change.after.is_none();
            assert_eq!(
                stored_clock(&c, key, deleted),
                Some(op.clock.encode()),
                "{what}: {}",
                key.describe()
            );
            if let Some(list) = list_key(key) {
                let newest = ops
                    .iter()
                    .filter(|o| list_key(&o.change.key().unwrap()).as_ref() == Some(&list))
                    .map(|o| o.clock.encode())
                    .max();
                assert_eq!(
                    stored_clock(&c, &RowKey::Preference(list.clone()), false),
                    newest,
                    "{what}: {list} has its newest item's clock"
                );
            }
        }
        db.op_file_written(1).unwrap();
        assert_eq!(db.reserve_op_files().unwrap(), Vec::new(), "{what}");
    }
}

#[test]
fn a_write_that_changes_nothing_emits_no_change_and_a_device_preference_none() {
    let dir = Dir::new("unchanged");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    fill(&db);
    db.take_applied_changes();
    db.save_preferred_order(&["kjv".into(), "web".into()])
        .unwrap();
    db.set_preference("highlights.palette", "standard").unwrap();
    db.set_preference("library.tags", r#"{"kjv":["older"]}"#)
        .unwrap();
    assert!(db.set_reading_read(first_plan(&db), 0, false).unwrap());
    db.set_preference("view", r#"{"theme":"dark"}"#).unwrap();
    db.set_preference("reading.visual_filters", "[]").unwrap();
    assert_eq!(db.take_applied_changes(), Vec::new());
    assert_eq!(
        db.preference("view").unwrap().as_deref(),
        Some(r#"{"theme":"dark"}"#)
    );
}

/// No Change, from this device or another, writes a preference that is not Synced data (D1:
/// Visual filters stay on their device).
#[test]
fn a_change_to_a_device_preference_is_refused_and_leaves_it_as_it_is() {
    let dir = Dir::new("device-preference-change");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let key = "reading.visual_filters";
    let mine = r#"[{"id":1}]"#;
    db.set_preference(key, mine).unwrap();
    let row = |value: &str| {
        Row::Preference(SyncedPreference {
            key: key.into(),
            value: value.into(),
        })
    };
    for (name, change) in [
        ("update", Change::update(row(mine), row("[]"))),
        ("delete", Change::delete(row(mine))),
        ("insert", Change::insert(row("[]"))),
    ] {
        let refused = db.apply_changes(&[change]).unwrap_err();
        assert!(
            matches!(refused, StoreError::Rejected(_)),
            "{name}: {refused}"
        );
        assert_eq!(raw(&dir.db(), key).as_deref(), Some(mine), "{name}");
    }
    let refused = db
        .apply_changes(&[Change::insert(Row::Preference(SyncedPreference {
            key: "view".into(),
            value: "{}".into(),
        }))])
        .unwrap_err();
    assert!(matches!(refused, StoreError::Rejected(_)), "{refused}");
    assert_eq!(raw(&dir.db(), "view"), None);
    assert_eq!(tombstones(&dir.db()), Vec::new());
}

#[test]
fn the_default_notebook_is_made_through_apply_changes() {
    let dir = Dir::new("default-notebook");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let changes = db.take_applied_changes();
    assert!(matches!(
        changes.as_slice(),
        [Change { before: None, after: Some(Row::Notebook(n)) }] if n.is_default
    ));
}

#[test]
fn a_delete_leaves_a_tombstone_and_undoing_it_takes_the_tombstone_away() {
    let dir = Dir::new("tombstone");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    fill(&db);
    let favorite = db.favorites().unwrap().remove(0);
    let uid: String = Connection::open(dir.db())
        .unwrap()
        .query_row(
            "SELECT uid FROM favorite WHERE id = ?1",
            [favorite.id],
            |r| r.get(0),
        )
        .unwrap();
    db.take_applied_changes();
    assert!(db.remove_favorite(favorite.id).unwrap());
    let stones = tombstones(&dir.db());
    assert_eq!(stones.len(), 1);
    assert_eq!((stones[0].0.as_str(), &stones[0].1), ("favorite", &uid));
    let clock = obs_store::Hlc::parse(&stones[0].2).expect("the delete's clock");
    assert_eq!(clock.device_id, db.device_id().unwrap());
    db.apply_changes(&inverse(&db.take_applied_changes()))
        .unwrap();
    assert_eq!(tombstones(&dir.db()), Vec::new());
    assert_eq!(db.favorites().unwrap()[0].label, "Jn 3:16");

    // A Collection deleted is an item's tombstone; replacing the Preferred order leaves none.
    db.set_preference("library.collections", "[]").unwrap();
    db.save_preferred_order(&["web".into()]).unwrap();
    let stones = tombstones(&dir.db());
    assert_eq!(stones.len(), 1);
    assert_eq!(
        (stones[0].0.as_str(), stones[0].1.as_str()),
        ("collection", "Greek")
    );
    assert!(obs_store::Hlc::parse(&stones[0].2).unwrap() > clock);
}

#[test]
fn removing_a_reading_plan_deletes_its_readings_first_and_a_bare_plan_delete_is_refused() {
    let dir = Dir::new("plan");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    fill(&db);
    let id = first_plan(&db);
    db.take_applied_changes();
    let before = synced_dump(&dir.db());

    // The plan's row deleted alone would take its Readings with it unrecorded: refused.
    db.remove_reading_plan(id).unwrap();
    let changes = db.take_applied_changes();
    db.apply_changes(&inverse(&changes)).unwrap();
    db.take_applied_changes();
    let plan_row = changes.last().unwrap().clone();
    assert!(matches!(plan_row.before, Some(Row::ReadingPlan(_))));
    assert_eq!(synced_dump(&dir.db()), before);
    let refused = db.apply_changes(&[plan_row]).unwrap_err();
    assert!(
        matches!(&refused, StoreError::Rejected(why) if why.contains("still has Readings")),
        "{refused}"
    );
    assert_eq!(synced_dump(&dir.db()), before, "nothing changed");

    // The store's own remove deletes each Reading, then the plan, as before.
    let id = first_plan(&db);
    assert!(db.remove_reading_plan(id).unwrap());
    let changes = db.take_applied_changes();
    assert_eq!(changes.len(), 3);
    assert!(matches!(changes[2].before, Some(Row::ReadingPlan(_))));
    assert!(db.reading_plans().unwrap().is_empty());
    assert!(db.plan_readings(id).unwrap().is_empty());
    assert!(!db.remove_reading_plan(id).unwrap());
}

/// A list preference's value as it is now, read raw.
fn raw(db: &std::path::Path, key: &str) -> Option<String> {
    Connection::open(db)
        .unwrap()
        .query_row("SELECT value FROM preference WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .ok()
}

#[test]
fn a_list_preference_not_in_shape_is_written_whole_and_never_loses_its_value() {
    let good = [
        (
            "library.collections",
            r#"[{"name":"A","rules":[],"resources":[]}]"#,
        ),
        ("library.tags", r#"{"kjv":["older"]}"#),
        ("copy.presets", r#"[{"name":"Plain","options":{}}]"#),
        ("search.saved", r#"[{"name":"Love","query":"love"}]"#),
        (
            "guide-arrangement/word_study",
            r#"{"sections":[{"id":"forms","collapsed":false,"hidden":false}]}"#,
        ),
    ];
    let damaged = [
        r#"[{"name":"A"},{"name":"A"}]"#,
        r#"{"kjv":["a"],"kjv":["b"]}"#,
        "not json",
        r#"[{"query":"no name"}]"#,
        r#"{"sections":[],"from_a_later_version":true}"#,
    ];
    for ((key, good), damaged) in good.into_iter().zip(damaged) {
        let dir = Dir::new(&format!("damaged-{}", key.replace('/', "-")));
        let (db, _) = UserStore::open(&dir.db()).unwrap();
        let whole = |value: &str| {
            Row::Preference(SyncedPreference {
                key: key.into(),
                value: value.into(),
            })
        };

        // A damaged value written over a good one is written whole, exactly.
        db.set_preference(key, good).unwrap();
        db.take_applied_changes();
        db.set_preference(key, damaged).unwrap();
        assert_eq!(raw(&dir.db(), key).as_deref(), Some(damaged), "{key}");
        assert_eq!(
            db.take_applied_changes(),
            vec![Change::update(whole(good), whole(damaged))],
            "{key}"
        );

        // An item edit planned against it is refused and leaves it as it is.
        let some_item = {
            let scratch = Dir::new(&format!("scratch-{}", key.replace('/', "-")));
            let (other, _) = UserStore::open(&scratch.db()).unwrap();
            other.take_applied_changes();
            other.set_preference(key, good).unwrap();
            other.take_applied_changes().remove(0)
        };
        assert!(matches!(some_item.key(), Some(k) if !matches!(k, RowKey::Preference(_))));
        let refused = db.apply_changes(&[some_item]).unwrap_err();
        assert!(
            matches!(refused, StoreError::Rejected(_)),
            "{key}: {refused}"
        );
        assert_eq!(raw(&dir.db(), key).as_deref(), Some(damaged), "{key}");

        // A good value written over the damaged one replaces it whole, as one Change whose
        // `before` holds the damaged value: nothing of it is dropped unrecorded.
        db.set_preference(key, good).unwrap();
        assert_eq!(raw(&dir.db(), key).as_deref(), Some(good), "{key}");
        assert_eq!(
            db.take_applied_changes(),
            vec![Change::update(whole(damaged), whole(good))],
            "{key}"
        );
    }
}

#[test]
fn an_empty_list_written_where_there_was_none_is_written_whole() {
    let dir = Dir::new("empty-list");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    db.take_applied_changes();
    db.set_preference("copy.presets", "[]").unwrap();
    assert_eq!(raw(&dir.db(), "copy.presets").as_deref(), Some("[]"));
    assert_eq!(
        db.take_applied_changes(),
        vec![Change::insert(Row::Preference(SyncedPreference {
            key: "copy.presets".into(),
            value: "[]".into(),
        }))]
    );
}

#[test]
fn list_items_from_two_devices_both_stay() {
    // What two devices adding a Collection each send: an item Change each. Applied one after
    // the other here, the second does not replace the first (whole-value writes would).
    let dir = Dir::new("two-items");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    db.set_preference("library.collections", r#"[{"name":"A"}]"#)
        .unwrap();
    let from = |value: &str| {
        let scratch = Dir::new("two-items-other");
        let (other, _) = UserStore::open(&scratch.db()).unwrap();
        other.set_preference("library.collections", value).unwrap();
        other.take_applied_changes().pop().unwrap()
    };
    let b = from(r#"[{"name":"B"}]"#);
    db.apply_changes(&[b]).unwrap();
    assert_eq!(
        raw(&dir.db(), "library.collections").as_deref(),
        Some(r#"[{"name":"A"},{"name":"B"}]"#)
    );
}

#[test]
fn notes_fts_follows_changes_applied_from_elsewhere() {
    let dir = Dir::new("fts");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let nb = db.notebooks().unwrap().remove(0);
    let written = note(&nb.id, "faith hope love");
    let edited = Annotation {
        content: Content::Note {
            body: "grace alone".into(),
        },
        modified_at: 30,
        ..written.clone()
    };
    let changes = vec![
        Change::insert(Row::Annotation(written.clone())),
        Change::update(Row::Annotation(written), Row::Annotation(edited.clone())),
    ];
    db.apply_changes(&changes).unwrap();
    assert_eq!(db.search_notes("grace").unwrap(), vec![edited.id.clone()]);
    assert!(db.search_notes("hope").unwrap().is_empty());
    db.annotation_dump().unwrap();
    db.apply_changes(&inverse(&changes)).unwrap();
    assert!(db.search_notes("grace").unwrap().is_empty());
    db.annotation_dump().unwrap();
}

#[test]
fn a_standard_style_edit_is_a_change_too() {
    // The seeded Standard palette's styles are rows like any other.
    let dir = Dir::new("standard");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let style = db
        .highlight_styles()
        .unwrap()
        .into_iter()
        .find(|s| s.palette_id == STANDARD_PALETTE_ID)
        .unwrap();
    let renamed = HighlightStyle {
        name: "Sun".into(),
        ..style.clone()
    };
    db.take_applied_changes();
    db.apply_changes(&[Change::update(
        Row::HighlightStyle(style),
        Row::HighlightStyle(renamed),
    )])
    .unwrap();
    assert_eq!(db.take_applied_changes().len(), 1);
}

#[test]
fn moving_and_deleting_list_items_stores_exactly_the_new_value_and_undoes_exactly() {
    let key = "guide-arrangement/passage_guide";
    let section = |id: &str| format!(r#"{{"id":"{id}","collapsed":false,"hidden":false}}"#);
    let value = |ids: &[&str]| {
        let items: Vec<String> = ids.iter().map(|id| section(id)).collect();
        format!(r#"{{"sections":[{}]}}"#, items.join(","))
    };
    let dir = Dir::new("moves");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let steps = [
        value(&["a", "b", "c", "d"]),
        value(&["b", "c", "d", "a"]),
        value(&["d", "c", "b", "a"]),
        value(&["c", "a"]),
        value(&["e", "c", "a", "b"]),
    ];
    let mut was = raw(&dir.db(), key);
    for step in steps {
        db.take_applied_changes();
        db.set_preference(key, &step).unwrap();
        assert_eq!(raw(&dir.db(), key).as_deref(), Some(step.as_str()));
        let changes = db.take_applied_changes();
        assert!(
            changes
                .iter()
                .all(|c| matches!(c.key(), Some(RowKey::GuideSection(..))))
        );
        if was.is_some() {
            db.apply_changes(&inverse(&changes)).unwrap();
            assert_eq!(raw(&dir.db(), key), was, "undone");
            db.apply_changes(&changes).unwrap();
            assert_eq!(
                raw(&dir.db(), key).as_deref(),
                Some(step.as_str()),
                "redone"
            );
        }
        was = Some(step);
    }
}
