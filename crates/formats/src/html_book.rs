//! Project Gutenberg's posted HTML (`files/<n>/<n>-h/<n>-h.htm`, #345): one eBook → a Book
//! (ADR 0023) plus an [`ImportReport`].
//!
//! PG's HTML is written by hand, book by book, mostly HTML 4, not always well-formed, so it
//! is read by a tolerant tokenizer rather than an XML parser. Only the text between PG's
//! "*** START OF THE PROJECT GUTENBERG EBOOK" and "*** END OF …" lines is read, and any block
//! naming Project Gutenberg is dropped: what is left is the book's own text, which PG's
//! licence leaves free of its trademark and licence.
//!
//! | HTML | Book |
//! |---|---|
//! | `h1`–`h6` at the shallowest level used more than once | Divisions, titled by the heading |
//! | deeper headings | `heading` |
//! | `p`, `div` with text, `blockquote`, `td` | `para` prose (a table row: its cells) |
//! | `li` | `para` list |
//! | a line of a `pre`, or a line ended by `br` in a poem (`class="poem"`, `"stanza"`, …) | `para` poetry |
//! | `i`, `em`, `cite` / `b`, `strong` | `italic` / `bold` |
//! | `<span class="pagenum">[Pg 23]</span>`, else `<a id="Page_23">` | zero-width `page` "23" |
//! | `<a class="fnanchor" href="#Footnote_1">` and the block holding `id="Footnote_1"` | a note unit and a zero-width `note_ref` |
//! | Bible references in the text and notes | `scripture_ref` |
//! | `img`, `script`, `style` | dropped (images counted in the report) |
//!
//! The licence is the caller's (the Open catalogue entry's, ADR 0008), never read here.

use std::collections::HashMap;

use obs_model::{
    FORMAT_VERSION, LICENSE_UNKNOWN, Manifest, NoteKind, Origin, ParaStyle, Resource,
    ResourceBuilder, ResourceType, SpanKind, UnitId, UnitKind, is_valid_resource_id,
};

use crate::error::ImportError;
use crate::report::ImportReport;
use crate::text_buf::TextBuf;

/// `source_format` of a Project Gutenberg book.
pub const PG_HTML_FORMAT: &str = "pg-html";

/// A file larger than this is refused (PG's largest HTML books are some 20 MB).
pub const MAX_HTML_BYTES: usize = 64 * 1024 * 1024;
/// The most tokens read.
const MAX_TOKENS: usize = 20_000_000;
/// Elements nested deeper than this are read as if they were not there.
const MAX_DEPTH: usize = 256;
/// A Division's title is cut, at a word, to about this many characters.
const MAX_TITLE_CHARS: usize = 120;

/// Identity of the Resource a PG import creates.
#[derive(Debug, Clone, Default)]
pub struct HtmlBookOptions {
    pub resource_id: String,
    /// The Book's title; empty: the HTML `<title>`.
    pub title: String,
    pub abbreviation: String,
    /// BCP 47; empty: the `<html lang>`, else `en`.
    pub language: String,
    pub source_file: String,
    pub tool_version: String,
}

include!("html_entities.rs");

/// One piece of an HTML document.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Start {
        name: String,
        attrs: Vec<(String, String)>,
    },
    End(String),
    Text(String),
}

impl Token {
    fn attr(&self, key: &str) -> Option<&str> {
        match self {
            Token::Start { attrs, .. } => attrs
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str()),
            _ => None,
        }
    }

    fn has_class(&self, class: &str) -> bool {
        self.attr("class")
            .is_some_and(|c| c.split_whitespace().any(|w| w.eq_ignore_ascii_case(class)))
    }

    fn class_contains(&self, part: &str) -> bool {
        self.attr("class")
            .is_some_and(|c| c.to_ascii_lowercase().contains(part))
    }
}

