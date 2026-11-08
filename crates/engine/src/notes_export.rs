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
//! A Note's text is CommonMark (Note formatting, #186). The Markdown export writes it byte
//! for byte as typed, never re-written or wrapped; only a code fence or HTML block the Note
//! leaves open is closed by a line after it, so the rest of the export is not read into it.
//! The HTML export is a CommonMark reader's HTML (pulldown-cmark), with line breaks kept,
//! the Note's headings two levels down (under the Annotation's own), raw HTML shown as
//! typed, and only web links as links.

use std::collections::BTreeMap;

use obs_store::annotation::now_ms;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::annotations::{Anchor, Content, TextRange};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use crate::copy_verses::{html_document, html_escape, markdown_escape, markdown_escape_line_start};
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
    /// What each Highlight style is called (#181), by id.
    pub style_names: BTreeMap<String, String>,
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

/// How far a Note's headings move down in the HTML export: its `#` sits under the
/// Annotation's `##`.
const HEADING_SHIFT: u8 = 2;

/// A Note's text as Markdown: its source byte for byte, which is CommonMark (see the module
/// docs), never re-written or wrapped, so its links, reference definitions and white space
/// are as typed. Only if it ends inside a code fence or an HTML block is a closing line put
/// after it, so what follows in the export is not read into it.
pub fn note_markdown(body: &str) -> String {
    if ends_closed(body) {
        return body.to_string();
    }
    let longest = |c: char| {
        body.split(|x| x != c)
            .map(str::len)
            .max()
            .unwrap_or(0)
            .max(3)
    };
    let fences = ['`', '~']
        .into_iter()
        .flat_map(|c| (3..=longest(c)).map(move |n| c.to_string().repeat(n)));
    let html = [
        "-->",
        "?>",
        "]]>",
        ">",
        "</script>",
        "</pre>",
        "</style>",
        "</textarea>",
    ]
    .map(String::from);
    fences
        .chain(html)
        .map(|closer| {
            // The closer on a line of its own, after the text as typed (its last line
            // break, if any, ends the line before it).
            let gap = if body.ends_with('\n') { "" } else { "\n" };
            format!("{body}{gap}{closer}")
        })
        .find(|closed| ends_closed(closed))
        .unwrap_or_else(|| body.to_string())
}

/// Whether a heading after Markdown `md` (and a blank line) is read as one, not as part of
/// a code fence or HTML block left open in it.
fn ends_closed(md: &str) -> bool {
    let probe = format!("{md}\n\n# end");
    let events: Vec<Event<'_>> = Parser::new(&probe).collect();
    matches!(
        events.as_slice(),
        [
            ..,
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }),
            Event::Text(t),
            Event::End(TagEnd::Heading(HeadingLevel::H1)),
        ] if t.as_ref() == "end"
    )
}

/// Whether a link target is a web address the HTML export may link to.
fn web_target(t: &str) -> bool {
    ["https://", "http://", "mailto:"]
        .iter()
        .any(|p| t.starts_with(p))
}

/// A Note's text as HTML, by a CommonMark reader, as the app shows it: each line break a
/// line break (`<br />`), headings two levels down, raw HTML shown as typed (escaped), web
/// links as `<a href>` and a link to a Bible reference as its text followed by the reference
/// in parentheses when they differ; paragraphs take their direction from their first letter.
pub fn note_html(body: &str) -> String {
    let shift = |level: HeadingLevel| {
        HeadingLevel::try_from((level as usize + usize::from(HEADING_SHIFT)).min(6))
            .unwrap_or(HeadingLevel::H6)
    };
    let mut events: Vec<Event<'_>> = Vec::new();
    // Open links: for one not written as a link, its target and the text shown so far.
    let mut links: Vec<Option<(String, String)>> = Vec::new();
    for event in Parser::new(body) {
        match event {
            Event::SoftBreak => events.push(Event::HardBreak),
            Event::Start(Tag::Heading {
                level,
                id,
                classes,
                attrs,
            }) => events.push(Event::Start(Tag::Heading {
                level: shift(level),
                id,
                classes,
                attrs,
            })),
            Event::End(TagEnd::Heading(level)) => {
                events.push(Event::End(TagEnd::Heading(shift(level))));
            }
            // Raw HTML shows as typed, its lines as lines of a paragraph.
            Event::Start(Tag::HtmlBlock) => events.push(Event::Start(Tag::Paragraph)),
            Event::End(TagEnd::HtmlBlock) => {
                if events.last() == Some(&Event::HardBreak) {
                    events.pop();
                }
                events.push(Event::End(TagEnd::Paragraph));
            }
            Event::Html(t) => {
                let line = t.trim_end_matches(['\r', '\n']).to_string();
                let broke = line.len() < t.len();
                events.push(Event::Text(line.into()));
                if broke {
                    events.push(Event::HardBreak);
                }
            }
            Event::InlineHtml(t) => events.push(Event::Text(t)),
            Event::Start(Tag::Link { ref dest_url, .. }) if !web_target(dest_url) => {
                links.push(Some((dest_url.to_string(), String::new())));
            }
            Event::Start(Tag::Link { .. }) => {
                links.push(None);
                events.push(event);
            }
            Event::End(TagEnd::Link) => match links.pop() {
                Some(Some((target, text))) => {
                    if text.trim() != target.trim() {
                        events.push(Event::Text(format!(" ({target})").into()));
                    }
                }
                _ => events.push(event),
            },
            // An image is not fetched: its description shows.
            Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => {}
            Event::Text(ref t) | Event::Code(ref t) => {
                for (_, text) in links.iter_mut().flatten() {
                    text.push_str(t);
                }
                events.push(event);
            }
            other => events.push(other),
        }
    }
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    // Text is escaped, so `<p>` here is only ever the element.
    html.replace("<p>", "<p dir=\"auto\">")
        .trim_end()
        .to_string()
}

/// Why a Text range's quote was left out of the export (see [`Session::notes_export`]).
fn withheld_quote(card: &Card) -> Option<&str> {
    card.missing.as_deref().filter(|_| card.quote.is_none())
}

impl NotesExport {
    /// What a Highlight says in place of a Note's text: "Highlight (Promises)", its style's
    /// name as written by the user (to be escaped), or its id when the style is gone.
    fn highlight_label(&self, content: &Content) -> Option<String> {
        match content {
            Content::Highlight { style } => Some(format!(
                "Highlight ({})",
                self.style_names.get(style).unwrap_or(style)
            )),
            Content::Note { .. } => None,
        }
    }

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
            match self.highlight_label(&card.annotation.content) {
                Some(label) => out.push_str(&format!("\n*{}*\n", markdown_escape(&label))),
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
            match self.highlight_label(&card.annotation.content) {
                Some(label) => out.push(format!("<p><em>{}</em></p>", html_escape(&label))),
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
        let styles = self.highlight_styles().unwrap_or_default();
        let style_names = styles
            .iter()
            .map(|st| {
                (
                    st.id.clone(),
                    crate::highlight_styles::style_label(st, &styles),
                )
            })
            .collect();
        Ok(NotesExport {
            title,
            cards,
            attributions,
            style_names,
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
        s.write_export(std::path::Path::new(path), text.as_bytes())
            .map_err(failed)?;
    }
    Ok(json!({
        "title": export.title,
        "file_name": export.file_name(format),
        "annotations": export.cards.len(),
        "text": text,
    }))
}
