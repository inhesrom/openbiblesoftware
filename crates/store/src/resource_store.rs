use std::ops::Range;

use obs_model::{
    BibleVerse, CrossReference, Entity, Key, KeyEntry, KeyScheme, Manifest, Mention, MorphemePart,
    RenderingIndex, Resource, Span, SpanKind, TextUnit, UnitId, UnitKind,
};
use obs_scripture::{Passage, StrongsNumber, Verse, VerseMap};

use crate::error::StoreError;

/// A Bible Verse with the Standard Passage its Verse map gives it: `Some(None)` for an
/// Unmapped verse, `None` when the Bible has no Verse map.
pub type MappedVerse = (BibleVerse, Option<Option<Passage>>);

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

    /// One unit's kind and text, without its spans.
    fn unit_text(&self, id: &UnitId) -> Result<Option<(UnitKind, String)>, StoreError> {
        Ok(self.unit(id)?.map(|u| (u.kind, u.text)))
    }

    /// The Bible Verses of `unit` whose text overlaps `range`, in document order, each with
    /// its Standard Passage from the Verse map: `Some(None)` for an Unmapped verse, `None` when
    /// the Bible has no Verse map.
    fn verses_overlapping(
        &self,
        unit: &UnitId,
        range: Range<usize>,
    ) -> Result<Vec<MappedVerse>, StoreError> {
        let map = self.verse_map()?;
        Ok(self
            .verses()?
            .into_iter()
            .enumerate()
            .filter(|(_, v)| {
                v.unit == *unit && (v.start as usize) < range.end && range.start < v.end as usize
            })
            .map(|(i, v)| {
                let standard = map
                    .as_ref()
                    .map(|m| m.entries().get(i).and_then(|e| e.standard));
                (v, standard)
            })
            .collect())
    }

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

    /// Entries for a Strong's number. An extended number with no entry of its own falls back to
    /// its base number (H1254A → H1254), so a Lexicon keyed by plain Strong's still answers.
    /// Entries for the number itself come before those that only list it as a secondary Key.
    fn lookup_strongs(&self, number: StrongsNumber) -> Result<Vec<KeyEntry>, StoreError> {
        let mut entries = self.lookup_key(&Key::StrongsNumber(number))?;
        if entries.is_empty() && number.is_extended() {
            entries = self.lookup_key(&Key::StrongsNumber(number.base()))?;
        }
        entries.sort_by_key(|e| e.alias);
        Ok(entries)
    }

    /// Lemmas (original script) whose transliteration is `transliteration`, from the
    /// transliteration index. The text is reduced to index form first (words, lowercased,
    /// single spaces), so `"Agape"` finds ἀγάπη.
    fn lemmas_for_transliteration(&self, transliteration: &str) -> Result<Vec<String>, StoreError>;

    /// A Bible's English rendering → Strong's index and Strong's totals (#20 rule 5), as
    /// `ResourceBuilder` built it. `None` when the file predates the index; the caller can
    /// derive it from the units with [`RenderingIndex::build`].
    fn rendering_index(&self) -> Result<Option<RenderingIndex>, StoreError> {
        Ok(None)
    }

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

    /// Every entity of a People & places index, ordered by id.
    fn entities(&self) -> Result<Vec<Entity>, StoreError> {
        Ok(self.load()?.entities)
    }

    /// Call `f` with the parts of every Tagged word in the Resource, in no particular order.
    /// Used to count a Bible's Strong's numbers (Key words' totals, #54).
    fn for_each_tagged_word(&self, f: &mut dyn FnMut(&[MorphemePart])) -> Result<(), StoreError> {
        for id in self.unit_ids()? {
            let Some(unit) = self.unit(&id)? else {
                continue;
            };
            for span in &unit.spans {
                if let SpanKind::TaggedWord { parts } = &span.kind {
                    f(parts);
                }
            }
        }
        Ok(())
    }

    /// Call `f` with the unit, start byte and parts of every Tagged word in the Resource, in
    /// document order within each unit. Used to find where a Strong's number occurs (Word
    /// Study Occurrences and Forms, #56).
    fn for_each_tagged_word_at(
        &self,
        f: &mut dyn FnMut(&UnitId, u32, &[MorphemePart]),
    ) -> Result<(), StoreError> {
        for id in self.unit_ids()? {
            let Some(unit) = self.unit(&id)? else {
                continue;
            };
            for span in &unit.spans {
                if let SpanKind::TaggedWord { parts } = &span.kind {
                    f(&id, span.start, parts);
                }
            }
        }
        Ok(())
    }

    /// The whole Resource, validated, with its content hash checked against the manifest.
    fn load(&self) -> Result<Resource, StoreError>;
}