/// Decode character references in `s`: HTML 4's named ones, decimal and hexadecimal. An
/// unknown or malformed one is kept as written.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest[1..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
            .map_or(Some(rest.len()), |i| Some(i + 1));
        let Some(end) = end.filter(|&e| e > 1 && e <= 33) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = if let Some(num) = name.strip_prefix('#') {
            let n = if let Some(hex) = num.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                num.parse::<u32>().ok()
            };
            n.and_then(char::from_u32).filter(|c| *c != '\0')
        } else {
            ENTITIES
                .binary_search_by(|(k, _)| (*k).cmp(name))
                .ok()
                .map(|i| ENTITIES[i].1)
        };
        match decoded {
            Some(c) => {
                out.push(c);
                let skip = if rest[end..].starts_with(';') {
                    end + 1
                } else {
                    end
                };
                rest = &rest[skip..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Split `html` into tags and text: comments, doctypes and processing instructions dropped,
/// `script` and `style` content skipped, tag and attribute names lowercased, attribute values
/// decoded. Never fails: a `<` that starts no tag is text.
fn tokenize(html: &str) -> Result<Vec<Token>, ImportError> {
    let mut out = Vec::new();
    let b = html.as_bytes();
    let mut i = 0;
    let mut text_start = 0;
    let push_text = |out: &mut Vec<Token>, from: usize, to: usize| {
        if from < to {
            out.push(Token::Text(decode_entities(&html[from..to])));
        }
    };
    while i < b.len() {
        if out.len() > MAX_TOKENS {
            return Err(ImportError::TooLarge("HTML book".into()));
        }
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &html[i..];
        if rest.starts_with("<!--") {
            push_text(&mut out, text_start, i);
            i = rest.find("-->").map_or(b.len(), |e| i + e + 3);
            text_start = i;
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            push_text(&mut out, text_start, i);
            i = rest.find('>').map_or(b.len(), |e| i + e + 1);
            text_start = i;
            continue;
        }
        let closing = rest.as_bytes().get(1) == Some(&b'/');
        let name_from = i + 1 + usize::from(closing);
        let name_len = html[name_from..]
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == ':' || c == '-'))
            .unwrap_or(html.len() - name_from);
        if name_len == 0 || !b[name_from].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        push_text(&mut out, text_start, i);
        let name = html[name_from..name_from + name_len].to_ascii_lowercase();
        // The tag ends at the first `>` outside a quoted value.
        let mut j = name_from + name_len;
        let mut quote = None;
        while j < b.len() {
            match (quote, b[j]) {
                (None, b'>') => break,
                (None, q @ (b'"' | b'\'')) => quote = Some(q),
                (Some(q), c) if c == q => quote = None,
                _ => {}
            }
            j += 1;
        }
        let inside = &html[name_from + name_len..j.min(b.len())];
        i = (j + 1).min(b.len());
        text_start = i;
        if closing {
            out.push(Token::End(name));
            continue;
        }
        let attrs = attributes(inside);
        let raw = matches!(name.as_str(), "script" | "style");
        out.push(Token::Start {
            name: name.clone(),
            attrs,
        });
        if inside.trim_end().ends_with('/') && !raw {
            out.push(Token::End(name));
        } else if raw {
            let close = format!("</{name}");
            let end = html[i..]
                .to_ascii_lowercase()
                .find(&close)
                .map_or(b.len(), |e| i + e);
            out.push(Token::End(name));
            i = html[end..].find('>').map_or(b.len(), |e| end + e + 1);
            text_start = i;
        }
    }
    push_text(&mut out, text_start, b.len());
    Ok(out)
}

/// A tag's attributes: `name="value"`, `name='value'`, `name=value` or `name`.
fn attributes(inside: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let s = inside.trim().trim_end_matches('/');
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' {
            i += 1;
        }
        if start == i {
            i += 1;
            continue;
        }
        let key = s[start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let from = i + 1;
                i = from;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = decode_entities(&s[from..i.min(b.len())]);
                i += 1;
            } else {
                let from = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = decode_entities(&s[from..i]);
            }
        }
        if out.len() < 32 {
            out.push((key, value));
        }
    }
    out
}

/// Elements with no content.
fn is_void(name: &str) -> bool {
    matches!(
        name,
        "br" | "hr" | "img" | "meta" | "link" | "input" | "col" | "area" | "base" | "wbr"
    )
}

/// Elements that make a block of their own.
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "blockquote"
            | "li"
            | "dt"
            | "dd"
            | "pre"
            | "tr"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "table"
            | "ul"
            | "ol"
            | "dl"
            | "center"
            | "section"
            | "article"
            | "body"
            | "address"
            | "figure"
            | "figcaption"
    )
}

fn heading_level(name: &str) -> Option<u8> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Whether a text names Project Gutenberg (its header, footer, licence, or a credit inside).
fn names_gutenberg(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("gutenberg")
}

