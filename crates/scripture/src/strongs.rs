//! Strong's numbers.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The language half of the Strong's system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StrongsLanguage {
    /// `H` numbers: Hebrew and Aramaic.
    Hebrew,
    /// `G` numbers: Greek.
    Greek,
}

impl StrongsLanguage {
    pub const fn prefix(self) -> char {
        match self {
            StrongsLanguage::Hebrew => 'H',
            StrongsLanguage::Greek => 'G',
        }
    }
}

/// The identifier for a Lemma in the Strong's system, including the extended forms that tell
/// homographs apart (STEPBible's `H1254A`).
///
/// The normalized text form is a prefix letter, the number without zero padding, and an
/// optional uppercase homograph letter: `G5485`, `H1254A`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StrongsNumber {
    language: StrongsLanguage,
    number: u32,
    homograph: Option<char>,
}

/// Highest number accepted. Classic Strong's stops at H8674 / G5624; STEPBible extends both
/// ranges, so leave headroom without accepting arbitrary digits.
const MAX_NUMBER: u32 = 99_999;

/// Why a Strong's number could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrongsParseError {
    input: String,
}

impl fmt::Display for StrongsParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a Strong's number (expected e.g. G5485 or H1254A)",
            self.input
        )
    }
}

impl std::error::Error for StrongsParseError {}

impl StrongsNumber {
    /// A Strong's number, or `None` if `number` is 0 or too large, or `homograph` is not an
    /// ASCII letter.
    pub fn new(language: StrongsLanguage, number: u32, homograph: Option<char>) -> Option<Self> {
        if number == 0 || number > MAX_NUMBER {
            return None;
        }
        let homograph = match homograph {
            None => None,
            Some(c) if c.is_ascii_alphabetic() => Some(c.to_ascii_uppercase()),
            Some(_) => return None,
        };
        Some(Self {
            language,
            number,
            homograph,
        })
    }

    pub const fn language(self) -> StrongsLanguage {
        self.language
    }

    pub const fn number(self) -> u32 {
        self.number
    }

    pub const fn homograph(self) -> Option<char> {
        self.homograph
    }

    /// Whether this is an extended form with a homograph letter.
    pub const fn is_extended(self) -> bool {
        self.homograph.is_some()
    }

    /// The base number without its homograph letter (`H1254A` → `H1254`). Lexicon lookup
    /// falls back to it.
    pub const fn base(self) -> Self {
        Self {
            language: self.language,
            number: self.number,
            homograph: None,
        }
    }

    /// Whether `self` matches `other` as a search term: an extended number matches only
    /// itself, a base number matches all of its homographs.
    pub fn matches(self, other: StrongsNumber) -> bool {
        if self.is_extended() {
            self == other
        } else {
            self == other.base()
        }
    }
}

impl FromStr for StrongsNumber {
    type Err = StrongsParseError;

    /// Accepts `G5485`, `g05485`, `H1254a`, `H1254A`; surrounding whitespace is ignored.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || StrongsParseError {
            input: s.to_string(),
        };
        let t = s.trim();
        let mut chars = t.chars();
        let language = match chars.next() {
            Some('H' | 'h') => StrongsLanguage::Hebrew,
            Some('G' | 'g') => StrongsLanguage::Greek,
            _ => return Err(err()),
        };
        let rest = chars.as_str();
        let digits_end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let (digits, suffix) = rest.split_at(digits_end);
        if digits.is_empty() || digits.len() > 5 {
            return Err(err());
        }
        let number: u32 = digits.parse().map_err(|_| err())?;
        let homograph = match suffix.len() {
            0 => None,
            1 => suffix.chars().next(),
            _ => return Err(err()),
        };
        Self::new(language, number, homograph).ok_or_else(err)
    }
}

impl fmt::Display for StrongsNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.language.prefix(), self.number)?;
        if let Some(h) = self.homograph {
            write!(f, "{h}")?;
        }
        Ok(())
    }
}

impl Serialize for StrongsNumber {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for StrongsNumber {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> StrongsNumber {
        text.parse().unwrap()
    }

    #[test]
    fn normalizes() {
        assert_eq!(s("G05485").to_string(), "G5485");
        assert_eq!(s("g5485").to_string(), "G5485");
        assert_eq!(s("H1254a").to_string(), "H1254A");
        assert_eq!(s(" H1254A ").to_string(), "H1254A");
        assert_eq!(s("G0001").to_string(), "G1");
        assert_eq!(s("H1254A").language(), StrongsLanguage::Hebrew);
        assert_eq!(s("H1254A").number(), 1254);
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "",
            "G",
            "5485",
            "X5485",
            "G0",
            "G000",
            "G-1",
            "G54 85",
            "G5485AB",
            "G5485!",
            "G123456",
            "H1254é",
            "GG12",
            "strong:G26",
        ] {
            assert!(bad.parse::<StrongsNumber>().is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn base_and_matching() {
        let ext = s("H1254A");
        let base = s("H1254");
        assert_eq!(ext.base(), base);
        assert!(ext.is_extended() && !base.is_extended());
        assert!(base.matches(ext));
        assert!(base.matches(s("H1254B")));
        assert!(ext.matches(ext));
        assert!(!ext.matches(s("H1254B")));
        assert!(!ext.matches(base));
        assert!(!base.matches(s("G1254")));
    }

    #[test]
    fn serde_as_normalized_string() {
        assert_eq!(serde_json::to_string(&s("g05485")).unwrap(), "\"G5485\"");
        assert_eq!(
            serde_json::from_str::<StrongsNumber>("\"h1254a\"").unwrap(),
            s("H1254A")
        );
        assert!(serde_json::from_str::<StrongsNumber>("\"Q1\"").is_err());
    }
}
