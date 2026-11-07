//! Note formatting (#186): a Note's text is CommonMark (ADR 0010).
//!
//! The text is read by a CommonMark parser (pulldown-cmark, no extensions), so headings,
//! bulleted and numbered lists (numbered as CommonMark numbers them), block quotes,
//! `**bold**`/`__bold__`, `*italic*`/`_italic_`, `[text](target)` links (a target with
//! spaces, such as a Bible reference, in `<…>`), autolinks and `\` escapes follow the spec.
//! Code spans and blocks show their text; raw HTML shows as typed (it is never run). A code
//! span is opaque: one over lines shows its whole text (line breaks read as spaces, as
//! CommonMark reads them) on the line it starts on.
//!
//! What is shown is laid out **per line of the text**: a line break inside a paragraph (a
//! CommonMark soft line break) is a line break where the Note is shown, as it was when Notes
//! were plain text (the way comment boxes on many sites read Markdown), and each blank line
//! is a gap. [`parse_note_text`] gives each line of the body its shown text with its
//! [`NoteMark`]s and a map back to the body, so the editor can colour the source and the
//! Notes Tool can draw the result. A span (bold, a link) that runs over a line break is
//! marked on each line it covers. [`escape_note_text`] turns plain text into a body that
//! shows it exactly as typed: the User database migration runs it over every Note written
//! before formatting existed.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// What one line of a Note is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteLineKind {
    /// Text (its leading spaces kept).
    Text,
    /// Empty or only white space: a gap.
    Blank,
    /// A heading, level 1 to 6.
    Heading(u8),
    /// A bulleted list item; `level` 0 is not nested.
    Bullet { level: u8 },
    /// A numbered list item. `number` is CommonMark's: the list's first item shows the
    /// number typed, and each later item the one before plus one, whatever was typed.
    Numbered { level: u8, number: u32 },
    /// A block-quote line.
    Quote,
}

impl NoteLineKind {
    /// A list item's nesting level.
    pub fn list_level(self) -> Option<u8> {
        match self {
            NoteLineKind::Bullet { level } | NoteLineKind::Numbered { level, .. } => Some(level),
            _ => None,
        }
    }
}

/// How a span of a line's shown text is formatted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum NoteMarkKind {
    Bold,
    Italic,
    /// A link to `target`: a Bible reference or a URL, as written.
    Link(String),
}

/// A formatted span: byte range in [`NoteLine::text`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NoteMark {
    pub range: Range<usize>,
    pub kind: NoteMarkKind,
    /// The whole span in the body, its markup (`**`, `[…](…)`) included, as the parser read
    /// it; the same on each line a span over lines covers.
    pub source: Range<usize>,
}

/// A heading in the body, as the parser read it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NoteHeading {
    /// The whole heading: its `#` markers and closing `#`s, or for a setext heading its
    /// underline (a line of `=` or `-`, itself shown as a gap).
    pub source: Range<usize>,
    /// Its inline content, markup inside it included (`**`, `\` escapes) but none of the
    /// heading's own: from the start of its first inline to the end of its last. Empty, at
    /// the end of `source`, for a heading with no text.
    pub content: Range<usize>,
}

/// One line of a Note, as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteLine {
    pub kind: NoteLineKind,
    /// The text shown: markup removed, escapes resolved.
    pub text: String,
    /// Formatting, by byte range in `text`; ranges nest and never cross.
    pub marks: Vec<NoteMark>,
    /// The line in the body (without its line break).
    pub source: Range<usize>,
    /// Where its content starts in the body, after its heading, list or quote marker.
    pub content: usize,
    /// The heading this line is part of, as the parser read it.
    pub heading: Option<NoteHeading>,
    /// For each byte of `text`, the body bytes it was read from: one byte, or for a
    /// character reference (`&#233;`) the whole reference, so it is one character both ways.
    offsets: Vec<Range<usize>>,
}

impl NoteLine {
    /// Where `range` of [`Self::text`] is in the body (the markup around it excluded).
    pub fn source_range(&self, range: Range<usize>) -> Range<usize> {
        let at = |i: usize| self.offsets.get(i);
        let start = at(range.start).map_or(self.source.end, |o| o.start);
        let end = match range.end.checked_sub(1).and_then(at) {
            Some(last) if range.end > range.start => last.end,
            _ => start,
        };
        start..end.max(start)
    }