/// The tokens of the book's own text: after the "*** START OF" line, before the "*** END OF"
/// line. A file with neither (PG's newest posted HTML leaves the boilerplate to its generated
/// files) is read from its `<body>`; a block naming PG is dropped as anywhere else. `None` when
/// only one of the lines is there.
fn body(tokens: &[Token]) -> Option<&[Token]> {
    let marked = |word: &str| {
        tokens.iter().any(|t| match t {
            Token::Text(s) => {
                let upper = s.to_uppercase();
                upper.contains("***") && upper.contains(word) && upper.contains("PROJECT GUTENBERG")
            }
            _ => false,
        })
    };
    if !marked("START OF") && !marked("END OF") {
        let start = tokens
            .iter()
            .position(|t| matches!(t, Token::Start { name, .. } if name == "body"))?;
        return Some(&tokens[start + 1..]);
    }
    marked_body(tokens)
}

/// [`body`] of a file with PG's start and end lines.
fn marked_body(tokens: &[Token]) -> Option<&[Token]> {
    let marker = |t: &Token, word: &str| match t {
        Token::Text(s) => {
            let upper = s.to_uppercase();
            upper.contains("***") && upper.contains(word) && upper.contains("PROJECT GUTENBERG")
        }
        _ => false,
    };
    let start = tokens.iter().position(|t| marker(t, "START OF"))?;
    let end = tokens[start..]
        .iter()
        .position(|t| marker(t, "END OF"))
        .map(|e| start + e)?;
    Some(&tokens[start + 1..end])
}

/// A printed page's label from a page number's text or id: "[Pg 23]", "[p. iv]", "Page_23",
/// "pg23" → "23", "iv".
fn page_label(raw: &str) -> Option<String> {
    let mut s = raw
        .trim()
        .trim_matches(['[', ']', '{', '}', '(', ')'])
        .trim();
    for prefix in [
        "Page_", "page_", "Page ", "page ", "Pg ", "pg ", "Pg", "pg", "p. ", "P. ", "p.", "Page",
        "page",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
            break;
        }
    }
    let s = s.trim().trim_matches(['[', ']', '.']).trim();
    // A number ("23", "23a", "12-13") or Roman numerals ("iv").
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    let number = digits > 0
        && digits <= 6
        && s.chars()
            .skip(digits)
            .all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit())
        && s.len() <= 12;
    let roman = !s.is_empty() && s.len() <= 10 && s.chars().all(|c| "ivxlcdmIVXLCDM".contains(c));
    (number || roman).then(|| s.to_string())
}

/// Whether an element is a page number (`span class="pagenum"`, `pageno`, `page-number`,
/// `pgmark`, `pagemark`, `pgnum`).
fn is_pagenum(t: &Token) -> bool {
    [
        "pagenum", "pageno", "page-num", "pgmark", "pagemark", "pgnum",
    ]
    .iter()
    .any(|c| t.class_contains(c))
}

/// Text a transcriber keeps out of the reading text: the brackets around a page number
/// (`<span class="ns">[p</span>`).
fn is_unread(t: &Token) -> bool {
    t.has_class("ns") || t.has_class("hidden") || t.has_class("hide")
}

/// Whether an element is one footnote's container (`div class="footnote"`).
fn is_note_container(t: &Token) -> bool {
    t.has_class("footnote") || t.has_class("fn")
}

/// Elements a footnote's text may be held by.
fn can_hold_note(name: &str) -> bool {
    matches!(
        name,
        "p" | "div" | "li" | "td" | "dd" | "blockquote" | "aside" | "section"
    )
}

/// A footnote anchor's target id: `<a class="fnanchor" href="#Footnote_1_1">`, or a link to
/// an id starting `Footnote`/`FN`/`fn`/`note`.
fn note_target(t: &Token) -> Option<String> {
    let href = t.attr("href")?.strip_prefix('#')?;
    let lower = href.to_ascii_lowercase();
    let looks = t.class_contains("fnanchor")
        || t.class_contains("noteref")
        || lower.starts_with("footnote")
        || lower.starts_with("fn")
        || lower.starts_with("note");
    (looks && !lower.contains("anchor") && !href.is_empty()).then(|| href.to_string())
}

/// What is being written: the main text or a footnote's.
struct Target {
    buf: TextBuf,
    /// Where the open block started, and its kind.
    block: Option<(usize, SpanKind)>,
    /// Pages turned between blocks: they open the next block.
    pages: Vec<String>,
}

impl Target {
    fn new() -> Self {
        Self {
            buf: TextBuf::new(),
            block: None,
            pages: Vec::new(),
        }
    }

    fn open(&mut self, kind: SpanKind) {
        self.close();
        let start = self.buf.start_block();
        for label in std::mem::take(&mut self.pages) {
            self.buf.add_span(start..start, SpanKind::Page { label });
        }
        self.block = Some((start, kind));
    }

