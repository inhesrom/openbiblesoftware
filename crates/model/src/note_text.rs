//! Note formatting (#186): the Markdown subset a Note's text is written in (ADR 0010).
//!
//! A Note body is read **line by line**: every line break in the text is a line break where it
//! is shown, as it was when Notes were plain text, and a blank line is a gap.
//!
//! - **Line kinds**: `# `, `## ` … `###### ` start a heading; `- `, `* ` or `+ ` a bulleted
//!   item and `1. ` or `1) ` a numbered item (indented two spaces per level under the item
//!   above); `>` a block-quote line. Anything else is text, its leading spaces kept.
//! - **Inline**: `**bold**` and `__bold__`, `*italic*` and `_italic_` (CommonMark's flanking
//!   rules, so `snake_case` and `2 * 3 * 4` stay as typed), `[text](target)` links, whose
//!   target is a Bible reference or a URL, and `\` before ASCII punctuation for the character
//!   itself. A `\` ending a line is CommonMark's hard line break, which every line break is
//!   here, so it shows nothing. Nothing else is markup: no code, HTML, tables or images.
//!
//! [`parse_note_text`] gives each line's shown text with its [`NoteMark`]s and a map back to
//! the body, so the editor can colour the source and the Notes Tool can draw the result.
//! [`escape_note_text`] turns plain text into a body that shows it exactly as typed: the User
//! database migration runs it over every Note written before formatting existed.

use std::ops::Range;

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
    /// A numbered list item, shown with the number typed.
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

/// Read a Note body: one [`NoteLine`] per line.
pub fn parse_note_text(body: &str) -> Vec<NoteLine> {
    let mut lines = Vec::new();
    let mut start = 0;
    // The list level of the line above, if it was a list item.
    let mut above: Option<u8> = None;
    for raw in body.split('\n') {
        let line = parse_line(raw, start, above);
        above = line.kind.list_level();
        start += raw.len() + 1;
        lines.push(line);
    }
    lines
}

