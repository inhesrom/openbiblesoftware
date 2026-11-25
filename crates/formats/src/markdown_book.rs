//! Markdown book Import (ADR 0023): a book kept as Markdown files, such as the Christian Open
//! Library's (#344), → [`ResourceBuilder`] → a Book [`Resource`] plus an [`ImportReport`].
//!
//! | Source | span |
//! |---|---|
//! | YAML front matter `key: value` (`title`, `author`, `date`, `license`, `source`, `language`) | manifest |
//! | `#`…`######` headings | `heading` (level 1-6); the shallowest level used more than once splits the book into Divisions (text units) |
//! | paragraphs, list items, block quotes | `para` prose or list; the lines of a paragraph broken with hard breaks (`\\` or two spaces) are `poetry1` lines |
//! | `*…*`, `**…**` | `italic`, `bold` |
//! | `<!-- p. 23 -->` | zero-width `page` with the printed page's label |
//! | `[^n]` with its `[^n]: …` | note unit + zero-width `note_ref` |
//! | Bible references (`Rom 8:28`, `Rom. 8. 28`, `Rom. viii. 28`) | `scripture_ref` |
//!
//! Link text is kept and its target dropped. Images, tables, code, raw HTML and other comments
//! are dropped and counted in the report. A title heading that opens each file and the `*by …*`
//! line under it are the title, not text. A work split into Part files is one Resource, its
//! Parts read in numeric order; the main file then only gives the front matter (its body is the
//! list of Parts).

use std::collections::{BTreeMap, HashMap};
use std::ops::Range;
use std::sync::OnceLock;

use obs_model::{
    FORMAT_VERSION, LICENSE_UNKNOWN, Manifest, NoteKind, Origin, ParaStyle, Resource,
    ResourceBuilder, ResourceType, SpanKind, UnitId, UnitKind, is_valid_resource_id,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use regex::Regex;

use crate::error::ImportError;
use crate::report::ImportReport;
use crate::text_buf::TextBuf;

/// `source_format` of a Markdown book.
pub const MARKDOWN_BOOK_FORMAT: &str = "markdown-book";

/// Front matter longer than this is not read as metadata (it is a damaged or hostile file).
const MAX_FRONT_MATTER: usize = 16 * 1024;
/// One front-matter value is cut to this many characters.
const MAX_META_CHARS: usize = 2_000;
/// One page label is cut to this many characters.
const MAX_PAGE_LABEL: usize = 40;
/// A Markdown file larger than this is refused (COL's largest files are far smaller).
pub const MAX_BOOK_FILE_BYTES: usize = 64 * 1024 * 1024;

/// Identity of the Resource a Markdown book Import creates.
#[derive(Debug, Clone, Default)]
pub struct MarkdownBookOptions {
    pub resource_id: String,
    /// File or folder name shown in About and the report.
    pub source_file: String,
    /// Version of the tool doing the Import, recorded in the manifest.
    pub tool_version: String,
}

/// One Markdown file of a book: its name (shown in the report) and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookFile {
    pub name: String,
    pub text: String,
}

/// The files of one book: its main file, and its Part files in numeric order. A folder may
/// hold Part files whose main file is missing; their book has no main file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookFiles {
    pub main: Option<String>,
    pub parts: Vec<String>,
}

impl BookFiles {
    /// Every file of the book, the main file first.
    pub fn files(&self) -> impl Iterator<Item = &String> {
        self.main.iter().chain(self.parts.iter())
    }
}

#[expect(clippy::expect_used, reason = "the patterns are source-code constants")]
fn part_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^(?P<stem>.+?)(?: — Part |, Part )(?P<n>\d{1,6})\.md$")
            .expect("constant regular expression")
    })
}

/// Whether a file name is a Markdown file (`.md`, `.markdown`, any case).
pub fn is_markdown_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// The name a book's files share: a file name without its extension or `— Part N`
/// (`Owen - Mortification — Part 2.md` → `Owen - Mortification`).
pub fn book_stem(file: &str) -> &str {
    if let Some(c) = part_re().captures(file)
        && let Some(stem) = c.name("stem")
    {
        return stem.as_str();
    }
    file.rsplit_once('.').map_or(file, |(stem, _)| stem)
}