    /// Close the open block. A block naming Project Gutenberg is dropped, its pages kept for
    /// the next block.
    fn close(&mut self) {
        let Some((start, kind)) = self.block.take() else {
            return;
        };
        let end = self.buf.pos();
        if names_gutenberg(&self.buf.text[start..end]) {
            let pages: Vec<String> = self
                .buf
                .spans
                .iter()
                .filter(|(r, _)| r.start >= start)
                .filter_map(|(_, k)| match k {
                    SpanKind::Page { label } => Some(label.clone()),
                    _ => None,
                })
                .collect();
            self.buf.truncate_to(start);
            self.pages.extend(pages);
            return;
        }
        if start < end {
            self.buf.add_span(start..end, kind);
        }
    }

    fn ensure_open(&mut self, style: ParaStyle) {
        if self.block.is_none() {
            self.open(SpanKind::Para { style });
        }
    }

    fn page(&mut self, label: String) {
        if self.block.is_some() {
            self.buf.flush_space();
            let at = self.buf.pos();
            self.buf.add_span(at..at, SpanKind::Page { label });
        } else {
            self.pages.push(label);
        }
    }
}

/// One Division as it is written.
struct Division {
    title: String,
    target: Target,
}

/// An open element and what it started.
struct Open {
    name: String,
    /// A styled run (italic, bold) and where it started.
    styled: Option<(usize, SpanKind)>,
    /// Whether this element opened a block, closed with it.
    block: bool,
    /// A footnote block: notes are written to `notes[i]` until it closes.
    note: Option<usize>,
    /// Its text is not copied (a page number, a footnote's back-link label).
    skip: bool,
    /// A footnote's container (`class="footnote"`), preferred as the note's element.
    note_container: bool,
    /// A page number's text, gathered while it is open.
    page_text: Option<String>,
    /// A heading at the Division level: its text becomes the Division's title.
    division_heading: bool,
    poem: bool,
    pre: bool,
}

struct Reader<'r> {
    report: &'r mut ImportReport,
    divisions: Vec<Division>,
    notes: Vec<Option<Target>>,
    note_of_id: HashMap<String, usize>,
    stack: Vec<Open>,
    division_level: Option<u8>,
    /// No `pagenum` element in the book: `<a id="Page_23">` marks pages instead.
    anchor_pages: bool,
}