/// The text a Note shows, markup removed: one line per line (for search and filtering).
pub fn note_plain_text(body: &str) -> String {
    parse_note_text(body)
        .into_iter()
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A body that shows `plain` exactly as typed: each line as text, with what would be markup
/// backslash-escaped. Lines that already read as plain text are left as they are.
pub fn escape_note_text(plain: &str) -> String {
    plain
        .split('\n')
        .map(|line| {
            let parsed = parse_line(line, 0, None);
            let as_typed = match parsed.kind {
                NoteLineKind::Blank => true,
                NoteLineKind::Text => parsed.text == line && parsed.marks.is_empty(),
                _ => false,
            };
            if as_typed {
                line.to_string()
            } else {
                escape_line(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape_line(line: &str) -> String {
    let body = line.trim_start_matches([' ', '\t']);
    let mut out = String::with_capacity(line.len() + 8);
    out.push_str(&line[..line.len() - body.len()]);
    let mut rest = body;
    if let Some(c) = rest
        .chars()
        .next()
        .filter(|c| matches!(c, '#' | '>' | '-' | '+'))
    {
        out.push('\\');
        out.push(c);
        rest = &rest[1..];
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && rest[digits..].starts_with(['.', ')']) {
            out.push_str(&rest[..digits]);
            out.push('\\');
            rest = &rest[digits..];
        }
    }
    for c in rest.chars() {
        if matches!(c, '\\' | '*' | '_' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Leading spaces (a tab counts as four) and the rest.
fn indent(raw: &str) -> (usize, usize) {
    let mut cols = 0;
    let mut bytes = 0;
    for c in raw.chars() {
        match c {
            ' ' => cols += 1,
            '\t' => cols += 4,
            _ => break,
        }
        bytes += 1;
    }
    (cols, bytes)
}

fn parse_line(raw: &str, start: usize, above: Option<u8>) -> NoteLine {
    let source = start..start + raw.len();
    if raw.trim().is_empty() {
        return NoteLine {
            kind: NoteLineKind::Blank,
            text: String::new(),
            marks: Vec::new(),
            content: source.start,
            source,
            offsets: Vec::new(),
        };
    }
    // Text keeps its leading spaces: its content is the whole line.
    let (kind, content) = line_kind(raw, above);
    let (text, marks, offsets) = parse_inline(&raw[content..], start + content);
    NoteLine {
        kind,
        text,
        marks,
        source,
        content: start + content,
        offsets,
    }
}

/// The line's kind and the byte where its content starts.
fn line_kind(raw: &str, above: Option<u8>) -> (NoteLineKind, usize) {
    let hashes = raw.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&hashes) && raw[hashes..].starts_with(' ') {
        return (NoteLineKind::Heading(hashes as u8), hashes + 1);
    }
    if let Some(rest) = raw.strip_prefix('>') {
        let skip = usize::from(rest.starts_with(' '));
        return (NoteLineKind::Quote, 1 + skip);
    }
    let (cols, lead) = indent(raw);
    // Nested two columns per level, at most one level deeper than the item above.
    let level = match above {
        Some(a) => (cols / 2).min(usize::from(a) + 1),
        None if cols < 2 => 0,
        None => return (NoteLineKind::Text, 0),
    };
    let level = level.min(8) as u8;
    let body = &raw[lead..];
    if body.len() >= 2 && body.starts_with(['-', '*', '+']) && body.as_bytes()[1] == b' ' {
        return (NoteLineKind::Bullet { level }, lead + 2);
    }
    let digits = body.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits)
        && body[digits..].starts_with(['.', ')'])
        && body[digits + 1..].starts_with(' ')
        && let Ok(number) = body[..digits].parse()
    {
        return (NoteLineKind::Numbered { level, number }, lead + digits + 2);
    }
    (NoteLineKind::Text, 0)
}

/// A run of `*` or `_` while emphasis is matched.
struct Delims {
    ch: char,
    /// Its first byte in the body, and its length.
    at: usize,
    len: usize,
    open: bool,
    close: bool,
    /// Taken from its start by the emphasis it closes, and from its end by what it opens.
    closed: usize,
    opened: usize,
    /// The emphasis it closes and opens, by match index.
    closes: Vec<usize>,
    opens: Vec<usize>,
}

impl Delims {
    fn left(&self) -> usize {
        self.len - self.closed - self.opened
    }
}

/// A piece of a line before emphasis is matched.
enum Piece {
    /// Shown text, each byte with its byte in the body.
    Text(String, Vec<usize>),
    Delims(Delims),
    /// A link, its text already parsed.
    Link {
        text: String,
        marks: Vec<NoteMark>,
        offsets: Vec<usize>,
        target: String,
    },
}

fn is_punct(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_whitespace()))
}

fn is_space(c: Option<char>) -> bool {
    c.is_none_or(char::is_whitespace)
}

/// A link's `(target)` at the start of `s` (just after the text's `]`): the target and the
/// bytes it takes. `<…>` encloses a target with spaces.
fn link_target(s: &str) -> Option<(String, usize)> {
    let rest = s.strip_prefix('(')?;
    if let Some(angled) = rest.strip_prefix('<') {
        let end = angled.find('>')?;
        if end == 0 || !angled[end + 1..].starts_with(')') {
            return None;
        }
        return Some((angled[..end].to_string(), 1 + 1 + end + 1 + 1));
    }
    let end = rest.find(')')?;
    let target = rest[..end].trim();
    if target.is_empty() || target.contains(['(', '<']) {
        return None;
    }
    Some((target.to_string(), 1 + end + 1))
}

/// The byte of the `]` that closes a link text starting at `s[0]` (after its `[`), skipping
/// escapes and nested brackets.
fn link_text_end(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '[' => depth += 1,
            ']' if depth == 0 => return Some(i),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Shown text, marks and offsets being built.
#[derive(Default)]
struct Shown {
    text: String,
    marks: Vec<NoteMark>,
    offsets: Vec<usize>,
}

impl Shown {
    fn push(&mut self, c: char, at: usize) {
        self.text.push(c);
        self.offsets.extend((0..c.len_utf8()).map(|k| at + k));
    }
}

/// Inline markup of `s` (which starts at body byte `base`): shown text, marks, offsets.
fn parse_inline(s: &str, base: usize) -> (String, Vec<NoteMark>, Vec<usize>) {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut run = Shown::default();
    let flush = |pieces: &mut Vec<Piece>, run: &mut Shown| {
        if !run.text.is_empty() {
            let r = std::mem::take(run);
            pieces.push(Piece::Text(r.text, r.offsets));
        }
    };
    let mut i = 0;
    while let Some(c) = s[i..].chars().next() {
        let rest = &s[i..];
        match c {
            '\\' => match rest[1..].chars().next() {
                Some(n) if n.is_ascii_punctuation() => {
                    run.push(n, base + i + 1);
                    i += 2;
                }
                // CommonMark's hard line break; every line break is one here already.
                None => i += 1,
                _ => {
                    run.push(c, base + i);
                    i += 1;
                }
            },
            '*' | '_' => {
                let len = rest.bytes().take_while(|b| *b == c as u8).count();
                let before = s[..i].chars().next_back();
                let after = s[i + len..].chars().next();
                let left_flanking =
                    !is_space(after) && (!is_punct(after) || is_space(before) || is_punct(before));
                let right_flanking =
                    !is_space(before) && (!is_punct(before) || is_space(after) || is_punct(after));
                let (open, close) = if c == '*' {
                    (left_flanking, right_flanking)
                } else {
                    (
                        left_flanking && (!right_flanking || is_punct(before)),
                        right_flanking && (!left_flanking || is_punct(after)),
                    )
                };
                flush(&mut pieces, &mut run);
                pieces.push(Piece::Delims(Delims {
                    ch: c,
                    at: base + i,
                    len,
                    open,
                    close,
                    closed: 0,
                    opened: 0,
                    closes: Vec::new(),
                    opens: Vec::new(),
                }));
                i += len;
            }
            '[' => {
                let link = link_text_end(&rest[1..]).and_then(|end| {
                    let (target, used) = link_target(&rest[1 + end + 1..])?;
                    Some((end, target, used))
                });
                match link {
                    Some((end, target, used)) => {
                        flush(&mut pieces, &mut run);
                        let (text, marks, offsets) = parse_inline(&rest[1..1 + end], base + i + 1);
                        pieces.push(Piece::Link {
                            text,
                            marks,
                            offsets,
                            target,
                        });
                        i += 1 + end + 1 + used;
                    }
                    None => {
                        run.push(c, base + i);
                        i += 1;
                    }
                }
            }
            _ => {
                run.push(c, base + i);
                i += c.len_utf8();
            }
        }
    }
    flush(&mut pieces, &mut run);
    let strong = match_emphasis(&mut pieces);
    assemble(pieces, &strong)
}

/// CommonMark's emphasis matching, simplified: each closer takes the nearest opener of its
/// character, two from each when both have two left, else one; runs between them can no
/// longer match. Returns, per match, whether it is bold.
fn match_emphasis(pieces: &mut [Piece]) -> Vec<bool> {
    let mut strong: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < pieces.len() {
        let closer = match &pieces[i] {
            Piece::Delims(d) if d.close && d.left() > 0 => Some(d.ch),
            _ => None,
        };
        let Some(ch) = closer else {
            i += 1;
            continue;
        };
        let opener = (0..i).rev().find(
            |&j| matches!(&pieces[j], Piece::Delims(d) if d.ch == ch && d.open && d.left() > 0),
        );
        let Some(j) = opener else {
            i += 1;
            continue;
        };
        let left = |p: &Piece| match p {
            Piece::Delims(d) => d.left(),
            _ => 0,
        };
        let n = if left(&pieces[j]) >= 2 && left(&pieces[i]) >= 2 {
            2
        } else {
            1
        };
        let m = strong.len();
        strong.push(n == 2);
        if let Piece::Delims(d) = &mut pieces[j] {
            d.opened += n;
            d.opens.push(m);
        }
        if let Piece::Delims(d) = &mut pieces[i] {
            d.closed += n;
            d.closes.push(m);
        }
        for p in &mut pieces[j + 1..i] {
            if let Piece::Delims(d) = p {
                d.open = false;
                d.close = false;
            }
        }
        // The same closer may go on to close an earlier opener with what is left of it.
    }
    strong
}

fn assemble(pieces: Vec<Piece>, strong: &[bool]) -> (String, Vec<NoteMark>, Vec<usize>) {
    let mut out = Shown::default();
    let mut starts: Vec<Option<usize>> = vec![None; strong.len()];
    for p in pieces {
        match p {
            Piece::Text(t, o) => {
                out.text.push_str(&t);
                out.offsets.extend(o);
            }
            Piece::Link {
                text,
                marks,
                offsets,
                target,
            } => {
                let at = out.text.len();
                out.marks.push(NoteMark {
                    range: at..at + text.len(),
                    kind: NoteMarkKind::Link(target),
                });
                out.marks.extend(marks.into_iter().map(|m| NoteMark {
                    range: m.range.start + at..m.range.end + at,
                    kind: m.kind,
                }));
                out.text.push_str(&text);
                out.offsets.extend(offsets);
            }
            Piece::Delims(d) => {
                for &m in &d.closes {
                    if let Some(start) = starts[m] {
                        out.marks.push(NoteMark {
                            range: start..out.text.len(),
                            kind: if strong[m] {
                                NoteMarkKind::Bold
                            } else {
                                NoteMarkKind::Italic
                            },
                        });
                    }
                }
                // What no match took shows as typed: closers take a run's start, openers
                // its end.
                for k in 0..d.left() {
                    out.push(d.ch, d.at + d.closed + k);
                }
                for &m in &d.opens {
                    starts[m] = Some(out.text.len());
                }
            }
        }
    }
    out.marks
        .sort_by_key(|m| (m.range.start, std::cmp::Reverse(m.range.end)));
    (out.text, out.marks, out.offsets)
}

#[cfg(test)]
#[path = "note_text_tests.rs"]
mod tests;
