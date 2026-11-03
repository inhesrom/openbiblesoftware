//! A Bible opened for reading: its text units, blocks in reading order, and verse list.

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use obs_layout::{BlockSource, unit_blocks};
use obs_model::{BibleVerse, Direction, Manifest, Resource, TextUnit, UnitKind};
use obs_scripture::{ListedVersification, Verse};
#[cfg(not(target_arch = "wasm32"))]
use obs_store::{ObrReader, ResourceStore, StoreError};

/// One block of the document: which unit, and which part of it.
#[derive(Debug, Clone)]
pub struct DocBlock {
    pub unit: usize,
    pub source: BlockSource,
}

/// A Resource's readable text as one continuous document (a whole Bible scrolls across books,
/// #14 rule 7).
#[derive(Debug)]
pub struct ReadingDoc {
    pub manifest: Manifest,
    pub units: Vec<TextUnit>,
    pub blocks: Vec<DocBlock>,
    /// Bible Verses in document order (empty for non-Bibles).
    pub verses: Vec<BibleVerse>,
    /// Index into `units` of each verse.
    verse_units: Vec<usize>,
    /// The Bible's own numbering (references typed for it are read in it, ADR 0002 rule 10).
    pub versification: ListedVersification,
}

impl ReadingDoc {
    /// Open a `.obr` file (native only: the `.obr` reader needs SQLite).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let reader = ObrReader::open(path)?;
        let mut units = Vec::new();
        for id in reader.unit_ids()? {
            if let Some(u) = reader.unit(&id)?
                && u.kind != UnitKind::Note
            {
                units.push(u);
            }
        }
        Ok(Self::build(
            reader.manifest().clone(),
            units,
            reader.verses()?,
        ))
    }

    /// From a Resource in memory (tests, imports in progress).
    pub fn from_resource(resource: Resource) -> Self {
        let units = resource
            .units
            .into_iter()
            .filter(|u| u.kind != UnitKind::Note)
            .collect();
        Self::build(resource.manifest, units, resource.verses)
    }

    fn build(manifest: Manifest, units: Vec<TextUnit>, verses: Vec<BibleVerse>) -> Self {
        let mut blocks = Vec::new();
        for (i, u) in units.iter().enumerate() {
            for source in unit_blocks(u) {
                blocks.push(DocBlock { unit: i, source });
            }
        }
        let verse_units = verses
            .iter()
            .map(|v| units.iter().position(|u| u.id == v.unit).unwrap_or(0))
            .collect();
        let versification = ListedVersification::new(verses.iter().map(|v| (v.verse, v.last)));
        Self {
            manifest,
            units,
            blocks,
            verses,
            verse_units,
            versification,
        }
    }

    pub fn direction(&self) -> Direction {
        self.manifest.direction
    }

    /// The Bible Verse (own numbering) containing `verse`, as (unit index, start byte).
    pub fn verse_position(&self, verse: Verse) -> Option<(usize, usize)> {
        let i = self.verses.iter().position(|v| {
            v.verse.book == verse.book
                && v.verse.chapter == verse.chapter
                && v.verse.verse <= verse.verse
                && verse.verse <= v.last
        })?;
        Some((self.verse_units[i], self.verses[i].start as usize))
    }

    /// The block of `unit` that contains unit byte `byte` (or the next block after it).
    pub fn block_at(&self, unit: usize, byte: usize) -> Option<usize> {
        let first = self.blocks.partition_point(|b| b.unit < unit);
        let end = self.blocks.partition_point(|b| b.unit <= unit);
        let within = &self.blocks[first..end];
        let i = within.partition_point(|b| {
            b.source.range.end <= byte
                && !(b.source.range.is_empty() && b.source.range.start == byte)
        });
        (first + i < end).then_some(first + i)
    }

    /// Verses of `unit` whose start lies in `bytes` (document order).
    pub fn verses_starting_in(&self, unit: usize, bytes: std::ops::Range<usize>) -> Vec<Verse> {
        self.verses
            .iter()
            .zip(&self.verse_units)
            .filter(|(v, u)| **u == unit && bytes.contains(&(v.start as usize)))
            .map(|(v, _)| v.verse)
            .collect()
    }

    /// The verse of `unit` containing unit byte `byte`: the last one starting at or before it.
    pub fn verse_containing(&self, unit: usize, byte: usize) -> Option<Verse> {
        self.verses
            .iter()
            .zip(&self.verse_units)
            .rfind(|(v, u)| **u == unit && v.start as usize <= byte)
            .map(|(v, _)| v.verse)
    }
}
