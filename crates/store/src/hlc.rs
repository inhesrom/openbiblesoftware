//! The hybrid logical clock Sync stamps every change of Synced data with (ADR 0022 rules 2–3):
//! wall-clock milliseconds, a counter for changes within one millisecond (or while the wall
//! clock is behind the last clock handed out) and the Synced device's id.
//!
//! Its text form is what the User database's `clock` columns, the tombstones, the item clocks
//! and the op files hold: `{wall_ms:012x}-{counter:04x}-{device_id}`, lowercase hexadecimal of
//! fixed width followed by the device's UUIDv7, so comparing two clocks as text (SQLite's
//! `BINARY` collation, `MAX(clock)`) compares them by wall time, then counter, then device.
//! A `NULL` or empty clock means "changed before this device recorded clocks" and sorts before
//! every clock. `docs/dev/sync.md` documents the encoding.
//!
//! Compiles everywhere (wasm included).

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::annotation::is_valid_id;
use crate::error::StoreError;

/// The largest wall time a clock holds: 48 bits of milliseconds, as a UUIDv7's (year 10889).
pub const MAX_WALL_MS: u64 = 0xFFFF_FFFF_FFFF;

/// How far ahead of this device's wall clock a clock it takes in may be: 24 hours. A clock
/// further ahead (a computer whose clock was once set years ahead, a damaged or crafted
/// Backup or op) would pin every clock handed out after it at its wall time, so the clock
/// generator does not take it in: not when it seeds from the database (S28a), not when a
/// merge observes it (S28b). The row holding it is kept as it is (ADR 0006); only the
/// clocks this device hands out do not follow it.
pub const MAX_FORWARD_SKEW_MS: u64 = 24 * 60 * 60 * 1000;

/// The latest wall time of a clock this device takes in at wall time `now_ms`
/// ([`MAX_FORWARD_SKEW_MS`] ahead of it, at most [`MAX_WALL_MS`]).
pub fn latest_wall_taken_in(now_ms: i64) -> u64 {
    u64::try_from(now_ms)
        .unwrap_or(0)
        .saturating_add(MAX_FORWARD_SKEW_MS)
        .min(MAX_WALL_MS)
}

/// One clock: when, in what order within that millisecond, and on which Synced device.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hlc {
    /// Unix milliseconds, at most [`MAX_WALL_MS`].
    pub wall_ms: u64,
    /// Orders the clocks one device hands out within one `wall_ms`.
    pub counter: u16,
    /// The Synced device's id (`device.device_id`, a lowercase hyphenated UUID).
    pub device_id: String,
}

impl Hlc {
    /// The clock's text form (see the module documentation), or
    /// [`StoreError::Rejected`] for a clock that has none: a wall time above
    /// [`MAX_WALL_MS`] or a device id that is not a UUID. It is never cut to fit.
    pub fn encode(&self) -> Result<String, StoreError> {
        if self.wall_ms > MAX_WALL_MS || !is_valid_id(&self.device_id) {
            return Err(StoreError::Rejected(format!(
                "`{self}` is not a clock: its wall time is at most {MAX_WALL_MS:#x} and its device a UUID"
            )));
        }
        Ok(self.to_string())
    }

    /// The clock `text` encodes; `None` when it is not one (an empty clock included).
    pub fn parse(text: &str) -> Option<Hlc> {
        let lower_hex = |s: &str| s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        let wall = text.get(0..12)?;
        let counter = text.get(13..17)?;
        let device_id = text.get(18..)?;
        if text.get(12..13)? != "-"
            || text.get(17..18)? != "-"
            || !lower_hex(wall)
            || !lower_hex(counter)
            || !is_valid_id(device_id)
        {
            return None;
        }
        Some(Hlc {
            wall_ms: u64::from_str_radix(wall, 16).ok()?,
            counter: u16::from_str_radix(counter, 16).ok()?,
            device_id: device_id.to_string(),
        })
    }

    /// The clock after `last` that a change made at wall time `now_ms` on `device_id` gets:
    /// the wall time when it is later than `last`'s, else `last`'s with the counter counted
    /// on; a counter at its end moves to the next millisecond instead. Always above `last`:
    /// when `last` is the largest clock there is (wall [`MAX_WALL_MS`], counter at its end),
    /// none is, and it is [`StoreError::Rejected`].
    pub fn next(last: Option<(u64, u16)>, now_ms: i64, device_id: &str) -> Result<Hlc, StoreError> {
        let now = u64::try_from(now_ms).unwrap_or(0).min(MAX_WALL_MS);
        let (wall_ms, counter) = match last {
            Some((wall, _)) if now > wall => (now, 0),
            Some((wall, counter)) => match counter.checked_add(1) {
                Some(counter) => (wall, counter),
                None if wall < MAX_WALL_MS => (wall + 1, 0),
                None => {
                    return Err(StoreError::Rejected(format!(
                        "no clock comes after wall time {wall:#x}, counter {counter:#x}"
                    )));
                }
            },
            None => (now, 0),
        };
        Ok(Hlc {
            wall_ms,
            counter,
            device_id: device_id.to_string(),
        })
    }

    /// Its wall time and counter, the part a device's next clock must pass.
    pub fn tick(&self) -> (u64, u16) {
        (self.wall_ms, self.counter)
    }
}

/// The text form; a clock without one (see [`Hlc::encode`]) shows its fields as they are,
/// which [`Hlc::parse`] refuses.
impl fmt::Display for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:012x}-{:04x}-{}",
            self.wall_ms, self.counter, self.device_id
        )
    }
}

