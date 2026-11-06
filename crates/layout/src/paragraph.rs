//! The layout input: a paragraph of styled runs, each traced back to its source.

use obs_model::{NoteKind, UnitId};
use obs_scripture::Verse;

use crate::interlinear::Interlinear;
use crate::style::RunStyle;

/// Text the layout adds that is not in the unit (verse numbers, note markers). It sits at an
/// anchor byte of the unit and hit-tests as the object, not as text.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InlineObject {
    /// A verse number (or, for verse 1, the chapter number) before a Bible Verse.
    VerseNumber(Verse),
    /// A zero-width Scripture-reference span rendered as a hoverable marker.
    ScriptureReference {
        /// Index in the source TextUnit's spans; anchors need not be unique.
        span: usize,
    },
    /// A footnote or cross-reference marker for a note unit.
    NoteMarker { note: UnitId, kind: NoteKind },
    /// One interlinear line under a Tagged word (#143), anchored at the word's first byte;
    /// `row` 0 is the line just under the word.
    InterlinearCell { row: usize },
}

/// Where a run's text comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RunSource {
    /// Unit text starting at this byte; the run's bytes map one-to-one onto the unit's.
    Text { unit_start: usize },
    /// A synthetic object anchored at a unit byte.
    Object { anchor: usize, object: InlineObject },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: RunStyle,
    pub source: RunSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Left in a left-to-right paragraph, right in a right-to-left one.
    #[default]
    Start,
    Center,
}

/// One block's worth of text, ready to lay out.
#[derive(Debug, Clone, PartialEq)]
pub struct Paragraph {
    pub runs: Vec<Run>,
    /// Base direction (from the Resource's script direction, #14 rule 10).
    pub rtl: bool,
    pub line_spacing: f32,
    /// First-line indent in px (poetry indents every line by the block's `indent`).
    pub indent: f32,
    /// Indent of every line in px.
    pub inset: f32,
    pub space_before: f32,
    pub space_after: f32,
    pub align: Align,
    /// Share of Hebrew line metrics to count (see `TextSettings::hebrew_line_height`).
    pub hebrew_line_height: f32,
    /// Interlinear lines under its Tagged words (#143), when the Tab shows them.
    pub interlinear: Option<Interlinear>,
}

impl Paragraph {
    /// Concatenated text of all runs.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }
}
