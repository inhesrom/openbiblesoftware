//! Annotation records of the User database (#17, #61): Notebooks, Notes and Highlights with
//! their Anchors, and the row-level [`Change`]s that edit them.
//!
//! These types compile everywhere (wasm included); only the SQLite code that stores them is
//! desktop-only. Every edit of the annotation tables is a list of [`Change`]s applied in one
//! transaction, so undo is the same list inverted ([`Change::inverse`]).

use std::sync::atomic::{AtomicU64, Ordering};

use obs_model::UnitId;
use obs_scripture::Passage;
use serde::{Deserialize, Serialize};

/// Characters of context kept on each side of a quote (ADR 0006: "about 32").
pub const CONTEXT_CHARS: usize = 32;

/// A named container for Annotations. Exactly one Notebook is the default; it always exists
/// and cannot be deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notebook {
    /// UUIDv7.
    pub id: String,
    pub name: String,
    pub is_default: bool,
    /// **Show in text** (#17 rule 18): whether its Annotations are drawn in Resource text.
    /// Hidden ones still list in the Notes Tool.
    #[serde(default = "shown")]
    pub show_in_text: bool,
    /// Milliseconds since the Unix epoch.
    pub created_at: i64,
    pub modified_at: i64,
}

fn shown() -> bool {
    true
}

/// One look from the small fixed set used to render a Highlight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighlightStyle {
    Yellow,
    Green,
    Blue,
    Pink,
    RedUnderline,
}

impl HighlightStyle {
    /// Every style, in toolbar order (Ctrl+1 … Ctrl+5).
    pub const ALL: [HighlightStyle; 5] = [
        HighlightStyle::Yellow,
        HighlightStyle::Green,
        HighlightStyle::Blue,
        HighlightStyle::Pink,
        HighlightStyle::RedUnderline,
    ];

    /// Stable id, as stored.
    pub fn id(self) -> &'static str {
        match self {
            HighlightStyle::Yellow => "yellow",
            HighlightStyle::Green => "green",
            HighlightStyle::Blue => "blue",
            HighlightStyle::Pink => "pink",
            HighlightStyle::RedUnderline => "red_underline",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// A span of text inside one specific Resource, with what ADR 0006 needs to find it again
/// after the Resource changes: the quoted text, about [`CONTEXT_CHARS`] characters of context
/// on each side (fewer only at the edge of the text unit) and the content hash of the
/// revision it was made against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextRange {
    pub resource_id: String,
    pub unit: UnitId,
    /// UTF-8 byte offsets into the unit's text, end exclusive.
    pub start: u32,
    pub end: u32,
    pub quote: String,
    pub prefix: String,
    pub suffix: String,
    /// `sha256:…` of the Resource revision the range was made against.
    pub content_hash: String,
    /// For an Unmapped verse: the Bible's own reference (OSIS, e.g. `Esth.10.4` or
    /// `Esth.10.4-Esth.10.6`), since no Standard Passage exists for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<String>,
    /// In a Bible: the range covers every Verse it touches whole (white space and punctuation
    /// at a Verse's ends aside). Other Bibles then highlight those Verses; otherwise they show
    /// only a margin mark beside the verse number (#17 rule 16).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub whole_verses: bool,
    /// Re-anchoring (#65, ADR 0006) could not find the quote after the Resource changed: an
    /// **Orphaned annotation**. The range keeps what it last pointed at (quote, context,
    /// offsets, content hash) and is not drawn until the user re-attaches or detaches it.
    /// Only a Text-range anchor is ever orphaned; a Passage anchor's origin is dropped
    /// instead.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub orphaned: bool,
}

/// What ties an Annotation to the text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Anchor {
    /// A Passage in the Standard versification (ADR 0001), optionally with the Text range in
    /// the Bible where it was made, for word-exact rendering there.
    Passage {
        passage: Passage,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<TextRange>,
    },
    /// A Text range in one Resource, shown only there: non-Bible Resources and Unmapped
    /// verses.
    TextRange { range: TextRange },
}

impl Anchor {
    /// The Text range it stores, if any.
    pub fn text_range(&self) -> Option<&TextRange> {
        match self {
            Anchor::Passage { origin, .. } => origin.as_ref(),
            Anchor::TextRange { range } => Some(range),
        }
    }

    pub fn passage(&self) -> Option<Passage> {
        match self {
            Anchor::Passage { passage, .. } => Some(*passage),
            Anchor::TextRange { .. } => None,
        }
    }

    /// Whether it is the Text-range anchor of an Orphaned annotation.
    pub fn is_orphaned(&self) -> bool {
        matches!(self, Anchor::TextRange { range } if range.orphaned)
    }
}

