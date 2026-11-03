//! USFM Import: USFM files → [`ResourceBuilder`] → [`Resource`] plus an [`ImportReport`].
//!
//! Parsing is done by `usfm3` (error-tolerant, USFM 3.1); [`crate::usfm_tree`] turns its
//! syntax tree into a USJ-shaped tree that we walk. Markers map
//! to the span vocabulary as follows; anything else is dropped and counted in the report.
//!
//! | USFM | span |
//! |---|---|
//! | `\c` | `chapter` over the chapter's text |
//! | `\v n`, `\v n-m`, `\v na` | Bible Verse (own numbering); a range is one Verse with `last`; lettered verses belong to their number |
//! | `\p \m \pi \mi \nb \pc \pm…` | `para` prose; `\q1-3` poetry 1-3; `\li` list; `\b` blank |
//! | `\s \s1-4 \ms \sp` | `heading` |
//! | `\d` | `psalm_title`; in Psalms also Verse 0 (the title slot) |
//! | `\w …|strong=…` | `tagged_word` (also `lemma`, `x-morph`) |
//! | `\wj \add \nd \it \bd \em \tl \qs \wh \wg` | `woj`, `added`, `divine_name`, `italic`, `bold`, `emphasis`, `foreign`, `selah`, `foreign` |
//! | `\f \fe` / `\x` | note unit + zero-width `note_ref` (`\x` also adds `xref`) |
//!
//! `\id \h \toc \mt` are metadata and not counted as dropped. Introductions (`\ip \is \io…`),
//! figures and other markers are dropped. Books outside the 66-book canon (and `FRT`, `GLO`…)
//! are imported as text units without verses and listed in the report.

use std::collections::{BTreeMap, HashSet};
use std::fmt;

use obs_model::{
    Direction, FORMAT_VERSION, KeyScheme, LICENSE_UNKNOWN, Manifest, MorphemePart, NoteKind,
    Origin, ParaStyle, Resource, ResourceBuilder, ResourceType, SpanKind, UnitId, UnitKind,
    ValidationErrors, is_valid_resource_id,
};
use obs_scripture::{Book, PassageList, Standard, StrongsNumber, Verse, parse_reference};
use serde_json::Value;

use crate::report::{BookReport, ImportReport, VerseMapCounts};
use crate::text_buf::TextBuf;
use crate::verse_map::{VerseFacts, build_verse_map};

/// Identity and metadata for the Resource an Import creates. Empty fields get defaults.
#[derive(Debug, Clone, Default)]
pub struct UsfmOptions {
    pub resource_id: String,
    pub title: String,
    pub abbreviation: String,
    /// BCP 47; defaults to `und`.
    pub language: String,
    pub direction: Direction,
    /// File or directory name shown in About and the report.
    pub source_file: String,
    /// Version of the tool doing the Import, recorded in the manifest.
    pub tool_version: String,
}

/// Why an Import failed outright.
#[derive(Debug)]
pub enum ImportError {
    NoBooks,
    /// A file without a `\id` line.
    MissingId(String),
    DuplicateBook {
        code: String,
        first: String,
        second: String,
    },
    InvalidResourceId(String),
    Usj(String),
    Invalid(ValidationErrors),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImportError::NoBooks => f.write_str("no Bible books found"),
            ImportError::MissingId(name) => write!(f, "{name}: no \\id line"),
            ImportError::DuplicateBook {
                code,
                first,
                second,
            } => write!(f, "book {code} appears in both {first} and {second}"),
            ImportError::InvalidResourceId(id) => write!(
                f,
                "resource id `{id}` must be 1-64 chars of a-z, 0-9 and `-`"
            ),
            ImportError::Usj(e) => write!(f, "could not read parser output: {e}"),
            ImportError::Invalid(e) => write!(f, "imported resource is invalid: {e}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Collects USFM books, then imports them all at once in canonical order.
#[derive(Debug, Default)]
pub struct UsfmImport {
    options: UsfmOptions,
    books: Vec<(String, String, String)>,
}

/// Turn a name into a resource id: lowercase, runs of other characters become `-`.
pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out.truncate(64);
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        out.push_str("imported");
    }
    out
}