/// Group the Markdown file names of one folder (relative paths with `/`) into books: each main
/// file with the Part files beside it (`<Title> — Part 2.md` or `<Title>, Part 2.md`), in
/// numeric order. Books come in the order of their first file name; other names are ignored.
pub fn group_book_files(names: &[String]) -> Vec<BookFiles> {
    let mut sorted: Vec<&String> = names.iter().filter(|n| is_markdown_name(n)).collect();
    sorted.sort();
    sorted.dedup();
    // (folder, stem) → the book, and its Part files with their numbers.
    type Grouped = (BookFiles, Vec<(u64, String)>);
    let mut books: BTreeMap<(String, String), Grouped> = BTreeMap::new();
    for name in sorted {
        let (folder, file) = match name.rsplit_once('/') {
            Some((d, f)) => (d.to_string(), f),
            None => (String::new(), name.as_str()),
        };
        let stem = book_stem(file).to_string();
        if let Some(c) = part_re().captures(file) {
            let n = c["n"].parse::<u64>().unwrap_or(u64::MAX);
            books
                .entry((folder, stem))
                .or_default()
                .1
                .push((n, name.clone()));
        } else {
            books.entry((folder, stem)).or_default().0.main = Some(name.clone());
        }
    }
    let mut out: Vec<BookFiles> = books
        .into_values()
        .map(|(mut book, mut parts)| {
            parts.sort();
            book.parts = parts.into_iter().map(|(_, name)| name).collect();
            book
        })
        .collect();
    out.sort_by(|a, b| a.files().next().cmp(&b.files().next()));
    out
}

/// The front matter of a file (`---` lines around `key: value` lines at its very start) and
/// the text after it. Only plain one-line scalars are read: anything else (lists, nesting,
/// anchors, aliases) is skipped, so no value can expand. Keys are lowercased.
pub fn split_front_matter(text: &str) -> (BTreeMap<String, String>, &str) {
    let mut meta = BTreeMap::new();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (meta, text);
    };
    let mut at = 0;
    let mut end = None;
    for line in rest.split_inclusive('\n') {
        if at > MAX_FRONT_MATTER {
            break;
        }
        if line.trim_end() == "---" || line.trim_end() == "..." {
            end = Some(at + line.len());
            break;
        }
        if let Some((key, value)) = meta_line(line) {
            meta.entry(key).or_insert(value);
        }
        at += line.len();
    }
    match end {
        Some(end) => (meta, &rest[end..]),
        None => (BTreeMap::new(), text),
    }
}

fn meta_line(line: &str) -> Option<(String, String)> {
    if line.starts_with([' ', '\t', '-', '#']) {
        return None;
    }
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty()
        || key.len() > 40
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let mut value = value.trim();
    if value.starts_with(['&', '*', '|', '>', '[', '{', '!']) {
        return None; // anchors, aliases, block scalars, flow collections, tags
    }
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value = &value[1..value.len() - 1];
    }
    let value = value.replace("\\\"", "\"").replace("''", "'");
    let value: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_META_CHARS)
        .collect();
    let value = value.trim().to_string();
    (!value.is_empty()).then(|| (key.to_ascii_lowercase(), value))
}

