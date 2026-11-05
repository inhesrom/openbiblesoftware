//! A Resource opened for reading: text units, blocks, Bible Verses and non-Bible Keys.

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use obs_engine::resource_navigation::ResourceNavigation;
use std::ops::Range;
use std::sync::OnceLock;

use obs_layout::{BlockSource, SpanIndex, unit_blocks};
use obs_model::{
    BibleVerse, Direction, KeyEntry, Manifest, MorphemePart, ParaStyle, Resource, Span, SpanKind,
    TextUnit, UnitId, UnitKind,
};
use obs_scripture::{ListedVersification, Passage, Verse, VerseMap};
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
    pub navigation: std::sync::Arc<ResourceNavigation>,
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
    ///
    /// Set it only while building the document (as `open` and `from_resource` do), before
    /// anything looks up Verses by Standard Passage: the first such lookup
    /// (`verses_for_standard`, and so `marks_for`) caches an index built from this map in
    /// `standard_index`, and changing the map afterwards leaves that index stale.
    pub verse_map: Option<std::sync::Arc<VerseMap>>,
    /// Each unit's interactive spans, indexed on first hover.
    span_indexes: Vec<OnceLock<SpanIndex>>,
    /// The Standard Passage of each Verse, built on first use (Annotations in this Bible).
    standard_index: OnceLock<StandardIndex>,
}

/// Bible Verses by the Standard Passage they map to (ADR 0002), for finding where a Passage
/// anchor shows in this Bible: entries sorted by Standard start, with the running maximum of
/// their ends so a lookup can binary-search both sides.
#[derive(Debug, Default)]
struct StandardIndex {
    /// (Standard Passage, index into `verses`), by Passage start.
    entries: Vec<(Passage, usize)>,
    /// `max_end[i]`: the latest end among `entries[..=i]`.
    max_end: Vec<Verse>,
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
        let mut keys = Vec::new();
        for scheme in &reader.manifest().key_schemes {
            keys.extend(reader.keys(*scheme)?);
        }
        let mut doc = Self::build(reader.manifest().clone(), units, reader.verses()?, keys);
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
        let mut doc = Self::build(resource.manifest, units, resource.verses, resource.keys);
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
        Self::build(manifest, vec![unit], Vec::new(), Vec::new())
    }

    fn build(
        manifest: Manifest,
        units: Vec<TextUnit>,
        verses: Vec<BibleVerse>,
        keys: Vec<KeyEntry>,
    ) -> Self {
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
        let span_indexes = units.iter().map(|_| OnceLock::new()).collect();
        Self {
            manifest,
            navigation: std::sync::Arc::new(ResourceNavigation::new(keys)),
            units,
            blocks,
            verses,
            verse_units,
            verse_index,
            versification,
            verse_map: None,
            span_indexes,
            standard_index: OnceLock::new(),
        }
    }

    fn standard_index(&self) -> &StandardIndex {
        self.standard_index.get_or_init(|| {
            let mapped: Option<std::collections::HashMap<Verse, Option<Passage>>> = self
                .verse_map
                .as_ref()
                .map(|m| m.entries().iter().map(|e| (e.verse, e.standard)).collect());
            let mut entries: Vec<(Passage, usize)> = self
                .verses
                .iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    let standard = match &mapped {
                        // An Unmapped verse, or one the map does not know, has no Passage.
                        Some(map) => map.get(&v.verse).copied().flatten(),
                        // Without a Verse map, the Bible numbers like the Standard.
                        None => Passage::new(
                            v.verse,
                            Verse::new(v.verse.book, v.verse.chapter, v.last.max(v.verse.verse)),
                        )
                        .ok(),
                    };
                    standard.map(|p| (p, i))
                })
                .collect();
            entries.sort_by_key(|(p, i)| (p.start(), *i));
            let mut max_end = Vec::with_capacity(entries.len());
            let mut latest: Option<Verse> = None;
            for (p, _) in &entries {
                let end = latest.map_or(p.end(), |l| l.max(p.end()));
                latest = Some(end);
                max_end.push(end);
            }
            StandardIndex { entries, max_end }
        })
    }

    /// The Bible Verses (indexes into `verses`, document order) that correspond to any part
    /// of a Standard Passage, through the Verse map (ADR 0002). Empty for non-Bibles and for
    /// Passages this Bible lacks (Absent verses).
    pub fn verses_for_standard(&self, passage: Passage) -> Vec<usize> {
        let index = self.standard_index();
        // Entries that can overlap: running max end reaches the start, and start ≤ end.
        let lo = index.max_end.partition_point(|e| *e < passage.start());
        let hi = index
            .entries
            .partition_point(|(p, _)| p.start() <= passage.end());
        let mut found: Vec<usize> = index
            .entries
            .get(lo..hi)
            .unwrap_or_default()
            .iter()
            .filter(|(p, _)| p.end() >= passage.start())
            .map(|(_, i)| *i)
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }

    /// Index into `units` of Bible Verse `i` (an index into `verses`).
    pub fn verse_unit(&self, i: usize) -> Option<usize> {
        self.verse_units.get(i).copied()
    }

    /// The innermost Tagged word at unit byte `byte`: its bytes and its morphemes.
    pub fn tagged_word_at(
        &self,
        unit: usize,
        byte: usize,
    ) -> Option<(Range<usize>, &[MorphemePart])> {
        let u = self.units.get(unit)?;
        let index = self
            .span_indexes
            .get(unit)?
            .get_or_init(|| SpanIndex::new(u));
        index.at(u, byte).into_iter().find_map(|s| match &s.kind {
            SpanKind::TaggedWord { parts } if !parts.is_empty() => {
                Some((s.start as usize..s.end as usize, parts.as_slice()))
            }
            _ => None,
        })
    }

    pub fn entry_position(&self, entry: &KeyEntry) -> Option<(usize, usize)> {
        Some((
            self.units.iter().position(|u| u.id == entry.unit)?,
            entry.range.map_or(0, |r| r.0 as usize),
        ))
    }

    pub fn direction(&self) -> Direction {
        self.manifest.direction
    }

    /// The Bible Verse (own numbering) containing `verse`, as (unit index, start byte).
    pub fn verse_position(&self, verse: Verse) -> Option<(usize, usize)> {
        if self.manifest.resource_type == obs_model::ResourceType::Commentary {
            return self.entry_position(self.navigation.section(verse)?);
        }
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
        if self.manifest.resource_type == obs_model::ResourceType::Commentary {
            let verse = self.verse_containing(unit, byte)?;
            let entry = self.navigation.section(verse)?;
            return Some(
                entry
                    .range
                    .map_or(0..self.units.get(unit)?.text.len(), |(a, b)| {
                        a as usize..b as usize
                    }),
            );
        }
        self.verses
            .iter()
            .zip(&self.verse_units)
            .filter(|(v, u)| **u == unit && v.start as usize <= byte)
            .map(|(v, _)| v.start as usize..v.end as usize)
            .next_back()
    }

    /// The verse of `unit` containing unit byte `byte`: the last one starting at or before it.
    pub fn verse_containing(&self, unit: usize, byte: usize) -> Option<Verse> {
        if self.manifest.resource_type == obs_model::ResourceType::Commentary {
            return self.navigation.passage_at(&self.units.get(unit)?.id, byte);
        }
        self.verses
            .iter()
            .zip(&self.verse_units)
            .rfind(|(v, u)| **u == unit && v.start as usize <= byte)
            .map(|(v, _)| v.verse)
    }
}