impl Serialize for Hlc {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.encode().map_err(serde::ser::Error::custom)?)
    }
}

impl<'de> Deserialize<'de> for Hlc {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Hlc::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("`{text}` is not a clock")))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    const A: &str = "0193a1b2-c3d4-7000-8000-000000000001";
    const B: &str = "0193a1b2-c3d4-7000-8000-000000000002";

    fn hlc(wall_ms: u64, counter: u16, device_id: &str) -> Hlc {
        Hlc {
            wall_ms,
            counter,
            device_id: device_id.into(),
        }
    }

    #[test]
    fn a_clock_encodes_as_fixed_width_text_and_parses_back() {
        let c = hlc(0x0193_a1b2_c3d4, 0x2a, A);
        assert_eq!(c.encode().unwrap(), format!("0193a1b2c3d4-002a-{A}"));
        assert_eq!(Hlc::parse(&c.encode().unwrap()), Some(c.clone()));
        assert_eq!(serde_json::to_string(&c).unwrap(), format!("\"{c}\""));
        assert_eq!(serde_json::from_str::<Hlc>(&format!("\"{c}\"")).unwrap(), c);
        for bad in [
            "",
            "0193a1b2c3d4-002a",
            "0193A1B2C3D4-002a-0193a1b2-c3d4-7000-8000-000000000001",
            "0193a1b2c3d4_002a-0193a1b2-c3d4-7000-8000-000000000001",
            "0193a1b2c3d4-002a-not-a-device",
            "193a1b2c3d4-002a-0193a1b2-c3d4-7000-8000-000000000001",
        ] {
            assert_eq!(Hlc::parse(bad), None, "{bad}");
        }
        assert!(serde_json::from_str::<Hlc>("\"\"").is_err());
    }

    #[test]
    fn a_clock_with_no_text_form_is_refused_not_cut_to_fit() {
        assert!(hlc(MAX_WALL_MS, u16::MAX, A).encode().is_ok());
        for bad in [hlc(MAX_WALL_MS + 1, 0, A), hlc(1, 0, "not-a-device")] {
            assert!(
                matches!(bad.encode(), Err(StoreError::Rejected(_))),
                "{bad}"
            );
            assert!(serde_json::to_string(&bad).is_err(), "{bad}");
            assert_eq!(Hlc::parse(&bad.to_string()), None, "{bad}");
        }
    }

    #[test]
    fn clocks_sorted_as_text_sort_by_wall_then_counter_then_device() {
        // Two devices tying on wall time and counter, a counter at its end and the next
        // millisecond it moves to, the smallest and largest wall times.
        let last = hlc(1_700_000_000_000, u16::MAX, A);
        let bumped = Hlc::next(Some(last.tick()), 1_699_999_999_000, A).unwrap();
        assert_eq!(bumped.tick(), (1_700_000_000_001, 0));
        let mut clocks = vec![
            hlc(1_700_000_000_000, 7, B),
            hlc(1_700_000_000_000, 7, A),
            last.clone(),
            bumped,
            hlc(1_700_000_000_000, 0x100, A),
            hlc(1_700_000_000_000, 0xff, B),
            hlc(0, 0, B),
            hlc(MAX_WALL_MS, u16::MAX, A),
            hlc(1, 0, A),
            hlc(0x10, 0, A),
            hlc(0xf, 0, B),
        ];
        let encode = |c: &Hlc| c.encode().unwrap();
        let mut as_text: Vec<String> = clocks.iter().map(encode).collect();
        clocks.sort();
        as_text.sort();
        assert_eq!(
            as_text,
            clocks.iter().map(encode).collect::<Vec<_>>(),
            "text order is (wall, counter, device) order"
        );
        // The empty clock (a tombstone written before clocks) sorts before every clock.
        assert!(as_text.iter().all(|c| "" < c.as_str()));
    }

    #[test]
    fn the_next_clock_passes_the_last_even_when_the_wall_clock_steps_back() {
        let next = |last: Option<&Hlc>, now| Hlc::next(last.map(Hlc::tick), now, A).unwrap();
        let first = next(None, 5_000);
        assert_eq!(first.tick(), (5_000, 0));
        let same_ms = next(Some(&first), 5_000);
        assert_eq!(same_ms.tick(), (5_000, 1));
        let stepped_back = next(Some(&same_ms), 4_000);
        assert_eq!(stepped_back.tick(), (5_000, 2));
        let later = next(Some(&stepped_back), 6_000);
        assert_eq!(later.tick(), (6_000, 0));
        assert!(first < same_ms && same_ms < stepped_back && stepped_back < later);
        // No clock on the platform (bare wasm) still counts on.
        assert_eq!(next(Some(&later), 0).tick(), (6_000, 1));
        assert_eq!(next(None, -1).tick(), (0, 0));
    }

    #[test]
    fn the_next_clock_never_comes_at_or_below_the_last_even_at_the_largest_wall_time() {
        let at_end = |wall| Some((wall, u16::MAX));
        // The wall clock past the largest wall time: held at it.
        let held = Hlc::next(Some((MAX_WALL_MS, 0)), i64::MAX, A).unwrap();
        assert_eq!(held.tick(), (MAX_WALL_MS, 1));
        let last_ms = Hlc::next(at_end(MAX_WALL_MS - 1), 0, A).unwrap();
        assert_eq!(last_ms.tick(), (MAX_WALL_MS, 0));
        // The largest clock there is: none comes after it.
        assert!(matches!(
            Hlc::next(at_end(MAX_WALL_MS), i64::MAX, A),
            Err(StoreError::Rejected(_))
        ));
    }
}
