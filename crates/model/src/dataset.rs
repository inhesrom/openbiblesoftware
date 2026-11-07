//! Dataset records: the typed tables of a Cross-reference set and a People & places index
//! (#11 rule 10). Neither Resource type stores text units; both speak the Standard
//! versification.

use obs_scripture::{Passage, StrongsNumber};
use serde::{Deserialize, Serialize};

/// One Cross-reference: a link from one Passage to a related Passage, both in the Standard
/// versification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CrossReference {
    pub from: Passage,
    pub to: Passage,
    /// Net votes from the source's users (OpenBible.info); higher ranks first. May be
    /// negative.
    pub votes: i32,
}

/// What a person or place in a People & places index is (TIPNR's `Type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Male,
    Female,
    /// A people, tribe or clan.
    Group,
    Place,
}

/// What the related entity is to the entity that lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Father,
    Mother,
    /// Listed as a parent but an ancestor, or the forebear a group descends from.
    Ancestor,
    Sibling,
    Partner,
    Offspring,
    /// A place or group this entity founded.
    Founded,
    /// Who founded (or rebuilt) this place.
    Founder,
    /// Someone who lived in this place.
    Inhabitant,
}

/// A link from one entity to another, by entity id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EntityRelation {
    pub relation: Relation,
    pub entity: String,
    /// The source marks the link as a decision on an ambiguous text.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub uncertain: bool,
}

/// One person, group or place in a People & places index. Only structured facts are kept:
/// names, kind, Strong's number, region and relations. Descriptive prose is never stored.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Entity {
    /// Stable id from the source, e.g. TIPNR's unique name `Aaron@Exo.4.14-Heb`.
    pub id: String,
    /// The usual English name, e.g. `Aaron`.
    pub name: String,
    pub kind: EntityKind,
    /// The Strong's number that identifies this individual (TIPNR's unified Strong's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strongs: Option<StrongsNumber>,
    /// The Strong's numbers of its other names and languages (TIPNR's dStrongs other than
    /// [`Entity::strongs`]): Abraham's are `H87` (Abram) and `G11` (Greek).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_strongs: Vec<StrongsNumber>,
    /// Other names and spellings, e.g. `Abram` for Abraham.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_names: Vec<String>,
    /// A person's tribe or nation; a place's geographical area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<EntityRelation>,
    /// A place's Place locations, most confident first (#150): where modern scholarship
    /// puts it. Empty for people, and for places an index does not locate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<PlaceLocation>,
}

/// One modern location a place has been identified with, and how confident that
/// identification is (OpenBible.info's geocoding, #150).
///
/// Coordinates are whole millionths of a degree, so the record stays exact, hashable and
/// ordered.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlaceLocation {
    /// The modern location's name, e.g. `Antioch in Pisidia` or `Konya`.
    pub name: String,
    /// Longitude in millionths of a degree (east positive).
    pub lon_e6: i32,
    /// Latitude in millionths of a degree (north positive).
    pub lat_e6: i32,
    /// Confidence in the identification, 0 to [`PlaceLocation::MAX_SCORE`] (OpenBible.info's
    /// score; 500 and above is a high-confidence identification).
    pub score: u16,
}

impl PlaceLocation {
    /// The highest [`PlaceLocation::score`].
    pub const MAX_SCORE: u16 = 1000;

    /// Longitude in degrees.
    pub fn lon(&self) -> f64 {
        f64::from(self.lon_e6) / 1e6
    }

    /// Latitude in degrees.
    pub fn lat(&self) -> f64 {
        f64::from(self.lat_e6) / 1e6
    }
}

/// A Passage (Standard versification) that mentions an entity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Mention {
    pub entity: String,
    pub passage: Passage,
}

/// What one record of a Timeline's Events dataset is (#185).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A period of the biblical story (the Patriarchs, the Exile), drawn as a band.
    Era,
    /// One person's life, from birth to death.
    Lifespan,
    /// Something that happened, on a day or a year, or over years (a reign, a journey).
    Event,
}

/// A date in history: an astronomical year (1 BC is `0`, 2 BC is `-1`, AD 30 is `30`), with
/// the month and day when the source fixes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HistoricalDate {
    pub year: i32,
    /// 1 to 12.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month: Option<u8>,
    /// 1 to 31; only with a month.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<u8>,
}

impl HistoricalDate {
    pub fn year(year: i32) -> Self {
        Self {
            year,
            month: None,
            day: None,
        }
    }

    /// A year written the usual way: `1012 BC` is `-1011`, `AD 30` is `30`.
    pub fn bc(year: i32) -> Self {
        Self::year(1 - year)
    }
}

/// A person or place an Event names: its People & places id when an index knows it, and the
/// name to show.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EventEntity {
    /// The entity's id in a People & places index (TIPNR's unique name), so a click opens its
    /// Factbook; `None` when no index has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    pub name: String,
}

/// One record of a Timeline's Events dataset, carried by a People & places index (ADR 0011):
/// an Era, a Lifespan or an Event, with its dates, the people and places it names and the
/// Passages that tell it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Event {
    /// Stable id within the Resource: `event:162`, `lifespan:david_994`, `era:exile`.
    pub id: String,
    /// `Construction of Solomon's Temple`; a Lifespan's is the person's name.
    pub title: String,
    pub kind: EventKind,
    pub start: HistoricalDate,
    /// When it ended, for one that lasts (a reign, a life, an era); `None` for one on a single
    /// day or within one year. An Era's end is the date the next Era begins, not its own last
    /// day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<HistoricalDate>,
    /// The source marks the date as approximate ("c. 2242 BC").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub approximate: bool,
    /// The id of the longer Event (or Lifespan) this one is part of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part_of: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<EventEntity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub places: Vec<EventEntity>,
    /// The Passages (Standard versification) that tell it, in Bible order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passages: Vec<Passage>,
}
