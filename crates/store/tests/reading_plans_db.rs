#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]
//! Reading plans in the User database (#147).

use std::path::PathBuf;

use obs_store::{PlanReadingRow, ReadingPlanRow, USER_SCHEMA_VERSION, UserStore};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-plans-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("user.db")
}

fn plan() -> ReadingPlanRow {
    ReadingPlanRow {
        id: 0,
        name: "Bible in a year (canonical)".into(),
        template: "bible-canonical".into(),
        bible_id: "bsb".into(),
        start_day: "2026-01-01".into(),
        weekdays: 0b111_1111,
        created_at: 0,
    }
}

fn reading(passages: &str, day: &str) -> PlanReadingRow {
    PlanReadingRow {
        position: 0,
        passages: passages.into(),
        day: day.into(),
        read_at: None,
    }
}

#[test]
fn a_plan_keeps_its_readings_progress_and_new_days_across_a_reopen() {
    let path = tmp("reopen");
    let id = {
        let (db, _) = UserStore::open(&path).unwrap();
        let id = db
            .add_reading_plan(
                &plan(),
                &[
                    reading("Gen 1-3", "2026-01-01"),
                    reading("Gen 4-7", "2026-01-02"),
                    reading("Gen 8-11", "2026-01-03"),
                ],
            )
            .unwrap();
        assert!(db.set_reading_read(id, 0, true).unwrap());
        assert!(!db.set_reading_read(id, 9, true).unwrap());
        db.move_readings(id, &[(1, "2026-01-05".into()), (2, "2026-01-05".into())])
            .unwrap();
        id
    };
    let (db, _) = UserStore::open(&path).unwrap();
    let plans = db.reading_plans().unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].id, id);
    assert_eq!(plans[0].template, "bible-canonical");
    assert_eq!(plans[0].weekdays, 0b111_1111);
    let readings = db.plan_readings(id).unwrap();
    let days: Vec<(u32, &str, &str, bool)> = readings
        .iter()
        .map(|r| {
            (
                r.position,
                r.passages.as_str(),
                r.day.as_str(),
                r.read_at.is_some(),
            )
        })
        .collect();
    assert_eq!(
        days,
        [
            (0, "Gen 1-3", "2026-01-01", true),
            (1, "Gen 4-7", "2026-01-05", false),
            (2, "Gen 8-11", "2026-01-05", false),
        ]
    );
    // Unticking forgets when it was read.
    assert!(db.set_reading_read(id, 0, false).unwrap());
    assert_eq!(db.plan_readings(id).unwrap()[0].read_at, None);
}

#[test]
fn removing_a_plan_removes_its_readings() {
    let path = tmp("remove");
    let (db, _) = UserStore::open(&path).unwrap();
    let a = db
        .add_reading_plan(&plan(), &[reading("Ps 1", "2026-01-01")])
        .unwrap();
    let b = db
        .add_reading_plan(&plan(), &[reading("Prov 1", "2026-01-01")])
        .unwrap();
    assert!(db.remove_reading_plan(a).unwrap());
    assert!(!db.remove_reading_plan(a).unwrap());
    assert!(db.plan_readings(a).unwrap().is_empty());
    assert_eq!(db.plan_readings(b).unwrap().len(), 1);
    assert_eq!(db.reading_plans().unwrap().len(), 1);
}

#[test]
fn a_v7_database_gains_empty_reading_plans() {
    let path = tmp("migrate");
    {
        let (v7, _) = UserStore::open_to(&path, 7).unwrap();
        v7.add_favorite("", "Gen 1:1 · BSB", "{}").unwrap();
    }
    let (db, report) = UserStore::open(&path).unwrap();
    assert_eq!(report.found_version, 7);
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    assert_eq!(db.favorites().unwrap().len(), 1);
    assert!(db.reading_plans().unwrap().is_empty());
}