impl Reader<'_> {
    fn current_note(&self) -> Option<usize> {
        self.stack.iter().rev().find_map(|o| o.note)
    }

    fn skipping(&self) -> bool {
        self.stack.iter().any(|o| o.skip)
    }

    fn in_poem(&self) -> bool {
        self.stack.iter().any(|o| o.poem)
    }

    fn in_pre(&self) -> bool {
        self.stack.iter().any(|o| o.pre)
    }

    fn target(&mut self) -> &mut Target {
        if let Some(i) = self.current_note() {
            return self.notes[i].get_or_insert_with(Target::new);
        }
        if self.divisions.is_empty() {
            self.divisions.push(Division {
                title: String::new(),
                target: Target::new(),
            });
        }
        let last = self.divisions.len() - 1;
        &mut self.divisions[last].target
    }

    fn note_index(&mut self, id: &str) -> usize {
        if let Some(&i) = self.note_of_id.get(id) {
            return i;
        }
        self.notes.push(None);
        let i = self.notes.len() - 1;
        self.note_of_id.insert(id.to_string(), i);
        i
    }

    fn style(&self) -> ParaStyle {
        if self.in_poem() || self.in_pre() {
            ParaStyle::Poetry1
        } else if self.stack.iter().any(|o| o.name == "li") {
            ParaStyle::List
        } else {
            ParaStyle::Prose
        }
    }

    fn text(&mut self, s: &str) {
        if let Some(page) = self
            .stack
            .iter_mut()
            .rev()
            .find_map(|o| o.page_text.as_mut())
        {
            page.push_str(s);
            return;
        }
        if self.skipping() {
            return;
        }
        if self.in_pre() {
            // A `pre`'s lines are lines.
            let style = self.style();
            for (i, line) in s.split('\n').enumerate() {
                if i > 0 {
                    let t = self.target();
                    t.close();
                }
                if !line.trim().is_empty() {
                    let t = self.target();
                    t.ensure_open(style);
                    t.buf.push_text(line);
                }
            }
            return;
        }
        if s.trim().is_empty() && self.target().block.is_none() {
            return;
        }
        let style = self.style();
        let t = self.target();
        t.ensure_open(style);
        t.buf.push_text(s);
    }

    fn start(&mut self, tok: &Token) {
        let Token::Start { name, .. } = tok else {
            return;
        };
        if self.stack.len() > MAX_DEPTH {
            return;
        }
        let name = name.as_str();
        // With no page-number elements in the book, `<a id="Page_23">` marks a page.
        if self.anchor_pages
            && !self.skipping()
            && self.current_note().is_none()
            && let Some(id) = tok.attr("id").or_else(|| tok.attr("name"))
            && let Some(label) = id
                .strip_prefix("Page_")
                .or_else(|| id.strip_prefix("page_"))
                .or_else(|| id.strip_prefix("Pg"))
                .and_then(page_label)
        {
            self.target().page(label);
        }
        match name {
            "img" => {
                // A heading printed as an image: its words are the alt text.
                let alt = tok.attr("alt").map(str::trim).filter(|a| !a.is_empty());
                let in_heading = self.stack.iter().any(|o| heading_level(&o.name).is_some());
                match alt {
                    Some(alt) if in_heading => {
                        let alt = alt.to_string();
                        self.text(&alt);
                    }
                    _ => {
                        *self.report.dropped_markup.entry("img".into()).or_default() += 1;
                    }
                }
                return;
            }
            "br" => {
                if self.in_poem() {
                    let t = self.target();
                    t.close();
                } else if !self.skipping() {
                    self.target().buf.push_text(" ");
                }
                return;
            }
            _ if is_void(name) => return,
            _ => {}
        }
        let mut open = Open {
            name: name.to_string(),
            styled: None,
            block: false,
            note: None,
            skip: false,
            note_container: is_note_container(tok),
            page_text: None,
            division_heading: false,
            poem: tok.class_contains("poem")
                || tok.class_contains("stanza")
                || tok.class_contains("verse"),
            pre: name == "pre",
        };
        if is_pagenum(tok) {
            open.page_text = Some(String::new());
            self.stack.push(open);
            return;
        }
        // Text kept out of the reading: a transcriber's brackets, a link back to the
        // contents.
        let to_contents = name == "a"
            && tok.attr("href").is_some_and(|h| {
                let h = h.to_ascii_lowercase();
                h.starts_with('#') && (h.contains("toc") || h.contains("contents"))
            });
        if is_unread(tok) || to_contents || tok.class_contains("toclink") {
            open.skip = true;
            self.stack.push(open);
            return;
        }
        // An element whose id a footnote reference pointed at is that footnote (an anchor at
        // the start of a paragraph or of a `class="footnote"` container, or the block itself).
        if let Some(id) = tok.attr("id").or_else(|| tok.attr("name")) {
            self.maybe_note_start(id, name == "a", &mut open);
        }
        if name == "a" {
            if let Some(target) = note_target(tok) {
                let i = self.note_index(&target);
                if self.current_note().is_none() && !self.skipping() {
                    let style = self.style();
                    let t = self.target();
                    t.ensure_open(style);
                    t.buf.flush_space();
                    let at = t.buf.pos();
                    t.buf.add_span(
                        at..at,
                        SpanKind::NoteRef {
                            note: UnitId::note(u32::try_from(i + 1).unwrap_or(u32::MAX)),
                            note_kind: NoteKind::Footnote,
                        },
                    );
                }
                open.skip = true;
                self.stack.push(open);
                return;
            }
            // In a footnote, a back-link's label ("[1]") is not text.
            if self.current_note().is_some() && tok.attr("href").is_some_and(|h| h.starts_with('#'))
            {
                open.skip = true;
            }
        }
        if let Some(level) = heading_level(name) {
            let at_division =
                self.current_note().is_none() && self.division_level.is_some_and(|l| level <= l);
            if at_division {
                self.close_all_blocks();
                self.divisions.push(Division {
                    title: String::new(),
                    target: Target::new(),
                });
                open.division_heading = true;
            }
            let shown = self
                .division_level
                .map_or(2, |l| level.saturating_sub(l).saturating_add(1))
                .clamp(1, 6);
            self.target().open(SpanKind::Heading { level: shown });
            open.block = true;
            self.stack.push(open);
            return;
        }
        if is_block(name) {
            // Its block is opened by its first text, so an empty one leaves nothing.
            if open.note.is_none() {
                self.target().close();
            }
            open.block = true;
            self.stack.push(open);
            return;
        }
        let kind = match name {
            "i" | "em" | "cite" | "var" | "dfn" => Some(SpanKind::Italic),
            "b" | "strong" => Some(SpanKind::Bold),
            _ => None,
        };
        if let Some(kind) = kind
            && !self.skipping()
        {
            let style = self.style();
            let t = self.target();
            t.ensure_open(style);
            t.buf.flush_space();
            let at = t.buf.pos();
            t.buf.tracked_end = at;
            open.styled = Some((at, kind));
        }
        if matches!(name, "head" | "title" | "script" | "style" | "svg" | "math") {
            open.skip = true;
        }
        self.stack.push(open);
    }

    /// `id` was pointed at by a footnote reference: the note's text is what its element
    /// holds. An anchor (`a`) gives it to the innermost `class="footnote"` container, else the
    /// innermost block, when nothing has been written in that block yet; another element is
    /// the note itself.
    fn maybe_note_start(&mut self, id: &str, anchor: bool, open: &mut Open) {
        let Some(&i) = self.note_of_id.get(id) else {
            return;
        };
        if self.current_note().is_some() || self.notes[i].is_some() || self.skipping() {
            return;
        }
        if !anchor {
            self.notes[i] = Some(Target::new());
            open.note = Some(i);
            return;
        }
        // Only at the start of a paragraph: an anchor in running text is not a note.
        if self.divisions.last().is_some_and(|d| {
            d.target
                .block
                .as_ref()
                .is_some_and(|(s, _)| *s < d.target.buf.pos())
        }) {
            return;
        }
        let holder = self
            .stack
            .iter()
            .rposition(|o| o.note_container)
            .or_else(|| self.stack.iter().rposition(|o| can_hold_note(&o.name)));
        if let Some(h) = holder {
            self.notes[i] = Some(Target::new());
            self.stack[h].note = Some(i);
        }
    }

    fn close_all_blocks(&mut self) {
        if let Some(d) = self.divisions.last_mut() {
            d.target.close();
        }
    }

    fn end(&mut self, name: &str) {
        let Some(at) = self.stack.iter().rposition(|o| o.name == name) else {
            return;
        };
        while self.stack.len() > at {
            let Some(open) = self.stack.pop() else { break };
            self.finish(open);
        }
    }

    fn finish(&mut self, open: Open) {
        if let Some(raw) = open.page_text {
            if let Some(label) = page_label(&raw)
                && !self.skipping()
            {
                self.target().page(label);
            }
            return;
        }
        if let Some((start, kind)) = open.styled {
            let t = self.target();
            let end = t.buf.tracked_end.clamp(start, t.buf.pos());
            if start < end {
                t.buf.add_span(start..end, kind);
            }
        }
        if let Some(i) = open.note {
            if let Some(Some(t)) = self.notes.get_mut(i) {
                t.close();
            }
            return;
        }
        if open.division_heading {
            let t = self.target();
            let title = t
                .block
                .as_ref()
                .map(|(s, _)| t.buf.text[*s..].to_string())
                .unwrap_or_default();
            t.close();
            if let Some(d) = self.divisions.last_mut() {
                d.title = cut_at_word(title.trim(), MAX_TITLE_CHARS);
            }
            return;
        }
        if open.block || is_block(&open.name) || heading_level(&open.name).is_some() {
            self.target().close();
        }
    }
}

