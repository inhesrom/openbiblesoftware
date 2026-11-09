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

/// A height estimate for `para` at `width` px, from character counts and font size.
pub fn estimate_height(para: &Paragraph, width: f32) -> f32 {
    if para.hidden {
        return 0.0;
    }
    let mut text_width = 0.0f32;
    let mut line_size = 0.0f32;
    for r in &para.runs {
        let per_char = if r.style.face == FaceId::Hebrew || script_of(&r.text) == Script::Hebrew {
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
    if line_size == 0.0 {
        line_size = 16.0 * 1.46;
    }
    let avail = (width - para.inset - para.indent * 0.5).max(1.0);
    // Interlinear lines (#143) widen each Tagged word to its column and add rows under
    // every line.
    let (widen, rows) = match &para.interlinear {
        Some(il) if il.rows > 0 => (
            INTERLINEAR_WIDEN,
            il.rows as f32 * il.size * 1.35 + line_size / 1.46 * 0.5,
        ),
        _ => (1.0, 0.0),
    };
    let lines = (text_width * widen / avail).ceil().max(1.0);
    para.space_before + lines * (line_size * para.line_spacing + rows) + para.space_after
}

/// Combining marks (Hebrew points and accents, Greek and Latin diacritics).
fn unicode_normalization_like_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7)
}