/// What an Annotation is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Content {
    /// The user's own text.
    Note {
        body: String,
    },
    Highlight {
        style: HighlightStyle,
    },
}

/// A Note or a Highlight. A Highlight has exactly one Anchor; a Note has one or none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annotation {
    /// UUIDv7.
    pub id: String,
    pub notebook_id: String,
    pub content: Content,
    pub anchor: Option<Anchor>,
    /// Milliseconds since the Unix epoch.
    pub created_at: i64,
    pub modified_at: i64,
}

impl Annotation {
    pub fn is_note(&self) -> bool {
        matches!(self.content, Content::Note { .. })
    }

    /// Whether it is an Orphaned annotation (it needs attention).
    pub fn is_orphaned(&self) -> bool {
        self.anchor.as_ref().is_some_and(Anchor::is_orphaned)
    }

    /// A Note's text; empty for a Highlight.
    pub fn body(&self) -> &str {
        match &self.content {
            Content::Note { body } => body,
            Content::Highlight { .. } => "",
        }
    }

    /// Whether it breaks a rule of the model (a Highlight without an Anchor).
    pub fn problem(&self) -> Option<&'static str> {
        match (&self.content, &self.anchor) {
            (Content::Highlight { .. }, None) => Some("a Highlight needs an Anchor"),
            _ => None,
        }
    }
}

/// One row of the annotation tables: a Notebook, or an Annotation with its Anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "row", rename_all = "snake_case")]
// Rows are few and short-lived (one undo step's worth); boxing would only add noise.
#[allow(clippy::large_enum_variant)]
pub enum Row {
    Notebook(Notebook),
    Annotation(Annotation),
}

/// Which row a [`Row`] is.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowKey {
    Notebook(String),
    Annotation(String),
}

impl Row {
    pub fn key(&self) -> RowKey {
        match self {
            Row::Notebook(n) => RowKey::Notebook(n.id.clone()),
            Row::Annotation(a) => RowKey::Annotation(a.id.clone()),
        }
    }
}

/// One row going from `before` to `after` (`None`: the row does not exist). Applying a
/// Change checks that the row is still `before`, so an edit never lands on a row that changed
/// since it was planned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub before: Option<Row>,
    pub after: Option<Row>,
}

impl Change {
    pub fn insert(row: Row) -> Self {
        Self {
            before: None,
            after: Some(row),
        }
    }

    pub fn delete(row: Row) -> Self {
        Self {
            before: Some(row),
            after: None,
        }
    }

    pub fn update(before: Row, after: Row) -> Self {
        Self {
            before: Some(before),
            after: Some(after),
        }
    }

    /// The row this Change is about; `None` if it is about nothing or about two rows.
    pub fn key(&self) -> Option<RowKey> {
        match (&self.before, &self.after) {
            (Some(b), Some(a)) => (b.key() == a.key()).then(|| a.key()),
            (Some(r), None) | (None, Some(r)) => Some(r.key()),
            (None, None) => None,
        }
    }

    /// The Change that undoes this one.
    pub fn inverse(&self) -> Self {
        Self {
            before: self.after.clone(),
            after: self.before.clone(),
        }
    }
}

/// The Changes that undo `changes` (applied in one transaction): each inverted, in reverse
/// order.
pub fn inverse(changes: &[Change]) -> Vec<Change> {
    changes.iter().rev().map(Change::inverse).collect()
}

/// Milliseconds since the Unix epoch (0 where the platform has no clock, as on bare wasm).
pub fn now_ms() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// Last (millisecond × 4096 + counter) handed out, so ids from one process always sort in
/// creation order (RFC 9562 §6.2 method 1: a 12-bit counter in `rand_a`).
static LAST_ID_CLOCK: AtomicU64 = AtomicU64::new(0);

/// 64 unpredictable bits from the standard library's per-process random hash keys, mixed
/// with a counter and the clock.
fn random_u64(salt: u64) -> u64 {
    use std::hash::{BuildHasher, Hasher};
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(salt);
    h.write_u64(CALLS.fetch_add(1, Ordering::Relaxed));
    h.finish()
}

/// A new UUIDv7 (RFC 9562), lowercase hyphenated: 48 bits of Unix milliseconds, a 12-bit
/// monotonic counter and 62 random bits. Made here rather than with the `uuid` crate, whose
/// random source does not build for wasm.
pub fn new_id() -> String {
    new_id_at(now_ms())
}

