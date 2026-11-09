//! Help → User guide (#218): the offline guide bundled with the app. Its pages are task-based
//! Markdown files under `docs/user-guide/`, compiled in, and shown in one keyless,
//! single-instance Tab with the Notes renderer (Note formatting, ADR 0010): headings, lists,
//! bold, italic, links (to a Bible reference, a web address or another page) and Bible
//! references as live links. A line that is only an image (`![alt](../previews/x.png)`) is a
//! screenshot from the screenshot gallery (`docs/previews/`), which the UI draws. The Tab's
//! header box searches every page.

use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::notes::{NoteBodyLine, note_body};
use crate::session::Session;
use crate::workspace::{OpenRequest, PanelId, TabKind};

/// The User guide Tab's title.
pub const USER_GUIDE_TITLE: &str = "User guide";

/// One page of the User guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuidePage {
    /// Its file name without `.md`, which links and `help.user_guide {page}` use.
    pub id: &'static str,
    /// Its title, the file's first heading.
    pub title: &'static str,
    /// Its Markdown (CommonMark, as a Note's text).
    pub text: &'static str,
}

macro_rules! pages {
    ($(($id:literal, $title:literal)),* $(,)?) => {
        /// The pages, in the order the Tab lists them.
        pub const USER_GUIDE: &[GuidePage] = &[$(GuidePage {
            id: $id,
            title: $title,
            text: include_str!(concat!("../../../docs/user-guide/", $id, ".md")),
        }),*];
    };
}

pages![
    ("getting-started", "Getting started"),
    ("read-and-compare", "Read and compare"),
    ("link-sets", "Link sets"),
    ("guides", "Guides"),
    ("word-study", "Word Study"),
    ("search", "Search"),
    ("notes-and-highlights", "Notes and Highlights"),
    ("get-resources", "Get Resources"),
    ("layouts", "Layouts"),
    ("keyboard", "Keyboard"),
];

/// One block of a page as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuideBlock {
    /// Lines of text, as a Note's are shown.
    Text(Vec<NoteBodyLine>),
    /// A screenshot: its file name in `docs/previews/` and its description.
    Image { file: String, alt: String },
}

/// The page with this id.
pub fn guide_page(id: &str) -> Option<&'static GuidePage> {
    USER_GUIDE.iter().find(|p| p.id == id)
}

/// The page a link target names (`link-sets.md`, `./search.md#scope`), if it is one.
pub fn page_for_link(target: &str) -> Option<&'static GuidePage> {
    let target = target.trim().trim_start_matches("./");
    let file = target.split('#').next()?;
    guide_page(file.strip_suffix(".md")?)
}

/// `![alt](path)` alone on a line: the image's file name and description.
fn image_line(line: &str) -> Option<(String, String)> {
    let rest = line.trim().strip_prefix("![")?;
    let (alt, rest) = rest.split_once("](")?;
    let path = rest.strip_suffix(')')?;
    let file = path.rsplit('/').next()?;
    (!file.is_empty() && !path.contains(char::is_whitespace))
        .then(|| (file.to_string(), alt.to_string()))
}

/// What `page` shows: its text (the Notes renderer's lines) between its screenshots.
pub fn guide_blocks(page: &GuidePage) -> Vec<GuideBlock> {
    let mut blocks = Vec::new();
    let mut text = String::new();
    let flush = |text: &mut String, blocks: &mut Vec<GuideBlock>| {
        let body = text.trim_matches('\n');
        if !body.is_empty() {
            blocks.push(GuideBlock::Text(note_body(body)));
        }
        text.clear();
    };
    for line in page.text.lines() {
        match image_line(line) {
            Some((file, alt)) => {
                flush(&mut text, &mut blocks);
                blocks.push(GuideBlock::Image { file, alt });
            }
            None => {
                text.push_str(line);
                text.push('\n');
            }
        }
    }
    flush(&mut text, &mut blocks);
    blocks
}

/// The screenshots `page` shows, by file name in `docs/previews/`.
pub fn guide_images(page: &GuidePage) -> Vec<String> {
    (page.text.lines())
        .filter_map(|l| image_line(l).map(|(file, _)| file))
        .collect()
}

/// A page that matches a search of the User guide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideMatch {
    pub page: &'static GuidePage,
    /// Up to three lines of the page (as shown) that hold a word of the search.
    pub lines: Vec<String>,
}

