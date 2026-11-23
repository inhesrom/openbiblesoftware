//! TEI texts of the Text Creation Partnership (EEBO-TCP, ECCO-TCP, Evans-TCP; #345): one TCP
//! text (`A53715.xml`) → a Book (ADR 0023) plus an [`ImportReport`].
//!
//! | TEI | Book |
//! |---|---|
//! | each `<div>` of `<front>`, `<body>`, `<back>`; a `<div>` holding two or more `<div>`s is split into them | Divisions (text units), titled by their `<head>`, else by their `type` and `n` ("Chapter 1") |
//! | `<head>` | `heading` |
//! | `<p>`, `<opener>`, `<closer>`, `<salute>`, `<signed>`, `<trailer>`, `<argument>`, `<byline>`, `<dateline>`, `<epigraph>`, …; `<q>`, `<bibl>` between blocks | `para` prose |
//! | `<item>` | `para` list |
//! | `<l>` | `para` poetry, one per line |
//! | `<hi>` | `italic` (`rend="sup"`, `"sub"`: plain) |
//! | `<foreign>` | `foreign` |
//! | `<pb n="23">` | zero-width `page` "23" (a `<pb>` with no `n` is an unnumbered leaf: no page) |
//! | `<note>` | a note unit and a zero-width `note_ref` footnote |
//! | `<g ref="char:EOLhyphen">` | nothing: the word broken at the line's end is joined |
//! | `<gap>` | "⟨…⟩", or its `<desc>` |
//! | TCP's marks the reading font lacks: `〈 〉` around a gap, `▪` (punctuation of unclear form), `╌` (a syllable break in a sung text), `✚`, `☞`, `☜`, `☝`, `⁎`, `♫` (music not transcribed) | `⟨ ⟩`, `·`, `-`, `+`, `→`, `←`, `↑`, `*`, "⟨music⟩" ([`tcp_glyph`]) |
//! | ornaments: `❧`, `❀` | dropped, counted in the report |
//! | Bible references in the text and notes (`Rom. 8. 13`, `Rom. viii. 13`) | `scripture_ref` |
//! | `<figure>`, `<table>`, `<graphic>` | dropped, counted in the report |
//!
//! Spelling is the edition's own, as TCP keyed it. The TEI header gives the title, author and
//! date; the licence is the caller's (the Open catalogue entry's, ADR 0008), never read here.

use obs_model::{
    FORMAT_VERSION, LICENSE_UNKNOWN, Manifest, NoteKind, Origin, ParaStyle, Resource,
    ResourceBuilder, ResourceType, SpanKind, UnitId, UnitKind, is_valid_resource_id,
};
use roxmltree::{Document, Node, ParsingOptions};

use crate::error::ImportError;
use crate::report::ImportReport;
use crate::text_buf::TextBuf;

/// `source_format` of a TCP book.
pub const TCP_TEI_FORMAT: &str = "tcp-tei";

/// A TEI file larger than this is refused (TCP's largest texts are some 30 MB).
pub const MAX_TEI_BYTES: usize = 128 * 1024 * 1024;
/// The most XML nodes read (a 30 MB text has some 3 million).
const MAX_NODES: u32 = 20_000_000;
/// Elements nested deeper than this are read as plain text.
const MAX_DEPTH: usize = 64;
/// One header value is cut to this many characters.
const MAX_META_CHARS: usize = 2_000;
/// A Division's title is cut, at a word, to about this many characters.
const MAX_TITLE_CHARS: usize = 120;
/// A part's title prefixes its chapters' only when it is at most this long ("Part I.",
/// "BOOK II.", "THE SECOND PART."), not when it is a sermon's or a dedication's whole head:
/// the Contents pane cuts a long title, and the parts would all read alike.
const MAX_PREFIX_CHARS: usize = 20;