/// The `\id` code of a USFM source, uppercased.
pub fn usfm_book_code(source: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let rest = line.trim_start().strip_prefix("\\id")?;
        if !rest.starts_with(char::is_whitespace) {
            return None;
        }
        rest.split_whitespace().next().map(str::to_ascii_uppercase)
    })
}

fn sort_key(code: &str) -> (usize, String) {
    match Book::from_usfm(code) {
        Some(b) => (b.index(), String::new()),
        None => (usize::MAX, code.to_string()),
    }
}

impl UsfmImport {
    pub fn new(options: UsfmOptions) -> Self {
        Self {
            options,
            books: Vec::new(),
        }
    }

    /// Add one book's USFM source; `name` is its file name, for messages.
    pub fn add_book(&mut self, name: &str, source: String) -> Result<(), ImportError> {
        let code = usfm_book_code(&source).ok_or_else(|| ImportError::MissingId(name.into()))?;
        if let Some((_, first, _)) = self.books.iter().find(|(c, _, _)| *c == code) {
            return Err(ImportError::DuplicateBook {
                code,
                first: first.clone(),
                second: name.into(),
            });
        }
        self.books.push((code, name.to_string(), source));
        Ok(())
    }

    /// Parse every book and build the Resource.
    pub fn finish(mut self) -> Result<(Resource, ImportReport), ImportError> {
        if self.books.is_empty() {
            return Err(ImportError::NoBooks);
        }
        self.books.sort_by_key(|(code, _, _)| sort_key(code));
        let o = &self.options;
        let resource_id = if o.resource_id.is_empty() {
            slugify(&o.source_file)
        } else {
            o.resource_id.clone()
        };
        if !is_valid_resource_id(&resource_id) {
            return Err(ImportError::InvalidResourceId(resource_id));
        }
        let title = if o.title.is_empty() {
            resource_id.clone()
        } else {
            o.title.clone()
        };
        let abbreviation = if o.abbreviation.is_empty() {
            resource_id.to_ascii_uppercase().chars().take(8).collect()
        } else {
            o.abbreviation.clone()
        };
        let language = if o.language.is_empty() {
            "und".to_string()
        } else {
            o.language.clone()
        };
        let mut manifest = Manifest::new(
            &resource_id,
            &title,
            abbreviation,
            ResourceType::Bible,
            language,
        );
        manifest.direction = o.direction;
        manifest.key_schemes = vec![KeyScheme::Passage];
        manifest.license = LICENSE_UNKNOWN.to_string();
        manifest.origin = Origin::Imported;
        manifest.source_format = "usfm".into();
        manifest.source_file = (!o.source_file.is_empty()).then(|| o.source_file.clone());
        manifest.import_tool_version = o.tool_version.clone();
        manifest.format_version = FORMAT_VERSION;

        let mut builder = ResourceBuilder::new(manifest);
        let mut ctx = Ctx::default();
        let mut report = ImportReport {
            resource_id,
            title,
            source_format: "usfm".into(),
            source_files: self.books.iter().map(|(_, n, _)| n.clone()).collect(),
            license_missing: true,
            ..ImportReport::default()
        };
        let mut facts: Vec<VerseFacts> = Vec::new();
        let mut other_verses = 0usize;
        for (code, name, source) in &self.books {
            let usj = crate::usfm_tree::parse(source)
                .map_err(|e| ImportError::Usj(format!("{name}: {e}")))?;
            let book = Book::from_usfm(code);
            let mut em = BookEmitter::new(code, book);
            em.walk_top(&usj, &mut ctx);
            let out = em.finish(&mut ctx);
            let kind = if book.is_some() {
                UnitKind::BibleBook
            } else {
                UnitKind::Division
            };
            report.books.push(BookReport {
                code: code.clone(),
                book,
                chapters: out.chapters,
                verses: out.verses.len().max(out.other_verses),
            });
            report.verses += out.verses.len();
            other_verses += out.other_verses;
            for (range, verse, last) in &out.verses {
                let words = out
                    .text
                    .get(range.clone())
                    .map_or(0, |t| obs_model::tokenize(t).len());
                facts.push(VerseFacts {
                    verse: *verse,
                    last: *last,
                    words: u32::try_from(words).unwrap_or(u32::MAX),
                });
            }
            let unit = out.unit.clone();
            builder.add_unit(out.unit, kind, out.title, out.text);
            for (range, kind) in out.spans {
                if matches!(kind, SpanKind::TaggedWord { .. }) {
                    report.tagged_words += 1;
                }
                builder.add_span(&unit, range, kind);
            }
            for (range, verse, last) in out.verses {
                builder.add_verse(&unit, range, verse, last);
            }
        }
        report.notes = ctx.notes.len();
        for note in ctx.notes {
            builder.add_unit(note.id.clone(), UnitKind::Note, None, note.buf.text);
            for (range, kind) in note.buf.spans {
                builder.add_span(&note.id, range, kind);
            }
        }
        for (marker, count) in &ctx.dropped {
            for _ in 0..*count {
                builder.count_dropped(marker);
            }
        }
        if !facts.is_empty() {
            let outcome = build_verse_map(&facts);
            report.verse_map = Some(VerseMapCounts {
                // Verses of books outside the canon are Unmapped too.
                unmapped: outcome.map.unmapped().count() + other_verses,
                absent: outcome.map.absent().len(),
            });
            report.traditions = outcome.traditions;
            builder.set_verse_map(outcome.map);
        }
        let resource = builder.build().map_err(ImportError::Invalid)?;
        report.dropped_markup = resource.dropped_markup.clone();
        report.warnings = ctx.warnings;
        Ok((resource, report))
    }
}

