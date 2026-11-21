//! The hybrid logical clock of ADR 0022 rule 2: wall-clock milliseconds, a counter and the
//! Synced device's id, written as fixed-width text that sorts as the clock does
//! (`{wall_ms:012x}-{counter:04x}-{device_id}`, `docs/dev/sync.md`).
//!
//! It lives in `obs-store` (`obs_store::hlc`): the User database stamps every row
//! `apply_changes` writes with the next clock, including the writes the store plans itself,
//! so the store makes the clocks; Sync reads and writes the same type.

pub use obs_store::Hlc;
pub use obs_store::hlc::MAX_WALL_MS;