/// Identity of the Resource a TCP import creates.
#[derive(Debug, Clone, Default)]
pub struct TeiBookOptions {
    pub resource_id: String,
    /// The Book's title; empty: the TEI header's, cut at its first colon.
    pub title: String,
    /// Empty: the TCP id.
    pub abbreviation: String,
    /// BCP 47; empty: the header's (`langUsage`), else `en`.
    pub language: String,
    /// File name shown in About and the report.
    pub source_file: String,
    pub tool_version: String,
}

/// What a TCP text's header says of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeiHeader {
    /// The source edition's title.
    pub title: String,
    pub author: String,
    /// The source edition's date ("1668").
    pub date: String,
    /// The TCP id (`A53715`, `K027775.000`): `idno type="TCP"`, else `type="DLPS"`.
    pub id: String,
    /// The file's `<availability>` statement, whitespace collapsed.
    pub availability: String,
    /// BCP 47, from `profileDesc/langUsage/language/@ident` (`eng` → `en`, `wel` → `cy`,
    /// `lat` → `la`); `en` when the header names none.
    pub language: String,
}

/// A TEI header's language code (ISO 639-2, as TCP writes it) as BCP 47.
fn bcp47(code: &str) -> String {
    let code = code.trim().to_ascii_lowercase();
    let mapped = match code.as_str() {
        "eng" => "en",
        "wel" | "cym" => "cy",
        "lat" => "la",
        "fre" | "fra" => "fr",
        "ger" | "deu" => "de",
        "dut" | "nld" => "nl",
        "ita" => "it",
        "spa" => "es",
        "gle" => "ga",
        "gla" => "gd",
        // TCP's Greek is ancient.
        "gre" | "grc" => "grc",
        "heb" => "he",
        _ => "",
    };
    if !mapped.is_empty() {
        mapped.to_string()
    } else if (2..=3).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_lowercase()) {
        code
    } else {
        "en".to_string()
    }
}

fn child<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
}

fn is_element(node: &Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

/// What TCP's text marks are written as: the reading font (Gentium) has no glyph for these,
/// so each is its typographic equivalent, an ornament nothing. Everything else is itself.
fn tcp_glyph(c: char) -> Option<&'static str> {
    Some(match c {
        // Around a gap ("〈◊〉", "〈 in non-Latin alphabet 〉").
        '\u{3008}' => "\u{27e8}",
        '\u{3009}' => "\u{27e9}",
        // A punctuation mark of unclear form.
        '\u{25aa}' => "\u{b7}",
        // A word divided into syllables, in a sung text ("o╌pen").
        '\u{254c}' => "-",
        '\u{271a}' => "+",
        '\u{261e}' => "\u{2192}",
        '\u{261c}' => "\u{2190}",
        '\u{261d}' => "\u{2191}",
        '\u{204e}' => "*",
        // Music the encoders did not transcribe.
        '\u{266b}' => "\u{27e8}music\u{27e9}",
        // Ornaments.
        '\u{2767}' | '\u{2740}' => "",
        _ => return None,
    })
}

/// `text` with [`tcp_glyph`]'s marks written as their equivalents; `ornaments` counts those
/// dropped.
fn tcp_text<'t>(text: &'t str, ornaments: &mut u64) -> std::borrow::Cow<'t, str> {
    if !text.chars().any(|c| tcp_glyph(c).is_some()) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match tcp_glyph(c) {
            Some("") => *ornaments += 1,
            Some(s) => out.push_str(s),
            None => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// A node's text, whitespace collapsed, at most [`MAX_META_CHARS`] characters.
fn plain(node: Node<'_, '_>) -> String {
    let mut s = String::new();
    let mut ornaments = 0u64;
    // A word broken at the line's end goes on with the next text that is not blank.
    let mut join = false;
    for d in node.descendants() {
        if is_element(&d, "g") && d.attribute("ref") == Some("char:EOLhyphen") {
            join = true;
            continue;
        }
        if !d.is_text() {
            continue;
        }
        let text = tcp_text(d.text().unwrap_or_default(), &mut ornaments);
        if join {
            if text.trim().is_empty() {
                continue;
            }
            join = false;
            if s.ends_with(' ') {
                s.pop();
            }
            s.push_str(text.trim_start());
        } else {
            s.push_str(&text);
        }
        s.push(' ');
        if s.len() > MAX_META_CHARS * 4 {
            break;
        }
    }
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_META_CHARS)
        .collect()
}

