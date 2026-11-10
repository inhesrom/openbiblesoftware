//! Estimated block heights, for blocks not laid out yet (#14 rule 7). Refined once laid out;
//! the scroll position is anchored to a verse, so a wrong estimate never moves the view.

use obs_fonts::{FaceId, Script, script_of};

use crate::paragraph::Paragraph;

/// Average advance per character as a share of the font size: measured on Genesis in Gentium
/// Plus (English ≈ 0.43) and Ezra SIL (pointed Hebrew, per base letter ≈ 0.55); other reading
/// fonts (#138) use the same averages for their script.
const LATIN_ADVANCE: f32 = 0.43;
const HEBREW_ADVANCE: f32 = 0.55;
/// How much wider a paragraph runs with interlinear lines under its words (#143): tuned on
/// KJV Jn 3:16 with a Greek Lemma and a Strong's number under each word.
const INTERLINEAR_WIDEN: f32 = 1.6;

/// One line's height before line spacing in a paragraph with no text, px.
const EMPTY_LINE: f32 = 16.0 * 1.46;

/// A height estimate for `para` at `width` px, from character counts and font size.
pub fn estimate_height(para: &Paragraph, width: f32) -> f32 {
    HeightEstimate::of(para).at(width)
}

/// What a paragraph's estimated height depends on besides the width: measured once, so a new
/// width (a Panel's edge dragged) is a little arithmetic per block, not the paragraph styled
/// again (#255).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct HeightEstimate {
    /// The text's advance on one line, px.
    text_width: f32,
    /// One line's height before line spacing, px (0: no text, [`EMPTY_LINE`]).
    line_size: f32,
    line_spacing: f32,
    /// Width taken from every line, px.
    taken: f32,
    space_before: f32,
    space_after: f32,
    /// How much wider interlinear lines make the text, and their height under each line.
    widen: f32,
    rows: f32,
}

impl HeightEstimate {
    pub fn of(para: &Paragraph) -> Self {
        // Nothing of a hidden paragraph is drawn (#209): no height at any width.
        if para.hidden {
            return Self::default();
        }
        let mut text_width = 0.0f32;
        let mut line_size = 0.0f32;
        for r in &para.runs {
            let per_char = if r.style.face == FaceId::Hebrew || script_of(&r.text) == Script::Hebrew
            {
                HEBREW_ADVANCE
            } else {
                LATIN_ADVANCE
            };
            // Marks take no advance; count base characters only.
            let chars = r
                .text
                .chars()
                .filter(|c| !unicode_normalization_like_mark(*c))
                .count();
            text_width += chars as f32 * per_char * r.style.size;
            let k = if r.style.face == FaceId::Hebrew {
                para.hebrew_line_height
            } else {
                1.0
            };
            line_size = line_size.max(r.style.size * 1.46 * k);
        }
        // Interlinear lines (#143) widen each Tagged word to its column and add rows under
        // every line.
        let (widen, rows) = match &para.interlinear {
            Some(il) if il.rows > 0 => (
                INTERLINEAR_WIDEN,
                il.rows as f32 * il.size * 1.35 + or_empty_line(line_size) / 1.46 * 0.5,
            ),
            _ => (1.0, 0.0),
        };
        Self {
            text_width,
            line_size,
            line_spacing: para.line_spacing,
            taken: para.inset + para.indent * 0.5,
            space_before: para.space_before,
            space_after: para.space_after,
            widen,
            rows,
        }
    }

    /// The estimated height at `width` px.
    pub fn at(&self, width: f32) -> f32 {
        let avail = (width - self.taken).max(1.0);
        let lines = (self.text_width * self.widen / avail).ceil().max(1.0);
        self.space_before
            + lines * (or_empty_line(self.line_size) * self.line_spacing + self.rows)
            + self.space_after
    }

    /// The estimate of the same paragraph styled at `k` times the Text size, without styling
    /// it again (#299): every length in a styled paragraph is in proportion to the Text size,
    /// so this is what [`Self::of`] would measure at the new size.
    pub fn scaled(&self, k: f32) -> Self {
        Self {
            text_width: self.text_width * k,
            line_size: self.line_size * k,
            line_spacing: self.line_spacing,
            taken: self.taken * k,
            space_before: self.space_before * k,
            space_after: self.space_after * k,
            widen: self.widen,
            rows: self.rows * k,
        }
    }
}

/// A line's height before line spacing: `line_size`, or [`EMPTY_LINE`] in a paragraph with no
/// text (0).
fn or_empty_line(line_size: f32) -> f32 {
    if line_size == 0.0 {
        EMPTY_LINE
    } else {
        line_size
    }
}

/// Combining marks (Hebrew points and accents, Greek and Latin diacritics).
fn unicode_normalization_like_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7)
}
