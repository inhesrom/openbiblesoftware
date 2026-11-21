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

/// The largest wall time a clock holds: 48 bits of milliseconds, as a UUIDv7's (year 10889).
pub const MAX_WALL_MS: u64 = 0xFFFF_FFFF_FFFF;

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
    /// The clock's text form (see the module documentation).
    pub fn encode(&self) -> String {
        format!(
            "{:012x}-{:04x}-{}",
            self.wall_ms & MAX_WALL_MS,
            self.counter,
            self.device_id
        )
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
    /// on; a counter at its end moves to the next millisecond instead.
    pub fn next(last: Option<(u64, u16)>, now_ms: i64, device_id: &str) -> Hlc {
        let now = u64::try_from(now_ms).unwrap_or(0).min(MAX_WALL_MS);
        let (wall_ms, counter) = match last {
            Some((wall, _)) if now > wall => (now, 0),
            Some((wall, counter)) => match counter.checked_add(1) {
                Some(counter) => (wall, counter),
                None => ((wall + 1).min(MAX_WALL_MS), 0),
            },
            None => (now, 0),
        };
        Hlc {
            wall_ms,
            counter,
            device_id: device_id.to_string(),
        }
    }

    /// Its wall time and counter, the part a device's next clock must pass.
    pub fn tick(&self) -> (u64, u16) {
        (self.wall_ms, self.counter)
    }
}

impl fmt::Display for Hlc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.encode())
    }
}

impl Serialize for Hlc {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.encode())
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
        assert_eq!(c.encode(), format!("0193a1b2c3d4-002a-{A}"));
        assert_eq!(Hlc::parse(&c.encode()), Some(c.clone()));
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
    fn clocks_sorted_as_text_sort_by_wall_then_counter_then_device() {
        // Two devices tying on wall time and counter, a counter at its end and the next
        // millisecond it moves to, the smallest and largest wall times.
        let last = hlc(1_700_000_000_000, u16::MAX, A);
        let bumped = Hlc::next(Some(last.tick()), 1_699_999_999_000, A);
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
        let mut as_text: Vec<String> = clocks.iter().map(Hlc::encode).collect();
        clocks.sort();
        as_text.sort();
        assert_eq!(
            as_text,
            clocks.iter().map(Hlc::encode).collect::<Vec<_>>(),
            "text order is (wall, counter, device) order"
        );
        // The empty clock (a tombstone written before clocks) sorts before every clock.
        assert!(as_text.iter().all(|c| "" < c.as_str()));
    }

    #[test]
    fn the_next_clock_passes_the_last_even_when_the_wall_clock_steps_back() {
        let first = Hlc::next(None, 5_000, A);
        assert_eq!(first.tick(), (5_000, 0));
        let same_ms = Hlc::next(Some(first.tick()), 5_000, A);
        assert_eq!(same_ms.tick(), (5_000, 1));
        let stepped_back = Hlc::next(Some(same_ms.tick()), 4_000, A);
        assert_eq!(stepped_back.tick(), (5_000, 2));
        let later = Hlc::next(Some(stepped_back.tick()), 6_000, A);
        assert_eq!(later.tick(), (6_000, 0));
        assert!(first < same_ms && same_ms < stepped_back && stepped_back < later);
        // No clock on the platform (bare wasm) still counts on.
        assert_eq!(Hlc::next(Some(later.tick()), 0, A).tick(), (6_000, 1));
        assert_eq!(Hlc::next(None, -1, A).tick(), (0, 0));
    }
}
