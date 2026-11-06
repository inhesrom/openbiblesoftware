//! Reading plans in the User database (#147).
//!
//! A plan keeps what it was made from (template, Bible, start day, days of the week) and each
//! of its Readings: the Passages as a Bible reference, the day it is due and when it was
//! marked read. Catch up and Reschedule only move unread Readings to other days.

use rusqlite::params;

use crate::error::StoreError;
use crate::reading_plans::{PlanReadingRow, ReadingPlanRow};
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
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        tx.execute(
            "INSERT INTO reading_plan (name, template, bible_id, start_day, weekdays, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                plan.name,
                plan.template,
                plan.bible_id,
                plan.start_day,
                plan.weekdays,
                crate::user::now()
            ],
        )
        .map_err(sql)?;
        let id = tx.last_insert_rowid();
        {
            let mut stmt = tx
                .prepare_cached(
                    "INSERT INTO plan_reading (plan_id, position, passages, day)
                     VALUES (?1, ?2, ?3, ?4)",
                )
                .map_err(sql)?;
            for (position, r) in readings.iter().enumerate() {
                let position = i64::try_from(position).unwrap_or(i64::MAX);
                stmt.execute(params![id, position, r.passages, r.day])
                    .map_err(sql)?;
            }
        }
        tx.commit().map_err(sql)?;
        Ok(id)
    }

    /// Mark a Reading read (now) or unread; false if the plan has no such Reading.
    pub fn set_reading_read(
        &self,
        plan_id: i64,
        position: u32,
        read: bool,
    ) -> Result<bool, StoreError> {
        let n = self
            .conn
            .execute(
                "UPDATE plan_reading
                 SET read_at = CASE WHEN ?3 THEN COALESCE(read_at, ?4) ELSE NULL END
                 WHERE plan_id = ?1 AND position = ?2",
                params![plan_id, position, read, crate::user::now()],
            )
            .map_err(sql)?;
        Ok(n > 0)
    }

    /// Move Readings to other days: `(position, day)` pairs, in one transaction.
    pub fn move_readings(&self, plan_id: i64, days: &[(u32, String)]) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        {
            let mut stmt = tx
                .prepare_cached(
                    "UPDATE plan_reading SET day = ?3 WHERE plan_id = ?1 AND position = ?2",
                )
                .map_err(sql)?;
            for (position, day) in days {
                stmt.execute(params![plan_id, position, day]).map_err(sql)?;
            }
        }
        tx.commit().map_err(sql)
    }

    /// Delete a Reading plan and its Readings; false if there was none with that id.
    pub fn remove_reading_plan(&self, id: i64) -> Result<bool, StoreError> {
        let n = self
            .conn
            .execute("DELETE FROM reading_plan WHERE id = ?1", [id])
            .map_err(sql)?;
        Ok(n > 0)
    }
}
