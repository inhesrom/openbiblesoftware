//! Note formatting (#186): a Note's text is CommonMark (ADR 0010).
//!
//! The text is read by a CommonMark parser (pulldown-cmark, no extensions), so headings,
//! bulleted and numbered lists (numbered as CommonMark numbers them), block quotes,
//! `**bold**`/`__bold__`, `*italic*`/`_italic_`, `[text](target)` links (a target with
//! spaces, such as a Bible reference, in `<…>`), autolinks and `\` escapes follow the spec.
//! Code spans and blocks show their text; raw HTML shows as typed (it is never run).
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

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

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
    /// For each byte of `text`, its byte in the body.
    offsets: Vec<usize>,
}

impl NoteLine {
    /// Where `range` of [`Self::text`] is in the body (the markup around it excluded).
    pub fn source_range(&self, range: Range<usize>) -> Range<usize> {
        let at = |i: usize| self.offsets.get(i).copied();
        let start = at(range.start).unwrap_or(self.source.end);
        let end = match range.end.checked_sub(1).and_then(at) {
            Some(last) if range.end > range.start => last + 1,
            _ => start,
        };
        start..end.max(start)
    }

    /// The part of [`Self::text`] shown from body bytes `source`: from the first shown byte
    /// typed in it to the last (empty, at the end of the text, when none is).
    pub fn text_range(&self, source: Range<usize>) -> Range<usize> {
        let inside = |o: &usize| source.contains(o);
        match (
            self.offsets.iter().position(inside),
            self.offsets.iter().rposition(inside),
        ) {
            (Some(first), Some(last)) => first..last + 1,
            _ => self.text.len()..self.text.len(),
        }
    }