struct Note {
    id: UnitId,
    buf: TextBuf,
}

/// State shared across books: note bodies, dropped-markup counts, warnings.
#[derive(Default)]
struct Ctx {
    notes: Vec<Note>,
    dropped: BTreeMap<String, u64>,
    warnings: Vec<String>,
    /// The Verse being read, for relative references in notes.
    current_verse: Option<Verse>,
}

impl Ctx {
    fn drop_marker(&mut self, marker: &str) {
        *self.dropped.entry(format!("\\{marker}")).or_insert(0) += 1;
    }

    fn warn(&mut self, message: String) {
        self.warnings.push(message);
    }
}

fn marker(node: &Value) -> &str {
    node.get("marker").and_then(Value::as_str).unwrap_or("")
}

fn content(node: &Value) -> &[Value] {
    node.get("content")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn attr<'a>(node: &'a Value, name: &str) -> Option<&'a str> {
    node.get(name).and_then(Value::as_str)
}

/// Markers that only carry metadata; consumed, not counted as dropped.
fn is_metadata(m: &str) -> bool {
    matches!(
        m,
        "id" | "ide" | "h" | "h1" | "h2" | "h3" | "rem" | "usfm" | "sts"
    ) || m.starts_with("toc")
        || m.starts_with("mt")
}

fn para_style(m: &str) -> Option<ParaStyle> {
    Some(match m {
        "p" | "m" | "po" | "pr" | "cls" | "pmo" | "pm" | "pmc" | "pmr" | "pi" | "pi1" | "pi2"
        | "pi3" | "pi4" | "mi" | "nb" | "pc" | "ph" | "ph1" | "ph2" | "ph3" | "lit" => {
            ParaStyle::Prose
        }
        "q" | "q1" | "qr" | "qc" | "qa" | "qm" | "qm1" | "qd" => ParaStyle::Poetry1,
        "q2" | "qm2" => ParaStyle::Poetry2,
        "q3" | "q4" | "qm3" => ParaStyle::Poetry3,
        "li" | "li1" | "li2" | "li3" | "li4" | "lim" | "lim1" | "lim2" | "lh" | "lf" => {
            ParaStyle::List
        }
        "b" => ParaStyle::Blank,
        _ => return None,
    })
}

fn heading_level(m: &str) -> Option<u8> {
    Some(match m {
        "s" | "s1" | "ms" | "ms1" | "mte" | "mte1" => 1,
        "s2" | "ms2" | "mte2" => 2,
        "s3" | "ms3" | "sp" => 3,
        "s4" => 4,
        _ => return None,
    })
}