/// The SPDX id of a book's `license:` value, when it is one of the whole values this allowlist
/// names (case, spacing, `-` and `_` aside); `None` for anything else, which is kept as written
/// and makes the Resource's licence unknown (ADR 0023).
pub fn book_license_spdx(value: &str) -> Option<&'static str> {
    let key: String = value
        .to_lowercase()
        .replace(['-', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let key = key.trim_end_matches('.');
    Some(match key {
        "public domain" | "publicdomain" | "pd" | "licenseref publicdomain" => {
            "LicenseRef-PublicDomain"
        }
        "cc0" | "cc0 1.0" | "cc0 1.0 universal" | "cc zero" => "CC0-1.0",
        "cc by 4.0" | "cc by 4.0 international" | "creative commons attribution 4.0" => "CC-BY-4.0",
        "cc by sa 4.0"
        | "cc bysa 4.0"
        | "creative commons attribution sharealike 4.0"
        | "creative commons attribution share alike 4.0" => "CC-BY-SA-4.0",
        _ => return None,
    })
}

/// What the import keeps of one file.
#[derive(Debug, Default)]
struct Parsed {
    blocks: Vec<Block>,
    /// Footnote bodies by label, each a list of paragraphs of inline content.
    notes: HashMap<String, Vec<Vec<Inline>>>,
}

#[derive(Debug, Clone)]
enum BlockKind {
    Heading(u8),
    Para(ParaStyle),
    /// A page marker on a line of its own.
    Page(String),
}

#[derive(Debug, Clone)]
struct Block {
    kind: BlockKind,
    inlines: Vec<Inline>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    Italic,
    Bold,
}

#[derive(Debug, Clone)]
enum Inline {
    Text(String),
    Start(Style),
    End(Style),
    /// A footnote reference, by the label it has in its own file.
    Note(String),
    Page(String),
    SoftBreak,
    HardBreak,
}

#[expect(clippy::expect_used, reason = "the pattern is a source-code constant")]
fn page_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"<!--[ \t]{0,20}p\.[ \t]{0,20}([^<>\n]{0,200}?)[ \t]{0,20}-->")
            .expect("constant regular expression")
    })
}

/// A page marker's label, as COL's indexer reads it: `{iii}`, `page 3`, `pg3`, `p.2` → the
/// page as printed.
fn page_label(raw: &str) -> String {
    let s = raw.trim().trim_matches(['{', '}']).trim();
    let lower = s.to_ascii_lowercase();
    let s = ["page", "pg", "p."]
        .iter()
        .find_map(|p| lower.starts_with(p).then(|| &s[p.len()..]))
        .unwrap_or(s);
    let s = s.trim().trim_matches(['{', '}', ' ']);
    let s = if s.is_empty() { raw.trim() } else { s };
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_PAGE_LABEL)
        .collect::<String>()
        .trim()
        .to_string()
}

/// The page labels in a piece of HTML (a comment or an HTML block), in order.
fn page_labels(html: &str) -> Vec<String> {
    page_re()
        .captures_iter(html)
        .map(|c| page_label(&c[1]))
        .filter(|l| !l.is_empty())
        .collect()
}

/// A line of nothing but page markers between two lines of a paragraph joins the line before
/// it: in CommonMark such a line is an HTML block that would cut the paragraph in two, where
/// the printed page only turned (COL's indexer reads it the same way).
fn join_page_lines(body: &str) -> std::borrow::Cow<'_, str> {
    let lines: Vec<&str> = body.split('\n').collect();
    let is_marks = |l: &str| {
        let t = l.trim();
        !t.is_empty() && page_re().replace_all(t, "").trim().is_empty()
    };
    let is_text = |l: &str| {
        let t = l.trim_start();
        !t.is_empty() && !t.starts_with('#') && !is_marks(l)
    };
    let joins: Vec<bool> = (0..lines.len())
        .map(|i| {
            i > 0
                && i + 1 < lines.len()
                && is_marks(lines[i])
                && is_text(lines[i - 1])
                && is_text(lines[i + 1])
        })
        .collect();
    if !joins.contains(&true) {
        return std::borrow::Cow::Borrowed(body);
    }
    let mut out = String::with_capacity(body.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push(if joins[i] { ' ' } else { '\n' });
        }
        out.push_str(if joins[i] { line.trim() } else { line });
    }
    std::borrow::Cow::Owned(out)
}

