//! Project Gutenberg's plain-text eBooks (`<n>-0.txt`, `<n>.txt`, #345), for books PG posts
//! with no HTML: one eBook → a Book (ADR 0023) plus an [`ImportReport`].
//!
//! Only the text between PG's "*** START OF THE PROJECT GUTENBERG EBOOK" and "*** END OF …"
//! lines (in a text of before 2002, between the end of its "SMALL PRINT" and its "End of
//! Project Gutenberg" line) is read, and any paragraph naming Project Gutenberg is dropped (as for HTML,
//! [`crate::import_pg_html`]). PG's plain text follows a convention:
//!
//! | Text | Book |
//! |---|---|
//! | a short paragraph after three or more blank lines ("CHAPTER I.") | a Division, titled by it |
//! | a short paragraph in capitals, a chapter's subtitle | `heading` |
//! | a paragraph (lines between blank lines), its lines joined | `para` prose |
//! | a paragraph whose lines are indented | `para` poetry, one per line |
//! | `_words_` | `italic` |
//! | `[Illustration: …]` | dropped, counted in the report |
//! | `{23}`, `{vii}` (in a book that marks five or more pages so) | zero-width `page` |
//! | Bible references | `scripture_ref` |
//!
//! With fewer than three such Divisions, paragraphs that start "CHAPTER", "SERMON",
//! "LECTURE", "QUESTION", … are the Divisions instead. A transcriber's credit ("Produced by
//! …") and a date are never a chapter. The licence is the caller's (the Open catalogue entry's).

use obs_model::{
    FORMAT_VERSION, LICENSE_UNKNOWN, Manifest, Origin, ParaStyle, Resource, ResourceBuilder,
    ResourceType, SpanKind, UnitId, UnitKind, is_valid_resource_id,
};

use crate::error::ImportError;
use crate::html_book::HtmlBookOptions;
use crate::report::ImportReport;
use crate::text_buf::TextBuf;

/// `source_format` of a Project Gutenberg plain-text book.
pub const PG_TEXT_FORMAT: &str = "pg-text";

/// A file larger than this is refused.
pub const MAX_TEXT_BYTES: usize = 64 * 1024 * 1024;
/// A heading is at most this many characters a line, and this many lines.
const MAX_HEADING_CHARS: usize = 80;
const MAX_HEADING_LINES: usize = 3;
/// Blank lines before a paragraph that make it a chapter's heading.
const CHAPTER_GAP: usize = 3;

/// Words that start a chapter's heading.
const CHAPTER_WORDS: &[&str] = &[
    "CHAPTER",
    "SERMON",
    "LECTURE",
    "PART",
    "BOOK",
    "SECTION",
    "PSALM",
    "LETTER",
    "DISCOURSE",
    "ESSAY",
    "PREFACE",
    "INTRODUCTION",
    "APPENDIX",
    "CANTO",
    "QUESTION",
    "TREATISE",
];

/// A transcriber's credit ("Produced by …"), kept as text, never a chapter.
fn is_credit(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "produced by",
        "this ebook was produced",
        "e-text prepared",
        "etext prepared",
        "transcribed by",
        "transcriber",
    ]
    .iter()
    .any(|p| lower.starts_with(p))
}

/// A printed page's label in braces, as some PG texts mark pages: `{23}`, `{vii}`.
fn brace_page(token: &str) -> Option<&str> {
    let inner = token.strip_prefix('{')?.strip_suffix('}')?;
    let number = !inner.is_empty() && inner.len() <= 6 && inner.bytes().all(|b| b.is_ascii_digit());
    let roman = !inner.is_empty()
        && inner.len() <= 8
        && inner.chars().all(|c| "ivxlcdmIVXLCDM".contains(c));
    (number || roman).then_some(inner)
}

/// One paragraph of the text and the blank lines before it.
struct Paragraph<'a> {
    lines: Vec<&'a str>,
    gap: usize,
}

fn paragraphs(body: &str) -> Vec<Paragraph<'_>> {
    let mut out = Vec::new();
    let mut lines = Vec::new();
    let mut gap = 0;
    for line in body.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            if !lines.is_empty() {
                out.push(Paragraph {
                    lines: std::mem::take(&mut lines),
                    gap,
                });
                gap = 0;
            }
            gap += 1;
        } else {
            lines.push(line);
        }
    }
    if !lines.is_empty() {
        out.push(Paragraph { lines, gap });
    }
    out
}

impl Paragraph<'_> {
    fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn is_short(&self) -> bool {
        self.lines.len() <= MAX_HEADING_LINES
            && self
                .lines
                .iter()
                .all(|l| l.trim().chars().count() <= MAX_HEADING_CHARS)
    }

    fn starts_chapter_word(&self) -> bool {
        let first = self.lines.first().map_or("", |l| l.trim());
        CHAPTER_WORDS.iter().any(|w| {
            first
                .strip_prefix(w)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '.', ':']))
        })
    }

    /// In capitals (a subtitle): letters, none lowercase.
    fn in_capitals(&self) -> bool {
        let t = self.text();
        t.chars().any(char::is_alphabetic) && !t.chars().any(char::is_lowercase)
    }

    fn indented(&self) -> bool {
        self.lines.len() > 1 && self.lines.iter().all(|l| l.starts_with("  "))
    }
}

