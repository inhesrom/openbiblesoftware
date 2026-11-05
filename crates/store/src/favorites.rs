//! Favorites and History rows (#155); the SQLite side is `favorites_store.rs`.

/// History forgets Visits older than this many days.
pub const HISTORY_DAYS: i64 = 90;

/// History keeps at most this many Visits (the newest).
pub const HISTORY_LIMIT: i64 = 5_000;

/// One Favorite as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FavoriteRow {
    pub id: i64,
    /// Its Favorites folder; empty when it is in none.
    pub folder: String,
    pub label: String,
    /// Where it goes: the engine's JSON.
    pub location: String,
    /// Unix seconds.
    pub added_at: i64,
}

/// One Visit in History as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisitRow {
    pub id: i64,
    /// Unix seconds.
    pub visited_at: i64,
    /// The local date it was made on, `YYYY-MM-DD`.
    pub day: String,
    /// Whole days from `day` to today (0: today, 1: yesterday).
    pub days_ago: i64,
    /// The local time of day it was made at, `HH:MM`.
    pub time: String,
    pub label: String,
    pub location: String,
}
