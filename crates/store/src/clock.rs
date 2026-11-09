//! The clock the User database stamps its rows with and shows times by: the system clock, or,
//! with `test-support`, a fixed time set for one thread — the Screenshot gallery's fixed clock
//! (#222), so its Home, History and Backups read the same on every run. Under a fixed clock,
//! local time is UTC, so they read the same in every time zone too.

/// Milliseconds since the Unix epoch (0 where the platform has no clock, as on bare wasm).
pub fn now_ms() -> i64 {
    #[cfg(feature = "test-support")]
    if let Some(ms) = FIXED.with(std::cell::Cell::get) {
        return ms;
    }
    system_ms()
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> i64 {
    now_ms().div_euclid(1000)
}

fn system_ms() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// SQLite's date-and-time modifier from UTC to local time: none under a fixed clock.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn to_local() -> &'static str {
    #[cfg(feature = "test-support")]
    if FIXED.with(std::cell::Cell::get).is_some() {
        return "+0 seconds";
    }
    "localtime"
}

#[cfg(feature = "test-support")]
thread_local! {
    static FIXED: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
}

/// Stop this thread's clock at `ms` (Unix milliseconds, local time being UTC), or with `None`
/// let it run again.
#[cfg(feature = "test-support")]
pub fn fix(ms: Option<i64>) {
    FIXED.with(|f| f.set(ms));
}
