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
    /// Other names and spellings, e.g. `Abram` for Abraham.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub other_names: Vec<String>,
    /// A person's tribe or nation; a place's geographical area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<EntityRelation>,
}

/// A Passage (Standard versification) that mentions an entity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Mention {
    pub entity: String,
    pub passage: Passage,
}
