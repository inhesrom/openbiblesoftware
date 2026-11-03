//! Keys: the values a Resource is organized by and can be opened at.

use std::fmt;

use obs_scripture::{Passage, StrongsNumber};
use serde::{Deserialize, Serialize};

use crate::manifest::KeyScheme;
use crate::unit::UnitId;

/// One Key value. Non-Bible Passage Keys are in the Standard versification.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "scheme", content = "key", rename_all = "snake_case")]
pub enum Key {
    Passage(Passage),
    StrongsNumber(StrongsNumber),
    /// A Lemma in original script, NFC.
    Lemma(String),
    Headword(String),
}

impl Key {
    pub fn scheme(&self) -> KeyScheme {
        match self {
            Key::Passage(_) => KeyScheme::Passage,
            Key::StrongsNumber(_) => KeyScheme::StrongsNumber,
            Key::Lemma(_) => KeyScheme::Lemma,
            Key::Headword(_) => KeyScheme::Headword,
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Passage(p) => write!(f, "{p}"),
            Key::StrongsNumber(s) => write!(f, "{s}"),
            Key::Lemma(s) | Key::Headword(s) => f.write_str(s),
        }
    }
}

/// Where a Key opens: a unit, optionally narrowed to a byte range.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct KeyEntry {
    pub key: Key,
    pub unit: UnitId,
    /// Byte range inside the unit; `None` means the whole unit.
    pub range: Option<(u32, u32)>,
    /// True for a secondary Key: a Headword alias ("Abram" for "Abraham"), or the base Strong's
    /// number of an entry for an extended number (H1254 on the H1254A entry).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub alias: bool,
}