/// Character markers that map to a span with no attributes.
fn char_span(m: &str) -> Option<SpanKind> {
    Some(match m {
        "wj" => SpanKind::Woj,
        "add" => SpanKind::Added,
        "nd" => SpanKind::DivineName,
        "it" => SpanKind::Italic,
        "bd" | "bdit" => SpanKind::Bold,
        "em" => SpanKind::Emphasis,
        "tl" | "wh" | "wg" | "wa" => SpanKind::Foreign,
        "qs" => SpanKind::Selah,
        _ => return None,
    })
}

/// Parse `12`, `12-14`, `12a`, `12b-13` into (first, last, lettered).
fn parse_verse_number(s: &str) -> Option<(u16, u16, bool)> {
    let (first, last) = match s.split_once('-') {
        Some((a, b)) => (a.trim(), Some(b.trim())),
        None => (s.trim(), None),
    };
    let digits = |t: &str| {
        let end = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
        let n: u16 = t[..end].parse().ok()?;
        Some((n, end < t.len()))
    };
    let (a, lettered) = digits(first)?;
    let b = match last {
        Some(l) => digits(l)?.0.max(a),
        None => a,
    };
    Some((a, b, lettered && last.is_none()))
}

/// Strong's numbers from a `strong` attribute: `G1722`, `H1254,H0853`, `H1254 H0853`.
fn parse_strongs(attr: &str, ctx: &mut Ctx, at: &str) -> Vec<StrongsNumber> {
    let mut out = Vec::new();
    for part in attr.split([',', ' ', ';']).filter(|p| !p.is_empty()) {
        // Some sources prefix extra info (`H9002=a`), keep the number only.
        let token = part.split(['=', ':']).next().unwrap_or(part);
        match token.parse() {
            Ok(n) => out.push(n),
            Err(_) => ctx.warn(format!("{at}: bad Strong's number `{part}`")),
        }
    }
    out
}

struct OpenVerse {
    verse: Verse,
    last: u16,
    start: usize,
}

struct BookOutput {
    unit: UnitId,
    title: Option<String>,
    text: String,
    spans: Vec<(std::ops::Range<usize>, SpanKind)>,
    verses: Vec<(std::ops::Range<usize>, Verse, u16)>,
    chapters: u16,
    other_verses: usize,
}

struct BookEmitter {
    code: String,
    book: Option<Book>,
    buf: TextBuf,
    title: Option<String>,
    chapter: Option<u16>,
    chapter_start: Option<usize>,
    chapters: u16,
    verse: Option<OpenVerse>,
    verses: Vec<(std::ops::Range<usize>, Verse, u16)>,
    seen: HashSet<Verse>,
    other_verses: usize,
}

impl BookEmitter {
    fn new(code: &str, book: Option<Book>) -> Self {
        Self {
            code: code.to_string(),
            book,
            buf: TextBuf::new(),
            title: None,
            chapter: None,
            chapter_start: None,
            chapters: 0,
            verse: None,
            verses: Vec::new(),
            seen: HashSet::new(),
            other_verses: 0,
        }
    }

    fn at(&self) -> String {
        match (&self.chapter, &self.verse) {
            (Some(c), Some(v)) => format!("{} {c}:{}", self.code, v.verse.verse),
            (Some(c), None) => format!("{} {c}", self.code),
            _ => self.code.clone(),
        }
    }

    fn walk_top(&mut self, usj: &Value, ctx: &mut Ctx) {
        for node in content(usj) {
            match node.get("type").and_then(Value::as_str) {
                Some("book") => {}
                Some("chapter") => self.start_chapter(node, ctx),
                Some("para") => self.para(node, ctx),
                Some("verse") => self.start_verse(node, ctx),
                Some("table") | Some("sidebar") | Some("periph") => {
                    // Rare in Bibles; keep their text as prose.
                    let start = self.buf.start_block();
                    self.inline_all(content(node), ctx);
                    let end = self.buf.pos();
                    self.buf.add_span(
                        start..end,
                        SpanKind::Para {
                            style: ParaStyle::Prose,
                        },
                    );
                    ctx.drop_marker(node.get("type").and_then(Value::as_str).unwrap_or("?"));
                }
                Some("note") => self.note(node, ctx),
                Some("char") => self.inline(node, ctx),
                _ => {
                    if let Some(s) = node.as_str() {
                        self.buf.push_text(s);
                    }
                }
            }
        }
    }

