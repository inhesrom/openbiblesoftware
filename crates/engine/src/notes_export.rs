//! Export Notes (#154): a Notebook (or every Notebook), or the Notes a Notes search found,
//! as Markdown or HTML, or as a page to print / save as PDF.
//!
//! Each Annotation is a section headed by its Passage (the Notes Tool's reference, which
//! reads back as a Bible reference in the Standard versification), then its quoted text as a
//! block quote, then the Note's text (or which Highlight style it is). Notebook exports
//! list in Bible order; a search result keeps its order (newest change first). After them
//! comes the Attribution of each quoted Resource whose licence asks for credit. A quote's
//! credit is that of the revision it was quoted from (its content hash). The User database
//! keeps the credit of every revision seen in the library, confirmed each time it is quoted,
//! so a quote from one since removed (or whose file went missing) keeps it. A quote whose
//! revision's credit was never saved, or was not confirmed when the quote was taken (the
//! write failed), is left out with a note saying why, so no quote goes out without the
//! credit it may need.
//!
//! The Markdown keeps a Note's text as shown, Note formatting included (#186): headings
//! (two levels down, under the Annotation's own), bulleted and numbered lists, block quotes,
//! bold, italic and links are written as CommonMark (bold or italic spans that touch as one,
//! and with `_` rather than `*` where runs of `*` would run together and a reader pair them
//! otherwise); the rest of the text has CommonMark's
//! special characters backslash-escaped, lines end in a hard line break and leading spaces
//! are written as character references, so a Markdown reader shows (and a parser reads back)
//! the same text. A blank line separates a list or quote from text next to it, which
//! CommonMark would otherwise run together. A numbered item keeps the number typed: where it
//! does not follow on from the item above, the Markdown starts a new list there (CommonMark
//! numbers a list on from its first item) and the HTML gives the item its `value`. The HTML
//! has the same structure.

use std::collections::BTreeMap;

use obs_store::annotation::now_ms;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::annotations::{Anchor, Content, TextRange};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use obs_model::{NoteLine, NoteLineKind, NoteMark, NoteMarkKind, parse_note_text};

use crate::copy_verses::{
    html_document, html_escape, markdown_escape, markdown_escape_line_start, markdown_escape_part,
};
use crate::notes::{Card, NotesFilter};
use crate::session::Session;
use crate::workspace::TabId;

/// What to export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotesSource {
    /// One Notebook by id, or every Notebook (`None`).
    Notebook(Option<String>),
    /// The Notes a Search Tab's Notes search found.
    SearchResults(TabId),
}

/// The forms Notes can be exported in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotesFormat {
    Markdown,
    Html,
    /// An HTML page that opens the print dialog when the browser loads it.
    Print,
}

impl NotesFormat {
    pub fn id(self) -> &'static str {
        match self {
            NotesFormat::Markdown => "markdown",
            NotesFormat::Html => "html",
            NotesFormat::Print => "print",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [NotesFormat::Markdown, NotesFormat::Html, NotesFormat::Print]
            .into_iter()
            .find(|f| f.id() == id)
    }

    /// The file name extension.
    pub fn extension(self) -> &'static str {
        match self {
            NotesFormat::Markdown => "md",
            NotesFormat::Html | NotesFormat::Print => "html",
        }
    }
}

/// Annotations gathered for export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotesExport {
    pub title: String,
    pub cards: Vec<Card>,
    /// The Attributions of the quoted Resources whose licence requires one.
    pub attributions: Vec<String>,
}

/// Markdown for text that may span lines: each line escaped (block markers at its start
/// too), joined by hard line breaks; blank lines separate paragraphs. `prefix` starts every
/// line (`"> "` in a block quote).
fn markdown_lines(text: &str, prefix: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            out.push_str(prefix.trim_end());
        } else {
            out.push_str(prefix);
            let body = line.trim_start_matches([' ', '\t']);
            for c in line[..line.len() - body.len()].chars() {
                out.push_str(if c == '\t' { "&#9;" } else { "&#32;" });
            }
            out.push_str(&markdown_escape_line_start(&markdown_escape(body)));
            if lines.get(i + 1).is_some_and(|next| !next.is_empty()) {
                out.push('\\');
            }
        }
        out.push('\n');
    }
    out.truncate(out.trim_end_matches('\n').len());
    out
}