/// `text` cut at a word to at most `max` characters, with "…" when cut.
fn cut_at_word(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    let at = cut.rfind(' ').filter(|&i| i > max / 2).unwrap_or(cut.len());
    format!("{}…", cut[..at].trim_end_matches([',', ';', ':', ' ']))
}

/// The Division level: the shallowest heading level used three times or more (chapters),
/// else twice, else at all. A title's `h1` used once or twice does not split the book.
fn division_level(tokens: &[Token]) -> Option<u8> {
    let mut counts = [0usize; 7];
    for t in tokens {
        if let Token::Start { name, .. } = t
            && let Some(l) = heading_level(name)
        {
            counts[usize::from(l)] += 1;
        }
    }
    [3, 2, 1]
        .into_iter()
        .find_map(|n| (1..=6u8).find(|&l| counts[usize::from(l)] >= n))
}

/// The `<html lang>` (or `xml:lang`), lowercased, its region dropped.
fn document_language(tokens: &[Token]) -> Option<String> {
    tokens.iter().find_map(|t| match t {
        Token::Start { name, .. } if name == "html" => t
            .attr("lang")
            .or_else(|| t.attr("xml:lang"))
            .map(|l| l.split(['-', '_']).next().unwrap_or(l).to_ascii_lowercase())
            .filter(|l| (2..=3).contains(&l.len()) && l.bytes().all(|b| b.is_ascii_lowercase())),
        _ => None,
    })
}

