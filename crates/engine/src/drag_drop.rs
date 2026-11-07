//! Dropping a Bible reference or a word on the Workspace (#216): `workspace.drop`.
//!
//! The UI drags; this decides what a drop does, so the command box, scripts and tests can
//! drop too. Where it lands is one of the Drop targets (CONTEXT.md):
//!
//! - **a Tab** (its text): a Tab that follows Passages goes to a dropped Passage, as a jump its
//!   Link set follows; a Lexicon, Dictionary, Word Study, Factbook or Topic Guide looks a
//!   dropped word up (as typed into its box); a Search Tab searches for it. A Tab that can do
//!   neither takes the drop as its Panel's tab bar does;
//! - **a Panel's tab bar**: a new Tab in that Panel;
//! - **a Panel's right edge**: a new Panel beside it;
//! - **nowhere in particular** (an empty Workspace): a new Tab where Tabs of its kind go.
//!
//! A new Tab for a Passage is a Bible (the one dragged from, else the ★ Bible); for a word
//! that is not a Bible reference, a Search for it.

use obs_model::ResourceType;
use obs_scripture::{Passage, RefStyle, Standard, format_passage, parse_reference};
use serde_json::{Value, json};

use crate::bible_search::SearchRequestFields;
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::workspace::{Effect, HeaderBox, OpenRequest, PanelId, TabId, TabKind, Tool};

const ID: &str = "workspace.drop";

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: ID,
    label: "Drop on the Workspace",
    shortcut: None,
    params: &[
        ParamSpec {
            name: "passage",
            kind: ParamKind::String,
            required: false,
            help: "A Bible reference in the Standard numbering, e.g. `Rom 8:28` (a dragged reference)",
        },
        ParamSpec {
            name: "text",
            kind: ParamKind::String,
            required: false,
            help: "Dragged text, e.g. a selected word; a Bible reference with a chapter is read as one",
        },
        ParamSpec {
            name: "bible",
            kind: ParamKind::String,
            required: false,
            help: "The Bible a new Tab for a Passage shows (default: the ★ Bible)",
        },
        ParamSpec {
            name: "tab",
            kind: ParamKind::Integer,
            required: false,
            help: "Dropped on this Tab's text",
        },
        ParamSpec {
            name: "panel",
            kind: ParamKind::Integer,
            required: false,
            help: "Dropped on this Panel's tab bar: a new Tab in it",
        },
        ParamSpec {
            name: "beside",
            kind: ParamKind::Integer,
            required: false,
            help: "Dropped on this Panel's right edge: a new Panel beside it",
        },
    ],
    enabled: always,
    run: run_drop,
}];

/// Where a drop landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropAt {
    Tab(TabId),
    Panel(PanelId),
    Beside(PanelId),
    Anywhere,
}

fn failed(message: impl ToString) -> CommandError {
    CommandError::Failed {
        command: ID,
        message: message.to_string(),
    }
}

fn run_drop(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let number = |name: &str| p.i64(name).and_then(|n| u64::try_from(n).ok());
    let at = match (number("tab"), number("panel"), number("beside")) {
        (Some(t), _, _) => {
            let tab = TabId(t);
            if s.workspace.tab(tab).is_none() {
                return Err(failed(format!("no Tab {t}")));
            }
            DropAt::Tab(tab)
        }
        (None, Some(n), _) => DropAt::Panel(live_panel(s, n)?),
        (None, None, Some(n)) => DropAt::Beside(live_panel(s, n)?),
        (None, None, None) => DropAt::Anywhere,
    };
    let bible = p.str("bible");
    if let Some(text) = p.str("passage") {
        let passage = parse_reference(text, &Standard, None)
            .map_err(|e| failed(e.kind.to_string()))?
            .passages()
            .first()
            .copied()
            .ok_or_else(|| failed("no reference"))?;
        return drop_passage(s, passage, bible, at);
    }
    let text = p.str("text").map(str::trim).unwrap_or_default();
    if text.is_empty() {
        return Err(failed("Nothing was dropped"));
    }
    drop_text(s, text, bible, at)
}

fn live_panel(s: &Session, n: u64) -> Result<PanelId, CommandError> {
    let panel = PanelId(n);
    if s.workspace.panels().any(|(p, _)| p == panel) {
        Ok(panel)
    } else {
        Err(failed(format!("no Panel {n}")))
    }
}

/// `text` as a Passage when it is a Bible reference with a chapter or verse number ("Rom
/// 8:28", "Ps 23"). A bare book name ("Job", "Mark") is a word: dragged words are names more
/// often than whole books.
fn as_passage(text: &str) -> Option<Passage> {
    if !text.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let list = parse_reference(text, &Standard, None).ok()?;
    list.passages().first().copied()
}

