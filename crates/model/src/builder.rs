//! `ResourceBuilder`: the in-memory Resource every importer and the Starter build write into,
//! plus validation and the content hash (#11 rule 14).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::ops::Range;

use obs_scripture::{Passage, Standard, Verse, VerseMap, Versification};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::dataset::{CrossReference, Entity, Mention};
use crate::key::{Key, KeyEntry};
use crate::manifest::{FORMAT_VERSION, Manifest, ResourceType, is_valid_resource_id};
use crate::unit::{BibleVerse, Span, SpanKind, TextUnit, UnitId, UnitKind};

/// A validated Resource: manifest (with content hash), text units with canonically ordered
/// spans, the Bible verse list in document order, Keys, and the dataset records of a
/// Cross-reference set or People & places index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub manifest: Manifest,
    pub units: Vec<TextUnit>,
    pub verses: Vec<BibleVerse>,
    pub keys: Vec<KeyEntry>,
    /// Source markup that had no place in the span vocabulary, by marker, with counts.
    /// Reported in the Import report; not part of the content hash.
    pub dropped_markup: BTreeMap<String, u64>,
    /// Retired unit id → its replacement, followed by re-anchoring before searching by quote
    /// (#20 rule 2). Not part of the content hash.
    pub unit_aliases: BTreeMap<UnitId, UnitId>,
    /// A Bible's Verse map to the Standard versification (ADR 0002), built at Import. Derived
    /// from the verse list, so not part of the content hash.
    pub verse_map: Option<VerseMap>,
    /// A Cross-reference set's Cross-references, in canonical order.
    pub cross_references: Vec<CrossReference>,
    /// A People & places index's entities, ordered by id.
    pub entities: Vec<Entity>,
    /// A People & places index's mentions, ordered by entity, then Passage.
    pub mentions: Vec<Mention>,
}