    fn close_verse(&mut self) {
        if let Some(v) = self.verse.take() {
            let end = self.buf.tracked_end.max(v.start);
            self.verses.push((v.start..end, v.verse, v.last));
        }
    }

    fn close_chapter(&mut self) {
        self.close_verse();
        if let (Some(c), Some(start)) = (self.chapter, self.chapter_start.take()) {
            let end = self.buf.pos().max(start);
            self.buf
                .add_span(start..end, SpanKind::Chapter { chapter: c });
        }
    }

    fn start_chapter(&mut self, node: &Value, ctx: &mut Ctx) {
        self.close_chapter();
        let number = attr(node, "number").and_then(|n| n.trim().parse::<u16>().ok());
        match number {
            Some(n) if n > 0 => {
                self.chapter = Some(n);
                self.chapters += 1;
            }
            _ => {
                ctx.warn(format!(
                    "{}: bad chapter number {:?}",
                    self.code,
                    attr(node, "number")
                ));
                self.chapter = None;
            }
        }
    }

    /// Mark where the current chapter's text starts, if it has not started yet.
    fn touch_chapter(&mut self, at: usize) {
        if self.chapter.is_some() && self.chapter_start.is_none() {
            self.chapter_start = Some(at);
        }
    }

    fn open_verse(&mut self, verse: Verse, last: u16, ctx: &mut Ctx) {
        if !self.seen.insert(verse) {
            ctx.warn(format!(
                "{} {}:{}: verse appears twice; its second text is kept with the verse before",
                self.code, verse.chapter, verse.verse
            ));
            return;
        }
        self.close_verse();
        self.buf.flush_space();
        let start = self.buf.pos();
        self.touch_chapter(start);
        self.buf.tracked_end = start;
        self.buf.suppress_space();
        self.verse = Some(OpenVerse { verse, last, start });
        ctx.current_verse = Some(verse);
    }

    fn start_verse(&mut self, node: &Value, ctx: &mut Ctx) {
        let raw = attr(node, "number").unwrap_or("");
        let Some((first, last, lettered)) = parse_verse_number(raw) else {
            ctx.warn(format!("{}: bad verse number `{raw}`", self.at()));
            return;
        };
        if lettered && self.verse.as_ref().is_some_and(|v| v.verse.verse == first) {
            return; // `\v 5b` continues verse 5
        }
        let (Some(book), Some(chapter)) = (self.book, self.chapter) else {
            // Books outside the canon are kept as text; their verses count as Unmapped.
            if self.book.is_none() {
                self.other_verses += usize::from(last - first + 1);
            }
            return;
        };
        if first == 0 {
            ctx.warn(format!("{}: verse number 0", self.at()));
            return;
        }
        self.open_verse(Verse::new(book, chapter, first), last, ctx);
    }

    fn para(&mut self, node: &Value, ctx: &mut Ctx) {
        let m = marker(node);
        if m == "h" {
            let mut b = TextBuf::new();
            for c in content(node) {
                if let Some(s) = c.as_str() {
                    b.push_text(s);
                }
            }
            self.title = Some(b.text).filter(|t| !t.is_empty());
            return;
        }
        if is_metadata(m) {
            return;
        }
        if m == "d" {
            self.psalm_title(node, ctx);
            return;
        }
        if let Some(level) = heading_level(m) {
            let start = self.buf.start_block();
            self.buf.track = false;
            self.inline_all(content(node), ctx);
            self.buf.track = true;
            let end = self.buf.pos();
            if end > start {
                self.buf.add_span(start..end, SpanKind::Heading { level });
            }
            return;
        }
        let Some(style) = para_style(m) else {
            ctx.drop_marker(m);
            return;
        };
        let start = self.buf.start_block();
        self.touch_chapter(start);
        self.inline_all(content(node), ctx);
        let end = self.buf.pos().max(start);
        self.buf.add_span(start..end, SpanKind::Para { style });
    }

