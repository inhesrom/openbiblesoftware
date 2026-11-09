//! A unit's text under construction: whitespace collapsing, NFC, and spans by byte range.

use std::ops::Range;

use obs_model::SpanKind;
use unicode_normalization::UnicodeNormalization;

/// Builds one text unit from source text pieces. Runs of whitespace collapse to one space;
/// whitespace at the start or end of a block is dropped; blocks are separated by `\n`.
#[derive(Debug, Default)]
pub(crate) struct TextBuf {
    pub text: String,
    pub spans: Vec<(Range<usize>, SpanKind)>,
    pending_space: bool,
    at_block_start: bool,
    /// Whether text pushed now extends [`tracked_end`](Self::tracked_end).
    pub track: bool,
    /// End of the last non-space text pushed while `track` was on.
    pub tracked_end: usize,
}

impl TextBuf {
    pub fn new() -> Self {
        Self {
            at_block_start: true,
            track: true,
            ..Self::default()
        }
    }

    /// Where the next visible character would go if no space is pending.
    pub fn pos(&self) -> usize {
        self.text.len()
    }

    /// Start a new block (paragraph, heading); returns its start offset.
    pub fn start_block(&mut self) -> usize {
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        self.pending_space = false;
        self.at_block_start = true;
        self.text.len()
    }

    /// Emit a pending space now, so whatever starts next begins after it.
    pub fn flush_space(&mut self) {
        if self.pending_space && !self.at_block_start {
            self.text.push(' ');
        }
        self.pending_space = false;
    }

    /// Drop whitespace until the next visible character (after a verse number).
    pub fn suppress_space(&mut self) {
        self.pending_space = false;
        self.at_block_start = true;
    }

    pub fn push_text(&mut self, piece: &str) {
        for c in piece.nfc() {
            if c.is_whitespace() {
                if !self.at_block_start {
                    self.pending_space = true;
                }
                continue;
            }
            self.flush_space();
            self.text.push(c);
            self.at_block_start = false;
            if self.track {
                self.tracked_end = self.text.len();
            }
        }
    }

    pub fn add_span(&mut self, range: Range<usize>, kind: SpanKind) {
        self.spans.push((range, kind));
    }

    /// Open a Dictionary entry with its headword (see [`lead_with_headword`]).
    pub fn lead_with_headword(&mut self, headword: &str) {
        lead_with_headword(&mut self.text, &mut self.spans, headword);
    }
}

/// The level of the heading a Dictionary entry opens with: Easton's TEI `<title>` level.
pub(crate) const HEADWORD_LEVEL: u8 = 2;

/// Open a Dictionary entry with its headword as a heading block, unless the entry already
/// opens with a heading (Easton's TEI titles itself). A Dictionary Tab draws only an entry's
/// text, so without it Hitchcock's or Smith's entries never say which name they explain
/// (#261). Returns how far the existing text moved.
pub(crate) fn lead_with_headword(
    text: &mut String,
    spans: &mut Vec<(Range<usize>, SpanKind)>,
    headword: &str,
) -> usize {
    let headword: String = headword.split_whitespace().collect::<Vec<_>>().join(" ");
    let headed = spans
        .iter()
        .any(|(range, kind)| range.start == 0 && matches!(kind, SpanKind::Heading { .. }));
    if headword.is_empty() || headed {
        return 0;
    }
    let headword: String = headword.nfc().collect();
    let shift = if text.is_empty() {
        headword.len()
    } else {
        headword.len() + 1
    };
    for (range, _) in spans.iter_mut() {
        *range = range.start + shift..range.end + shift;
    }
    spans.insert(
        0,
        (
            0..headword.len(),
            SpanKind::Heading {
                level: HEADWORD_LEVEL,
            },
        ),
    );
    let mut led = headword;
    if !text.is_empty() {
        led.push('\n');
    }
    led.push_str(text);
    *text = led;
    shift
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_and_trims_whitespace() {
        let mut b = TextBuf::new();
        b.push_text("  In the \n beginning  ");
        b.push_text("God. ");
        let start = b.start_block();
        b.push_text("   And ");
        assert_eq!(b.text, "In the beginning God.\nAnd");
        assert_eq!(start, 22);
    }

    #[test]
    fn pending_space_is_emitted_before_the_next_text() {
        let mut b = TextBuf::new();
        b.push_text("a ");
        b.push_text(" b");
        assert_eq!(b.text, "a b");
        assert_eq!(b.tracked_end, 3);
    }

    #[test]
    fn a_dictionary_entry_opens_with_its_headword_unless_it_has_a_heading() {
        let mut b = TextBuf::new();
        b.push_text("the destroyer");
        b.add_span(0..13, SpanKind::Italic);
        b.lead_with_headword("ABADDON");
        assert_eq!(b.text, "ABADDON\nthe destroyer");
        assert_eq!(
            b.spans,
            [
                (0..7, SpanKind::Heading { level: 2 }),
                (8..21, SpanKind::Italic)
            ]
        );
        // Easton's entries title themselves: nothing is added.
        b.lead_with_headword("Abaddon");
        assert_eq!(b.text, "ABADDON\nthe destroyer");
        assert_eq!(b.spans.len(), 2);
    }

    #[test]
    fn nfc_on_push() {
        let mut b = TextBuf::new();
        b.push_text("e\u{301}");
        assert_eq!(b.text, "\u{e9}");
    }
}