/// One problem found while validating a Resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    InvalidResourceId(String),
    EmptyManifestField(&'static str),
    UnsupportedFormatVersion(u32),
    DuplicateUnit(UnitId),
    TextNotNfc(UnitId),
    UnknownUnit {
        what: &'static str,
        unit: UnitId,
    },
    SpanReversed {
        unit: UnitId,
        kind: &'static str,
        range: Range<usize>,
    },
    /// The span reaches past the end of its unit's text.
    SpanOutsideUnit {
        unit: UnitId,
        kind: &'static str,
        range: Range<usize>,
        len: usize,
    },
    SpanNotOnCharBoundary {
        unit: UnitId,
        kind: &'static str,
        offset: usize,
    },
    NoteRefNotZeroWidth {
        unit: UnitId,
        range: Range<usize>,
    },
    NoteTargetNotANote {
        unit: UnitId,
        note: UnitId,
    },
    TaggedWordWithoutParts {
        unit: UnitId,
        range: Range<usize>,
    },
    VersesInNonBible,
    VerseInWrongUnit {
        verse: Verse,
        unit: UnitId,
    },
    DuplicateVerse(Verse),
    VerseRangeReversed(Verse),
    /// Verses must be listed in document order and must not overlap.
    VerseOutOfOrder(Verse),
    /// A `verse` span with no matching verse-list entry, or the reverse.
    VerseSpanMismatch {
        verse: Verse,
        unit: UnitId,
    },
    KeySchemeNotDeclared(Key),
    /// A unit alias whose old id is still a live unit.
    AliasShadowsUnit(UnitId),
    VerseMapMismatch(String),
    /// Dataset records (`what`) in a Resource whose type does not hold them.
    DatasetInWrongType {
        what: &'static str,
        resource_type: ResourceType,
    },
    /// A dataset Passage that is not a valid reference in the Standard versification.
    NotStandardPassage {
        what: &'static str,
        passage: Passage,
    },
    DuplicateEntity(String),
    EmptyEntityField {
        entity: String,
        field: &'static str,
    },
    UnknownEntity {
        what: &'static str,
        entity: String,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ValidationError::*;
        match self {
            InvalidResourceId(id) => write!(
                f,
                "resource_id `{id}` must be 1-64 chars of a-z, 0-9 and `-`"
            ),
            EmptyManifestField(name) => write!(f, "manifest field `{name}` is empty"),
            UnsupportedFormatVersion(v) => write!(
                f,
                "format_version {v} is not {FORMAT_VERSION}, the version this build writes"
            ),
            DuplicateUnit(id) => write!(f, "text unit `{id}` is defined twice"),
            TextNotNfc(id) => write!(f, "text of unit `{id}` is not NFC-normalized"),
            UnknownUnit { what, unit } => write!(f, "{what} refers to unknown unit `{unit}`"),
            SpanReversed { unit, kind, range } => write!(
                f,
                "`{kind}` span {}..{} in `{unit}` ends before it starts",
                range.start, range.end
            ),
            SpanOutsideUnit {
                unit,
                kind,
                range,
                len,
            } => write!(
                f,
                "`{kind}` span {}..{} crosses the end of unit `{unit}` ({len} bytes)",
                range.start, range.end
            ),
            SpanNotOnCharBoundary { unit, kind, offset } => write!(
                f,
                "`{kind}` span in `{unit}` has offset {offset}, which is inside a UTF-8 character"
            ),
            NoteRefNotZeroWidth { unit, range } => write!(
                f,
                "`note_ref` span {}..{} in `{unit}` must be zero-width",
                range.start, range.end
            ),
            NoteTargetNotANote { unit, note } => {
                write!(
                    f,
                    "`note_ref` in `{unit}` points at `{note}`, which is not a note unit"
                )
            }
            TaggedWordWithoutParts { unit, range } => write!(
                f,
                "`tagged_word` span {}..{} in `{unit}` has no morpheme parts",
                range.start, range.end
            ),
            VersesInNonBible => f.write_str("only a Bible may have a verse list"),
            VerseInWrongUnit { verse, unit } => {
                write!(f, "verse {verse} is in unit `{unit}`, not its book's unit")
            }
            DuplicateVerse(v) => write!(f, "verse {v} is listed twice"),
            VerseRangeReversed(v) => write!(f, "verse {v} has a range end before its start"),
            VerseOutOfOrder(v) => write!(
                f,
                "verse {v} overlaps or precedes the previous verse in its unit"
            ),
            VerseSpanMismatch { verse, unit } => write!(
                f,
                "verse {verse} in `{unit}` has a verse-list entry and a `verse` span that disagree"
            ),
            VerseMapMismatch(why) => write!(f, "verse map: {why}"),
            AliasShadowsUnit(id) => {
                write!(f, "unit alias `{id}` is also a live unit id")
            }
            KeySchemeNotDeclared(key) => write!(
                f,
                "key `{key}` uses scheme {:?}, which the manifest does not declare",
                key.scheme()
            ),
            DatasetInWrongType {
                what,
                resource_type,
            } => write!(f, "a {resource_type:?} may not hold {what} records"),
            NotStandardPassage { what, passage } => write!(
                f,
                "{what} Passage {passage} is not a valid Standard versification reference"
            ),
            DuplicateEntity(id) => write!(f, "entity `{id}` is defined twice"),
            EmptyEntityField { entity, field } => {
                write!(f, "entity `{entity}` has an empty {field}")
            }
            UnknownEntity { what, entity } => {
                write!(f, "{what} refers to unknown entity `{entity}`")
            }
        }
    }
}

/// Every problem found in one validation pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationErrors(pub Vec<ValidationError>);

impl fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} validation error(s)", self.0.len())?;
        for e in self.0.iter().take(20) {
            write!(f, "\n  - {e}")?;
        }
        if self.0.len() > 20 {
            write!(f, "\n  - …and {} more", self.0.len() - 20)?;
        }
        Ok(())
    }
}

impl std::error::Error for ValidationErrors {}

/// Collects a Resource in any order, then validates, orders and hashes it in [`build`].
///
/// [`build`]: ResourceBuilder::build
#[derive(Debug, Clone)]
pub struct ResourceBuilder {
    manifest: Manifest,
    units: Vec<TextUnit>,
    spans: Vec<(UnitId, Span)>,
    verses: Vec<BibleVerse>,
    keys: Vec<KeyEntry>,
    dropped: BTreeMap<String, u64>,
    aliases: BTreeMap<UnitId, UnitId>,
    verse_map: Option<VerseMap>,
    cross_references: Vec<CrossReference>,
    entities: Vec<Entity>,
    mentions: Vec<Mention>,
}

