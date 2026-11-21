//! Reading plans in the User database (#147).
//!
//! A plan keeps what it was made from (template, Bible, start day, days of the week) and each
//! of its Readings: the Passages as a Bible reference, the day it is due and when it was
//! marked read. Catch up and Reschedule only move unread Readings to other days.

use std::collections::BTreeMap;

use crate::annotation::{Change, Row};
use crate::error::StoreError;
use crate::reading_plans::{PlanReadingRow, ReadingPlanRow};
use crate::synced::{SyncedReading, SyncedReadingPlan};
use crate::synced_store;
use crate::user::{UserStore, sql};

/// 7 → 8: Reading plans.
pub(crate) const SCHEMA_8: &str = "
CREATE TABLE reading_plan (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    template TEXT NOT NULL,
    bible_id TEXT NOT NULL,
    start_day TEXT NOT NULL,
    weekdays INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE plan_reading (
    plan_id INTEGER NOT NULL REFERENCES reading_plan (id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    passages TEXT NOT NULL,
    day TEXT NOT NULL,
    read_at INTEGER,
    PRIMARY KEY (plan_id, position)
);
";

impl UserStore {
    /// Every Reading plan, oldest first.
    pub fn reading_plans(&self) -> Result<Vec<ReadingPlanRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT id, name, template, bible_id, start_day, weekdays, created_at
                 FROM reading_plan ORDER BY id",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ReadingPlanRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    template: r.get(2)?,
                    bible_id: r.get(3)?,
                    start_day: r.get(4)?,
                    weekdays: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    /// A Reading plan's Readings, in order.
    pub fn plan_readings(&self, plan_id: i64) -> Result<Vec<PlanReadingRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT position, passages, day, read_at FROM plan_reading
                 WHERE plan_id = ?1 ORDER BY position",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map([plan_id], |r| {
                Ok(PlanReadingRow {
                    position: r.get(0)?,
                    passages: r.get(1)?,
                    day: r.get(2)?,
                    read_at: r.get(3)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    /// Add a Reading plan with its Readings (their `position`s are taken from their order and
    /// their `read_at` ignored). Returns its id.
    pub fn add_reading_plan(
        &self,
        plan: &ReadingPlanRow,
        readings: &[PlanReadingRow],
    ) -> Result<i64, StoreError> {
        let uid = crate::annotation::new_id();
        let mut changes = vec![Change::insert(Row::ReadingPlan(SyncedReadingPlan {
            uid: uid.clone(),
            name: plan.name.clone(),
            template: plan.template.clone(),
            bible_id: plan.bible_id.clone(),
            start_day: plan.start_day.clone(),
            weekdays: plan.weekdays,
            created_at: crate::user::now(),
        }))];
        for (position, r) in readings.iter().enumerate() {
            changes.push(Change::insert(Row::Reading(SyncedReading {
                plan_uid: uid.clone(),
                position: u32::try_from(position).unwrap_or(u32::MAX),
                passages: r.passages.clone(),
                day: r.day.clone(),
                read_at: None,
            })));
        }
        self.apply_changes(&changes)?;
        synced_store::local_id(&self.conn, "reading_plan", &uid)
    }

    /// Mark a Reading read (now) or unread; false if the plan has no such Reading.
    pub fn set_reading_read(
        &self,
        plan_id: i64,
        position: u32,
        read: bool,
    ) -> Result<bool, StoreError> {
        let Some(plan) = synced_store::read_plan_by_id(&self.conn, plan_id)? else {
            return Ok(false);
        };
        let Some(before) = synced_store::read_reading(&self.conn, &plan.uid, position)? else {
            return Ok(false);
        };
        let read_at = if read {
            before.read_at.or_else(|| Some(crate::user::now()))
        } else {
            None
        };
        if read_at != before.read_at {
            let after = SyncedReading {
                read_at,
                ..before.clone()
            };
            self.apply_changes(&[Change::update(Row::Reading(before), Row::Reading(after))])?;
        }
        Ok(true)
    }

    /// Move Readings to other days: `(position, day)` pairs, in one transaction.
    pub fn move_readings(&self, plan_id: i64, days: &[(u32, String)]) -> Result<(), StoreError> {
        let Some(plan) = synced_store::read_plan_by_id(&self.conn, plan_id)? else {
            return Ok(());
        };
        // A Reading moved twice is planned from where the first move put it.
        let mut moved: BTreeMap<u32, SyncedReading> = BTreeMap::new();
        let mut changes = Vec::new();
        for (position, day) in days {
            let current = match moved.get(position) {
                Some(r) => Some(r.clone()),
                None => synced_store::read_reading(&self.conn, &plan.uid, *position)?,
            };
            let Some(before) = current else {
                continue;
            };
            if before.day == *day {
                continue;
            }
            let after = SyncedReading {
                day: day.clone(),
                ..before.clone()
            };
            moved.insert(*position, after.clone());
            changes.push(Change::update(Row::Reading(before), Row::Reading(after)));
        }
        self.apply_changes(&changes)
    }

    /// Delete a Reading plan and its Readings; false if there was none with that id.
    pub fn remove_reading_plan(&self, id: i64) -> Result<bool, StoreError> {
        let Some(plan) = synced_store::read_plan_by_id(&self.conn, id)? else {
            return Ok(false);
        };
        let mut changes: Vec<Change> = synced_store::read_readings(&self.conn, &plan.uid)?
            .into_iter()
            .map(|r| Change::delete(Row::Reading(r)))
            .collect();
        changes.push(Change::delete(Row::ReadingPlan(plan)));
        self.apply_changes(&changes)?;
        Ok(true)
    }
}