fn drop_passage(
    s: &mut Session,
    passage: Passage,
    bible: Option<&str>,
    at: DropAt,
) -> Result<Value, CommandError> {
    let verse = passage.start();
    match at {
        DropAt::Tab(tab) if follows_passages(s, tab) => {
            s.workspace.focus(tab);
            s.workspace.navigate_standard(tab, verse);
            // A Guide or Tool shows a dropped chapter or range whole, as when it is typed.
            if let Some(t) = s.workspace.tab_mut(tab)
                && t.versification.is_none()
                && t.history.current() == Some(verse)
            {
                t.passage = Some(passage);
            }
            Ok(json!({ "tab": tab.0, "verse": verse.osis(), "opened": false }))
        }
        DropAt::Tab(tab) => {
            let panel = s.workspace.panel_of(tab);
            open_bible(s, passage, bible, panel, false)
        }
        DropAt::Panel(panel) => open_bible(s, passage, bible, Some(panel), false),
        DropAt::Beside(panel) => {
            focus_panel(s, panel);
            open_bible(s, passage, bible, None, true)
        }
        DropAt::Anywhere => open_bible(s, passage, bible, None, false),
    }
}

fn drop_text(
    s: &mut Session,
    text: &str,
    bible: Option<&str>,
    at: DropAt,
) -> Result<Value, CommandError> {
    if let DropAt::Tab(tab) = at {
        let Some(t) = s.workspace.tab(tab) else {
            return Err(failed(format!("no Tab {}", tab.0)));
        };
        if t.keeps_entries() {
            s.workspace.focus(tab);
            return s.run("tab.go", &json!({ "tab": tab.0, "text": text }));
        }
        if t.kind == TabKind::Tool(Tool::Search) {
            s.workspace.focus(tab);
            return s.run("search.run", &json!({ "tab": tab.0, "text": text }));
        }
        if t.header_box() == HeaderBox::Reference && follows_passages(s, tab) {
            let passage = as_passage(text)
                .ok_or_else(|| failed(format!("“{text}” is not a Bible reference")))?;
            return drop_passage(s, passage, bible, at);
        }
    }
    if let Some(passage) = as_passage(text) {
        return drop_passage(s, passage, bible, at);
    }
    // A word on a Panel: search for it there.
    let req = match at {
        DropAt::Tab(tab) => OpenRequest {
            panel: s.workspace.panel_of(tab),
            split: false,
        },
        DropAt::Panel(panel) => OpenRequest {
            panel: Some(panel),
            split: false,
        },
        DropAt::Beside(panel) => {
            focus_panel(s, panel);
            OpenRequest {
                panel: None,
                split: true,
            }
        }
        DropAt::Anywhere => OpenRequest::default(),
    };
    let kind = TabKind::Tool(Tool::Search);
    let tab = s.workspace.open(kind, Tool::Search.title().into(), req);
    // A query the Search Tab cannot run says why in the Tab, as when it is typed there.
    let error = s.run_search(
        tab,
        SearchRequestFields {
            query: text.to_string(),
            ..Default::default()
        },
    );
    Ok(json!({
        "tab": tab.0,
        "opened": true,
        "error": error.map(|e| e.message),
    }))
}

fn follows_passages(s: &Session, tab: TabId) -> bool {
    s.workspace
        .tab(tab)
        .is_some_and(|t| t.key_schemes.contains(&obs_model::KeyScheme::Passage))
}

/// The Panel's front Tab takes focus, so a split opens beside that Panel.
fn focus_panel(s: &mut Session, panel: PanelId) {
    if let Some(t) = s.workspace.active_in(panel) {
        s.workspace.focus(t);
    }
}

/// A new Tab of `bible` (else the ★ Bible) at `passage`: in `panel`, or split off beside the
/// focused Panel.
fn open_bible(
    s: &mut Session,
    passage: Passage,
    bible: Option<&str>,
    panel: Option<PanelId>,
    split: bool,
) -> Result<Value, CommandError> {
    let is_bible = |id: &&str| {
        s.library
            .get(id)
            .is_some_and(|e| e.manifest.resource_type == ResourceType::Bible)
    };
    let id = bible
        .filter(is_bible)
        .map(str::to_string)
        .or_else(|| {
            s.library
                .preferred(ResourceType::Bible)
                .map(|e| e.manifest.resource_id.clone())
        })
        .ok_or_else(|| failed("Add a Bible to your library to open this Passage"))?;
    let tab = crate::workspace_commands::open_tab(
        s,
        TabKind::Resource(id.clone()),
        OpenRequest { panel, split },
    );
    // The new Tab's Verse map is known once the UI has loaded it.
    s.workspace.push_effect(Effect::NavigateStandard {
        tab,
        verse: passage.start(),
    });
    Ok(json!({
        "tab": tab.0,
        "verse": passage.start().osis(),
        "passage": format_passage(passage, &Standard, RefStyle::Long),
        "bible": id,
        "opened": true,
    }))
}