fn offset(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl ResourceBuilder {
    pub fn new(manifest: Manifest) -> Self {
        Self {
            manifest,
            units: Vec::new(),
            spans: Vec::new(),
            verses: Vec::new(),
            keys: Vec::new(),
            dropped: BTreeMap::new(),
            aliases: BTreeMap::new(),
            verse_map: None,
            cross_references: Vec::new(),
            entities: Vec::new(),
            mentions: Vec::new(),
        }
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn manifest_mut(&mut self) -> &mut Manifest {
        &mut self.manifest
    }

    /// Add a text unit. Units keep the order they are added in (document order).
    pub fn add_unit(&mut self, id: UnitId, kind: UnitKind, title: Option<String>, text: String) {
        self.units.push(TextUnit {
            id,
            kind,
            title,
            text,
            spans: Vec::new(),
        });
    }

    /// Add a span over `unit`'s text. The unit may be added before or after.
    pub fn add_span(&mut self, unit: &UnitId, range: Range<usize>, kind: SpanKind) {
        self.spans.push((
            unit.clone(),
            Span {
                start: offset(range.start),
                end: offset(range.end),
                kind,
            },
        ));
    }

    /// Add one Bible Verse (own numbering) to the verse list and its `verse` span. Call in
    /// document order. `last` > `verse.verse` for a verse range such as `\v 1-2`.
    pub fn add_verse(&mut self, unit: &UnitId, range: Range<usize>, verse: Verse, last: u16) {
        self.add_span(unit, range.clone(), SpanKind::Verse { verse, last });
        self.verses.push(BibleVerse {
            verse,
            last,
            unit: unit.clone(),
            start: offset(range.start),
            end: offset(range.end),
        });
    }

    /// Add a Key opening at `unit` (or a byte range in it).
    pub fn add_key(&mut self, key: Key, unit: &UnitId, range: Option<Range<usize>>, alias: bool) {
        self.keys.push(KeyEntry {
            key,
            unit: unit.clone(),
            range: range.map(|r| (offset(r.start), offset(r.end))),
            alias,
        });
    }

    /// Count one piece of source markup that was dropped (e.g. `\fig`).
    /// Set the Bible's Verse map; it must have one entry per verse, in verse-list order.
    pub fn set_verse_map(&mut self, map: VerseMap) {
        self.verse_map = Some(map);
    }

    /// The verse list added so far, in document order.
    pub fn verses(&self) -> &[BibleVerse] {
        &self.verses
    }

    /// Record that unit id `old` (from an earlier revision) now lives at `new`.
    pub fn add_unit_alias(&mut self, old: UnitId, new: UnitId) {
        self.aliases.insert(old, new);
    }

    /// Add one Cross-reference (Cross-reference set). Exact duplicates are merged.
    pub fn add_cross_reference(&mut self, xref: CrossReference) {
        self.cross_references.push(xref);
    }

    /// Add one person, group or place (People & places index).
    pub fn add_entity(&mut self, entity: Entity) {
        self.entities.push(entity);
    }

    /// Record that `passage` mentions entity `entity`. Duplicates are merged.
    pub fn add_mention(&mut self, entity: &str, passage: Passage) {
        self.mentions.push(Mention {
            entity: entity.to_string(),
            passage,
        });
    }

    pub fn count_dropped(&mut self, marker: &str) {
        *self.dropped.entry(marker.to_string()).or_insert(0) += 1;
    }

    pub fn unit_count(&self) -> usize {
        self.units.len()
    }

    pub fn verse_count(&self) -> usize {
        self.verses.len()
    }

    /// Validate, put spans and keys in canonical order, and fill in the content hash.
    pub fn build(self) -> Result<Resource, ValidationErrors> {
        let mut errors = Vec::new();
        let mut units = self.units;
        let index: HashMap<UnitId, usize> = units
            .iter()
            .enumerate()
            .map(|(i, u)| (u.id.clone(), i))
            .collect();
        for (unit, span) in self.spans {
            match index.get(&unit) {
                Some(&i) => units[i].spans.push(span),
                None => errors.push(ValidationError::UnknownUnit {
                    what: span.kind.name(),
                    unit,
                }),
            }
        }
        for unit in &mut units {
            unit.spans.sort_by(compare_spans);
        }
        let mut keys = self.keys;
        keys.sort();
        keys.dedup();
        let mut cross_references = self.cross_references;
        cross_references.sort();
        cross_references.dedup();
        let mut entities = self.entities;
        entities.sort();
        let mut mentions = self.mentions;
        mentions.sort();
        mentions.dedup();
        let mut resource = Resource {
            manifest: self.manifest,
            units,
            verses: self.verses,
            keys,
            dropped_markup: self.dropped,
            unit_aliases: self.aliases,
            verse_map: self.verse_map,
            cross_references,
            entities,
            mentions,
        };
        if let Err(more) = resource.validate() {
            errors.extend(more.0);
        }
        if !errors.is_empty() {
            return Err(ValidationErrors(errors));
        }
        resource.manifest.content_hash = resource.content_hash();
        Ok(resource)
    }
}

/// Canonical span order: by start, then longer first, then kind, then attributes.
fn compare_spans(a: &Span, b: &Span) -> std::cmp::Ordering {
    a.start
        .cmp(&b.start)
        .then(b.end.cmp(&a.end))
        .then(a.kind.rank().cmp(&b.kind.rank()))
        .then_with(|| {
            let ja = serde_json::to_string(&a.kind).unwrap_or_default();
            let jb = serde_json::to_string(&b.kind).unwrap_or_default();
            ja.cmp(&jb)
        })
}

#[derive(Serialize)]
struct HashInput<'a> {
    format_version: u32,
    units: &'a [TextUnit],
    verses: &'a [BibleVerse],
    keys: &'a [KeyEntry],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    cross_references: &'a [CrossReference],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    entities: &'a [Entity],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    mentions: &'a [Mention],
}

impl Resource {
    /// `sha256:<hex>` over the content: units with their spans, the verse list, Keys and
    /// dataset records.
    /// Manifest metadata is excluded, so fixing a title does not orphan anchors.
    pub fn content_hash(&self) -> String {
        let input = HashInput {
            format_version: self.manifest.format_version,
            units: &self.units,
            verses: &self.verses,
            keys: &self.keys,
            cross_references: &self.cross_references,
            entities: &self.entities,
            mentions: &self.mentions,
        };
        let mut hasher = Sha256::new();
        // Serializing plain data into a Vec cannot fail; an empty hash input would still be
        // deterministic if it somehow did.
        let bytes = serde_json::to_vec(&input).unwrap_or_default();
        hasher.update(&bytes);
        let digest = hasher.finalize();
        let mut out = String::with_capacity(7 + 64);
        out.push_str("sha256:");
        for b in digest {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }

    pub fn unit(&self, id: &UnitId) -> Option<&TextUnit> {
        self.units.iter().find(|u| &u.id == id)
    }

    /// Check every invariant; used by the builder and by readers of untrusted files.
    pub fn validate(&self) -> Result<(), ValidationErrors> {
        let mut errors = Vec::new();
        self.validate_manifest(&mut errors);
        let units = self.validate_units(&mut errors);
        self.validate_verses(&units, &mut errors);
        self.validate_keys(&units, &mut errors);
        self.validate_datasets(&mut errors);
        if let Some(map) = &self.verse_map {
            if self.manifest.resource_type != ResourceType::Bible {
                errors.push(ValidationError::VerseMapMismatch(
                    "only a Bible may have a verse map".into(),
                ));
            } else if map.entries().len() != self.verses.len()
                || map
                    .entries()
                    .iter()
                    .zip(&self.verses)
                    .any(|(e, v)| e.verse != v.verse || e.last != v.last)
            {
                errors.push(ValidationError::VerseMapMismatch(
                    "verse map entries do not match the verse list".into(),
                ));
            }
        }
        for (old, new) in &self.unit_aliases {
            if units.contains_key(old) {
                errors.push(ValidationError::AliasShadowsUnit(old.clone()));
            }
            if !units.contains_key(new) {
                errors.push(ValidationError::UnknownUnit {
                    what: "unit_alias",
                    unit: new.clone(),
                });
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(ValidationErrors(errors))
        }
    }

    fn validate_manifest(&self, errors: &mut Vec<ValidationError>) {
        let m = &self.manifest;
        if !is_valid_resource_id(&m.resource_id) {
            errors.push(ValidationError::InvalidResourceId(m.resource_id.clone()));
        }
        for (name, value) in [
            ("title", &m.title),
            ("abbreviation", &m.abbreviation),
            ("language", &m.language),
            ("license", &m.license),
        ] {
            if value.trim().is_empty() {
                errors.push(ValidationError::EmptyManifestField(name));
            }
        }
        if m.format_version != FORMAT_VERSION {
            errors.push(ValidationError::UnsupportedFormatVersion(m.format_version));
        }
    }

    fn validate_units<'a>(
        &'a self,
        errors: &mut Vec<ValidationError>,
    ) -> HashMap<&'a UnitId, &'a TextUnit> {
        let mut by_id = HashMap::new();
        for unit in &self.units {
            if by_id.insert(&unit.id, unit).is_some() {
                errors.push(ValidationError::DuplicateUnit(unit.id.clone()));
            }
            if !unicode_normalization::is_nfc(&unit.text) {
                errors.push(ValidationError::TextNotNfc(unit.id.clone()));
            }
        }
        for unit in &self.units {
            for span in &unit.spans {
                let kind = span.kind.name();
                if !check_range(&unit.id, &unit.text, kind, span.range(), errors) {
                    continue;
                }
                match &span.kind {
                    SpanKind::NoteRef { note, .. } => {
                        if span.start != span.end {
                            errors.push(ValidationError::NoteRefNotZeroWidth {
                                unit: unit.id.clone(),
                                range: span.range(),
                            });
                        }
                        match by_id.get(note) {
                            None => errors.push(ValidationError::UnknownUnit {
                                what: "note_ref",
                                unit: note.clone(),
                            }),
                            Some(target) if target.kind != UnitKind::Note => {
                                errors.push(ValidationError::NoteTargetNotANote {
                                    unit: unit.id.clone(),
                                    note: note.clone(),
                                });
                            }
                            Some(_) => {}
                        }
                    }
                    SpanKind::TaggedWord { parts } if parts.is_empty() => {
                        errors.push(ValidationError::TaggedWordWithoutParts {
                            unit: unit.id.clone(),
                            range: span.range(),
                        });
                    }
                    _ => {}
                }
            }
        }
        by_id
    }

    fn validate_verses(
        &self,
        units: &HashMap<&UnitId, &TextUnit>,
        errors: &mut Vec<ValidationError>,
    ) {
        if !self.verses.is_empty() && self.manifest.resource_type != ResourceType::Bible {
            errors.push(ValidationError::VersesInNonBible);
            return;
        }
        let unit_order: HashMap<&UnitId, usize> = self
            .units
            .iter()
            .enumerate()
            .map(|(i, u)| (&u.id, i))
            .collect();
        let mut seen = HashSet::new();
        let mut listed = HashSet::new();
        let mut prev: Option<(usize, u32)> = None;
        for v in &self.verses {
            let Some(unit) = units.get(&v.unit) else {
                errors.push(ValidationError::UnknownUnit {
                    what: "verse",
                    unit: v.unit.clone(),
                });
                continue;
            };
            if unit.kind != UnitKind::BibleBook || unit.id != UnitId::book(v.verse.book) {
                errors.push(ValidationError::VerseInWrongUnit {
                    verse: v.verse,
                    unit: v.unit.clone(),
                });
            }
            if !seen.insert(v.verse) {
                errors.push(ValidationError::DuplicateVerse(v.verse));
            }
            if v.last < v.verse.verse {
                errors.push(ValidationError::VerseRangeReversed(v.verse));
            }
            check_range(&v.unit, &unit.text, "verse", v.range(), errors);
            let order = unit_order.get(&v.unit).copied().unwrap_or(0);
            if let Some((prev_unit, prev_end)) = prev
                && (order < prev_unit || (order == prev_unit && v.start < prev_end))
            {
                errors.push(ValidationError::VerseOutOfOrder(v.verse));
            }
            prev = Some((order, v.end));
            listed.insert((&v.unit, v.verse, v.last, v.start, v.end));
        }
        for unit in &self.units {
            for span in &unit.spans {
                if let SpanKind::Verse { verse, last } = span.kind
                    && !listed.remove(&(&unit.id, verse, last, span.start, span.end))
                {
                    errors.push(ValidationError::VerseSpanMismatch {
                        verse,
                        unit: unit.id.clone(),
                    });
                }
            }
        }
        for (unit, verse, ..) in listed {
            errors.push(ValidationError::VerseSpanMismatch {
                verse,
                unit: unit.clone(),
            });
        }
    }

    fn validate_keys(
        &self,
        units: &HashMap<&UnitId, &TextUnit>,
        errors: &mut Vec<ValidationError>,
    ) {
        for entry in &self.keys {
            if !self.manifest.declares(entry.key.scheme()) {
                errors.push(ValidationError::KeySchemeNotDeclared(entry.key.clone()));
            }
            match units.get(&entry.unit) {
                None => errors.push(ValidationError::UnknownUnit {
                    what: "key",
                    unit: entry.unit.clone(),
                }),
                Some(unit) => {
                    if let Some((s, e)) = entry.range {
                        check_range(&unit.id, &unit.text, "key", s as usize..e as usize, errors);
                    }
                }
            }
        }
    }
}

impl Resource {
    /// Cross-references only in a Cross-reference set, entities and mentions only in a People
    /// & places index; every Passage valid in the Standard versification; every entity named
    /// once and every link to an entity resolved.
    fn validate_datasets(&self, errors: &mut Vec<ValidationError>) {
        let ty = self.manifest.resource_type;
        let mut wrong_type = |what: &'static str, present: bool, allowed: ResourceType| {
            if present && ty != allowed {
                errors.push(ValidationError::DatasetInWrongType {
                    what,
                    resource_type: ty,
                });
            }
        };
        wrong_type(
            "cross-reference",
            !self.cross_references.is_empty(),
            ResourceType::CrossReferenceSet,
        );
        wrong_type(
            "entity",
            !self.entities.is_empty(),
            ResourceType::PeopleAndPlaces,
        );
        wrong_type(
            "mention",
            !self.mentions.is_empty(),
            ResourceType::PeopleAndPlaces,
        );
        for x in &self.cross_references {
            for (what, passage) in [
                ("cross-reference source", x.from),
                ("cross-reference target", x.to),
            ] {
                if !is_standard_passage(passage) {
                    errors.push(ValidationError::NotStandardPassage { what, passage });
                }
            }
        }
        let mut ids = HashSet::new();
        for e in &self.entities {
            if !ids.insert(e.id.as_str()) {
                errors.push(ValidationError::DuplicateEntity(e.id.clone()));
            }
            for (field, value) in [("id", &e.id), ("name", &e.name)] {
                if value.trim().is_empty() {
                    errors.push(ValidationError::EmptyEntityField {
                        entity: e.id.clone(),
                        field,
                    });
                }
            }
        }
        for e in &self.entities {
            for r in &e.relations {
                if !ids.contains(r.entity.as_str()) {
                    errors.push(ValidationError::UnknownEntity {
                        what: "relation",
                        entity: r.entity.clone(),
                    });
                }
            }
        }
        for m in &self.mentions {
            if !ids.contains(m.entity.as_str()) {
                errors.push(ValidationError::UnknownEntity {
                    what: "mention",
                    entity: m.entity.clone(),
                });
            }
            if !is_standard_passage(m.passage) {
                errors.push(ValidationError::NotStandardPassage {
                    what: "mention",
                    passage: m.passage,
                });
            }
        }
    }
}

/// Whether both ends of `passage` are Verses of the Standard versification.
fn is_standard_passage(passage: Passage) -> bool {
    Standard.contains(passage.start()) && Standard.contains(passage.end())
}

/// Checks order, bounds and UTF-8 boundaries; returns whether the range is usable.
fn check_range(
    unit: &UnitId,
    text: &str,
    kind: &'static str,
    range: Range<usize>,
    errors: &mut Vec<ValidationError>,
) -> bool {
    if range.start > range.end {
        errors.push(ValidationError::SpanReversed {
            unit: unit.clone(),
            kind,
            range,
        });
        return false;
    }
    if range.end > text.len() {
        errors.push(ValidationError::SpanOutsideUnit {
            unit: unit.clone(),
            kind,
            range,
            len: text.len(),
        });
        return false;
    }
    for offset in [range.start, range.end] {
        if !text.is_char_boundary(offset) {
            errors.push(ValidationError::SpanNotOnCharBoundary {
                unit: unit.clone(),
                kind,
                offset,
            });
            return false;
        }
    }
    true
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
