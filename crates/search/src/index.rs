//! The Search index seam (#58): what one Resource's index answers, independent of how it is
//! stored. `obs-store` implements it over a per-Resource SQLite FTS5 cache file (ADR 0003: the
//! index is a separate, rebuildable cache, never part of the `.obr`).
//!
//! Documents (#16 rule 2):
//! - **Bible:** one Bible Verse in the Bible's own numbering; the document id is the verse
//!   ordinal (its place in the Bible's verse list), so a range of the Bible is a range of ids.
//! - **Other Resources:** one paragraph span inside a text unit (or a run of text outside any
//!   paragraph, such as a heading), carrying the unit's Key and title.
//!
//! Fields (#16 rule 3): `text` (normalized surface words), `strong` (Strong's numbers of
//! Tagged-word morpheme parts, extended and base forms), `lemma` (normalized Lemmas) and
//! `morph` (each morpheme part's Strong's numbers, Morphology code token and Lemma in turn,
//! for Morphology criteria, #145).

use std::fmt;
use std::ops::{Range, RangeInclusive};

use obs_model::{Key, UnitId};

use crate::query::Compiled;

/// The version of the Search index: what a document is, its fields, the tokenizer and the
/// normalizer. A cache built with another version is stale and is rebuilt in the background.
/// Bump it whenever any of those change (including [`obs_model::NORMALIZER_VERSION`]).
/// Version 2 added the `morph` field (#145). Version 3 ends each morpheme part's words in
/// `lemma` and `morph` with a boundary, and gives `morph` a run per Lemma word (#257).
/// Version 4 leaves the Tagged words in a Bible Verse's headings out of its `strong`, `lemma`
/// and `morph`, as its `text` leaves the headings out (#211).
pub const INDEX_VERSION: u32 = 4;

// A normalizer change must bump INDEX_VERSION too (and then this pin).
const _: () = assert!(
    obs_model::NORMALIZER_VERSION == 1,
    "the normalizer changed: bump INDEX_VERSION and update this pin"
);

/// What a Resource's search documents are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    /// Bible Verses; document id = verse ordinal.
    Verse,
    /// Paragraph spans of text units.
    Paragraph,
}

impl DocumentKind {
    pub fn name(self) -> &'static str {
        match self {
            DocumentKind::Verse => "verse",
            DocumentKind::Paragraph => "paragraph",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "verse" => Some(DocumentKind::Verse),
            "paragraph" => Some(DocumentKind::Paragraph),
            _ => None,
        }
    }
}

/// How hits come back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitOrder {
    /// Document order: canonical order for a Bible (#16 rule 11).
    #[default]
    Document,
    /// Best first by `bm25` (Library results, #16 rule 12).
    Relevance,
}

/// Limits on one search of one index.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchOptions {
    /// Document-id ranges to search (a Bible's Search scope as verse-ordinal ranges);
    /// empty searches everything.
    pub documents: Vec<RangeInclusive<i64>>,
    pub order: HitOrder,
    /// At most this many hits; `None` for all of them.
    pub limit: Option<usize>,
}

/// One matching document.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// Document id: the verse ordinal for a Bible.
    pub document: i64,
    /// The text unit the document is in, and its byte range there (the Text range space,
    /// for highlighting on the display text).
    pub unit: UnitId,
    pub range: Range<u32>,
    /// The Verse (in the Bible's own numbering) for a Bible document; the unit's Key for
    /// other Resources.
    pub key: Option<Key>,
    /// The unit's title, if it has one.
    pub title: Option<String>,
    /// `bm25` score: lower is better. `0.0` when hits are in document order.
    pub score: f64,
}

/// Why an index could not be searched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchError(pub String);

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SearchError {}

/// One Resource's Search index.
pub trait SearchIndex {
    /// The Resource it indexes.
    fn resource_id(&self) -> &str;

    fn document_kind(&self) -> DocumentKind;

    /// How many documents it holds.
    fn document_count(&self) -> u64;

    /// Documents matching a compiled query ([`crate::compile`]).
    fn search(
        &self,
        query: &Compiled,
        options: &SearchOptions,
    ) -> Result<Vec<SearchHit>, SearchError>;
}
