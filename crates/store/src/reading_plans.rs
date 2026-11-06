//! Reading plan rows (#147); the SQLite side is `reading_plans_store.rs`.

/// One Reading plan as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingPlanRow {
    pub id: i64,
    pub name: String,
    /// The Reading plan template it was made from (the engine's template id).
    pub template: String,
    /// The Bible its Readings open in.
    pub bible_id: String,
    /// Its first day, `YYYY-MM-DD`.
    pub start_day: String,
    /// The days of the week it is read on: bit 0 Monday … bit 6 Sunday.
    pub weekdays: u8,
    /// Unix seconds.
    pub created_at: i64,
}

/// One Reading of a Reading plan as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanReadingRow {
    /// Its place in the plan, from 0.
    pub position: u32,
    /// Its Passages, as a Bible reference in the Standard versification.
    pub passages: String,
    /// The day it is due, `YYYY-MM-DD`.
    pub day: String,
    /// When it was marked read (Unix seconds), if it has been.
    pub read_at: Option<i64>,
}