    /// The part of [`Self::text`] shown from body bytes `source`: from the first character
    /// read from any of them to the last, whole characters (empty, at the end of the text,
    /// when none is). A character reference counts when any byte of it is in `source`.
    pub fn text_range(&self, source: Range<usize>) -> Range<usize> {
        let mut hit = self.text.char_indices().filter_map(|(k, c)| {
            let n = k + c.len_utf8();
            let from = self.offsets.get(k)?.start;
            let to = self.offsets.get(n - 1)?.end;
            (source.start < to && from < source.end).then_some(k..n)
        });
        match hit.next() {
            Some(first) => first.start..hit.next_back().unwrap_or(first).end,
            None => self.text.len()..self.text.len(),
        }
    }

    /// Which bytes of the line in the body are markup rather than shown text, as ranges.
    pub fn markup(&self) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = Vec::new();
        let mut at = self.source.start;
        for o in &self.offsets {
            if o.start > at {
                out.push(at..o.start);
            }
            at = at.max(o.end);
        }
        if self.source.end > at {
            out.push(at..self.source.end);
        }
        out
    }
}

/// Read a Note body: one [`NoteLine`] per line of the body.
pub fn parse_note_text(body: &str) -> Vec<NoteLine> {
    let mut lines = Vec::new();
    let mut start = 0;
    for raw in body.split('\n') {
        let source = start..start + raw.len();
        lines.push(NoteLine {
            kind: NoteLineKind::Blank,
            text: String::new(),
            marks: Vec::new(),
            content: source.end,
            source,
            heading: None,
            offsets: Vec::new(),
        });
        start += raw.len() + 1;
    }
    let mut b = Builder {
        body,
        kind_set: vec![false; lines.len()],
        first_inline: vec![None; lines.len()],
        lines,
        heading: None,
        headings: Vec::new(),
        quote_depth: 0,
        quotes: Vec::new(),
        lists: Vec::new(),
        open: Vec::new(),
    };
    for (event, r) in Parser::new_ext(body, Options::empty()).into_offset_iter() {
        b.event(event, r);
    }
    b.finish()
}

/// [`parse_note_text`] at work.
struct Builder<'a> {
    body: &'a str,
    lines: Vec<NoteLine>,
    /// Whether a line's kind is decided (by a block starting on it, or its first text).
    kind_set: Vec<bool>,
    /// Where the first inline content of each line starts in the body.
    first_inline: Vec<Option<usize>>,
    /// The level of the heading being read.
    heading: Option<u8>,
    /// Every heading in the body; its content grows as its inlines are read.
    headings: Vec<NoteHeading>,
    quote_depth: usize,
    quotes: Vec<Range<usize>>,
    /// Open lists, innermost last: the number the next item gets if numbered.
    lists: Vec<Option<u64>>,
    /// Open spans: what, its source, and the line and byte of its text they start at.
    open: Vec<(NoteMarkKind, Range<usize>, usize, usize)>,
}