/// Push `text`: `_italic_` runs as spans and, when `pages`, `{23}` as printed pages.
fn push_text(buf: &mut TextBuf, text: &str, pages: bool) {
    if !pages || !text.contains('{') {
        push_with_italics(buf, text);
        return;
    }
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}').map(|c| open + c);
        match close.and_then(|c| brace_page(&rest[open..=c]).map(|l| (c, l.to_string()))) {
            Some((c, label)) => {
                push_with_italics(buf, &rest[..open]);
                buf.flush_space();
                let at = buf.pos();
                buf.add_span(at..at, SpanKind::Page { label });
                rest = &rest[c + 1..];
            }
            None => {
                push_with_italics(buf, &rest[..=open]);
                rest = &rest[open + 1..];
            }
        }
    }
    push_with_italics(buf, rest);
}

/// Push `text` with `_italic_` runs as spans.
fn push_with_italics(buf: &mut TextBuf, text: &str) {
    let mut rest = text;
    while let Some(open) = rest.find('_') {
        let Some(close) = rest[open + 1..].find('_').map(|c| open + 1 + c) else {
            break;
        };
        // `_` inside a word (`snake_case`) is not italic.
        let word_before = rest[..open]
            .chars()
            .last()
            .is_some_and(char::is_alphanumeric);
        if word_before || close == open + 1 {
            buf.push_text(&rest[..=open]);
            rest = &rest[open + 1..];
            continue;
        }
        buf.push_text(&rest[..open]);
        buf.flush_space();
        let start = buf.pos();
        buf.tracked_end = start;
        buf.push_text(&rest[open + 1..close]);
        let end = buf.tracked_end.clamp(start, buf.pos());
        if start < end {
            buf.add_span(start..end, SpanKind::Italic);
        }
        rest = &rest[close + 1..];
    }
    buf.push_text(rest);
}