/// Read one file's body (front matter already removed) into blocks and footnotes. `dropped`
/// counts markup with no place in the span vocabulary.
fn parse_body(body: &str, dropped: &mut BTreeMap<String, u64>) -> Parsed {
    let body = join_page_lines(body);
    let body = body.as_ref();
    let mut out = Parsed::default();
    let options = Options::ENABLE_FOOTNOTES | Options::ENABLE_TABLES;
    // Block being filled, and the footnote being filled (its paragraphs).
    let mut current: Option<Block> = None;
    let mut note: Option<(String, Vec<Vec<Inline>>)> = None;
    let mut list_depth = 0usize;
    // Inside an image, a table or a code block: text there is dropped.
    let mut skip = 0usize;
    let drop = |name: &str, dropped: &mut BTreeMap<String, u64>| {
        *dropped.entry(name.to_string()).or_default() += 1;
    };
    fn push(current: &mut Option<Block>, note: &mut Option<(String, Vec<Vec<Inline>>)>, i: Inline) {
        if let Some((_, paras)) = note.as_mut() {
            if paras.is_empty() {
                paras.push(Vec::new());
            }
            if let Some(last) = paras.last_mut() {
                last.push(i);
            }
        } else if let Some(block) = current.as_mut() {
            block.inlines.push(i);
        }
    }
    fn open(
        current: &mut Option<Block>,
        note: &mut Option<(String, Vec<Vec<Inline>>)>,
        blocks: &mut Vec<Block>,
        kind: BlockKind,
    ) {
        if let Some((_, paras)) = note.as_mut() {
            paras.push(Vec::new());
            return;
        }
        if let Some(done) = current.take() {
            blocks.push(done);
        }
        *current = Some(Block {
            kind,
            inlines: Vec::new(),
        });
    }
    for event in Parser::new_ext(body, options) {
        match event {
            Event::Start(Tag::Image { .. }) => {
                skip += 1;
                drop("image", dropped);
            }
            Event::End(TagEnd::Image) => skip = skip.saturating_sub(1),
            Event::Start(Tag::Table(_)) => {
                skip += 1;
                drop("table", dropped);
            }
            Event::End(TagEnd::Table) => skip = skip.saturating_sub(1),
            Event::Start(Tag::CodeBlock(_)) => {
                skip += 1;
                drop("code", dropped);
            }
            Event::End(TagEnd::CodeBlock) => skip = skip.saturating_sub(1),
            _ if skip > 0 => {}
            Event::Start(Tag::Heading { level, .. }) => {
                let level = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                open(
                    &mut current,
                    &mut note,
                    &mut out.blocks,
                    BlockKind::Heading(level),
                );
            }
            Event::Start(Tag::Paragraph) => {
                if list_depth > 0
                    && let Some(block) = current.as_ref()
                    && matches!(block.kind, BlockKind::Para(ParaStyle::List))
                    && block.inlines.is_empty()
                {
                    continue; // the item's own paragraph
                }
                let style = if list_depth > 0 {
                    ParaStyle::List
                } else {
                    ParaStyle::Prose
                };
                open(
                    &mut current,
                    &mut note,
                    &mut out.blocks,
                    BlockKind::Para(style),
                );
            }
            Event::Start(Tag::Item) => open(
                &mut current,
                &mut note,
                &mut out.blocks,
                BlockKind::Para(ParaStyle::List),
            ),
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Start(Tag::FootnoteDefinition(label)) => {
                if let Some(done) = current.take() {
                    out.blocks.push(done);
                }
                note = Some((label.to_string(), Vec::new()));
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                if let Some((label, paras)) = note.take() {
                    out.notes.entry(label).or_insert(paras);
                }
            }
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::Item) => {
                if note.is_none()
                    && let Some(done) = current.take()
                {
                    out.blocks.push(done);
                }
            }
            Event::Start(Tag::Emphasis) => {
                push(&mut current, &mut note, Inline::Start(Style::Italic))
            }
            Event::End(TagEnd::Emphasis) => {
                push(&mut current, &mut note, Inline::End(Style::Italic))
            }
            Event::Start(Tag::Strong) => push(&mut current, &mut note, Inline::Start(Style::Bold)),
            Event::End(TagEnd::Strong) => push(&mut current, &mut note, Inline::End(Style::Bold)),
            Event::Text(t) | Event::Code(t) => {
                if current.is_none() && note.is_none() {
                    open(
                        &mut current,
                        &mut note,
                        &mut out.blocks,
                        BlockKind::Para(ParaStyle::Prose),
                    );
                }
                push(&mut current, &mut note, Inline::Text(t.to_string()));
            }
            Event::SoftBreak => push(&mut current, &mut note, Inline::SoftBreak),
            Event::HardBreak => push(&mut current, &mut note, Inline::HardBreak),
            Event::FootnoteReference(label) => {
                push(&mut current, &mut note, Inline::Note(label.to_string()));
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                let labels = page_labels(&html);
                if labels.is_empty() {
                    if !html.trim_start().starts_with("<!--") && !html.trim().is_empty() {
                        drop("html", dropped);
                    }
                } else if current.is_some() || note.is_some() {
                    for label in labels {
                        push(&mut current, &mut note, Inline::Page(label));
                    }
                } else {
                    for label in labels {
                        out.blocks.push(Block {
                            kind: BlockKind::Page(label),
                            inlines: Vec::new(),
                        });
                    }
                }
            }
            Event::Rule => {}
            Event::TaskListMarker(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {
                drop("other", dropped);
            }
            _ => {}
        }
    }
    if let Some(done) = current.take() {
        out.blocks.push(done);
    }
    out
}

