//! The Note margin (#210): where a reading pane in Margin mode (Note display) sets its text
//! column and the column of Note cards beside it, and where each card goes.
//!
//! - **Widths** follow the Tab's Reading layout (#209): the margin is 40% of the Reading
//!   column's own width (of the pane's room at Full width), between [`CARD_MIN`] and
//!   [`CARD_MAX`] points. Text and margin, with the [`GUTTER`] between them where leader lines
//!   run, are centred together in the pane. The margin is on the side where lines end: right
//!   of left-to-right text, left of right-to-left text.
//! - **Narrow panes**: when the column would leave the text less than [`MIN_TEXT_EMS`] ems,
//!   the margin shrinks towards [`CARD_MIN`]; when even that leaves too little, there is no
//!   margin ([`split`] gives `None`) and the Tab shows Icons.
//! - **Cards** sit beside their Note's marker; one that would overlap the card above is moved
//!   down just below it ([`stack`]), so cards never overlap and keep text order; a run of
//!   cards that would go past the pane's foot moves up into it ([`fit`]). Positions are
//!   worked out from the laid-out text every frame, so cards scroll with their Anchors.

use obs_layout::ColumnWidth;

use crate::tokens::note_margin::SHARE;
pub use crate::tokens::note_margin::{CARD_GAP, CARD_MAX, CARD_MIN, GUTTER, MIN_TEXT_EMS};
use crate::tokens::space;

/// A pane's room split into the text column and the Note margin, as offsets from the left of
/// the room, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarginSplit {
    /// Where the text column starts, and its width.
    pub text_left: f32,
    pub measure: f32,
    /// Where the Note margin starts, and its width.
    pub margin_left: f32,
    pub margin: f32,
}

/// Split `room` points (the pane less its edges and scrollbar) for text of `size` points in a
/// `column` Reading column, the margin on the left when `rtl`. `None`: too narrow for a
/// margin.
pub fn split(room: f32, size: f32, column: ColumnWidth, rtl: bool) -> Option<MarginSplit> {
    let own = column.ems().map_or(room, |ems| size * ems);
    let min_text = MIN_TEXT_EMS * size;
    let mut margin = (own * SHARE).clamp(CARD_MIN, CARD_MAX);
    let mut measure = own.min(room - GUTTER - margin);
    if measure < min_text {
        margin = (room - GUTTER - min_text).max(CARD_MIN);
        measure = room - GUTTER - margin;
    }
    if min_text - measure > space::PX / 2.0 {
        return None;
    }
    let left = ((room - (measure + GUTTER + margin)) / 2.0)
        .max(0.0)
        .floor();
    Some(if rtl {
        MarginSplit {
            text_left: left + margin + GUTTER,
            measure,
            margin_left: left,
            margin,
        }
    } else {
        MarginSplit {
            text_left: left,
            measure,
            margin_left: left + measure + GUTTER,
            margin,
        }
    })
}

/// The tops of cards `heights` tall that want to start at `wanted` (in text order, top to
/// bottom): each at its own place, or just below the card above when that one reaches it.
pub fn stack(wanted: &[f32], heights: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(wanted.len());
    let mut bottom = f32::NEG_INFINITY;
    for (w, h) in wanted.iter().zip(heights) {
        let top = w.max(bottom + CARD_GAP);
        bottom = top + h;
        out.push(top);
    }
    out
}