    /// Which bytes of the line in the body are markup rather than shown text, as ranges.
    pub fn markup(&self) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = Vec::new();
        let mut at = self.source.start;
        for &o in &self.offsets {
            if o > at {
                out.push(at..o);
            }
            at = at.max(o + 1);
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
                self.text(&t, r, true);
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
    /// typed on, and a line break (a space in a code span) shows as the break itself.
    fn text(&mut self, t: &str, r: Range<usize>, code: bool) {
        let src = &self.body[r.clone()];
        // For each byte of `t`, its byte in the body.
        let map: Option<Vec<usize>> = if src == t {
            Some(r.clone().collect())
        } else if code {
            // A span the alignment cannot follow stays whole on its first line, mapped onto
            // its opening backtick, rather than split where a guessed offset falls.
            Some(code_text_offsets(src, t).map_or_else(
                || vec![r.start; t.len()],
                |m| m.into_iter().map(|o| r.start + o).collect(),
            ))
        } else {
            src.find(t)
                .map(|i| (r.start + i..r.start + i + t.len()).collect())
        };
        let last = r.end.saturating_sub(1).max(r.start);
        for (k, c) in t.char_indices() {
            // Text that is not in the body as shown (a character reference) maps onto it.
            let at = |j: usize| map.as_ref().map_or((r.start + j).min(last), |m| m[j]);
            let o = at(k);
            // A line break (or the space a code span reads it as) is the break itself;
            // anything else is kept, on the line it maps to.
            let bytes = self.body.as_bytes();
            let line_break = bytes.get(o) == Some(&b'\n')
                || (bytes.get(o) == Some(&b'\r') && bytes.get(o + 1) == Some(&b'\n'));
            if c.is_whitespace() && line_break {
                continue;
            }
            let i = self.line_of(o);
            if !self.kind_set[i] {
                self.kind_set[i] = true;
                self.lines[i].kind = match (self.heading, self.quote_depth) {
                    (Some(n), _) => NoteLineKind::Heading(n),
                    (None, 0) => NoteLineKind::Text,
                    _ => NoteLineKind::Quote,
                };
            }
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
                .or(line.offsets.first().copied())
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

/// An inline span of a Note's body (bold, italic, a link, a code span) as the parser read
/// it, with the markup that opens and closes a part of it: what the Note editor writes to
/// cut a span in two at a line break that becomes a block boundary, so each part shows as
/// the whole did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSpan {
    /// The whole span in the body, its markup included.
    pub source: Range<usize>,
    /// What opens a part: `*`, `__`, `[`, a code span's backticks.
    pub open: String,
    /// What closes a part as the whole is closed: `*`, `__`, a code span's backticks, a
    /// link's `](target "title")` or `][label]` as typed; for a link written `[text]` or
    /// `[text][]` (whose label is its text), `](<target> "title")`.
    pub close: String,
    /// Where the span's own closing markup is in the body.
    pub closing: Range<usize>,
}

/// Every [`NoteSpan`] of `body`, by where it starts (an outer span before one inside it).
/// Autolinks (`<https://…>`) are left out: they cannot run over a line break.
pub fn note_spans(body: &str) -> Vec<NoteSpan> {
    // Open links: the link, its closing markup when it is rewritten, and where its text ends.
    let mut links: Vec<(Range<usize>, Option<String>, usize)> = Vec::new();
    let mut out = Vec::new();
    for (event, r) in Parser::new_ext(body, Options::empty()).into_offset_iter() {
        if !matches!(event, Event::End(TagEnd::Link)) {
            for link in &mut links {
                link.2 = link.2.max(r.end);
            }
        }
        let delimited = |n: usize| {
            let open = body.get(r.start..r.start + n)?.to_string();
            let closing = r.end.checked_sub(n)?..r.end;
            (body.get(closing.clone())? == open).then(|| NoteSpan {
                source: r.clone(),
                close: open.clone(),
                open,
                closing,
            })
        };
        let span = match event {
            Event::Start(Tag::Emphasis) => delimited(1),
            Event::Start(Tag::Strong) => delimited(2),
            Event::Code(_) => {
                delimited(body[r.clone()].len() - body[r.clone()].trim_start_matches('`').len())
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            }) => {
                let close = match link_type {
                    LinkType::Inline | LinkType::Reference => None,
                    _ => {
                        let esc = |s: &str, special: &[char]| {
                            s.chars().fold(String::new(), |mut o, c| {
                                if special.contains(&c) {
                                    o.push('\\');
                                }
                                o.push(c);
                                o
                            })
                        };
                        let title = if title.is_empty() {
                            String::new()
                        } else {
                            format!(" \"{}\"", esc(&title, &['\\', '"']))
                        };
                        Some(format!("](<{}>{title})", esc(&dest_url, &['\\', '<', '>'])))
                    }
                };
                links.push((r.clone(), close, r.start + 1));
                None
            }
            Event::End(TagEnd::Link) => links.pop().and_then(|(source, close, text_end)| {
                let closing = text_end..source.end;
                let typed = body.get(closing.clone()).filter(|c| c.starts_with(']'))?;
                Some(NoteSpan {
                    open: "[".into(),
                    close: close.unwrap_or_else(|| typed.to_string()),
                    source,
                    closing,
                })
            }),
            _ => None,
        };
        out.extend(span);
    }
    out.sort_by_key(|s| (s.source.start, std::cmp::Reverse(s.source.end)));
    out
}

/// Where each byte of a code span's text `t` is in its source `src` (backticks included).
/// The text is the source with bytes taken out, never added or reordered: CommonMark strips
/// the backticks and one space each side when both are there, and the parser takes the next
/// line's container prefix (block-quote markers, list indent) off before reading the span,
/// keeping any white space after it as typed. A line break reads as a space (pulldown-cmark
/// reads each of `\r\n` as one), which maps onto the break. So each byte of `t` maps onto
/// the first byte after the last one mapped that it can be, skipping what was taken out
/// (white space or `>` before a line's content, so literal indent is kept as shown text).
/// `None` when `t` does not fit `src` so.
fn code_text_offsets(src: &str, t: &str) -> Option<Vec<usize>> {
    let s = src.as_bytes();
    let ticks = s.len() - src.trim_start_matches('`').len();
    let end = s.len().checked_sub(ticks)?;
    let mut at = ticks;
    let mut out = Vec::with_capacity(t.len());
    for &c in t.as_bytes() {
        let fits = |b: u8| b == c || (c == b' ' && matches!(b, b'\n' | b'\r'));
        at += s.get(at..end)?.iter().position(|&b| fits(b))?;
        out.push(at);
        at += 1;
    }
    Some(out)
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