/// The header of a TCP text: the source edition's title, author and date
/// (`sourceDesc/biblFull`, else the file's own `titleStmt`), the TCP id (`idno type="DLPS"`)
/// and the availability statement.
pub fn tei_header(doc: &Document<'_>) -> TeiHeader {
    let Some(header) = child(doc.root_element(), "teiHeader") else {
        return TeiHeader::default();
    };
    let file_desc = child(header, "fileDesc");
    let bibl = file_desc
        .and_then(|f| child(f, "sourceDesc"))
        .and_then(|s| child(s, "biblFull"));
    let title_stmt = bibl
        .and_then(|b| child(b, "titleStmt"))
        .or_else(|| file_desc.and_then(|f| child(f, "titleStmt")));
    let pick = |parent: Option<Node<'_, '_>>, name: &str| {
        parent
            .and_then(|p| child(p, name))
            .map(plain)
            .unwrap_or_default()
    };
    let own_publication = file_desc.and_then(|f| child(f, "publicationStmt"));
    // ECCO-TCP texts name their TCP id in `idno type="TCP"` (their DLPS id is a number).
    let idno = |kind: &str| {
        own_publication.and_then(|p| {
            p.children()
                .find(|c| is_element(c, "idno") && c.attribute("type") == Some(kind))
        })
    };
    let id = idno("TCP")
        .or_else(|| idno("DLPS"))
        .map(plain)
        .unwrap_or_default();
    let language = child(header, "profileDesc")
        .and_then(|p| child(p, "langUsage"))
        .and_then(|l| child(l, "language"))
        .and_then(|l| l.attribute("ident"))
        .map_or_else(|| "en".to_string(), bcp47);
    TeiHeader {
        title: pick(title_stmt, "title"),
        author: pick(title_stmt, "author"),
        date: pick(bibl.and_then(|b| child(b, "publicationStmt")), "date"),
        id,
        availability: pick(own_publication, "availability"),
        language,
    }
}

/// Parse `xml` as [`import_tcp_tei`] does (no DTD, the same limits).
fn parse_tei(xml: &str) -> Result<Document<'_>, ImportError> {
    if xml.len() > MAX_TEI_BYTES {
        return Err(ImportError::TooLarge("TEI text".into()));
    }
    Document::parse_with_options(
        xml,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_NODES,
            ..ParsingOptions::default()
        },
    )
    .map_err(|e| ImportError::Xml(e.to_string()))
}

/// The header of the TCP text `xml`, read as an import reads it.
pub fn read_tei_header(xml: &str) -> Result<TeiHeader, ImportError> {
    Ok(tei_header(&parse_tei(xml)?))
}

/// Words that start a title page's subtitle or statement of responsibility, in the spellings
/// of the time ("… contayning meditations", "… preached at Paul's Cross", "… by Richard
/// Sibbes"). A title is cut at the first, when it is at least [`MIN_SHORT_TITLE`] bytes in.
const TITLE_CUTS: &[&str] = &[
    " / ",
    " by ",
    " wherein ",
    " wherin ",
    " whereunto ",
    " whereby ",
    " together with ",
    " preached ",
    " delivered ",
    " deliuered ",
    " published ",
    " written ",
    " being ",
    " with an ",
    " to which ",
    " in which ",
    " as it was ",
    " set forth ",
    " made by ",
    " composed ",
    " collected ",
    " translated ",
    " containing ",
    " contayning ",
    " conteining ",
    " conteyning ",
    " shewing ",
    " shewinge ",
    " declaring ",
    " discouering ",
    " discovering ",
    " concerning ",
    " touching ",
    " opened ",
    " explained ",
    " handled ",
    " in two ",
    " in three ",
    " in several ",
    " in sundry ",
    " divided into ",
];
/// A title keeps at least this many bytes before a cut.
const MIN_SHORT_TITLE: usize = 20;
/// A short title is cut, at a word, to about this many characters.
const MAX_SHORT_TITLE: usize = 100;