/// `tops` (from [`stack`], of cards `heights` tall) with a run of cards that would go past
/// `bottom` (the pane's foot) moved up, keeping their order and gaps, so the last of them
/// ends there; none moves up past `top` (the pane's top) for it. Cards taller together than
/// the pane still run past `bottom`.
pub fn fit(tops: &[f32], heights: &[f32], top: f32, bottom: f32) -> Vec<f32> {
    let mut out = tops.to_vec();
    let mut limit = bottom;
    for (t, h) in out.iter_mut().zip(heights).rev() {
        *t = t.min(limit - h).max(top.min(*t));
        limit = *t - CARD_GAP;
    }
    // Where the pane's top held a card back, the ones below go down again to clear it.
    let mut below = f32::NEG_INFINITY;
    for (t, h) in out.iter_mut().zip(heights) {
        *t = t.max(below + CARD_GAP);
        below = *t + h;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_margin_follows_the_reading_column_and_sits_where_lines_end() {
        // Medium (38 em of 20 pt = 760) in a wide pane: the margin is 40% of it.
        let s = split(1600.0, 20.0, ColumnWidth::Medium, false).unwrap();
        assert_eq!((s.measure, s.margin), (760.0, 304.0));
        assert!(s.margin_left >= s.text_left + s.measure + GUTTER - 0.01);
        // Centred together.
        let right = 1600.0 - (s.margin_left + s.margin);
        assert!((right - s.text_left).abs() <= 1.0, "{s:?}");
        // A Narrow column gets a narrower margin.
        let n = split(1600.0, 16.0, ColumnWidth::Narrow, false).unwrap();
        assert_eq!(n.measure, 16.0 * 30.0);
        assert!((n.margin - 16.0 * 30.0 * 0.4).abs() < 0.01, "{n:?}");
        // Full width: the text takes what the widest margin leaves.
        let f = split(1200.0, 20.0, ColumnWidth::Full, false).unwrap();
        assert_eq!(f.margin, CARD_MAX);
        assert_eq!(f.measure, 1200.0 - GUTTER - CARD_MAX);
        // Right to left: the margin is on the left.
        let r = split(1600.0, 20.0, ColumnWidth::Medium, true).unwrap();
        assert!(
            r.margin_left + r.margin + GUTTER <= r.text_left + 0.01,
            "{r:?}"
        );
    }

    #[test]
    fn a_narrow_pane_shrinks_the_margin_then_has_none() {
        // 600 points: the text keeps 16 em, the margin gets the rest.
        let s = split(600.0, 20.0, ColumnWidth::Medium, false).unwrap();
        assert_eq!(s.measure, 320.0);
        assert_eq!(s.margin, 600.0 - GUTTER - 320.0);
        // Too narrow for the smallest card beside 16 em of text.
        assert_eq!(
            split(
                320.0 + GUTTER + CARD_MIN - 2.0,
                20.0,
                ColumnWidth::Medium,
                false
            ),
            None
        );
        assert!(split(320.0 + GUTTER + CARD_MIN, 20.0, ColumnWidth::Medium, false).is_some());
    }

    #[test]
    fn overlapping_cards_stack_in_text_order_and_others_stay_put() {
        let tops = stack(&[0.0, 10.0, 200.0, 205.0], &[50.0, 50.0, 30.0, 30.0]);
        assert_eq!(tops, vec![0.0, 56.0, 200.0, 236.0]);
        // Pushed far enough, a card pushes the next one too.
        let tops = stack(&[0.0, 10.0, 60.0], &[100.0, 20.0, 20.0]);
        assert_eq!(tops, vec![0.0, 106.0, 132.0]);
    }

    #[test]
    fn cards_running_past_the_pane_move_up_into_it_but_not_past_its_top() {
        // In the pane: unchanged.
        let tops = [0.0, 56.0, 600.0];
        assert_eq!(fit(&tops, &[50.0, 50.0, 300.0], 0.0, 1000.0), tops);
        // The last card would end at 1100: it ends at 1000, and pushes the one above it up.
        let tops = [0.0, 700.0, 756.0];
        assert_eq!(
            fit(&tops, &[50.0, 50.0, 344.0], 0.0, 1000.0),
            vec![0.0, 600.0, 656.0]
        );
        // Taller together than the pane: from its top down, the rest past its foot.
        let tops = [0.0, 56.0, 112.0];
        assert_eq!(
            fit(&tops, &[50.0, 50.0, 2000.0], 0.0, 1000.0),
            vec![0.0, 56.0, 112.0]
        );
        // A card already above the pane's top (its marker scrolled up) stays where it is.
        let tops = [-40.0, 900.0];
        assert_eq!(fit(&tops, &[50.0, 200.0], 0.0, 1000.0), vec![-40.0, 800.0]);
    }
}