/// The `<title>`.
fn document_title(tokens: &[Token]) -> String {
    let Some(at) = tokens
        .iter()
        .position(|t| matches!(t, Token::Start { name, .. } if name == "title"))
    else {
        return String::new();
    };
    let mut s = String::new();
    for t in &tokens[at + 1..] {
        match t {
            Token::Text(x) => s.push_str(x),
            _ => break,
        }
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // "The Project Gutenberg eBook of Orthodoxy, by G. K. Chesterton" → "Orthodoxy".
    let s = s
        .strip_prefix("The Project Gutenberg eBook of ")
        .or_else(|| s.strip_prefix("The Project Gutenberg EBook of "))
        .unwrap_or(&s);
    let s = s.split(" | Project Gutenberg").next().unwrap_or(s);
    s.split(", by ").next().unwrap_or(s).trim().to_string()
}

/// Whether a Division holds only headings (and pages): a title page's or a part's.
fn only_headings(buf: &TextBuf) -> bool {
    !buf.spans
        .iter()
        .any(|(_, k)| matches!(k, SpanKind::Para { .. }))
}

/// `next` with `before`'s text and spans in front of it.
fn prepend(before: TextBuf, next: &mut TextBuf) {
    let shift = before.text.len() + 1;
    let mut text = before.text;
    text.push('\n');
    text.push_str(&next.text);
    next.text = text;
    for (r, _) in &mut next.spans {
        *r = r.start + shift..r.end + shift;
    }
    let mut spans = before.spans;
    spans.append(&mut next.spans);
    next.spans = spans;
}

/// Windows-1252's characters for bytes 0x80–0x9F (ISO 8859-1 has control codes there, which
/// no book uses; PG's "iso-8859-1" files are read as Windows-1252, as browsers read them).
const CP1252_HIGH: [char; 32] = [
    '\u{20ac}', '\u{81}', '\u{201a}', '\u{192}', '\u{201e}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{2c6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8d}', '\u{17d}', '\u{8f}',
    '\u{90}', '\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{2dc}', '\u{2122}', '\u{161}', '\u{203a}', '\u{153}', '\u{9d}', '\u{17e}', '\u{178}',
];

/// An HTML file's bytes as text: UTF-8 when they are, else read as Windows-1252 (PG's older
/// files declare `iso-8859-1`; any non-UTF-8 file is read so).
pub fn decode_html_bytes(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.strip_prefix('\u{feff}').unwrap_or(s).to_string(),
        Err(_) => bytes
            .iter()
            .map(|&b| match b {
                0x80..=0x9f => CP1252_HIGH[usize::from(b - 0x80)],
                _ => char::from(b),
            })
            .collect(),
    }
}

/// Bible references in a unit's text, as `scripture_ref` spans.
fn add_references(
    text: &str,
    report: &mut ImportReport,
    builder: &mut ResourceBuilder,
    unit: &UnitId,
) {
    for (range, targets) in crate::commentary::book_references(text) {
        builder.add_span(unit, range, SpanKind::ScriptureRef { targets });
        report.scripture_refs += 1;
    }
}