    fn psalm_title(&mut self, node: &Value, ctx: &mut Ctx) {
        let start = self.buf.start_block();
        self.touch_chapter(start);
        let title_slot = self.book == Some(Book::Psalms)
            && self.chapter.is_some()
            && self
                .verse
                .as_ref()
                .is_none_or(|v| v.verse.chapter != self.chapter.unwrap_or(0));
        if title_slot && let (Some(book), Some(c)) = (self.book, self.chapter) {
            self.open_verse(Verse::new(book, c, 0), 0, ctx);
        }
        // A `\d` inside a chapter (e.g. acrostic letters in Ps 119) is a heading, not verse text.
        self.buf.track = title_slot;
        self.inline_all(content(node), ctx);
        self.buf.track = true;
        let end = self.buf.pos().max(start);
        self.buf.add_span(start..end, SpanKind::PsalmTitle);
    }

    fn inline_all(&mut self, nodes: &[Value], ctx: &mut Ctx) {
        for n in nodes {
            self.inline(n, ctx);
        }
    }

    fn inline(&mut self, node: &Value, ctx: &mut Ctx) {
        if let Some(s) = node.as_str() {
            self.buf.push_text(s);
            return;
        }
        match node.get("type").and_then(Value::as_str) {
            Some("verse") => self.start_verse(node, ctx),
            Some("note") => self.note(node, ctx),
            Some("char") => self.body_char(node, ctx),
            Some("chapter") => self.start_chapter(node, ctx),
            Some("figure") | Some("optbreak") | Some("ref") | Some("milestone") | Some("ms") => {
                ctx.drop_marker(marker(node).trim_end_matches(['-', 's', 'e']).trim());
                // `\ref` keeps its text.
                if node.get("type").and_then(Value::as_str) == Some("ref") {
                    for c in content(node) {
                        if let Some(s) = c.as_str() {
                            self.buf.push_text(s);
                        }
                    }
                }
            }
            _ => ctx.drop_marker(marker(node)),
        }
    }

    /// A character-level node in the body text. Unlike [`emit_char`], verses and notes inside
    /// it are kept: some sources (BSB) open verses inside `\wj … \wj*`.
    fn body_char(&mut self, node: &Value, ctx: &mut Ctx) {
        let at = self.at();
        let kind = char_kind(node, ctx, &at, false);
        self.buf.flush_space();
        let start = self.buf.pos();
        for c in content(node) {
            // Verses, notes, nested markup and `\ref` text are handled as in a paragraph.
            self.inline(c, ctx);
        }
        let end = self.buf.pos();
        if let Some(kind) = kind
            && end > start
        {
            self.buf.add_span(start..end, kind);
        }
    }

    fn note(&mut self, node: &Value, ctx: &mut Ctx) {
        let m = marker(node);
        let kind = match m {
            "f" | "fe" | "ef" => NoteKind::Footnote,
            "x" | "ex" => NoteKind::CrossReference,
            _ => {
                ctx.drop_marker(m);
                return;
            }
        };
        let n = u32::try_from(ctx.notes.len() + 1).unwrap_or(u32::MAX);
        let id = UnitId::note(n);
        let mut body = TextBuf::new();
        let mut targets = PassageList::new();
        let at = self.at();
        for c in content(node) {
            if let Some(s) = c.as_str() {
                body.push_text(s);
                continue;
            }
            let cm = marker(c);
            match cm {
                // The note's own reference label and caller text repeat what the app shows.
                "fr" | "xo" => {}
                "xt" => {
                    let start = {
                        body.flush_space();
                        body.pos()
                    };
                    emit_char(&mut body, c, ctx, &at, true);
                    let end = body.pos();
                    let text = body.text[start..end].to_string();
                    if let Ok(list) = parse_reference(&text, &Standard, ctx.current_verse) {
                        body.add_span(
                            start..end,
                            SpanKind::ScriptureRef {
                                targets: list.clone(),
                            },
                        );
                        for p in list.iter() {
                            targets.push(*p);
                        }
                    }
                }
                _ => emit_char(&mut body, c, ctx, &at, true),
            }
        }
        // Right after the text the note annotates (a pending space is not emitted yet).
        let pos = self.buf.pos();
        self.buf.add_span(
            pos..pos,
            SpanKind::NoteRef {
                note: id.clone(),
                note_kind: kind,
            },
        );
        if kind == NoteKind::CrossReference && !targets.is_empty() {
            self.buf.add_span(pos..pos, SpanKind::Xref { targets });
        }
        ctx.notes.push(Note { id, buf: body });
    }

