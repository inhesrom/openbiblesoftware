//! A Bible opened for reading: its text units, blocks in reading order, and verse list.

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use obs_layout::{BlockSource, unit_blocks};
use obs_model::{
    BibleVerse, Direction, Manifest, ParaStyle, Resource, Span, SpanKind, TextUnit, UnitId,
    UnitKind,
};
use obs_scripture::{ListedVersification, Verse, VerseMap};
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
    /// Index into `verses` of each Bible Verse, by its first verse.
    verse_index: std::collections::BTreeMap<Verse, usize>,
    /// The Bible's own numbering (references typed for it are read in it, ADR 0002 rule 10).
    pub versification: ListedVersification,
    /// How its numbering corresponds to the Standard versification (none: identical).
    pub verse_map: Option<std::sync::Arc<VerseMap>>,
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
        let mut doc = Self::build(reader.manifest().clone(), units, reader.verses()?);
        doc.verse_map = reader.verse_map()?.map(std::sync::Arc::new);
        Ok(doc)
    }

    /// From a Resource in memory (tests, imports in progress).
    pub fn from_resource(resource: Resource) -> Self {
        let units = resource
            .units
            .into_iter()
            .filter(|u| u.kind != UnitKind::Note)
            .collect();
        let mut doc = Self::build(resource.manifest, units, resource.verses);
        doc.verse_map = resource.verse_map.map(std::sync::Arc::new);
        doc
    }

    /// Plain text shown through the text pipeline, such as a Resource's Attribution in its
    /// About Tab: one left-to-right unit, one paragraph per line, the text kept verbatim.
    pub fn from_text(mut manifest: Manifest, text: &str) -> Self {
        manifest.direction = Direction::Ltr;
        let mut spans = Vec::new();
        let mut start = 0usize;
        for line in text.split_inclusive('\n') {
            let end = start + line.trim_end_matches(['\n', '\r']).len();
            if end > start {
                spans.push(Span {
                    start: u32::try_from(start).unwrap_or(u32::MAX),
                    end: u32::try_from(end).unwrap_or(u32::MAX),
                    kind: SpanKind::Para {
                        style: ParaStyle::Prose,
                    },
                });
            }
            start += line.len();
        }
        let unit = TextUnit {
            id: UnitId::new("text").unwrap_or_else(|_| UnitId::note(0)),
            kind: UnitKind::Division,
            title: None,
            text: text.to_string(),
            spans,
        };
        Self::build(manifest, vec![unit], Vec::new())
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
        let verse_index = verses
            .iter()
            .enumerate()
            .map(|(i, v)| (v.verse, i))
            .collect();
        Self {
            manifest,
            units,
            blocks,
            verses,
            verse_units,
            verse_index,
            versification,
            verse_map: None,
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

    /// The Bible Verse (own numbering) containing `verse`: it, or a verse range (`\v 1-2`)
    /// starting before it in the same chapter.
    pub fn bible_verse_at(&self, verse: Verse) -> Option<&BibleVerse> {
        let (_, &i) = self.verse_index.range(..=verse).next_back()?;
        let v = self.verses.get(i)?;
        (v.verse.book == verse.book && v.verse.chapter == verse.chapter && verse.verse <= v.last)
            .then_some(v)
    }

    /// The text of the Bible Verse starting at `verse`, as written.
    pub fn verse_text(&self, verse: Verse) -> Option<&str> {
        let &i = self.verse_index.get(&verse)?;
        let v = self.verses.get(i)?;
        self.units.get(self.verse_units[i])?.text.get(v.range())
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

    /// Unit bytes of the Bible Verse at `byte` of `unit`.
    pub fn verse_range_at(&self, unit: usize, byte: usize) -> Option<std::ops::Range<usize>> {
        self.verses
            .iter()
            .zip(&self.verse_units)
            .filter(|(v, u)| **u == unit && v.start as usize <= byte)
            .map(|(v, _)| v.start as usize..v.end as usize)
            .next_back()
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