fn html_lines(text: &str) -> String {
    text.split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            let lines: Vec<String> = p.lines().map(html_escape).collect();
            // Its direction from its first letter: a Hebrew quote or Note reads right to left.
            format!("<p dir=\"auto\">{}</p>", lines.join("<br>\n"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// How far a Note's headings move down in an export: its `#` sits under the Annotation's `##`.
const HEADING_SHIFT: u8 = 2;

/// Marks in the order they open, outermost first at each start.
fn sorted(marks: &[NoteMark]) -> Vec<&NoteMark> {
    let mut marks: Vec<&NoteMark> = marks.iter().collect();
    marks.sort_by_key(|m| (m.range.start, std::cmp::Reverse(m.range.end)));
    marks
}

/// Write a line's shown text with its marks: `text` writes a span of plain text, `open` and
/// `close` a mark's ends. `open` is told what is written before it, and whether that ends
/// with a closing.
fn write_inline(
    line: &NoteLine,
    text: &dyn Fn(std::ops::Range<usize>) -> String,
    open: &dyn Fn(&NoteMark, &str, bool) -> String,
    close: &dyn Fn(&NoteMark) -> String,
) -> String {
    let mut out = String::new();
    let mut stack: Vec<&NoteMark> = Vec::new();
    let mut at = 0;
    // Where the last closing written ends in `out`.
    let mut closed = None;
    let close_to = |out: &mut String,
                    stack: &mut Vec<&NoteMark>,
                    at: &mut usize,
                    closed: &mut Option<usize>,
                    upto| {
        while let Some(top) = stack.last().copied().filter(|t| t.range.end <= upto) {
            out.push_str(&text(*at..top.range.end));
            *at = top.range.end;
            out.push_str(&close(top));
            *closed = Some(out.len());
            stack.pop();
        }
    };
    for m in sorted(&line.marks) {
        close_to(&mut out, &mut stack, &mut at, &mut closed, m.range.start);
        out.push_str(&text(at..m.range.start));
        at = m.range.start;
        let opening = open(m, &out, closed == Some(out.len()));
        out.push_str(&opening);
        stack.push(m);
    }
    close_to(&mut out, &mut stack, &mut at, &mut closed, usize::MAX);
    out.push_str(&text(at..line.text.len()));
    out
}

/// A link target as a CommonMark destination: `<…>` around one with spaces or parentheses.
fn markdown_target(target: &str) -> String {
    if target.contains([' ', '(', ')']) {
        format!("<{}>", target.replace(['<', '>'], ""))
    } else {
        target.to_string()
    }
}

fn is_emphasis(m: &NoteMark) -> bool {
    matches!(m.kind, NoteMarkKind::Bold | NoteMarkKind::Italic)
}

/// A line's marks with bold (or italic) spans that touch joined into one, where that
/// crosses no other mark, and one inside another of its kind left out: `_faith_*hope*` shows
/// one italic word, which `*faith**hope*` would not be to a CommonMark reader.
fn joined_marks(line: &NoteLine) -> Vec<NoteMark> {
    let mut marks = line.marks.clone();
    let crosses = |a: &std::ops::Range<usize>, b: &std::ops::Range<usize>| {
        (a.start < b.start && b.start < a.end && a.end < b.end)
            || (b.start < a.start && a.start < b.end && b.end < a.end)
    };
    'join: loop {
        for i in 0..marks.len() {
            for j in 0..marks.len() {
                if i == j || !is_emphasis(&marks[i]) || marks[i].kind != marks[j].kind {
                    continue;
                }
                let (outer, inner) = (&marks[i].range, &marks[j].range);
                if outer.start <= inner.start && inner.end <= outer.end {
                    marks.remove(j);
                    continue 'join;
                }
                if outer.end != inner.start {
                    continue;
                }
                let joined = marks[i].range.start..marks[j].range.end;
                if marks
                    .iter()
                    .enumerate()
                    .any(|(k, m)| k != i && k != j && crosses(&m.range, &joined))
                {
                    continue;
                }
                marks[i].range = joined;
                marks.remove(j);
                continue 'join;
            }
        }
        return marks;
    }
}

/// Whether a CommonMark reader takes `m` written with `_` as emphasis, `before` written
/// ahead of it: a `_` does not open or close inside a word. A mark that opens where `m`
/// closes, or one around it that closes there too, writes punctuation after it.
fn underscore_fits(line: &NoteLine, marks: &[NoteMark], m: &NoteMark, before: &str) -> bool {
    let word = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
    let closes_after = |o: &NoteMark| {
        o.range.end == m.range.end && (o.range.start < m.range.start || o.range == m.range)
    };
    !word(before.chars().next_back())
        && (marks
            .iter()
            .any(|o| o != m && (o.range.start == m.range.end || closes_after(o)))
            || !word(line.text[m.range.end..].chars().next()))
}

/// Whether `m` is better written with `_` than `*`, where a run of `*` would run on into
/// another and a CommonMark reader pair them otherwise:
/// - it opens just after a closing `*` (`**a _b_**_c_`, not `**a *b****c*`);
/// - an emphasis of the other kind inside it shares one end with it (`_**a**b_`, not
///   `***a**b*`);
///
/// but not just after a closing `_`.
fn wants_underscore(
    line: &NoteLine,
    marks: &[NoteMark],
    m: &NoteMark,
    before: &str,
    after_closing: bool,
) -> bool {
    let shares_one_end = marks.iter().any(|o| {
        is_emphasis(o)
            && o.kind != m.kind
            && m.range.start <= o.range.start
            && o.range.end <= m.range.end
            && ((o.range.start == m.range.start) != (o.range.end == m.range.end))
    });
    // After a closing `_` it is `*`, for the same reason.
    let after = |c| after_closing && before.ends_with(c);
    (after('*') || shares_one_end) && !after('_') && underscore_fits(line, marks, m, before)
}

fn inline_markdown(line: &NoteLine) -> String {
    let text = line.text.trim_end();
    let mut line = line.clone();
    line.marks = joined_marks(&line);
    // The emphasis written with `_` (see `wants_underscore`).
    let underscored: std::cell::RefCell<Vec<NoteMark>> = Default::default();
    let delimiter = |m: &NoteMark| {
        let mark = if underscored.borrow().contains(m) {
            "_"
        } else {
            "*"
        };
        mark.repeat(if m.kind == NoteMarkKind::Bold { 2 } else { 1 })
    };
    write_inline(
        &line,
        &|r| markdown_escape_part(text, r.start.min(text.len())..r.end.min(text.len())),
        &|m, before, after_closing| match m.kind {
            NoteMarkKind::Link(_) => "[".into(),
            _ => {
                if wants_underscore(&line, &line.marks, m, before, after_closing) {
                    underscored.borrow_mut().push(m.clone());
                }
                delimiter(m)
            }
        },
        &|m| match &m.kind {
            NoteMarkKind::Link(t) => format!("]({})", markdown_target(t)),
            _ => delimiter(m),
        },
    )
}

/// What kind of Markdown block a line belongs to; a change of kind between two lines needs a
/// blank line, except next to a heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Text,
    Blank,
    Heading,
    List,
    Quote,
}

fn group(kind: NoteLineKind) -> Group {
    match kind {
        NoteLineKind::Text => Group::Text,
        NoteLineKind::Blank => Group::Blank,
        NoteLineKind::Heading(_) => Group::Heading,
        NoteLineKind::Bullet { .. } | NoteLineKind::Numbered { .. } => Group::List,
        NoteLineKind::Quote => Group::Quote,
    }
}

/// A Note's text as Markdown (see the module docs).
pub fn note_markdown(body: &str) -> String {
    let lines = parse_note_text(body);
    let mut out: Vec<String> = Vec::new();
    // The last list item at each level: its content column, for nesting, and if numbered
    // the number that follows on from it and the delimiter written.
    let mut levels: Vec<(usize, Option<(u32, char)>)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let g = group(line.kind);
        if let Some(prev) = i.checked_sub(1).map(|p| group(lines[p].kind))
            && prev != g
            && ![prev, g]
                .iter()
                .any(|x| matches!(x, Group::Blank | Group::Heading))
        {
            out.push(String::new());
        }
        let next = lines.get(i + 1).map(|n| group(n.kind));
        let content = markdown_escape_line_start(&inline_markdown(line));
        let written = match line.kind {
            NoteLineKind::Blank => String::new(),
            NoteLineKind::Text => {
                let body = content.trim_start_matches([' ', '\t']);
                let lead: String = content[..content.len() - body.len()]
                    .chars()
                    .map(|c| if c == '\t' { "&#9;" } else { "&#32;" })
                    .collect();
                let mut text = format!("{lead}{body}");
                if next == Some(Group::Text) {
                    text.push('\\');
                }
                text
            }
            NoteLineKind::Heading(level) => {
                let hashes = "#".repeat(usize::from((level + HEADING_SHIFT).min(6)));
                format!("{hashes} {content}")
            }
            NoteLineKind::Quote => {
                let mut text = format!("> {content}");
                if next == Some(Group::Quote) {
                    text.push('\\');
                }
                text
            }
            NoteLineKind::Bullet { level } | NoteLineKind::Numbered { level, .. } => {
                let level = usize::from(level).min(levels.len());
                let before = levels.get(level).copied();
                levels.truncate(level);
                let indent = levels.last().map_or(0, |l| l.0);
                let (marker, numbered) = match line.kind {
                    NoteLineKind::Numbered { number, .. } => {
                        // CommonMark numbers a list's items on from its first, so a number
                        // that does not follow on starts a new list (at that number), which
                        // a change of delimiter does.
                        let delim = match before {
                            Some((_, Some((next, d)))) if next == number => d,
                            Some((_, Some((_, '.')))) => ')',
                            _ => '.',
                        };
                        // Nor may a list that starts at another number than 1 interrupt
                        // the text of the item above: a blank line ends that text first.
                        if level > 0 && before.is_none() && number != 1 {
                            out.push(String::new());
                        }
                        (
                            format!("{number}{delim} "),
                            Some((number.saturating_add(1), delim)),
                        )
                    }
                    _ => ("- ".to_string(), None),
                };
                levels.push((indent + marker.len(), numbered));
                format!("{}{marker}{content}", " ".repeat(indent))
            }
        };
        match g {
            Group::List => {}
            // A blank line does not end a CommonMark list: the next item may follow on.
            Group::Blank => levels.truncate(1),
            _ => levels.clear(),
        }
        out.push(written.trim_end_matches(' ').to_string());
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    while out.first().is_some_and(String::is_empty) {
        out.remove(0);
    }
    out.join("\n")
}

fn inline_html(line: &NoteLine) -> String {
    let url = |t: &str| {
        ["https://", "http://", "mailto:"]
            .iter()
            .any(|p| t.starts_with(p))
    };
    write_inline(
        line,
        &|r| html_escape(&line.text[r]),
        &|m, _, _| match &m.kind {
            NoteMarkKind::Bold => "<strong>".into(),
            NoteMarkKind::Italic => "<em>".into(),
            NoteMarkKind::Link(t) if url(t) => format!("<a href=\"{}\">", html_escape(t)),
            NoteMarkKind::Link(_) => String::new(),
        },
        &|m| match &m.kind {
            NoteMarkKind::Bold => "</strong>".into(),
            NoteMarkKind::Italic => "</em>".into(),
            NoteMarkKind::Link(t) if url(t) => "</a>".into(),
            // A Bible reference: its text, then where it points when that is not the same.
            NoteMarkKind::Link(t) if line.text[m.range.clone()].trim() == t.trim() => String::new(),
            NoteMarkKind::Link(t) => format!(" ({})", html_escape(t)),
        },
    )
}

/// A Note's text as HTML: paragraphs (`<br>` between lines), headings two levels down,
/// nested `<ul>`/`<ol>` lists and block quotes.
pub fn note_html(body: &str) -> String {
    let lines = parse_note_text(body);
    let mut out: Vec<String> = Vec::new();
    // Text or quote lines waiting for their paragraph.
    let mut para: Vec<String> = Vec::new();
    let mut para_quote = false;
    // Open lists: whether each is numbered, and the number that follows on in it.
    let mut lists: Vec<(bool, u32)> = Vec::new();
    let flush = |out: &mut Vec<String>, para: &mut Vec<String>, quote: bool| {
        if para.is_empty() {
            return;
        }
        // Its direction from its first letter: a Hebrew paragraph reads right to left.
        let p = format!("<p dir=\"auto\">{}</p>", para.join("<br>\n"));
        out.push(if quote {
            format!("<blockquote>{p}</blockquote>")
        } else {
            p
        });
        para.clear();
    };
    let close_lists = |out: &mut Vec<String>, lists: &mut Vec<(bool, u32)>, keep: usize| {
        while lists.len() > keep {
            let numbered = lists.pop().is_some_and(|l| l.0);
            out.push(if numbered { "</li></ol>" } else { "</li></ul>" }.into());
        }
    };
    for line in &lines {
        let quote = line.kind == NoteLineKind::Quote;
        if !matches!(line.kind, NoteLineKind::Text | NoteLineKind::Quote) || quote != para_quote {
            flush(&mut out, &mut para, para_quote);
        }
        if line.kind.list_level().is_none() {
            close_lists(&mut out, &mut lists, 0);
        }
        match line.kind {
            NoteLineKind::Blank => {}
            NoteLineKind::Text | NoteLineKind::Quote => {
                para_quote = quote;
                para.push(inline_html(line));
            }
            NoteLineKind::Heading(level) => {
                let n = (level + HEADING_SHIFT).min(6);
                out.push(format!("<h{n}>{}</h{n}>", inline_html(line)));
            }
            NoteLineKind::Bullet { level } | NoteLineKind::Numbered { level, .. } => {
                let number = match line.kind {
                    NoteLineKind::Numbered { number, .. } => Some(number),
                    _ => None,
                };
                let numbered = number.is_some();
                let level = usize::from(level).min(lists.len());
                close_lists(&mut out, &mut lists, level + 1);
                let item = |value: Option<u32>| match value {
                    Some(n) => format!("<li value=\"{n}\">{}", inline_html(line)),
                    None => format!("<li>{}", inline_html(line)),
                };
                let next = number.map_or(0, |n| n.saturating_add(1));
                match lists.get_mut(level) {
                    Some(open) if open.0 == numbered => {
                        // An item numbered as typed, not on from the one above.
                        let value = number.filter(|n| *n != open.1);
                        open.1 = next;
                        out.push(format!("</li>\n{}", item(value)));
                    }
                    other => {
                        // Another kind of list at this level: the old one ends here.
                        if other.is_some() {
                            close_lists(&mut out, &mut lists, level);
                        }
                        let open = match number {
                            Some(1) => "<ol>".to_string(),
                            Some(n) => format!("<ol start=\"{n}\">"),
                            None => "<ul>".to_string(),
                        };
                        out.push(format!("{open}\n{}", item(None)));
                        lists.push((numbered, next));
                    }
                }
            }
        }
    }
    flush(&mut out, &mut para, para_quote);
    close_lists(&mut out, &mut lists, 0);
    out.join("\n")
}

/// Why a Text range's quote was left out of the export (see [`Session::notes_export`]).
fn withheld_quote(card: &Card) -> Option<&str> {
    card.missing.as_deref().filter(|_| card.quote.is_none())
}

/// What a Highlight says in place of a Note's text.
fn highlight_label(content: &Content) -> Option<String> {
    match content {
        Content::Highlight { style } => {
            Some(format!("Highlight ({})", style.id().replace('_', " ")))
        }
        Content::Note { .. } => None,
    }
}

impl NotesExport {
    pub fn render(&self, format: NotesFormat) -> String {
        match format {
            NotesFormat::Markdown => self.markdown(),
            NotesFormat::Html => html_document(&self.title, &self.html(), false),
            NotesFormat::Print => html_document(&self.title, &self.html(), true),
        }
    }

    /// A file name for it: the title with characters file systems refuse replaced.
    pub fn file_name(&self, format: NotesFormat) -> String {
        let stem: String = self
            .title
            .chars()
            .map(|c| {
                if c.is_control() || "/\\:*?\"<>|“”".contains(c) {
                    ' '
                } else {
                    c
                }
            })
            .collect();
        let stem = stem.split_whitespace().collect::<Vec<_>>().join(" ");
        let stem = if stem.is_empty() {
            "Notes".into()
        } else {
            stem
        };
        format!("{stem}.{}", format.extension())
    }

    pub fn markdown(&self) -> String {
        let mut out = format!("# {}\n", markdown_escape(&self.title));
        for card in &self.cards {
            out.push_str(&format!("\n## {}\n", markdown_escape(&card.reference)));
            if let Some(quote) = card.quote.as_deref().filter(|q| !q.is_empty()) {
                out.push_str(&format!("\n{}\n", markdown_lines(quote, "> ")));
            } else if let Some(left_out) = withheld_quote(card) {
                out.push_str(&format!("\n*{}*\n", markdown_escape(left_out)));
            }
            match highlight_label(&card.annotation.content) {
                Some(label) => out.push_str(&format!("\n*{label}*\n")),
                None if !card.annotation.body().trim().is_empty() => {
                    out.push_str(&format!("\n{}\n", note_markdown(card.annotation.body())));
                }
                None => {}
            }
        }
        if !self.attributions.is_empty() {
            out.push_str("\n---\n");
            for a in &self.attributions {
                out.push_str(&format!("\n{}\n", markdown_lines(a, "")));
            }
        }
        out
    }

    /// The export as an HTML fragment.
    pub fn html(&self) -> String {
        let mut out = vec![format!("<h1>{}</h1>", html_escape(&self.title))];
        for card in &self.cards {
            out.push(format!("<h2>{}</h2>", html_escape(&card.reference)));
            if let Some(quote) = card.quote.as_deref().filter(|q| !q.is_empty()) {
                out.push(format!("<blockquote>{}</blockquote>", html_lines(quote)));
            } else if let Some(left_out) = withheld_quote(card) {
                out.push(format!("<p><em>{}</em></p>", html_escape(left_out)));
            }
            match highlight_label(&card.annotation.content) {
                Some(label) => out.push(format!("<p><em>{label}</em></p>")),
                None => out.push(note_html(card.annotation.body())),
            }
        }
        if !self.attributions.is_empty() {
            out.push("<hr>".into());
            for a in &self.attributions {
                out.push(format!("<p><small>{}</small></p>", html_escape(a)));
            }
        }
        out.retain(|s| !s.is_empty());
        out.join("\n")
    }
}

impl Session {
    /// Gather Annotations to export.
    pub fn notes_export(&self, source: &NotesSource) -> Result<NotesExport, String> {
        if self.user.is_none() {
            return Err("Notes are kept in the User database, which is not open".into());
        }
        let notebooks: BTreeMap<String, String> = self
            .notebooks()?
            .into_iter()
            .map(|n| (n.id, n.name))
            .collect();
        let (title, cards) = match source {
            NotesSource::Notebook(id) => {
                let title = match id {
                    Some(id) => notebooks
                        .get(id)
                        .cloned()
                        .ok_or_else(|| format!("No Notebook with id {id}"))?,
                    None => "All Notebooks".to_string(),
                };
                let mut cards: Vec<Card> = self
                    .annotations(id.as_deref())?
                    .into_iter()
                    .map(|a| self.card(a, &notebooks))
                    .collect();
                self.sort_in_bible_order(&mut cards);
                (title, cards)
            }
            NotesSource::SearchResults(tab) => {
                let st = self.search_tab(*tab).ok_or("That Search Tab is not open")?;
                let hits = st.notes.as_ref().ok_or("Run a Notes search first")?;
                let mut by_id: BTreeMap<String, crate::annotations::Annotation> = self
                    .annotations(None)?
                    .into_iter()
                    .map(|a| (a.id.clone(), a))
                    .collect();
                let cards = hits
                    .hits
                    .iter()
                    .filter_map(|h| by_id.remove(&h.id))
                    .map(|a| self.card(a, &notebooks))
                    .collect();
                (format!("Notes matching “{}”", st.query.trim()), cards)
            }
        };
        let mut cards = cards;
        let mut attributions: Vec<String> = Vec::new();
        for card in &mut cards {
            let Some(range) = card
                .annotation
                .anchor
                .as_ref()
                .and_then(Anchor::text_range)
                .filter(|_| card.quote.is_some())
            else {
                continue;
            };
            match self.quote_credit(range, card.annotation.created_at) {
                Some(Some(text)) => {
                    if !attributions.contains(&text) {
                        attributions.push(text);
                    }
                }
                Some(None) => {}
                // Without its licence the quote could go out without the credit it asks for.
                None => {
                    let name = range.resource_id.clone();
                    let why = if self.library.get(&name).is_some() {
                        format!(
                            "Quoted text left out: it comes from a revision of {name} no longer \
                             in the library, so the Attribution its licence may require is \
                             unknown. Re-attach the quote to export it."
                        )
                    } else {
                        format!(
                            "Quoted text left out: {name} is not in the library, so the \
                             Attribution its licence may require is unknown. Import it again \
                             to export the quote."
                        )
                    };
                    card.quote = None;
                    card.missing = Some(why);
                }
            }
        }
        Ok(NotesExport {
            title,
            cards,
            attributions,
        })
    }

    /// The credit a quote taken at `quoted_at` (the Annotation's creation) from `range` must
    /// carry: `Some(Some(text))` an Attribution, `Some(None)` none needed, `None` unknown.
    /// The credit is that of the revision quoted (its content hash): the library's while it
    /// holds that revision, else the one the User database kept for it, trusted only if it
    /// was confirmed no earlier than the quote was taken (a later licence change that could
    /// not be saved leaves it unconfirmed).
    fn quote_credit(&self, range: &TextRange, quoted_at: i64) -> Option<Option<String>> {
        if let Some(entry) = self
            .library
            .get(&range.resource_id)
            .filter(|e| e.manifest.content_hash == range.content_hash)
        {
            return Some(manifest_credit(&entry.manifest));
        }
        let kept = self.kept_attribution(&range.resource_id, &range.content_hash)?;
        (kept.confirmed_at >= quoted_at).then(|| Some(kept.credit).filter(|t| !t.is_empty()))
    }

    fn kept_attribution(&self, resource_id: &str, content_hash: &str) -> Option<KeptAttribution> {
        let value = self
            .user
            .as_ref()?
            .preference(&resource_attribution_key(resource_id, content_hash))
            .ok()??;
        serde_json::from_str(&value).ok()
    }

    /// Remember in the User database the credit revision `entry` asks for, so the quotes its
    /// Annotations hold still export with it once it leaves the library (#154). With
    /// `quoted_at` (a quote of it was just taken) the credit is confirmed as of then even if
    /// unchanged; otherwise it is written only when new or changed (a re-import under
    /// another licence).
    pub(crate) fn remember_attribution(
        &self,
        entry: &crate::library::LibraryEntry,
        quoted_at: Option<i64>,
    ) -> Result<(), String> {
        let Some(user) = &self.user else {
            return Ok(());
        };
        let m = &entry.manifest;
        let credit = manifest_credit(m).unwrap_or_default();
        if let Some(kept) = self.kept_attribution(&m.resource_id, &m.content_hash)
            && kept.credit == credit
            && quoted_at.is_none_or(|at| kept.confirmed_at >= at)
        {
            return Ok(());
        }
        let kept = KeptAttribution {
            credit,
            confirmed_at: now_ms().max(quoted_at.unwrap_or(i64::MIN)),
        };
        let value = serde_json::to_string(&kept).map_err(|e| e.to_string())?;
        user.set_preference(
            &resource_attribution_key(&m.resource_id, &m.content_hash),
            &value,
        )
    }

    /// [`Self::remember_attribution`] for every Resource in the library, confirmed as of
    /// the latest quote of each (a confirmation that failed when the quote was taken is
    /// made now). Run at library load and after Resources are added, with the Resource
    /// names.
    pub(crate) fn remember_attributions(&mut self) {
        let latest = self.latest_quotes();
        let failed = self.library.entries().iter().find_map(|entry| {
            let m = &entry.manifest;
            let quoted_at = latest
                .get(&(m.resource_id.clone(), m.content_hash.clone()))
                .copied();
            self.remember_attribution(entry, quoted_at).err()
        });
        if let Some(e) = failed {
            self.workspace
                .push_effect(crate::workspace::Effect::Message(format!(
                    "Could not save the Attributions of the library's Resources; Notes \
                     exports will leave out quotes from Resources removed later: {e}"
                )));
        }
    }

    /// When each revision (Resource id, content hash) was last quoted: the newest creation
    /// time of the Annotations whose Text range is in it.
    pub(crate) fn latest_quotes(&self) -> BTreeMap<(String, String), i64> {
        let mut latest = BTreeMap::new();
        let all = self
            .user
            .as_ref()
            .and_then(|u| u.annotations(None).ok())
            .unwrap_or_default();
        for a in all {
            if let Some(r) = a.anchor.as_ref().and_then(Anchor::text_range) {
                let at = latest
                    .entry((r.resource_id.clone(), r.content_hash.clone()))
                    .or_insert(a.created_at);
                *at = (*at).max(a.created_at);
            }
        }
        latest
    }

    /// A quote of Resource `resource_id` was taken at `quoted_at`: confirm its credit in the
    /// User database, so the quote exports with it once the Resource is gone.
    pub(crate) fn confirm_quoted_attribution(&mut self, resource_id: &str, quoted_at: i64) {
        let Some(entry) = self.library.get(resource_id) else {
            return;
        };
        if let Err(e) = self.remember_attribution(entry, Some(quoted_at)) {
            let name = entry.manifest.abbreviation.clone();
            self.workspace
                .push_effect(crate::workspace::Effect::Message(format!(
                    "Could not save the Attribution of {name}; Notes exports will leave out \
                     this quote once {name} leaves the library: {e}"
                )));
        }
    }
}

/// The Attribution text copied from Resource `m` must carry, if its licence asks for one.
fn manifest_credit(m: &obs_model::Manifest) -> Option<String> {
    m.requires_attribution()
        .then(|| m.attribution.trim().to_string())
}

/// What the User database keeps of the credit of one revision of a Resource.
#[derive(Debug, Serialize, Deserialize)]
struct KeptAttribution {
    /// Its Attribution; empty when its licence asks for none.
    credit: String,
    /// When (milliseconds since the Unix epoch) the library last showed this revision with
    /// this credit. Quotes taken after it were taken under a licence never saved.
    confirmed_at: i64,
}

/// The preference holding a [`KeptAttribution`] of revision `content_hash` of a Resource.
fn resource_attribution_key(resource_id: &str, content_hash: &str) -> String {
    format!("resource-attribution/{resource_id}/{content_hash}")
}

// ------------------------------------------------------------------------------ commands

fn has_user(s: &Session) -> bool {
    s.user.is_some()
}

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "notes.export",
    label: "Export Notes",
    shortcut: None,
    params: &[
        ParamSpec {
            name: "format",
            kind: ParamKind::String,
            required: true,
            help: "markdown, html, or print (a page that opens the print dialog)",
        },
        ParamSpec {
            name: "notebook",
            kind: ParamKind::String,
            required: false,
            help: "A Notebook id, or `all` (default: the Notebook shown in the Notes Tool)",
        },
        ParamSpec {
            name: "tab",
            kind: ParamKind::Integer,
            required: false,
            help: "A Search Tab: export the Notes its Notes search found",
        },
        ParamSpec {
            name: "path",
            kind: ParamKind::String,
            required: false,
            help: "Write the export to this file",
        },
    ],
    enabled: has_user,
    run: run_export,
}];

fn run_export(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "notes.export";
    let failed = |message: String| CommandError::Failed {
        command: CMD,
        message,
    };
    let format_id = p.str("format").unwrap_or_default();
    let format = NotesFormat::from_id(format_id)
        .ok_or_else(|| failed(format!("no export format `{format_id}`")))?;
    let source = match (p.i64("tab"), p.str("notebook")) {
        (Some(tab), _) => NotesSource::SearchResults(TabId(u64::try_from(tab).unwrap_or(0))),
        (None, Some("all")) => NotesSource::Notebook(None),
        (None, Some(id)) => NotesSource::Notebook(Some(id.to_string())),
        (None, None) => NotesSource::Notebook(match &s.notes.filter {
            NotesFilter::Notebook(id) => Some(id.clone()),
            NotesFilter::All | NotesFilter::NeedsAttention => None,
        }),
    };
    let export = s.notes_export(&source).map_err(failed)?;
    let text = export.render(format);
    if let Some(path) = p.str("path") {
        std::fs::write(path, &text).map_err(|e| failed(format!("{path}: {e}")))?;
    }
    Ok(json!({
        "title": export.title,
        "file_name": export.file_name(format),
        "annotations": export.cards.len(),
        "text": text,
    }))
}
