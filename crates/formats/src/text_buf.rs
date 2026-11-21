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
                // A space already emitted (`flush_space`, as a styled span opens) is this
                // run's one space: `word <i> is</i>` keeps one space before "is" (#305).
                if !self.at_block_start && !self.text.ends_with(' ') {
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

    /// Drop everything from `start` (a block's start, as [`Self::start_block`] returned it)
    /// to the end, with the line break before it and the spans starting in it; the next text
    /// starts a block.
    pub fn truncate_to(&mut self, start: usize) {
        let start = start.min(self.text.len());
        let cut = if start > 0 && self.text.as_bytes()[start - 1] == b'\n' {
            start - 1
        } else {
            start
        };
        self.text.truncate(cut);
        self.spans.retain(|(r, _)| r.start < start);
        for (r, _) in &mut self.spans {
            r.end = r.end.min(self.text.len());
        }
        self.pending_space = false;
        self.at_block_start = true;
        self.tracked_end = self.tracked_end.min(self.text.len());
    }

    /// Put the text in NFC where pieces pushed apart left it otherwise (a combining mark in
    /// an element of its own after its letter: TCP's `<g ref="char:cmbAbbrStroke">̄</g>`), and
    /// move the spans with it. Each letter is recomposed with the marks after it; an offset
    /// inside a letter and its marks moves to the end of what they became.
    pub fn normalize_nfc(&mut self) {
        use unicode_normalization::char::canonical_combining_class;
        if unicode_normalization::is_nfc(&self.text) {
            return;
        }
        let text = std::mem::take(&mut self.text);
        let mut out = String::with_capacity(text.len());
        // (old start, new start) of each letter and the marks after it.
        let mut clusters: Vec<(usize, usize)> = Vec::new();
        let mut chars = text.char_indices().peekable();
        while let Some((start, _)) = chars.next() {
            while chars
                .peek()
                .is_some_and(|(_, c)| canonical_combining_class(*c) != 0)
            {
                chars.next();
            }
            let end = chars.peek().map_or(text.len(), |(i, _)| *i);
            clusters.push((start, out.len()));
            out.extend(text[start..end].nfc());
        }
        clusters.push((text.len(), out.len()));
        let map = |offset: usize| -> usize {
            let i = clusters.partition_point(|(old, _)| *old <= offset);
            let (old, new) = clusters[i.saturating_sub(1)];
            if old == offset {
                new
            } else {
                clusters.get(i).map_or(out.len(), |(_, next)| *next)
            }
        };
        for (range, _) in &mut self.spans {
            *range = map(range.start)..map(range.end);
        }
        self.tracked_end = map(self.tracked_end.min(text.len()));
        // Letters that compose across clusters (Hangul jamo) are left as they are: rare, and
        // still valid text.
        self.text = if unicode_normalization::is_nfc(&out) {
            out
        } else {
            let whole: String = out.nfc().collect();
            if whole.len() == out.len() { whole } else { out }
        };
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

    /// READING-7 (#305): a styled span opened after a space (`flush_space`) whose own text
    /// starts with a space adds no second space.
    #[test]
    fn a_span_opened_after_a_space_that_starts_with_a_space_adds_no_second_one() {
        let mut b = TextBuf::new();
        b.push_text("Thy word ");
        b.flush_space();
        let start = b.pos();
        b.push_text(" is");
        let end = b.pos();
        b.push_text(" a lamp");
        assert_eq!(b.text, "Thy word is a lamp");
        assert_eq!(&b.text[start..end], "is");
    }

    #[test]
    fn nfc_on_push() {
        let mut b = TextBuf::new();
        b.push_text("e\u{301}");
        assert_eq!(b.text, "\u{e9}");
    }
}