/// The text between PG's start and end lines; `None` when either is missing.
fn body(text: &str) -> Option<&str> {
    let line_with = |from: usize, word: &str| {
        let mut at = from;
        for line in text[from..].split_inclusive('\n') {
            let upper = line.to_uppercase();
            if upper.contains("***") && upper.contains(word) && upper.contains("PROJECT GUTENBERG")
            {
                return Some((at, at + line.len()));
            }
            at += line.len();
        }
        None
    };
    if let Some((_, start)) = line_with(0, "START OF")
        && let Some((end, _)) = line_with(start, "END OF")
    {
        return Some(&text[start..end]);
    }
    // PG's texts of before 2002: the header ends "*END*THE SMALL PRINT! … *END*", the text
    // ends at "End of Project Gutenberg's …" or "End of the Project Gutenberg Etext …", or
    // at the end of the file.
    let mut at = 0;
    let mut start = None;
    for line in text.split_inclusive('\n') {
        let upper = line.to_uppercase();
        if start.is_none() && upper.contains("*END") && upper.contains("SMALL PRINT") {
            start = Some(at + line.len());
        } else if let Some(s) = start
            && (upper.trim_start().starts_with("END OF PROJECT GUTENBERG")
                || upper
                    .trim_start()
                    .starts_with("END OF THE PROJECT GUTENBERG"))
        {
            return Some(&text[s..at]);
        }
        at += line.len();
    }
    // Some end with no such line: to the end (a paragraph naming PG is dropped anyway).
    start.map(|s| &text[s..])
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

/// Import one Project Gutenberg plain-text book. `progress(done, total)` returns false to
/// cancel.
pub fn import_pg_text(
    text: &str,
    options: &HtmlBookOptions,
    mut progress: impl FnMut(usize, usize) -> bool,
) -> Result<(Resource, ImportReport), ImportError> {
    let resource_id = options.resource_id.clone();
    if !is_valid_resource_id(&resource_id) {
        return Err(ImportError::InvalidResourceId(resource_id));
    }
    if text.len() > MAX_TEXT_BYTES {
        return Err(ImportError::TooLarge(options.source_file.clone()));
    }
    let body = body(text).ok_or_else(|| {
        ImportError::Xml(
            "no \"*** START OF\" and \"*** END OF\" lines of a Project Gutenberg eBook".into(),
        )
    })?;
    let paras: Vec<Paragraph<'_>> = paragraphs(body)
        .into_iter()
        .filter(|p| !p.text().to_lowercase().contains("gutenberg"))
        .collect();
    // Pages in braces, when the book marks them so (`{23}`, at least five).
    let brace_pages = body
        .split(|c: char| c.is_whitespace())
        .filter(|w| {
            brace_page(w.trim_matches(|c: char| !"{}ivxlcdmIVXLCDM0123456789".contains(c)))
                .is_some()
        })
        .take(5)
        .count()
        >= 5;
    let only_page = |p: &Paragraph<'_>| brace_pages && brace_page(p.text().trim()).is_some();
    // A date ("1900"), a page number or a transcriber's credit after the gap is not a
    // chapter.
    let chapter = |p: &Paragraph<'_>| {
        let text = p.text();
        p.gap >= CHAPTER_GAP
            && p.is_short()
            && text.chars().any(char::is_alphabetic)
            && !is_credit(&text)
            && !only_page(p)
    };
    let by_gap = paras.iter().filter(|p| chapter(p)).count() >= 3;
    let starts_division = |p: &Paragraph<'_>| {
        if by_gap {
            chapter(p)
        } else {
            p.is_short() && p.starts_chapter_word()
        }
    };
    let title = Some(options.title.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| resource_id.clone());
    let language = Some(options.language.clone())
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "en".into());
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
    manifest.source_format = PG_TEXT_FORMAT.into();
    manifest.source_file = (!options.source_file.is_empty()).then(|| options.source_file.clone());
    manifest.import_tool_version = options.tool_version.clone();
    manifest.format_version = FORMAT_VERSION;
    let mut builder = ResourceBuilder::new(manifest);
    let mut report = ImportReport {
        resource_id: resource_id.clone(),
        title,
        source_format: PG_TEXT_FORMAT.into(),
        source_files: vec![options.source_file.clone()],
        license_missing: true,
        ..ImportReport::default()
    };
    // (title, text) of each Division.
    let mut divisions: Vec<(String, TextBuf)> = Vec::new();
    let mut current: (String, TextBuf) = (String::new(), TextBuf::new());
    let total = paras.len();
    for (i, p) in paras.iter().enumerate() {
        if i % 5_000 == 0 && !progress(i, total) {
            return Err(ImportError::Cancelled);
        }
        let text = p.text();
        if only_page(p) {
            let label = brace_page(text.trim()).unwrap_or_default().to_string();
            let buf = &mut current.1;
            buf.flush_space();
            let at = buf.pos();
            buf.add_span(at..at, SpanKind::Page { label });
            continue;
        }
        if text.starts_with("[Illustration") {
            *report
                .dropped_markup
                .entry("illustration".into())
                .or_default() += 1;
            continue;
        }
        if starts_division(p) {
            if !current.1.text.trim().is_empty() {
                divisions.push(std::mem::replace(
                    &mut current,
                    (String::new(), TextBuf::new()),
                ));
            }
            current.0 = text.clone();
            let buf = &mut current.1;
            let start = buf.start_block();
            buf.push_text(&text);
            let end = buf.pos();
            buf.add_span(start..end, SpanKind::Heading { level: 1 });
            continue;
        }
        let buf = &mut current.1;
        if p.is_short() && p.in_capitals() {
            let start = buf.start_block();
            buf.push_text(&text);
            let end = buf.pos();
            buf.add_span(start..end, SpanKind::Heading { level: 2 });
        } else if p.indented() {
            for line in &p.lines {
                let start = buf.start_block();
                push_text(buf, line.trim(), brace_pages);
                let end = buf.pos();
                buf.add_span(
                    start..end,
                    SpanKind::Para {
                        style: ParaStyle::Poetry1,
                    },
                );
            }
        } else {
            let start = buf.start_block();
            push_text(buf, &text, brace_pages);
            let end = buf.pos();
            buf.add_span(
                start..end,
                SpanKind::Para {
                    style: ParaStyle::Prose,
                },
            );
        }
    }
    if !current.1.text.trim().is_empty() {
        divisions.push(current);
    }
    let mut written = 0u32;
    for (i, (title, mut buf)) in divisions.into_iter().enumerate() {
        buf.normalize_nfc();
        written += 1;
        let id =
            UnitId::new(format!("div.{written}")).map_err(|e| ImportError::Xml(e.to_string()))?;
        let title = if !title.is_empty() {
            title.chars().take(MAX_HEADING_CHARS * 2).collect()
        } else if i == 0 {
            "Front matter".to_string()
        } else {
            format!("Section {written}")
        };
        add_references(&buf.text, &mut report, &mut builder, &id);
        builder.add_unit(id.clone(), UnitKind::Division, Some(title), buf.text);
        for (range, kind) in buf.spans {
            builder.add_span(&id, range, kind);
        }
    }
    if written == 0 {
        return Err(ImportError::NoText);
    }
    report.entries = written as usize;
    let resource = builder.build().map_err(ImportError::Invalid)?;
    Ok((resource, report))
}

#[cfg(test)]
#[path = "pg_text_tests.rs"]
mod tests;
