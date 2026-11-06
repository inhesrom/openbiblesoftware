//! Export Notes (#154): a Notebook (or every Notebook), or the Notes a Notes search found,
//! as Markdown or HTML, or as a page to print / save as PDF.
//!
//! Each Annotation is a section headed by its Passage (the Notes Tool's reference, which
//! reads back as a Bible reference in the Standard versification), then its quoted text as a
//! block quote, then the Note's text (or which Highlight style it is). Notebook exports
//! list in Bible order; a search result keeps its order (newest change first). After them
//! comes the Attribution of each quoted Resource whose licence asks for credit.
//!
//! The Markdown keeps a Note's text as typed: CommonMark's special characters are
//! backslash-escaped, lines end in a hard line break and leading spaces are written as
//! character references, so a Markdown reader shows (and a parser reads back) the same text.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::annotations::{Anchor, Content};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::copy_verses::{html_document, html_escape, markdown_escape};
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
            out.push_str(&escape_markers(&markdown_escape(body)));
            if lines.get(i + 1).is_some_and(|next| !next.is_empty()) {
                out.push('\\');
            }
        }
        out.push('\n');
    }
    out.truncate(out.trim_end_matches('\n').len());
    out
}

/// Escape what would start a block at the beginning of a line (already inline-escaped):
/// list and heading markers, thematic breaks, ordered-list numbers and character references.
fn escape_markers(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + 2);
    if line.starts_with(['-', '+', '=']) {
        out.push('\\');
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    let ordered = digits > 0 && line[digits..].starts_with(['.', ')']);
    for (i, c) in line.char_indices() {
        if (ordered && i == digits)
            || (c == '&'
                && line[i + 1..]
                    .chars()
                    .next()
                    .is_some_and(|n| n.is_alphanumeric() || n == '#'))
        {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn html_lines(text: &str) -> String {
    text.split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            let lines: Vec<String> = p.lines().map(html_escape).collect();
            format!("<p>{}</p>", lines.join("<br>\n"))
        })
        .collect::<Vec<_>>()
        .join("\n")
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
            }
            match highlight_label(&card.annotation.content) {
                Some(label) => out.push_str(&format!("\n*{label}*\n")),
                None if !card.annotation.body().trim().is_empty() => {
                    out.push_str(&format!(
                        "\n{}\n",
                        markdown_lines(card.annotation.body(), "")
                    ));
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
            }
            match highlight_label(&card.annotation.content) {
                Some(label) => out.push(format!("<p><em>{label}</em></p>")),
                None => out.push(html_lines(card.annotation.body())),
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
        let mut attributions: Vec<String> = Vec::new();
        for card in &cards {
            let quoted = card
                .annotation
                .anchor
                .as_ref()
                .and_then(Anchor::text_range)
                .filter(|_| card.quote.is_some());
            if let Some(range) = quoted
                && let Some(entry) = self.library.get(&range.resource_id)
                && entry.manifest.requires_attribution()
            {
                let text = entry.manifest.attribution.trim().to_string();
                if !attributions.contains(&text) {
                    attributions.push(text);
                }
            }
        }
        Ok(NotesExport {
            title,
            cards,
            attributions,
        })
    }
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