/// Import one Project Gutenberg HTML book. `progress(done, total)` returns false to cancel.
pub fn import_pg_html(
    html: &str,
    options: &HtmlBookOptions,
    mut progress: impl FnMut(usize, usize) -> bool,
) -> Result<(Resource, ImportReport), ImportError> {
    let resource_id = options.resource_id.clone();
    if !is_valid_resource_id(&resource_id) {
        return Err(ImportError::InvalidResourceId(resource_id));
    }
    if html.len() > MAX_HTML_BYTES {
        return Err(ImportError::TooLarge(options.source_file.clone()));
    }
    let tokens = tokenize(html)?;
    let body = body(&tokens).ok_or_else(|| {
        ImportError::Xml(
            "no \"*** START OF\" and \"*** END OF\" lines of a Project Gutenberg eBook".into(),
        )
    })?;
    let language = [
        options.language.clone(),
        document_language(&tokens).unwrap_or_default(),
        "en".into(),
    ]
    .into_iter()
    .find(|l| !l.is_empty())
    .unwrap_or_else(|| "en".into());
    let title = Some(options.title.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| document_title(&tokens));
    let title = if title.is_empty() {
        resource_id.clone()
    } else {
        title
    };
    let abbreviation = Some(options.abbreviation.trim().to_string())
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| "PG".into());
    let mut manifest = Manifest::new(
        &resource_id,
        &title,
        abbreviation,
        ResourceType::Book,
        language,
    );
    manifest.license = LICENSE_UNKNOWN.to_string();
    manifest.origin = Origin::Imported;
    manifest.source_format = PG_HTML_FORMAT.into();
    manifest.source_file = (!options.source_file.is_empty()).then(|| options.source_file.clone());
    manifest.import_tool_version = options.tool_version.clone();
    manifest.format_version = FORMAT_VERSION;
    let mut builder = ResourceBuilder::new(manifest);
    let mut report = ImportReport {
        resource_id: resource_id.clone(),
        title,
        source_format: PG_HTML_FORMAT.into(),
        source_files: vec![options.source_file.clone()],
        license_missing: true,
        ..ImportReport::default()
    };
    let anchor_pages = !body.iter().any(is_pagenum);
    let mut reader = Reader {
        report: &mut report,
        divisions: Vec::new(),
        notes: Vec::new(),
        note_of_id: HashMap::new(),
        stack: Vec::new(),
        division_level: division_level(body),
        anchor_pages,
    };
    let total = body.len();
    for (i, tok) in body.iter().enumerate() {
        if i % 50_000 == 0 && !progress(i, total) {
            return Err(ImportError::Cancelled);
        }
        match tok {
            Token::Start { .. } => reader.start(tok),
            Token::End(name) => reader.end(name),
            Token::Text(s) => reader.text(s),
        }
    }
    while let Some(open) = reader.stack.pop() {
        reader.finish(open);
    }
    let Reader {
        divisions, notes, ..
    } = reader;
    // Notes: only those whose block was found; a reference to another is dropped.
    let mut note_ids: Vec<Option<UnitId>> = Vec::with_capacity(notes.len());
    let mut written_notes = Vec::new();
    for (i, note) in notes.into_iter().enumerate() {
        match note {
            Some(mut t) if !t.buf.text.trim().is_empty() => {
                t.close();
                let id = UnitId::note(u32::try_from(i + 1).unwrap_or(u32::MAX));
                note_ids.push(Some(id.clone()));
                written_notes.push((id, t.buf));
            }
            _ => note_ids.push(None),
        }
    }
    let defined = |kind: &SpanKind| match kind {
        SpanKind::NoteRef { note, .. } => note_ids.iter().flatten().any(|n| n == note),
        _ => true,
    };
    // Each Division closed; one holding only headings (a title page's, a part's) opens the
    // next.
    let mut closed: Vec<(String, TextBuf)> = Vec::new();
    for mut d in divisions {
        d.target.close();
        let mut buf = d.target.buf;
        let end = buf.pos();
        for label in d.target.pages {
            buf.add_span(end..end, SpanKind::Page { label });
        }
        if buf.text.trim().is_empty() {
            continue;
        }
        closed.push((d.title, buf));
    }
    let mut merged: Vec<(String, TextBuf)> = Vec::new();
    let mut carry: Option<TextBuf> = None;
    let n = closed.len();
    for (i, (title, mut buf)) in closed.into_iter().enumerate() {
        if let Some(before) = carry.take() {
            prepend(before, &mut buf);
        }
        if i + 1 < n && only_headings(&buf) {
            carry = Some(buf);
            continue;
        }
        merged.push((title, buf));
    }
    let mut written = 0u32;
    for (i, (title, mut buf)) in merged.into_iter().enumerate() {
        buf.normalize_nfc();
        written += 1;
        let id =
            UnitId::new(format!("div.{written}")).map_err(|e| ImportError::Xml(e.to_string()))?;
        let title = if title.is_empty() {
            if i == 0 {
                "Front matter".to_string()
            } else {
                format!("Section {written}")
            }
        } else {
            title
        };
        add_references(&buf.text, &mut report, &mut builder, &id);
        builder.add_unit(id.clone(), UnitKind::Division, Some(title), buf.text);
        for (range, kind) in buf.spans {
            if defined(&kind) {
                builder.add_span(&id, range, kind);
            }
        }
    }
    if written == 0 {
        return Err(ImportError::NoText);
    }
    for (id, mut buf) in written_notes {
        buf.normalize_nfc();
        add_references(&buf.text, &mut report, &mut builder, &id);
        builder.add_unit(id.clone(), UnitKind::Note, None, buf.text);
        for (range, kind) in buf.spans {
            if !matches!(kind, SpanKind::NoteRef { .. }) {
                builder.add_span(&id, range, kind);
            }
        }
        report.notes += 1;
    }
    report.entries = written as usize;
    let resource = builder.build().map_err(ImportError::Invalid)?;
    Ok((resource, report))
}

#[cfg(test)]
#[path = "html_book_tests.rs"]
mod tests;
