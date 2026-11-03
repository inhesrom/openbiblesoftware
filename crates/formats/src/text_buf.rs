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
    fn nfc_on_push() {
        let mut b = TextBuf::new();
        b.push_text("e\u{301}");
        assert_eq!(b.text, "\u{e9}");
    }
}