/// A TCP title as a short title. TCP transcribes the whole title page ("The bruised reede, and
/// smoaking flax Some sermons contracted out of the 12. of Matth. 20. At the desire, and for
/// the good of weaker Christians. By R. Sibbes"); it is cut at its first colon or semicolon,
/// at the first of [`TITLE_CUTS`], or at a sentence's end (a full stop after a word of four or
/// more letters and before a capital, so "Mr. Cotton" and "Matth. 20" go on), whichever comes
/// first, then at a word to
/// about [`MAX_SHORT_TITLE`] characters.
pub fn short_title(full: &str) -> String {
    let full = full.trim();
    // ASCII lowercase keeps byte offsets.
    let lower = full.to_ascii_lowercase();
    let mut end = full.find([':', ';']).unwrap_or(full.len());
    let from = (MIN_SHORT_TITLE..=full.len())
        .find(|&i| full.is_char_boundary(i))
        .unwrap_or(full.len());
    for cut in TITLE_CUTS {
        if let Some(i) = lower[from..].find(cut) {
            end = end.min(from + i);
        }
    }
    for (i, _) in full.match_indices(". ") {
        let word = full[..i]
            .rsplit(|c: char| !c.is_alphabetic())
            .next()
            .unwrap_or("");
        let next = full[i + 2..].chars().next();
        if word.chars().count() >= 4 && next.is_some_and(char::is_uppercase) {
            end = end.min(i);
            break;
        }
    }
    let short = full[..end]
        .trim()
        .trim_end_matches([',', '.', ';', ':'])
        .trim();
    let short = if short.is_empty() { full } else { short };
    cut_at_word(short, MAX_SHORT_TITLE)
}

/// One Division: its title and the TEI nodes it is made of.
struct DivisionSource<'a, 'i> {
    title: String,
    nodes: Vec<Node<'a, 'i>>,
}