/// How many lines of a matching page a search shows.
const MATCH_LINES: usize = 3;

/// The pages whose title or text (as shown) holds every word of `query`, ignoring case: those
/// whose title holds one first, then by how often the words occur. An empty query matches
/// nothing.
pub fn search_user_guide(query: &str) -> Vec<GuideMatch> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(bool, usize, GuideMatch)> = Vec::new();
    for page in USER_GUIDE {
        let shown: Vec<String> = (guide_blocks(page).into_iter())
            .flat_map(|b| match b {
                GuideBlock::Text(lines) => lines.into_iter().map(|l| l.line.text).collect(),
                GuideBlock::Image { alt, .. } => vec![alt],
            })
            .filter(|l| !l.trim().is_empty())
            .collect();
        let all = format!("{}\n{}", page.title, shown.join("\n")).to_lowercase();
        if !words.iter().all(|w| all.contains(w.as_str())) {
            continue;
        }
        let title = page.title.to_lowercase();
        let in_title = words.iter().any(|w| title.contains(w.as_str()));
        let count = words.iter().map(|w| all.matches(w.as_str()).count()).sum();
        let lines = (shown.iter())
            // Not the page's own heading: the result shows the title already.
            .filter(|l| l.as_str() != page.title)
            .filter(|l| {
                let l = l.to_lowercase();
                words.iter().any(|w| l.contains(w.as_str()))
            })
            .take(MATCH_LINES)
            .cloned()
            .collect();
        found.push((in_title, count, GuideMatch { page, lines }));
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    found.into_iter().map(|(_, _, m)| m).collect()
}

/// Which page the User guide Tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserGuide {
    page: &'static GuidePage,
}

impl Default for UserGuide {
    fn default() -> Self {
        Self {
            page: &USER_GUIDE[0],
        }
    }
}

impl UserGuide {
    /// The page the Tab shows.
    pub fn page(&self) -> &'static GuidePage {
        self.page
    }
}

impl Session {
    pub fn user_guide(&self) -> &UserGuide {
        &self.user_guide
    }
}

// -------------------------------------------------------------------------------- commands

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// The page `p` names (`page`), if it names one; an error if it names none that exists.
fn named_page(
    command: &'static str,
    p: &Params<'_>,
) -> Result<Option<&'static GuidePage>, CommandError> {
    let Some(id) = p.str("page") else {
        return Ok(None);
    };
    match guide_page(id) {
        Some(page) => Ok(Some(page)),
        None => Err(failed(
            command,
            format!(
                "There is no User guide page “{id}”; the pages are {}",
                USER_GUIDE
                    .iter()
                    .map(|p| p.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

fn open(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "help.user_guide";
    if let Some(page) = named_page(ID, &p)? {
        s.user_guide.page = page;
    }
    let req = OpenRequest {
        panel: p
            .i64("panel")
            .and_then(|n| u64::try_from(n).ok())
            .map(PanelId),
        split: p.bool("split").unwrap_or(false),
    };
    let tab = crate::workspace_commands::open_tab(s, TabKind::UserGuide, req);
    Ok(json!({ "tab": tab.0, "page": s.user_guide.page.id }))
}

fn show_page(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "user_guide.page";
    let page = named_page(ID, &p)?.ok_or_else(|| failed(ID, "`page` is required"))?;
    s.user_guide.page = page;
    Ok(json!({ "page": page.id }))
}

const PAGE: ParamSpec = ParamSpec {
    name: "page",
    kind: ParamKind::String,
    required: false,
    help: "A page's id: getting-started, read-and-compare, link-sets, guides, word-study, search, notes-and-highlights, get-resources, layouts or keyboard",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "help.user_guide",
        label: "User guide",
        shortcut: Some("F1"),
        params: &[
            ParamSpec {
                name: "panel",
                kind: ParamKind::Integer,
                required: false,
                help: "Panel to open in",
            },
            ParamSpec {
                name: "split",
                kind: ParamKind::Boolean,
                required: false,
                help: "Open in a new Panel split off to the right",
            },
            PAGE,
        ],
        enabled: always,
        run: open,
    },
    CommandSpec {
        id: "user_guide.page",
        label: "Show a User guide page",
        shortcut: None,
        params: &[ParamSpec {
            required: true,
            ..PAGE
        }],
        enabled: always,
        run: show_page,
    },
];