    fn finish(mut self, _ctx: &mut Ctx) -> BookOutput {
        self.close_chapter();
        let unit = match self.book {
            Some(b) => UnitId::book(b),
            None => UnitId::new(self.code.clone()).unwrap_or_else(|_| UnitId::book(Book::Genesis)),
        };
        BookOutput {
            unit,
            title: self.title,
            text: self.buf.text,
            spans: self.buf.spans,
            verses: self.verses,
            chapters: self.chapters,
            other_verses: self.other_verses,
        }
    }
}

/// Emit a character-level node (and its children) into `buf`. In notes (`in_note`), note
/// parts like `\ft` are plain text and `\fq`/`\fqa` italic.
fn emit_char(buf: &mut TextBuf, node: &Value, ctx: &mut Ctx, at: &str, in_note: bool) {
    let kind = char_kind(node, ctx, at, in_note);
    buf.flush_space();
    let start = buf.pos();
    for c in content(node) {
        if let Some(s) = c.as_str() {
            buf.push_text(s);
        } else if c.get("type").and_then(Value::as_str) == Some("char") {
            emit_char(buf, c, ctx, at, in_note);
        } else {
            ctx.drop_marker(marker(c));
        }
    }
    let end = buf.pos();
    if let Some(kind) = kind
        && end > start
    {
        buf.add_span(start..end, kind);
    }
}

/// The span a character marker maps to; unknown markers are counted as dropped.
fn char_kind(node: &Value, ctx: &mut Ctx, at: &str, in_note: bool) -> Option<SpanKind> {
    let m = marker(node);
    match m {
        "w" => {
            let parts: Vec<MorphemePart> = attr(node, "strong")
                .map(|s| parse_strongs(s, ctx, at))
                .unwrap_or_default()
                .into_iter()
                .map(|strongs| MorphemePart {
                    strongs,
                    lemma: attr(node, "lemma").map(str::to_string),
                    morph: attr(node, "x-morph").map(str::to_string),
                    gloss: None,
                })
                .collect();
            (!parts.is_empty()).then_some(SpanKind::TaggedWord { parts })
        }
        "fq" | "fqa" if in_note => Some(SpanKind::Italic),
        "fk" | "fl" if in_note => Some(SpanKind::Bold),
        "ft" | "fv" | "fp" | "xt" | "xk" | "xq" | "fw" | "xta" if in_note => None,
        other => match char_span(other) {
            Some(k) => Some(k),
            None => {
                ctx.drop_marker(other);
                None
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verse_numbers() {
        assert_eq!(parse_verse_number("12"), Some((12, 12, false)));
        assert_eq!(parse_verse_number("15-16"), Some((15, 16, false)));
        assert_eq!(parse_verse_number("5a"), Some((5, 5, true)));
        assert_eq!(parse_verse_number("5b-6"), Some((5, 6, false)));
        assert_eq!(parse_verse_number("x"), None);
    }

    #[test]
    fn slugs_and_ids() {
        assert_eq!(slugify("eng-kjv2006_usfm"), "eng-kjv2006-usfm");
        assert_eq!(slugify("  My Bible!  "), "my-bible");
        assert_eq!(slugify("€€€"), "imported");
        assert_eq!(usfm_book_code("\\id jhn John\n\\c 1"), Some("JHN".into()));
        assert_eq!(usfm_book_code("\\ide UTF-8\n"), None);
    }
}