/// A new UUIDv7 for the time `ms` (clamped to 48 bits).
pub fn new_id_at(ms: i64) -> String {
    let ms = u64::try_from(ms).unwrap_or(0) & 0xFFFF_FFFF_FFFF;
    let seed = random_u64(ms);
    // A fresh millisecond starts its counter at a random point in the lower half, leaving
    // room to count up; within the same millisecond (or a clock step back) count on.
    let fresh = (ms << 12) | (seed & 0x7FF);
    let mut prev = LAST_ID_CLOCK.load(Ordering::Relaxed);
    let clock = loop {
        let next = if fresh > prev { fresh } else { prev + 1 };
        match LAST_ID_CLOCK.compare_exchange_weak(prev, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => break next,
            Err(p) => prev = p,
        }
    };
    let ms = (clock >> 12) & 0xFFFF_FFFF_FFFF;
    let counter = clock & 0xFFF;
    let rand_b = random_u64(clock) & 0x3FFF_FFFF_FFFF_FFFF;
    let hi = (ms << 16) | (0x7 << 12) | counter;
    let lo = (0b10 << 62) | rand_b;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        hi >> 32,
        (hi >> 16) & 0xFFFF,
        hi & 0xFFFF,
        lo >> 48,
        lo & 0xFFFF_FFFF_FFFF
    )
}

/// Whether `id` looks like an id this module makes (a lowercase hyphenated UUID).
pub fn is_valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, &c)| match i {
            8 | 13 | 18 | 23 => c == b'-',
            _ => c.is_ascii_digit() || (b'a'..=b'f').contains(&c),
        })
}

/// The text of `text[start..end]` with up to [`CONTEXT_CHARS`] characters of context on each
/// side: `(prefix, quote, suffix)`. `None` if the range is reversed, past the end, or not on
/// character boundaries.
pub fn quote_with_context(text: &str, start: usize, end: usize) -> Option<(&str, &str, &str)> {
    let quote = text.get(start..end)?;
    let before = text.get(..start)?;
    let after = text.get(end..)?;
    let prefix_at = before
        .char_indices()
        .rev()
        .nth(CONTEXT_CHARS - 1)
        .map_or(0, |(i, _)| i);
    let suffix_to = after
        .char_indices()
        .nth(CONTEXT_CHARS)
        .map_or(after.len(), |(i, _)| i);
    Some((before.get(prefix_at..)?, quote, after.get(..suffix_to)?))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn ids_are_uuid_v7_and_sort_in_creation_order() {
        let ids: Vec<String> = (0..5000).map(|_| new_id_at(1_700_000_000_000)).collect();
        for id in &ids {
            assert!(is_valid_id(id), "{id}");
            assert_eq!(&id[14..15], "7", "version nibble of {id}");
            assert!(
                matches!(&id[19..20], "8" | "9" | "a" | "b"),
                "variant of {id}"
            );
        }
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, ids, "ids are unique and increasing");
        // The time is in the first 48 bits.
        let later = new_id_at(1_800_000_000_000);
        assert!(later > ids[4999]);
        assert!(later.starts_with(&format!("{:08x}", 1_800_000_000_000_u64 >> 16)));
    }

    #[test]
    fn context_is_32_characters_each_side_or_up_to_the_unit_edge() {
        let text = "αβγδεζηθικλμνξοπρστυφχψωαβγδεζηθ-QUOTE-0123456789abcdefghijklmnopqrstuvwxyz";
        let start = text.find("QUOTE").unwrap();
        let (p, q, s) = quote_with_context(text, start, start + 5).unwrap();
        assert_eq!(q, "QUOTE");
        assert_eq!(p.chars().count(), 32);
        assert!(p.ends_with("θ-"));
        assert_eq!(s, "-0123456789abcdefghijklmnopqrstu");
        let (p, q, s) = quote_with_context("short text", 0, 5).unwrap();
        assert_eq!((p, q, s), ("", "short", " text"));
        assert!(quote_with_context("αβ", 1, 2).is_none());
        assert!(quote_with_context("ab", 2, 1).is_none());
        assert!(quote_with_context("ab", 0, 3).is_none());
    }

    #[test]
    fn a_change_inverts_and_a_list_inverts_in_reverse() {
        let nb = |name: &str| {
            Row::Notebook(Notebook {
                id: "n".into(),
                name: name.into(),
                is_default: false,
                show_in_text: true,
                created_at: 1,
                modified_at: 1,
            })
        };
        let changes = vec![Change::insert(nb("a")), Change::update(nb("a"), nb("b"))];
        let inv = inverse(&changes);
        assert_eq!(inv[0], Change::update(nb("b"), nb("a")));
        assert_eq!(inv[1], Change::delete(nb("a")));
        assert_eq!(inverse(&inv), changes);
        assert_eq!(changes[1].key(), Some(RowKey::Notebook("n".into())));
    }
}
