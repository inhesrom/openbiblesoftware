//! Interlinear lines (#143): a block of short lines (a Lemma, a transliteration, a Strong's
//! number, Morphology, a Gloss) stacked under each Tagged word of a Bible paragraph.
//!
//! The paragraph's own text is laid out as usual, except that a Tagged word is never broken
//! and is as wide as its widest line (centred in that column); the lines are laid out on their
//! own under it, each in its own direction, so a Hebrew Lemma under an English word and an
//! English Gloss under a Hebrew word both read correctly. The lines are not unit text: they
//! hit-test as an [`InlineObject::InterlinearCell`] anchored at the word's first byte, so
//! selections and Annotation anchors made over them stay on the word.

use std::ops::Range;

use obs_model::{MorphemePart, SpanKind, TextUnit};

use crate::paragraph::{InlineObject, Paragraph, Run, RunSource};
use crate::present::{BlockSource, Flags, push_text};
use crate::style::{ColorRole, TextSettings};

/// Interlinear lines are drawn at this share of the text size.
pub const INTERLINEAR_SCALE: f32 = 0.72;

/// The interlinear lines of one paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct Interlinear {
    /// How many lines each Tagged word has under it (every line of the paragraph leaves room
    /// for all of them, so the rows line up across the paragraph).
    pub rows: usize,
    /// Size of the lines' text in px.
    pub size: f32,
    /// The Tagged words with their lines, in unit order. Empty in a paragraph built only to
    /// estimate its height.
    pub words: Vec<InterlinearWord>,
}

impl Interlinear {
    /// Room for `rows` lines under each word, without their text: enough to estimate a
    /// block's height before its lines are looked up.
    pub fn rows_only(rows: usize, settings: &TextSettings) -> Self {
        Self {
            rows,
            size: settings.size * INTERLINEAR_SCALE,
            words: Vec::new(),
        }
    }
}

/// One Tagged word and the lines under it.
#[derive(Debug, Clone, PartialEq)]
pub struct InterlinearWord {
    /// Unit bytes of the Tagged word.
    pub word: Range<usize>,
    /// One cell per row, top to bottom, each as styled runs (none: the row is blank here).
    /// Every run's source is an [`InlineObject::InterlinearCell`] anchored at `word.start`.
    pub cells: Vec<Vec<Run>>,
}

/// The Tagged words of a block: outermost first, nested or overlapping ones skipped, only
/// those with original-language data (non-empty `parts`) and some text.
pub fn tagged_words<'a>(
    unit: &'a TextUnit,
    block: &BlockSource,
) -> Vec<(Range<usize>, &'a [MorphemePart])> {
    let mut words: Vec<(Range<usize>, &[MorphemePart])> = unit
        .spans
        .iter()
        .filter_map(|s| match &s.kind {
            SpanKind::TaggedWord { parts } if !parts.is_empty() && s.start < s.end => {
                let r = s.range();
                (r.start >= block.range.start && r.end <= block.range.end)
                    .then_some((r, parts.as_slice()))
            }
            _ => None,
        })
        .collect();
    words.sort_by_key(|(r, _)| (r.start, std::cmp::Reverse(r.end)));
    let mut out: Vec<(Range<usize>, &[MorphemePart])> = Vec::with_capacity(words.len());
    for (r, parts) in words {
        if out.last().is_some_and(|(prev, _)| r.start < prev.end) {
            continue;
        }
        out.push((r, parts));
    }
    out
}

/// Add `rows` interlinear lines under each Tagged word of a block's paragraph. `cells` gives a
/// word's lines from its text as written and its morphemes: one string per row (missing or
/// empty strings leave that row blank under the word).
pub fn add_interlinear(
    para: &mut Paragraph,
    unit: &TextUnit,
    block: &BlockSource,
    settings: &TextSettings,
    rows: usize,
    cells: &mut dyn FnMut(&str, &[MorphemePart]) -> Vec<String>,
) {
    if rows == 0 {
        para.interlinear = None;
        return;
    }
    let mut interlinear = Interlinear::rows_only(rows, settings);
    let flags = Flags {
        role: Some(ColorRole::Interlinear),
        ..Flags::default()
    };
    for (range, parts) in tagged_words(unit, block) {
        let written = unit.text.get(range.clone()).unwrap_or_default().trim();
        let texts = cells(written, parts);
        let mut word_cells = Vec::with_capacity(rows);
        for row in 0..rows {
            let mut runs = Vec::new();
            if let Some(text) = texts.get(row).map(|t| t.trim()).filter(|t| !t.is_empty()) {
                push_text(&mut runs, text, 0, flags, interlinear.size, settings);
                for r in &mut runs {
                    r.source = RunSource::Object {
                        anchor: range.start,
                        object: InlineObject::InterlinearCell { row },
                    };
                }
            }
            word_cells.push(runs);
        }
        interlinear.words.push(InterlinearWord {
            word: range,
            cells: word_cells,
        });
    }
    // A block without Tagged words (a heading) has no lines to make room for.
    para.interlinear = (!interlinear.words.is_empty()).then_some(interlinear);
}
