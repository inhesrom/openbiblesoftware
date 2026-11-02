//! Interval lookup over a unit's interactive spans: what is under a hit (#14 rule 8).

use obs_model::{Span, SpanKind, TextUnit};

/// A span the pointer can act on.
fn interactive(kind: &SpanKind) -> bool {
    matches!(
        kind,
        SpanKind::TaggedWord { .. } | SpanKind::ScriptureRef { .. } | SpanKind::Xref { .. }
    )
}

/// Interactive spans of one unit, sorted by start, for fast "what is at byte b?" queries.
#[derive(Debug, Clone)]
pub struct SpanIndex {
    /// Indexes into the unit's spans, by start.
    items: Vec<(u32, u32, usize)>,
    /// Longest interactive span, which bounds how far back a query looks.
    max_len: u32,
}

impl SpanIndex {
    pub fn new(unit: &TextUnit) -> Self {
        let mut items: Vec<(u32, u32, usize)> = unit
            .spans
            .iter()
            .enumerate()
            .filter(|(_, s)| interactive(&s.kind) && s.end > s.start)
            .map(|(i, s)| (s.start, s.end, i))
            .collect();
        items.sort_unstable();
        let max_len = items.iter().map(|(a, b, _)| b - a).max().unwrap_or(0);
        Self { items, max_len }
    }

    /// Interactive spans covering unit byte `b`, innermost (shortest) first.
    pub fn at<'u>(&self, unit: &'u TextUnit, b: usize) -> Vec<&'u Span> {
        let b = u32::try_from(b).unwrap_or(u32::MAX);
        let from = b.saturating_sub(self.max_len);
        let lo = self.items.partition_point(|(s, _, _)| *s < from);
        let hi = self.items.partition_point(|(s, _, _)| *s <= b);
        let mut out: Vec<&Span> = self.items[lo..hi]
            .iter()
            .filter(|(s, e, _)| *s <= b && b < *e)
            .filter_map(|(_, _, i)| unit.spans.get(*i))
            .collect();
        out.sort_by_key(|s| s.end - s.start);
        out
    }
}
