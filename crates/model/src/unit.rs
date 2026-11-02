//! Text units and the standoff span vocabulary laid over them (ADR 0003, #11 rules 3–7).

use std::fmt;
use std::ops::Range;

use obs_scripture::{Book, PassageList, StrongsNumber, Verse};
use serde::{Deserialize, Serialize};

/// Stable id of a text unit, derived from its Key: `JHN`, `JHN.3.16-21`, `G5485`, `Abraham`,
/// `note.12`. Annotations depend on these staying stable across Resource revisions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct UnitId(String);

/// Why a unit id was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitIdError(pub String);

impl fmt::Display for UnitIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a valid unit id (1-200 bytes, no control characters, no leading or trailing space)",
            self.0
        )
    }
}

impl std::error::Error for UnitIdError {}

impl UnitId {
    pub fn new(id: impl Into<String>) -> Result<Self, UnitIdError> {
        let id = id.into();
        let ok = !id.is_empty()
            && id.len() <= 200
            && id.trim() == id
            && !id.chars().any(char::is_control);
        if ok {
            Ok(Self(id))
        } else {
            Err(UnitIdError(id))
        }
    }

    /// The text unit of one Bible book: its USFM code.
    pub fn book(book: Book) -> Self {
        Self(book.usfm().to_string())
    }

    /// The text unit of a note body: `note.<n>`.
    pub fn note(n: u32) -> Self {
        Self(format!("note.{n}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for UnitId {
    type Error = UnitIdError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        UnitId::new(s)
    }
}

impl From<UnitId> for String {
    fn from(id: UnitId) -> Self {
        id.0
    }
}

impl fmt::Display for UnitId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a text unit holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    /// One Bible book of a Bible.
    BibleBook,
    /// One Commentary section keyed to a Passage.
    Section,
    /// One Lexicon or Dictionary entry.
    Entry,
    /// One chapter or division of general prose.
    Division,
    /// A footnote or cross-reference note body, referenced by a zero-width `note_ref`.
    Note,
}

/// Paragraph style of a `para` span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParaStyle {
    Prose,
    Poetry1,
    Poetry2,
    Poetry3,
    List,
    /// A blank line between paragraphs (usually zero-width).
    Blank,
}

/// What a `note_ref` points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    Footnote,
    CrossReference,
}

/// One morpheme of a Tagged word: Hebrew prefixes and suffixes are their own parts (TAHOT);
/// a Greek or English word is usually one part.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MorphemePart {
    pub strongs: StrongsNumber,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lemma: Option<String>,
    /// Source morphology code (expanded to plain English for display elsewhere).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub morph: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gloss: Option<String>,
}

impl MorphemePart {
    pub fn new(strongs: StrongsNumber) -> Self {
        Self {
            strongs,
            lemma: None,
            morph: None,
            gloss: None,
        }
    }
}

/// The closed, versioned span vocabulary. Markup outside it is dropped and counted in the
/// Import report. Changing it bumps [`crate::FORMAT_VERSION`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpanKind {
    /// One Bible Verse in the Bible's own numbering; `last` > `verse.verse` for a verse range
    /// such as USFM `\v 1-2`. Added with [`crate::ResourceBuilder::add_verse`].
    Verse {
        verse: Verse,
        last: u16,
    },
    Chapter {
        chapter: u16,
    },
    Para {
        style: ParaStyle,
    },
    Heading {
        level: u8,
    },
    TaggedWord {
        parts: Vec<MorphemePart>,
    },
    /// Zero-width marker pointing at a note body unit.
    NoteRef {
        note: UnitId,
        note_kind: NoteKind,
    },
    /// A cross-reference whose targets are in the Standard versification.
    Xref {
        targets: PassageList,
    },
    /// A Bible reference found in the text, parsed at import.
    ScriptureRef {
        targets: PassageList,
    },
    /// Words of Jesus.
    Woj,
    /// Translator additions (italics in the KJV).
    Added,
    /// LORD / GOD as the divine name (drawn in small caps).
    DivineName,
    Emphasis,
    Italic,
    Bold,
    Foreign,
    Selah,
    PsalmTitle,
}

impl SpanKind {
    /// Stable name of the kind, as stored.
    pub fn name(&self) -> &'static str {
        match self {
            SpanKind::Verse { .. } => "verse",
            SpanKind::Chapter { .. } => "chapter",
            SpanKind::Para { .. } => "para",
            SpanKind::Heading { .. } => "heading",
            SpanKind::TaggedWord { .. } => "tagged_word",
            SpanKind::NoteRef { .. } => "note_ref",
            SpanKind::Xref { .. } => "xref",
            SpanKind::ScriptureRef { .. } => "scripture_ref",
            SpanKind::Woj => "woj",
            SpanKind::Added => "added",
            SpanKind::DivineName => "divine_name",
            SpanKind::Emphasis => "emphasis",
            SpanKind::Italic => "italic",
            SpanKind::Bold => "bold",
            SpanKind::Foreign => "foreign",
            SpanKind::Selah => "selah",
            SpanKind::PsalmTitle => "psalm_title",
        }
    }

    /// Position in canonical span order for spans with the same range: structure first.
    pub(crate) fn rank(&self) -> u8 {
        match self {
            SpanKind::Chapter { .. } => 0,
            SpanKind::Para { .. } => 1,
            SpanKind::Heading { .. } => 2,
            SpanKind::PsalmTitle => 3,
            SpanKind::Verse { .. } => 4,
            SpanKind::Woj => 5,
            SpanKind::Xref { .. } => 6,
            SpanKind::ScriptureRef { .. } => 7,
            SpanKind::TaggedWord { .. } => 8,
            SpanKind::NoteRef { .. } => 9,
            SpanKind::Added => 10,
            SpanKind::DivineName => 11,
            SpanKind::Emphasis => 12,
            SpanKind::Italic => 13,
            SpanKind::Bold => 14,
            SpanKind::Foreign => 15,
            SpanKind::Selah => 16,
        }
    }
}

/// One span over a unit's text, by UTF-8 byte range.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    #[serde(flatten)]
    pub kind: SpanKind,
}

impl Span {
    pub fn range(&self) -> Range<usize> {
        self.start as usize..self.end as usize
    }
}

/// One text unit: a plain NFC string plus its spans, in canonical order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextUnit {
    pub id: UnitId,
    pub kind: UnitKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub text: String,
    pub spans: Vec<Span>,
}

/// One Bible Verse in the Bible's own numbering, in document order (its index is the
/// file-internal ordinal).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BibleVerse {
    pub verse: Verse,
    /// Last verse number covered: equal to `verse.verse` except for verse ranges (`\v 1-2`).
    pub last: u16,
    pub unit: UnitId,
    pub start: u32,
    pub end: u32,
}

impl BibleVerse {
    pub fn range(&self) -> Range<usize> {
        self.start as usize..self.end as usize
    }
}

/// Builds a unit's text piece by piece, NFC-normalizing each piece and returning its byte range.
#[derive(Debug, Clone, Default)]
pub struct TextAccumulator {
    text: String,
}

impl TextAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current byte length, the start of whatever is pushed next.
    pub fn pos(&self) -> usize {
        self.text.len()
    }

    /// Append `piece` (NFC-normalized) and return the byte range it occupies.
    pub fn push(&mut self, piece: &str) -> Range<usize> {
        use unicode_normalization::UnicodeNormalization;
        let start = self.text.len();
        self.text.extend(piece.nfc());
        start..self.text.len()
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn into_string(self) -> String {
        self.text
    }
}