/// A `div`'s title: its `<head>`, else its `type` and `n` ("Chapter 1", "Title page"),
/// prefixed with its parent's when it is a part of one ("Part 1 · Chapter 2").
fn div_title(div: Node<'_, '_>, parent: Option<&str>) -> String {
    let title = child(div, "head")
        .map(plain)
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| {
            let kind = div.attribute("type").unwrap_or("section").replace('_', " ");
            let mut chars = kind.chars();
            let kind: String = chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default();
            match div.attribute("n") {
                Some(n) => format!("{kind} {n}"),
                None => kind,
            }
        });
    let title = cut_at_word(&title, MAX_TITLE_CHARS);
    match parent {
        Some(p) if p.chars().count() <= MAX_PREFIX_CHARS && !title.starts_with(p) => {
            format!("{p} · {title}")
        }
        _ => title,
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

/// The Divisions of `<front>`, `<body>` and `<back>`, in order: each child `div`. A `div`
/// holding two or more `div`s (a part of chapters) is split into them, what comes before its
/// first one (beyond its head) a Division of its own; their own inner `div`s are headings
/// inside them. Text outside any `div` is a Division named for where it is.
fn divisions<'a, 'i>(text: Node<'a, 'i>) -> Vec<DivisionSource<'a, 'i>> {
    let mut out = Vec::new();
    text_divisions(text, 0, &mut out);
    out
}

/// The Divisions of one `<text>`. A `<group>` (several works printed together, each its own
/// `<text>`), where a `<text>` or its `<front>`, `<body>` or `<back>` holds one, gives each
/// of its texts' Divisions in turn.
fn text_divisions<'a, 'i>(text: Node<'a, 'i>, depth: usize, out: &mut Vec<DivisionSource<'a, 'i>>) {
    if depth > MAX_DEPTH {
        return;
    }
    let group = |g: Node<'a, 'i>, out: &mut Vec<DivisionSource<'a, 'i>>| {
        for t in g.children().filter(|c| is_element(c, "text")) {
            text_divisions(t, depth + 1, out);
        }
    };
    for part in text.children().filter(|c| {
        c.is_element() && matches!(c.tag_name().name(), "front" | "body" | "back" | "group")
    }) {
        if is_element(&part, "group") {
            group(part, out);
            continue;
        }
        let loose_title = match part.tag_name().name() {
            "front" => "Front matter",
            "back" => "Back matter",
            _ => "Text",
        };
        let mut loose: Vec<Node<'a, 'i>> = Vec::new();
        for node in part.children().filter(Node::is_element) {
            if is_element(&node, "group") {
                if !loose.is_empty() {
                    out.push(DivisionSource {
                        title: loose_title.into(),
                        nodes: std::mem::take(&mut loose),
                    });
                }
                group(node, out);
                continue;
            }
            if !is_element(&node, "div") {
                loose.push(node);
                continue;
            }
            if !loose.is_empty() {
                out.push(DivisionSource {
                    title: loose_title.into(),
                    nodes: std::mem::take(&mut loose),
                });
            }
            split_div(node, out);
        }
        if !loose.is_empty() {
            out.push(DivisionSource {
                title: loose_title.into(),
                nodes: loose,
            });
        }
    }
}

fn split_div<'a, 'i>(div: Node<'a, 'i>, out: &mut Vec<DivisionSource<'a, 'i>>) {
    let inner = div.children().filter(|c| is_element(c, "div")).count();
    if inner < 2 {
        out.push(DivisionSource {
            title: div_title(div, None),
            nodes: vec![div],
        });
        return;
    }
    let parent = div_title(div, None);
    let elements: Vec<Node<'a, 'i>> = div.children().filter(Node::is_element).collect();
    let first_div = elements
        .iter()
        .position(|n| is_element(n, "div"))
        .unwrap_or(elements.len());
    let before = &elements[..first_div];
    if before.iter().any(|n| !is_element(n, "head")) {
        out.push(DivisionSource {
            title: parent.clone(),
            nodes: before.to_vec(),
        });
    } else if !before.is_empty() {
        // Only the part's head: it opens the first chapter.
    }
    let mut current: Option<DivisionSource<'a, 'i>> = None;
    for (i, n) in elements[first_div..].iter().enumerate() {
        if is_element(n, "div") {
            out.extend(current.take());
            let mut nodes = Vec::new();
            // A part whose only text is its head: the head opens its first chapter.
            if i == 0 && !before.is_empty() && before.iter().all(|b| is_element(b, "head")) {
                nodes.extend(before.iter().copied());
            }
            nodes.push(*n);
            // A long part title (a sermon's) is not a prefix: the first of its parts, which
            // opens with its head, is titled by it instead.
            let title = if i == 0
                && !before.is_empty()
                && before.iter().all(|b| is_element(b, "head"))
                && parent.chars().count() > MAX_PREFIX_CHARS
            {
                parent.clone()
            } else {
                div_title(*n, Some(&parent))
            };
            current = Some(DivisionSource { title, nodes });
        } else if let Some(d) = current.as_mut() {
            d.nodes.push(*n);
        }
    }
    out.extend(current);
}

/// Elements read as blocks of their own.
fn is_block(name: &str) -> bool {
    matches!(
        name,
        "p" | "head"
            | "opener"
            | "closer"
            | "salute"
            | "signed"
            | "trailer"
            | "argument"
            | "byline"
            | "dateline"
            | "epigraph"
            | "docTitle"
            | "titlePart"
            | "docImprint"
            | "docAuthor"
            | "docDate"
            | "imprimatur"
            | "postscript"
            | "speaker"
            | "l"
            | "item"
            | "lg"
            | "list"
            | "sp"
            | "titlePage"
            | "div"
            | "floatingText"
            | "body"
            | "front"
            | "back"
            | "group"
            | "text"
            | "table"
            | "figure"
    )
}

/// Writes one Division's text, and the notes in it.
struct Writer<'r> {
    buf: TextBuf,
    report: &'r mut ImportReport,
    notes: &'r mut Vec<(UnitId, TextBuf)>,
    note_count: &'r mut u32,
    /// Where the open block started, while one is open.
    block: Option<(usize, SpanKind)>,
    /// Pages turned between blocks: they open the next block.
    pages: Vec<String>,
    /// Skip whitespace before the next text: a word broken at the line's end goes on.
    join: bool,
}