/// The plain text of some inline content (for comparing a heading with the title).
fn plain(inlines: &[Inline]) -> String {
    let mut s = String::new();
    for i in inlines {
        match i {
            Inline::Text(t) => s.push_str(t),
            Inline::SoftBreak | Inline::HardBreak => s.push(' '),
            _ => {}
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn same_title(a: &str, b: &str) -> bool {
    let norm = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    !a.is_empty() && norm(a) == norm(b)
}

/// Drop what opens a file as its title page: a level-1 heading naming the book (or, with no
/// title known, the first level-1 heading) among its first three blocks, and a `*by …*` line
/// right after it. Returns the heading's text when one was dropped.
fn drop_title(blocks: &mut Vec<Block>, title: &str) -> Option<String> {
    let at = blocks.iter().take(3).position(|b| {
        matches!(b.kind, BlockKind::Heading(1))
            && (title.is_empty() || same_title(&plain(&b.inlines), title))
    })?;
    let heading = plain(&blocks[at].inlines);
    blocks.remove(at);
    if let Some(next) = blocks.get(at)
        && matches!(next.kind, BlockKind::Para(_))
        && plain(&next.inlines).to_lowercase().starts_with("by ")
        && matches!(next.inlines.first(), Some(Inline::Start(Style::Italic)))
    {
        blocks.remove(at);
    }
    Some(heading)
}

/// The heading level that splits the book into Divisions: the shallowest level used more
/// than once. `None` when no level is (the book is one unit per file).
fn division_level<'a>(blocks: impl Iterator<Item = &'a Block>) -> Option<u8> {
    let mut counts = [0usize; 7];
    for b in blocks {
        if let BlockKind::Heading(level) = b.kind {
            counts[level as usize] += 1;
        }
    }
    (1..=6u8).find(|&l| counts[l as usize] > 1)
}

/// Writes the units of a Book: Divisions, and note units for its footnotes.
struct Writer {
    builder: ResourceBuilder,
    report: ImportReport,
    /// The Division being written: its title and its text.
    unit: Option<(String, TextBuf)>,
    /// Page markers read but not yet written: they open the next block, so a page that
    /// starts with a Division's heading opens that Division.
    pages: Vec<String>,
    divisions: u32,
    notes: u32,
}

impl Writer {
    fn start_unit(&mut self, title: String) {
        self.finish_unit();
        self.unit = Some((title, TextBuf::new()));
    }

    fn buf(&mut self) -> &mut TextBuf {
        if self.unit.is_none() {
            self.unit = Some(("Front matter".to_string(), TextBuf::new()));
        }
        // Just set when missing.
        #[expect(clippy::unwrap_used, reason = "set just above")]
        &mut self.unit.as_mut().unwrap().1
    }

    fn finish_unit(&mut self) {
        let Some((title, buf)) = self.unit.take() else {
            return;
        };
        if buf.text.trim().is_empty() {
            return;
        }
        self.divisions += 1;
        let id = UnitId::new(format!("div.{}", self.divisions))
            .unwrap_or_else(|_| UnitId::note(self.divisions));
        add_references(&buf.text, &mut self.report, &mut self.builder, &id);
        self.builder
            .add_unit(id.clone(), UnitKind::Division, Some(title), buf.text);
        for (range, kind) in buf.spans {
            self.builder.add_span(&id, range, kind);
        }
    }

    /// Start a block in the current Division, opening it with the page markers read before
    /// it; returns where its text starts.
    fn begin_block(&mut self) -> usize {
        let pages = std::mem::take(&mut self.pages);
        let buf = self.buf();
        let start = buf.start_block();
        for label in pages {
            buf.add_span(start..start, SpanKind::Page { label });
        }
        start
    }

    /// Write the page markers no block followed at the end of the current Division.
    fn end_pages(&mut self) {
        let pages = std::mem::take(&mut self.pages);
        if pages.is_empty() || self.unit.is_none() {
            return;
        }
        let buf = self.buf();
        let at = buf.pos();
        for label in pages {
            buf.add_span(at..at, SpanKind::Page { label });
        }
    }

    /// Add a note unit for a footnote and return its id.
    fn note(&mut self, paras: &[Vec<Inline>]) -> UnitId {
        self.notes += 1;
        let id = UnitId::note(self.notes);
        let mut buf = TextBuf::new();
        for para in paras {
            buf.start_block();
            write_inlines(&mut buf, para, &HashMap::new());
        }
        add_references(&buf.text, &mut self.report, &mut self.builder, &id);
        self.builder
            .add_unit(id.clone(), UnitKind::Note, None, buf.text);
        for (range, kind) in buf.spans {
            self.builder.add_span(&id, range, kind);
        }
        self.report.notes += 1;
        id
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

/// Write inline content into `buf`: text, styles, page markers and footnote references
/// (`notes`: the note unit of each footnote label; a label without one is left out). Line
/// breaks are spaces.
fn write_inlines(buf: &mut TextBuf, inlines: &[Inline], notes: &HashMap<String, UnitId>) {
    let mut open: Vec<(Style, usize)> = Vec::new();
    for inline in inlines {
        match inline {
            Inline::Text(t) => buf.push_text(t),
            Inline::Start(style) => {
                buf.flush_space();
                open.push((*style, buf.pos()));
            }
            Inline::End(style) => {
                if let Some(at) = open.iter().rposition(|(s, _)| s == style) {
                    let (_, start) = open.remove(at);
                    let end = buf.tracked_end.max(start).min(buf.pos());
                    if start < end {
                        let kind = match style {
                            Style::Italic => SpanKind::Italic,
                            Style::Bold => SpanKind::Bold,
                        };
                        buf.add_span(start..end, kind);
                    }
                }
            }
            Inline::Note(label) => {
                if let Some(id) = notes.get(label).cloned() {
                    let at = buf.pos();
                    buf.add_span(
                        at..at,
                        SpanKind::NoteRef {
                            note: id,
                            note_kind: NoteKind::Footnote,
                        },
                    );
                }
            }
            Inline::Page(label) => {
                buf.flush_space();
                let at = buf.pos();
                buf.add_span(
                    at..at,
                    SpanKind::Page {
                        label: label.clone(),
                    },
                );
            }
            Inline::SoftBreak | Inline::HardBreak => buf.push_text(" "),
        }
    }
}

/// Whether a paragraph is verse: its lines are broken on purpose (CommonMark hard breaks, a
/// `\\` or two spaces at a line's end). A plain line break is a space, as CommonMark reads it,
/// so prose wrapped in its file (or OCR text keeping the scan's lines) stays one paragraph.
fn is_verse(inlines: &[Inline]) -> bool {
    inlines.iter().any(|i| matches!(i, Inline::HardBreak))
}

/// Import one Markdown book: its main file (front matter, and its text when it has no Parts)
/// and its Part files in order. `progress(done, total)` returns false to cancel.
pub fn import_markdown_book(
    main: Option<&BookFile>,
    parts: &[BookFile],
    options: &MarkdownBookOptions,
    mut progress: impl FnMut(usize, usize) -> bool,
) -> Result<(Resource, ImportReport), ImportError> {
    let resource_id = options.resource_id.clone();
    if !is_valid_resource_id(&resource_id) {
        return Err(ImportError::InvalidResourceId(resource_id));
    }
    let Some(first) = main.or(parts.first()) else {
        return Err(ImportError::NoText);
    };
    for f in main.iter().copied().chain(parts) {
        if f.text.len() > MAX_BOOK_FILE_BYTES {
            return Err(ImportError::TooLarge(f.name.clone()));
        }
    }
    let (meta, _) = split_front_matter(&first.text);
    let text_files: Vec<&BookFile> = if parts.is_empty() {
        main.into_iter().collect()
    } else {
        parts.iter().collect()
    };

    let mut dropped = BTreeMap::new();
    let mut title = meta.get("title").cloned().unwrap_or_default();
    let mut files: Vec<Parsed> = Vec::new();
    for (i, file) in text_files.iter().enumerate() {
        if !progress(i, text_files.len() * 2) {
            return Err(ImportError::Cancelled);
        }
        let (_, body) = split_front_matter(&file.text);
        let mut parsed = parse_body(body, &mut dropped);
        if let Some(heading) = drop_title(&mut parsed.blocks, &title)
            && title.is_empty()
        {
            title = heading;
        }
        files.push(parsed);
    }
    if title.is_empty() {
        title = first
            .name
            .rsplit('/')
            .next()
            .unwrap_or(&first.name)
            .trim_end_matches(".md")
            .to_string();
    }
    let title: String = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let author = meta.get("author").cloned();
    let year = meta
        .get("date")
        .or_else(|| meta.get("year"))
        .and_then(|d| year_of(d));
    let license_value = meta.get("license").or_else(|| meta.get("licence")).cloned();
    let spdx = license_value.as_deref().and_then(book_license_spdx);
    let source = meta.get("source").cloned();
    let language = meta
        .get("language")
        .or_else(|| meta.get("lang"))
        .filter(|l| {
            !l.is_empty()
                && l.len() <= 35
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        .cloned()
        .unwrap_or_else(|| "en".to_string());

    let mut manifest = Manifest::new(
        &resource_id,
        &title,
        abbreviation(&title),
        ResourceType::Book,
        language,
    );
    manifest.license = spdx.unwrap_or(LICENSE_UNKNOWN).to_string();
    manifest.edition = year.map(|y| y.to_string());
    manifest.publisher = author.clone();
    manifest.source_url = source
        .clone()
        .filter(|s| s.starts_with("https://") || s.starts_with("http://"));
    manifest.attribution = attribution(
        &title,
        author.as_deref(),
        year,
        source.as_deref(),
        license_value.as_deref().filter(|_| spdx.is_none()),
    );
    manifest.origin = Origin::Imported;
    manifest.source_format = MARKDOWN_BOOK_FORMAT.into();
    manifest.source_file = (!options.source_file.is_empty()).then(|| options.source_file.clone());
    manifest.import_tool_version = options.tool_version.clone();
    manifest.format_version = FORMAT_VERSION;

    let mut writer = Writer {
        builder: ResourceBuilder::new(manifest),
        report: ImportReport {
            resource_id: resource_id.clone(),
            title: title.clone(),
            source_format: MARKDOWN_BOOK_FORMAT.into(),
            source_files: main
                .iter()
                .copied()
                .chain(parts)
                .map(|f| f.name.clone())
                .collect(),
            license_missing: spdx.is_none(),
            ..ImportReport::default()
        },
        unit: None,
        pages: Vec::new(),
        divisions: 0,
        notes: 0,
    };
    if let Some(value) = &license_value
        && spdx.is_none()
    {
        writer.report.warnings.push(format!(
            "Licence \"{value}\" is not one the app recognises; the book is for personal use"
        ));
    }

    let level = division_level(files.iter().flat_map(|f| f.blocks.iter()));
    let total = files.len();
    for (i, file) in files.iter().enumerate() {
        if !progress(total + i, total * 2) {
            return Err(ImportError::Cancelled);
        }
        if level.is_none() {
            let label = if total > 1 {
                format!("Part {}", i + 1)
            } else {
                title.clone()
            };
            writer.start_unit(label);
        }
        // This file's footnote labels → note units, made as first referenced.
        let mut made: HashMap<String, UnitId> = HashMap::new();
        for block in &file.blocks {
            if let BlockKind::Heading(l) = block.kind
                && Some(l) == level
            {
                writer.start_unit(plain(&block.inlines));
            }
            for inline in &block.inlines {
                let Inline::Note(label) = inline else {
                    continue;
                };
                if made.contains_key(label) {
                    continue;
                }
                match file.notes.get(label) {
                    Some(paras) => {
                        let id = writer.note(paras);
                        made.insert(label.clone(), id);
                    }
                    None => {
                        *dropped
                            .entry("footnote without text".to_string())
                            .or_default() += 1;
                    }
                }
            }
            match &block.kind {
                BlockKind::Page(label) => writer.pages.push(label.clone()),
                BlockKind::Heading(l) => {
                    let start = writer.begin_block();
                    let buf = writer.buf();
                    write_inlines(buf, &block.inlines, &made);
                    let end = buf.pos();
                    if start < end {
                        buf.add_span(start..end, SpanKind::Heading { level: *l });
                    }
                }
                BlockKind::Para(style)
                    if *style == ParaStyle::Prose && is_verse(&block.inlines) =>
                {
                    for line in block.inlines.split(|i| matches!(i, Inline::HardBreak)) {
                        let start = writer.begin_block();
                        let buf = writer.buf();
                        write_inlines(buf, line, &made);
                        let end = buf.pos();
                        if start < end {
                            buf.add_span(
                                start..end,
                                SpanKind::Para {
                                    style: ParaStyle::Poetry1,
                                },
                            );
                        }
                    }
                }
                BlockKind::Para(style) => {
                    let start = writer.begin_block();
                    let buf = writer.buf();
                    write_inlines(buf, &block.inlines, &made);
                    let end = buf.pos();
                    if start < end {
                        buf.add_span(start..end, SpanKind::Para { style: *style });
                    }
                }
            }
        }
    }
    writer.end_pages();
    writer.finish_unit();
    if writer.divisions == 0 {
        return Err(ImportError::NoText);
    }
    writer.report.entries = writer.divisions as usize;
    writer.report.dropped_markup = dropped;
    let Writer {
        builder, report, ..
    } = writer;
    let resource = builder.build().map_err(ImportError::Invalid)?;
    Ok((resource, report))
}

/// A year (100 to 2100) in a date value such as `1650`, `1650-03-01` or `c. 1650`.
fn year_of(value: &str) -> Option<u16> {
    let digits: Vec<Range<usize>> = {
        let mut out = Vec::new();
        let mut start = None;
        for (i, c) in value.char_indices().chain([(value.len(), ' ')]) {
            match (c.is_ascii_digit(), start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    out.push(s..i);
                    start = None;
                }
                _ => {}
            }
        }
        out
    };
    digits.into_iter().find_map(|r| {
        let n = &value[r];
        (3..=4)
            .contains(&n.len())
            .then(|| n.parse::<u16>().ok())?
            .filter(|y| (100..=2100).contains(y))
    })
}

/// A short abbreviation for a Book: the initials of its title's main words, at most 8.
fn abbreviation(title: &str) -> String {
    const MINOR: &[&str] = &[
        "a", "an", "and", "the", "of", "on", "in", "to", "or", "for", "by",
    ];
    let initials: String = title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !MINOR.contains(&w.to_lowercase().as_str()))
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .take(8)
        .collect();
    if initials.is_empty() {
        "BOOK".to_string()
    } else {
        initials
    }
}

/// The credit shown in About: who wrote it, when, where the copy came from, and a licence the
/// app does not recognise, as the file gives it.
fn attribution(
    title: &str,
    author: Option<&str>,
    year: Option<u16>,
    source: Option<&str>,
    unrecognised_license: Option<&str>,
) -> String {
    let mut s = String::new();
    if let Some(a) = author {
        s.push_str(a);
        s.push_str(", ");
    }
    s.push_str(title);
    if let Some(y) = year {
        s.push_str(&format!(" ({y})"));
    }
    s.push('.');
    if let Some(src) = source {
        s.push_str(&format!(" Source: {src}."));
    }
    if let Some(l) = unrecognised_license {
        s.push_str(&format!(" Licence as given: {l}."));
    }
    s
}

#[cfg(test)]
#[path = "markdown_book_tests.rs"]
mod tests;
