use std::ops::Range;

use obs_model::{
    BibleVerse, CrossReference, Entity, Key, KeyEntry, KeyScheme, Manifest, Mention, Resource,
    Span, TextUnit, UnitId,
};
use obs_scripture::{Passage, Verse, VerseMap};

use crate::error::StoreError;

/// Row counts of an open Resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub units: u64,
    pub spans: u64,
    pub verses: u64,
    pub keys: u64,
    pub unit_aliases: u64,
    pub cross_references: u64,
    pub entities: u64,
    pub mentions: u64,
}

/// Read access to one Resource. The engine talks to Resources only through this trait, so the
/// backing store (rusqlite on desktop, sqlite-wasm-rs later) can change.
pub trait ResourceStore {
    fn manifest(&self) -> &Manifest;

    fn summary(&self) -> Result<Summary, StoreError>;

    /// Unit ids in document order.
    fn unit_ids(&self) -> Result<Vec<UnitId>, StoreError>;

    /// One unit with all of its spans, or `None`.
    fn unit(&self, id: &UnitId) -> Result<Option<TextUnit>, StoreError>;

    /// Spans of `unit` that overlap `range` (zero-width spans inside or at its edges count),
    /// in canonical order.
    fn spans_in(&self, unit: &UnitId, range: Range<usize>) -> Result<Vec<Span>, StoreError>;

    /// A Bible Verse in the Bible's own numbering. A verse inside a verse range (`\v 1-2`)
    /// finds the range's entry.
    fn verse(&self, verse: Verse) -> Result<Option<BibleVerse>, StoreError>;

    /// Every Bible Verse, in document order.
    fn verses(&self) -> Result<Vec<BibleVerse>, StoreError>;

    /// Entries for one Key. Headwords and Lemmas match after normalization.
    fn lookup_key(&self, key: &Key) -> Result<Vec<KeyEntry>, StoreError>;

    /// Every Key of one scheme, in key order.
    fn keys(&self, scheme: KeyScheme) -> Result<Vec<KeyEntry>, StoreError>;

    /// Where a retired unit id now lives, if it was aliased.
    fn unit_alias(&self, old: &UnitId) -> Result<Option<UnitId>, StoreError>;

    /// A Bible's Verse map, if it has one.
    fn verse_map(&self) -> Result<Option<VerseMap>, StoreError>;

    /// A Cross-reference set's Cross-references whose source overlaps `passage` (Standard
    /// versification), by source Verse and then by votes, highest first.
    fn cross_references_from(&self, passage: Passage) -> Result<Vec<CrossReference>, StoreError>;

    /// One entity of a People & places index, by id.
    fn entity(&self, id: &str) -> Result<Option<Entity>, StoreError>;

    /// A People & places index's mentions that overlap `passage` (Standard versification).
    fn mentions_in(&self, passage: Passage) -> Result<Vec<Mention>, StoreError>;

    /// The whole Resource, validated, with its content hash checked against the manifest.
    fn load(&self) -> Result<Resource, StoreError>;
}