impl Builder<'_> {
    /// The line body byte `at` is on.
    fn line_of(&self, at: usize) -> usize {
        self.lines
            .partition_point(|l| l.source.start <= at)
            .saturating_sub(1)
    }

    fn set_kind(&mut self, at: usize, kind: NoteLineKind) {
        let i = self.line_of(at);
        self.lines[i].kind = kind;
        self.kind_set[i] = true;
    }

    /// Line `i` takes text in the block being read: unless a block starting on it decided
    /// its kind, it is that block's (heading, quote or text).
    fn mark_kind(&mut self, i: usize) {
        if !self.kind_set[i] {
            self.kind_set[i] = true;
            self.lines[i].kind = match (self.heading, self.quote_depth) {
                (Some(n), _) => NoteLineKind::Heading(n),
                (None, 0) => NoteLineKind::Text,
                _ => NoteLineKind::Quote,
            };
        }
    }

    /// An inline read from body bytes `r` (from `r.start` on its line).
    fn inline_at(&mut self, r: Range<usize>) {
        let i = self.line_of(r.start);
        let first = &mut self.first_inline[i];
        *first = Some(first.map_or(r.start, |f| f.min(r.start)));
        if self.heading.is_some()
            && let Some(h) = self.headings.last_mut()
        {
            h.content = if h.content.is_empty() {
                r
            } else {
                h.content.start.min(r.start)..h.content.end.max(r.end)
            };
        }
    }

    fn event(&mut self, event: Event<'_>, r: Range<usize>) {
        match event {
            Event::Start(tag) => self.start(tag, r),
            Event::End(tag) => self.end(tag),
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => {
                // An escaped character: its `\` goes with it.
                let escaped = r.start > 0
                    && self.body.as_bytes()[r.start - 1] == b'\\'
                    && t.starts_with(|c: char| c.is_ascii_punctuation());
                self.inline_at(r.start - usize::from(escaped)..r.end);
                self.text(&t, r, false);
            }
            Event::Code(t) => {
                self.inline_at(r.clone());
                self.text(&t, r.clone(), true);
                // The lines a code span runs on to are its block's, though its text shows
                // on the first.
                for i in self.line_of(r.start) + 1..=self.line_of(r.end.saturating_sub(1)) {
                    self.mark_kind(i);
                }
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>, r: Range<usize>) {
        match tag {
            Tag::Heading { level, .. } => {
                let n = level as u8;
                self.heading = Some(n);
                self.headings.push(NoteHeading {
                    content: r.end..r.end,
                    source: r.clone(),
                });
                self.set_kind(r.start, NoteLineKind::Heading(n));
            }
            Tag::BlockQuote(_) => {
                self.quote_depth += 1;
                self.quotes.push(r.clone());
                self.set_kind(r.start, NoteLineKind::Quote);
            }
            Tag::List(first) => self.lists.push(first),
            Tag::Item => {
                let level = u8::try_from(self.lists.len().saturating_sub(1)).unwrap_or(u8::MAX);
                let kind = match self.lists.last_mut() {
                    Some(Some(next)) => {
                        let number = u32::try_from(*next).unwrap_or(u32::MAX);
                        *next = next.saturating_add(1);
                        NoteLineKind::Numbered { level, number }
                    }
                    _ => NoteLineKind::Bullet { level },
                };
                self.set_kind(r.start, kind);
            }
            Tag::Emphasis | Tag::Strong | Tag::Link { .. } => {
                self.inline_at(r.clone());
                let kind = match tag {
                    Tag::Emphasis => NoteMarkKind::Italic,
                    Tag::Strong => NoteMarkKind::Bold,
                    Tag::Link { dest_url, .. } => NoteMarkKind::Link(dest_url.to_string()),
                    _ => return,
                };
                let line = self.line_of(r.start);
                let at = self.lines[line].text.len();
                self.open.push((kind, r, line, at));
            }
            Tag::Image { .. } => self.inline_at(r),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => self.heading = None,
            TagEnd::BlockQuote(_) => self.quote_depth = self.quote_depth.saturating_sub(1),
            TagEnd::List(_) => {
                self.lists.pop();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link => {
                let Some((kind, source, first, at)) = self.open.pop() else {
                    return;
                };
                // Its text is the last added: it ends on the last line that took text.
                let last = (first..self.lines.len())
                    .rev()
                    .find(|&i| !self.lines[i].text.is_empty() || i == first)
                    .unwrap_or(first);
                for i in first..=last {
                    let from = if i == first { at } else { 0 };
                    let to = self.lines[i].text.len();
                    if from < to {
                        self.lines[i].marks.push(NoteMark {
                            range: from..to,
                            kind: kind.clone(),
                            source: source.clone(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    /// Shown text `t`, read from body bytes `r`: each character goes to the line it was
    /// typed on, and a line break shows as the break itself. A code span is opaque (`code`).
    fn text(&mut self, t: &str, r: Range<usize>, code: bool) {
        let src = &self.body[r.clone()];
        // For each byte of `t`, its byte in the body; `None` for a character reference.
        let reference = src != t && !code && src.starts_with('&') && src.ends_with(';');
        let map: Option<Vec<usize>> = if src == t {
            Some(r.clone().collect())
        } else if code {
            // A code span is opaque: its text is where it is typed when that is one run of
            // its source (`` ` a ` `` shows `a`); otherwise (over lines) the whole text stays
            // on its first line, mapped onto its opening backtick. No position inside the
            // span is worked out from what CommonMark stripped.
            Some(match src.find(t) {
                Some(i) if !t.is_empty() && !src.contains(['\n', '\r']) => {
                    (r.start + i..r.start + i + t.len()).collect()
                }
                _ => vec![r.start; t.len()],
            })
        } else if reference {
            None
        } else {
            src.find(t)
                .map(|i| (r.start + i..r.start + i + t.len()).collect())
        };
        for (k, c) in t.char_indices() {
            // Text that is not in the body as shown (a character reference) is read from
            // all of it: one character, never a part of one.
            let at = |j: usize| map.as_ref().map_or(r.clone(), |m| m[j]..m[j] + 1);
            let o = at(k).start;
            // A line break is the break itself; anything else is kept, on the line it maps to.
            let bytes = self.body.as_bytes();
            let line_break = bytes.get(o) == Some(&b'\n')
                || (bytes.get(o) == Some(&b'\r') && bytes.get(o + 1) == Some(&b'\n'));
            if c.is_whitespace() && line_break {
                continue;
            }
            let i = self.line_of(o);
            self.mark_kind(i);
            let line = &mut self.lines[i];
            line.text.push(c);
            line.offsets.extend((k..k + c.len_utf8()).map(at));
        }
    }

    fn finish(mut self) -> Vec<NoteLine> {
        for (i, line) in self.lines.iter_mut().enumerate() {
            // A `>` line with nothing after it, inside a block quote, is part of the quote.
            let raw = &self.body[line.source.clone()];
            if !self.kind_set[i]
                && raw.trim_start().starts_with('>')
                && self
                    .quotes
                    .iter()
                    .any(|q| q.start <= line.source.start && line.source.start < q.end)
            {
                line.kind = NoteLineKind::Quote;
            }
            line.content = self.first_inline[i]
                .or(line.offsets.first().map(|o| o.start))
                .filter(|c| line.source.contains(c))
                .unwrap_or(line.source.end);
            line.heading = self
                .headings
                .iter()
                .find(|h| h.source.start <= line.source.end && line.source.start < h.source.end)
                .cloned();
            line.marks
                .sort_by_key(|m| (m.range.start, std::cmp::Reverse(m.range.end)));
        }
        self.lines
    }
}

/// An inline span of a Note's body (bold, italic, a code span, a link, an image, an
/// autolink) as the parser read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSpan {
    /// The whole span in the body, its markup included.
    pub source: Range<usize>,
    /// For bold or italic, its delimiter as typed (`*`, `__`): what the Note editor writes to
    /// close it at the end of a line and open it again on the next, so a span over a line
    /// break that becomes a block boundary keeps its formatting on each line. `None` for a
    /// code span, link, image or autolink: cut in two, its parts would not show as the whole
    /// did (a code span's stripped spaces, a link's target), so the editor does not cut it.
    pub delimiter: Option<String>,
}

/// Every [`NoteSpan`] of `body`, by where it starts (an outer span before one inside it).
pub fn note_spans(body: &str) -> Vec<NoteSpan> {
    let mut out = Vec::new();
    for (event, r) in Parser::new_ext(body, Options::empty()).into_offset_iter() {
        let delimited = |n: usize| {
            let open = body.get(r.start..r.start + n)?;
            let close = body.get(r.end.checked_sub(n)?..r.end)?;
            (open == close).then(|| open.to_string())
        };
        // Bold or italic whose delimiters are not as typed at both ends is left out: it is
        // neither cut nor in the way of a change.
        let delimiter = match event {
            Event::Start(Tag::Emphasis) => match delimited(1) {
                Some(d) => Some(d),
                None => continue,
            },
            Event::Start(Tag::Strong) => match delimited(2) {
                Some(d) => Some(d),
                None => continue,
            },
            Event::Code(_) | Event::Start(Tag::Link { .. } | Tag::Image { .. }) => None,
            _ => continue,
        };
        out.push(NoteSpan {
            source: r,
            delimiter,
        });
    }
    out.sort_by_key(|s| (s.source.start, std::cmp::Reverse(s.source.end)));
    out
}

/// A run of what a Note shows, as its CommonMark parse reads it: the Note editor's check
/// that a change shows the same text, formatted the same ([`note_shown`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteShown {
    /// Text, with what it is in or marked with (sorted, each once).
    Text {
        text: String,
        marks: Vec<NoteShownMark>,
    },
    /// A line break, or a block's start or end.
    Break,
}

/// What a [`NoteShown`] run of text is in, or marked with.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NoteShownMark {
    Bold,
    Italic,
    /// A link to its target, as the parser read it.
    Link(String),
    /// An image's text, the image's address.
    Image(String),
    /// A code span's text.
    CodeSpan,
    /// A code block's text (indented or fenced).
    CodeBlock,
    /// Raw HTML, shown as typed.
    Html,
}

/// What `body` shows, as runs of text with their marks between breaks: adjacent text
/// marked the same is one run, breaks next to each other are one, and white space at a
/// break (outside code and HTML) is left out, as a block's edges do not show it. Two bodies
/// with the same runs show the same text, formatted the same, line breaks and blocks
/// (lists, quotes, headings) aside.
pub fn note_shown(body: &str) -> Vec<NoteShown> {
    note_shown_except(body, |_| false)
}

/// [`note_shown`] with the marks `except` picks taken off first (text marked only so is
/// plain text, joined with the text around it): what a change that puts in or takes off
/// such marks must keep.
pub fn note_shown_except(body: &str, except: impl Fn(&NoteShownMark) -> bool) -> Vec<NoteShown> {
    let mut raw: Vec<NoteShown> = Vec::new();
    // What the text being read is in, innermost last; `None` for a block or a tag that
    // marks nothing.
    let mut open: Vec<Option<NoteShownMark>> = Vec::new();
    let marks = |open: &[Option<NoteShownMark>], with: Option<NoteShownMark>| {
        let mut m: Vec<NoteShownMark> = open
            .iter()
            .flatten()
            .cloned()
            .chain(with)
            .filter(|m| !except(m))
            .collect();
        m.sort();
        m.dedup();
        m
    };
    for event in Parser::new_ext(body, Options::empty()) {
        let run = match event {
            Event::Start(tag) => {
                let (mark, block) = match tag {
                    Tag::Emphasis => (Some(NoteShownMark::Italic), false),
                    Tag::Strong => (Some(NoteShownMark::Bold), false),
                    Tag::Link { dest_url, .. } => {
                        (Some(NoteShownMark::Link(dest_url.to_string())), false)
                    }
                    Tag::Image { dest_url, .. } => {
                        (Some(NoteShownMark::Image(dest_url.to_string())), false)
                    }
                    Tag::CodeBlock(_) => (Some(NoteShownMark::CodeBlock), true),
                    Tag::HtmlBlock => (Some(NoteShownMark::Html), true),
                    _ => (None, true),
                };
                open.push(mark);
                if block {
                    raw.push(NoteShown::Break);
                }
                continue;
            }
            Event::End(tag) => {
                open.pop();
                if !matches!(
                    tag,
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link | TagEnd::Image
                ) {
                    raw.push(NoteShown::Break);
                }
                continue;
            }
            Event::Text(t) => (t, None),
            Event::Code(t) => (t, Some(NoteShownMark::CodeSpan)),
            Event::Html(t) | Event::InlineHtml(t) => (t, Some(NoteShownMark::Html)),
            _ => {
                raw.push(NoteShown::Break);
                continue;
            }
        };
        raw.push(NoteShown::Text {
            text: run.0.to_string(),
            marks: marks(&open, run.1),
        });
    }
    shown_runs(raw)
}

/// [`note_shown`]'s runs: merged, white space at breaks left out, one break between runs.
fn shown_runs(raw: Vec<NoteShown>) -> Vec<NoteShown> {
    let verbatim = |m: &[NoteShownMark]| {
        m.iter().any(|m| {
            matches!(
                m,
                NoteShownMark::CodeSpan | NoteShownMark::CodeBlock | NoteShownMark::Html
            )
        })
    };
    // Merged, with one break between runs of text.
    let mut merged: Vec<NoteShown> = Vec::new();
    for r in raw {
        match (merged.last_mut(), r) {
            (Some(NoteShown::Break), NoteShown::Break) | (None, NoteShown::Break) => {}
            (Some(NoteShown::Text { text, marks }), NoteShown::Text { text: t, marks: m })
                if *marks == m =>
            {
                text.push_str(&t)
            }
            (_, r) => merged.push(r),
        }
    }
    // White space at a break, outside code and HTML, is not shown.
    let n = merged.len();
    for i in 0..n {
        let before = i == 0 || merged[i - 1] == NoteShown::Break;
        let after = i + 1 == n || merged[i + 1] == NoteShown::Break;
        if let NoteShown::Text { text, marks } = &mut merged[i]
            && !verbatim(marks)
        {
            if after {
                text.truncate(text.trim_end().len());
            }
            if before {
                *text = text.trim_start().to_string();
            }
        }
    }
    let mut out: Vec<NoteShown> = Vec::new();
    for r in merged {
        match r {
            NoteShown::Text { text, .. } if text.is_empty() => {}
            NoteShown::Break if matches!(out.last(), None | Some(NoteShown::Break)) => {}
            r => out.push(r),
        }
    }
    if out.last() == Some(&NoteShown::Break) {
        out.pop();
    }
    out
}

/// A block of a Note, as its CommonMark parse reads it ([`note_outline`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoteBlockKind {
    Paragraph,
    Heading(u8),
    Quote,
    /// A list: numbered from its first number, or bulleted (`None`).
    List(Option<u64>),
    Item,
    Code,
    Html,
    Rule,
}

/// Every block of `body` in order, each with how deep it is (0 for a top-level block): the
/// blocks a change that only marks text (bold, italic, a link) must leave as they are.
pub fn note_outline(body: &str) -> Vec<(usize, NoteBlockKind)> {
    let mut out = Vec::new();
    // Whether each open tag is a block.
    let mut open: Vec<bool> = Vec::new();
    for event in Parser::new_ext(body, Options::empty()) {
        let depth = open.iter().filter(|b| **b).count();
        let kind = match event {
            Event::Start(tag) => {
                let kind = match tag {
                    Tag::Paragraph => Some(NoteBlockKind::Paragraph),
                    Tag::Heading { level, .. } => Some(NoteBlockKind::Heading(level as u8)),
                    Tag::BlockQuote(_) => Some(NoteBlockKind::Quote),
                    Tag::List(first) => Some(NoteBlockKind::List(first)),
                    Tag::Item => Some(NoteBlockKind::Item),
                    Tag::CodeBlock(_) => Some(NoteBlockKind::Code),
                    Tag::HtmlBlock => Some(NoteBlockKind::Html),
                    _ => None,
                };
                open.push(kind.is_some());
                kind
            }
            Event::End(_) => {
                open.pop();
                None
            }
            Event::Rule => Some(NoteBlockKind::Rule),
            _ => None,
        };
        out.extend(kind.map(|k| (depth, k)));
    }
    out
}

/// The parts of `body` the Note editor's formats do not act on: each top-level block that
/// is not simple, and each line that is no block's (a link reference definition). Simple
/// is a paragraph or heading; a block quote of paragraphs; a list whose items hold
/// paragraphs and, one level down, a list whose items hold paragraphs. Anything else (an
/// indented or fenced code block, raw HTML, a rule, a quote or heading inside a list or
/// quote, a list nested deeper) reads its lines in ways a marker put in or taken out could
/// change, so the formats leave it as it is.
pub fn note_unformattable(body: &str) -> Vec<Range<usize>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Block {
        Quote,
        List,
        Item,
        Leaf,
        Inline,
    }
    let parser = Parser::new_ext(body, Options::empty());
    let definitions: Vec<Range<usize>> = parser
        .reference_definitions()
        .iter()
        .map(|(_, d)| d.span.clone())
        .collect();
    let mut out = Vec::new();
    let mut tops: Vec<Range<usize>> = Vec::new();
    let mut open: Vec<Block> = Vec::new();
    for (event, r) in parser.into_offset_iter() {
        let blocks: Vec<Block> = open
            .iter()
            .copied()
            .filter(|b| *b != Block::Inline)
            .collect();
        let (simple, kind) = match &event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => (
                    matches!(
                        blocks.as_slice(),
                        [] | [Block::Quote]
                            | [Block::List, Block::Item]
                            | [Block::List, Block::Item, Block::List, Block::Item]
                    ),
                    Block::Leaf,
                ),
                Tag::Heading { .. } => (blocks.is_empty(), Block::Leaf),
                Tag::BlockQuote(_) => (blocks.is_empty(), Block::Quote),
                Tag::List(_) => (
                    matches!(blocks.as_slice(), [] | [Block::List, Block::Item]),
                    Block::List,
                ),
                Tag::Item => (true, Block::Item),
                Tag::Emphasis | Tag::Strong | Tag::Link { .. } | Tag::Image { .. } => {
                    (true, Block::Inline)
                }
                _ => (false, Block::Leaf),
            },
            Event::End(_) => {
                open.pop();
                continue;
            }
            Event::Rule => (false, Block::Leaf),
            _ => (true, Block::Inline),
        };
        if blocks.is_empty() && kind != Block::Inline {
            tops.push(r.clone());
        }
        if !simple {
            out.push(tops.last().filter(|t| t.end >= r.end).cloned().unwrap_or(r));
        }
        if matches!(event, Event::Start(_)) {
            open.push(kind);
        }
    }
    // A link reference definition: the block it is in, or its own lines.
    for d in definitions {
        let top = tops.iter().find(|t| t.start <= d.start && d.end <= t.end);
        out.push(top.cloned().unwrap_or(d));
    }
    // A line with text that no block reads (one of several definitions with one label).
    let mut start = 0;
    for line in body.split('\n') {
        let r = start..start + line.len();
        start = r.end + 1;
        if !line.trim().is_empty() && !tops.iter().any(|t| t.start < r.end && r.start < t.end) {
            out.push(r);
        }
    }
    out.sort_by_key(|r| (r.start, r.end));
    out.dedup();
    out
}

/// The text a Note shows, markup removed: one line per line (for search and filtering).
pub fn note_plain_text(body: &str) -> String {
    parse_note_text(body)
        .into_iter()
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `plain` already shows exactly as typed: each line as text (or a gap), unmarked.
fn shows_as_typed(plain: &str) -> bool {
    let lines = parse_note_text(plain);
    lines.iter().zip(plain.split('\n')).all(|(l, typed)| {
        if typed.trim().is_empty() {
            l.kind == NoteLineKind::Blank
        } else {
            l.kind == NoteLineKind::Text && l.text == typed && l.marks.is_empty()
        }
    })
}

/// A body that shows `plain` exactly as typed, one text line per line. Plain text that
/// already reads so is left as it is; otherwise each line has what CommonMark would read as
/// markup backslash-escaped, and its leading and trailing spaces written as character
/// references (`&#32;`), which CommonMark would otherwise drop.
pub fn escape_note_text(plain: &str) -> String {
    if shows_as_typed(plain) {
        return plain.to_string();
    }
    plain
        .split('\n')
        .map(escape_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape_line(line: &str) -> String {
    if line.trim().is_empty() {
        return line.to_string();
    }
    let entity = |c: char| if c == '\t' { "&#9;" } else { "&#32;" };
    let body = line.trim_start_matches([' ', '\t']);
    let core = body.trim_end_matches([' ', '\t']);
    let mut out = String::with_capacity(line.len() + 8);
    out.extend(line[..line.len() - body.len()].chars().map(entity));
    // What would start a block at the start of a line: a heading, quote, list item, setext
    // underline or code fence; a numbered item's `.` or `)`.
    let digits = core.bytes().take_while(u8::is_ascii_digit).count();
    let numbered = digits > 0 && core[digits..].starts_with(['.', ')']);
    let mut chars = core.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let next = chars.peek().map(|(_, n)| *n);
        let escape = match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' => true,
            // A character reference (`&amp;`) would show as the character.
            '&' => next.is_some_and(|n| n.is_alphanumeric() || n == '#'),
            '#' | '>' | '-' | '+' | '=' | '~' => i == 0,
            '.' | ')' => numbered && i == digits,
            _ => false,
        };
        if escape {
            out.push('\\');
        }
        out.push(c);
    }
    out.extend(body[core.len()..].chars().map(entity));
    out
}

#[cfg(test)]
#[path = "note_text_tests.rs"]
mod tests;
