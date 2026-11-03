//! Estimated block heights, for blocks not laid out yet (#14 rule 7). Refined once laid out;
//! the scroll position is anchored to a verse, so a wrong estimate never moves the view.

use obs_fonts::FaceId;

use crate::paragraph::Paragraph;

/// Average advance per character as a share of the font size: measured on Genesis in Gentium
/// Plus (English ≈ 0.43) and Ezra SIL (pointed Hebrew, per base letter ≈ 0.55).
const LATIN_ADVANCE: f32 = 0.43;
const HEBREW_ADVANCE: f32 = 0.55;

/// A height estimate for `para` at `width` px, from character counts and font size.
pub fn estimate_height(para: &Paragraph, width: f32) -> f32 {
    let mut text_width = 0.0f32;
    let mut line_size = 0.0f32;
    for r in &para.runs {
        let per_char = if r.style.face == FaceId::Hebrew {
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
    let lines = (text_width / avail).ceil().max(1.0);
    para.space_before + lines * line_size * para.line_spacing + para.space_after
}

/// Combining marks (Hebrew points and accents, Greek and Latin diacritics).
fn unicode_normalization_like_mark(c: char) -> bool {
    matches!(c as u32, 0x0300..=0x036F | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2 | 0x05C4..=0x05C5 | 0x05C7)
}