impl Writer<'_> {
    fn dropped(&mut self, what: &str) {
        *self
            .report
            .dropped_markup
            .entry(what.to_string())
            .or_default() += 1;
    }

    fn open(&mut self, kind: SpanKind) {
        self.close();
        let start = self.buf.start_block();
        for label in std::mem::take(&mut self.pages) {
            self.buf.add_span(start..start, SpanKind::Page { label });
        }
        self.block = Some((start, kind));
    }

    fn close(&mut self) {
        if let Some((start, kind)) = self.block.take() {
            let end = self.buf.pos();
            if start < end {
                self.buf.add_span(start..end, kind);
            }
        }
    }

    fn text(&mut self, t: &str) {
        let mut ornaments = 0u64;
        let t = tcp_text(t, &mut ornaments);
        if ornaments > 0 {
            *self
                .report
                .dropped_markup
                .entry("ornament".to_string())
                .or_default() += ornaments;
        }
        let t: &str = &t;
        let t = if self.join && !t.trim().is_empty() {
            self.join = false;
            t.trim_start()
        } else if self.join {
            ""
        } else {
            t
        };
        if t.trim().is_empty() && self.block.is_none() {
            return;
        }
        if self.block.is_none() {
            self.open(SpanKind::Para {
                style: ParaStyle::Prose,
            });
        }
        self.buf.push_text(t);
    }

    /// A node of the Division's structure: a block, a container of blocks, or inline content.
    fn node(&mut self, node: Node<'_, '_>, depth: usize, level: u8) {
        if node.is_text() {
            self.text(node.text().unwrap_or_default());
            return;
        }
        if !node.is_element() {
            return;
        }
        if depth > MAX_DEPTH {
            self.text(&plain(node));
            return;
        }
        let name = node.tag_name().name();
        match name {
            "div" | "floatingText" | "body" | "front" | "back" | "group" | "text" | "lg"
            | "list" | "sp" | "titlePage" => {
                let inner = if name == "div" {
                    (level + 1).min(6)
                } else {
                    level
                };
                for c in node.children() {
                    self.node(c, depth + 1, inner);
                }
                self.close();
            }
            "head" => self.block(node, SpanKind::Heading { level }, depth, level),
            "l" => self.block(
                node,
                SpanKind::Para {
                    style: ParaStyle::Poetry1,
                },
                depth,
                level,
            ),
            "item" => self.block(
                node,
                SpanKind::Para {
                    style: ParaStyle::List,
                },
                depth,
                level,
            ),
            "figure" | "table" | "graphic" => self.dropped(name),
            "note" => self.note(node),
            // A quotation or citation standing between blocks (an epigraph's text, a title
            // page's motto) is a block; inside a paragraph it is inline.
            n if is_block(n) || matches!(n, "q" | "quote" | "cit" | "bibl") => self.block(
                node,
                SpanKind::Para {
                    style: ParaStyle::Prose,
                },
                depth,
                level,
            ),
            _ => self.inline(node, depth),
        }
    }

    /// A block element: its inline content under `kind`. A block child (a list or verse in a
    /// paragraph) closes it, is read as its own block, and the rest goes on in a new one.
    fn block(&mut self, node: Node<'_, '_>, kind: SpanKind, depth: usize, level: u8) {
        self.open(kind.clone());
        for c in node.children() {
            if c.is_element() && c.tag_name().name() != "note" && is_block(c.tag_name().name()) {
                self.close();
                self.node(c, depth + 1, level);
                self.open(kind.clone());
            } else {
                self.inline_node(c, depth + 1);
            }
        }
        self.close();
    }

    fn inline_node(&mut self, node: Node<'_, '_>, depth: usize) {
        if node.is_text() {
            self.text(node.text().unwrap_or_default());
        } else if node.is_element() {
            if depth > MAX_DEPTH {
                self.text(&plain(node));
            } else {
                self.inline(node, depth);
            }
        }
    }

    fn inline_children(&mut self, node: Node<'_, '_>, depth: usize) {
        for c in node.children() {
            self.inline_node(c, depth + 1);
        }
    }

    fn inline(&mut self, node: Node<'_, '_>, depth: usize) {
        let name = node.tag_name().name();
        match name {
            "g" if node.attribute("ref") == Some("char:EOLhyphen") => self.join = true,
            "pb" => {
                let Some(n) = node.attribute("n").map(str::trim).filter(|n| !n.is_empty()) else {
                    return;
                };
                let label: String = n.chars().filter(|c| !c.is_control()).take(40).collect();
                if self.block.is_some() {
                    self.buf.flush_space();
                    let at = self.buf.pos();
                    self.buf.add_span(at..at, SpanKind::Page { label });
                } else {
                    self.pages.push(label);
                }
            }
            "lb" => self.text(" "),
            "gap" => {
                let desc = child(node, "desc").map(plain).filter(|d| !d.is_empty());
                self.text(&desc.unwrap_or_else(|| "\u{27e8}\u{2026}\u{27e9}".into()));
            }
            "hi" if matches!(node.attribute("rend"), Some("sup" | "sub")) => {
                self.inline_children(node, depth);
            }
            "hi" => self.styled(node, SpanKind::Italic, depth),
            "foreign" => self.styled(node, SpanKind::Foreign, depth),
            "note" => self.note(node),
            "figure" | "table" | "graphic" => self.dropped(name),
            "milestone" | "anchor" | "fw" => {}
            _ => self.inline_children(node, depth),
        }
    }

    fn styled(&mut self, node: Node<'_, '_>, kind: SpanKind, depth: usize) {
        if self.block.is_none() {
            self.open(SpanKind::Para {
                style: ParaStyle::Prose,
            });
        }
        self.buf.flush_space();
        let start = self.buf.pos();
        self.buf.tracked_end = start;
        self.inline_children(node, depth);
        let end = self.buf.tracked_end.clamp(start, self.buf.pos());
        if start < end {
            self.buf.add_span(start..end, kind);
        }
    }

    /// A note: a unit of its own, referenced where it stands.
    fn note(&mut self, node: Node<'_, '_>) {
        let text = plain(node);
        if text.is_empty() {
            return;
        }
        *self.note_count += 1;
        let id = UnitId::note(*self.note_count);
        let mut buf = TextBuf::new();
        let start = buf.start_block();
        buf.push_text(&text);
        let end = buf.pos();
        buf.add_span(
            start..end,
            SpanKind::Para {
                style: ParaStyle::Prose,
            },
        );
        self.notes.push((id.clone(), buf));
        if self.block.is_none() {
            self.open(SpanKind::Para {
                style: ParaStyle::Prose,
            });
        }
        let at = self.buf.pos();
        self.buf.add_span(
            at..at,
            SpanKind::NoteRef {
                note: id,
                note_kind: NoteKind::Footnote,
            },
        );
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

/// Import one TCP text. `progress(done, total)` returns false to cancel.
pub fn import_tcp_tei(
    xml: &str,
    options: &TeiBookOptions,
    mut progress: impl FnMut(usize, usize) -> bool,
) -> Result<(Resource, ImportReport), ImportError> {
    let resource_id = options.resource_id.clone();
    if !is_valid_resource_id(&resource_id) {
        return Err(ImportError::InvalidResourceId(resource_id));
    }
    if xml.len() > MAX_TEI_BYTES {
        return Err(ImportError::TooLarge(options.source_file.clone()));
    }
    let doc = parse_tei(xml)?;
    let header = tei_header(&doc);
    let text = doc
        .root_element()
        .children()
        .find(|n| is_element(n, "text"))
        .ok_or(ImportError::NoText)?;
    let title = Some(options.title.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| short_title(&header.title));
    let title = if title.is_empty() {
        resource_id.clone()
    } else {
        title
    };
    let abbreviation = [options.abbreviation.trim(), header.id.as_str(), "TCP"]
        .into_iter()
        .find(|a| !a.is_empty())
        .unwrap_or("TCP")
        .to_string();
    let language = [options.language.as_str(), header.language.as_str(), "en"]
        .into_iter()
        .find(|l| !l.is_empty())
        .unwrap_or("en")
        .to_string();
    let mut manifest = Manifest::new(
        &resource_id,
        &title,
        abbreviation,
        ResourceType::Book,
        language,
    );
    manifest.license = LICENSE_UNKNOWN.to_string();
    manifest.publisher = Some(header.author.clone()).filter(|a| !a.is_empty());
    manifest.edition = Some(header.date.clone()).filter(|d| !d.is_empty());
    manifest.origin = Origin::Imported;
    manifest.source_format = TCP_TEI_FORMAT.into();
    manifest.source_file = (!options.source_file.is_empty()).then(|| options.source_file.clone());
    manifest.import_tool_version = options.tool_version.clone();
    manifest.format_version = FORMAT_VERSION;
    let mut builder = ResourceBuilder::new(manifest);
    let mut report = ImportReport {
        resource_id: resource_id.clone(),
        title,
        source_format: TCP_TEI_FORMAT.into(),
        source_files: vec![options.source_file.clone()],
        license_missing: true,
        ..ImportReport::default()
    };
    let parts = divisions(text);
    let mut notes: Vec<(UnitId, TextBuf)> = Vec::new();
    let mut note_count = 0u32;
    let mut written = 0u32;
    for (i, part) in parts.iter().enumerate() {
        if !progress(i, parts.len()) {
            return Err(ImportError::Cancelled);
        }
        let mut writer = Writer {
            buf: TextBuf::new(),
            report: &mut report,
            notes: &mut notes,
            note_count: &mut note_count,
            block: None,
            pages: Vec::new(),
            join: false,
        };
        for n in &part.nodes {
            writer.node(*n, 1, 2);
        }
        writer.close();
        // Pages turned at the end of the Division mark its end.
        let mut buf = writer.buf;
        let end = buf.pos();
        for label in writer.pages {
            buf.add_span(end..end, SpanKind::Page { label });
        }
        buf.normalize_nfc();
        if buf.text.trim().is_empty() {
            continue;
        }
        written += 1;
        let id =
            UnitId::new(format!("div.{written}")).map_err(|e| ImportError::Xml(e.to_string()))?;
        add_references(&buf.text, &mut report, &mut builder, &id);
        builder.add_unit(
            id.clone(),
            UnitKind::Division,
            Some(part.title.clone()),
            buf.text,
        );
        for (range, kind) in buf.spans {
            builder.add_span(&id, range, kind);
        }
    }
    if written == 0 {
        return Err(ImportError::NoText);
    }
    for (id, buf) in notes {
        add_references(&buf.text, &mut report, &mut builder, &id);
        builder.add_unit(id.clone(), UnitKind::Note, None, buf.text);
        for (range, kind) in buf.spans {
            builder.add_span(&id, range, kind);
        }
        report.notes += 1;
    }
    report.entries = written as usize;
    let resource = builder.build().map_err(ImportError::Invalid)?;
    Ok((resource, report))
}

#[cfg(test)]
#[path = "tcp_tei_tests.rs"]
mod tests;
