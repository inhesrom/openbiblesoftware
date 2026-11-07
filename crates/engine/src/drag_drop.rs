//! Dropping a Bible reference or a word on the Workspace (#216): `workspace.drop`.
//!
//! The UI drags; this decides what a drop does, so the command box, scripts and tests can
//! drop too. Where it lands is one of the Drop targets (CONTEXT.md):
//!
//! - **a Tab** (its text): a Tab that follows Passages goes to a dropped Passage, as a jump its
//!   Link set follows; a Lexicon, Dictionary, Word Study, Factbook or Topic Guide looks a
//!   dropped word up (as typed into its box); a Search Tab searches for it, a dragged reference
//!   as written. A Tab that can do neither takes the drop as its Panel's tab bar does, as a
//!   lookup Tab takes a dragged reference;
//! - **a Panel's tab bar**: a new Tab in that Panel;
//! - **a Panel's right edge**: a new Panel beside it;
//! - **nowhere in particular** (an empty Workspace): a new Tab where Tabs of its kind go.
//!
//! A new Tab for a Passage is a Bible (the one dragged from, else the ★ Bible); for a word
//! that is not a Bible reference, a Search for it. Dropped text that is a Bible reference is
//! read in the numbering of where it was written (CONTEXT.md, Bible reference): the Bible's
//! own when it was dragged out of a Bible Tab (`from`), translated through its Verse map.

use obs_model::ResourceType;
use obs_scripture::{
    Passage, RefStyle, Standard, Verse, VerseMap, format_passage, parse_reference,
};
use serde_json::{Value, json};

use crate::bible_search::SearchRequestFields;
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::link::{ToStandard, to_standard};
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
            help: "A Bible reference in the Standard numbering, e.g. `Rom 8:28` (a dragged reference); a Search Tab searches for `text`, when given, instead",
        },
        ParamSpec {
            name: "text",
            kind: ParamKind::String,
            required: false,
            help: "Dragged text, e.g. a selected word or a reference as written; a Bible reference with a chapter is read as one",
        },
        ParamSpec {
            name: "bible",
            kind: ParamKind::String,
            required: false,
            help: "The Bible a new Tab for a Passage shows (default: the first Bible in Preferred order)",
        },
        ParamSpec {
            name: "from",
            kind: ParamKind::Integer,
            required: false,
            help: "The Tab `text` was dragged from: a Bible reference in a Bible's text is read in its numbering",
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
    let text = p.str("text").map(str::trim).unwrap_or_default();
    // A Search Tab searches for a dropped reference as written (`text`), else as `passage`.
    if let DropAt::Tab(tab) = at
        && s.workspace
            .tab(tab)
            .is_some_and(|t| t.kind == TabKind::Tool(Tool::Search))
        && let Some(typed) = Some(text)
            .filter(|t| !t.is_empty())
            .or_else(|| p.str("passage").map(str::trim))
    {
        return drop_text(s, typed, number("from").map(TabId), bible, at);
    }
    if let Some(text) = p.str("passage") {
        let passage = parse_reference(text, &Standard, None)
            .map_err(|e| failed(e.kind.to_string()))?
            .passages()
            .first()
            .copied()
            .ok_or_else(|| failed("no reference"))?;
        return drop_passage(s, passage, bible, at);
    }
    if text.is_empty() {
        return Err(failed("Nothing was dropped"));
    }
    drop_text(s, text, number("from").map(TabId), bible, at)
}

fn live_panel(s: &Session, n: u64) -> Result<PanelId, CommandError> {
    let panel = PanelId(n);
    if s.workspace.panels().any(|(p, _)| p == panel) {
        Ok(panel)
    } else {
        Err(failed(format!("no Panel {n}")))
    }
}

/// `text` as a Standard Passage when it is a Bible reference with a chapter or verse number
/// ("Rom 8:28", "Ps 23"). A bare book name ("Job", "Mark") is a word: dragged words are names
/// more often than whole books. Text dragged out of a Bible Tab (`from`) is read in that
/// Bible's numbering and translated through its Verse map; a reference to an Unmapped verse
/// fails, as it has nowhere to go.
fn as_passage(
    s: &Session,
    text: &str,
    from: Option<TabId>,
) -> Result<Option<Passage>, CommandError> {
    if !text.chars().any(|c| c.is_ascii_digit()) {
        return Ok(None);
    }
    let bible = from
        .and_then(|t| s.workspace.tab(t))
        .filter(|t| match &t.kind {
            TabKind::Resource(id) => s
                .library
                .get(id)
                .is_some_and(|e| e.manifest.resource_type == ResourceType::Bible),
            _ => false,
        });
    let own = bible.and_then(|t| t.versification.as_deref());
    let list = match own {
        Some(v) => parse_reference(text, v, None),
        None => parse_reference(text, &Standard, None),
    };
    let Some(native) = list.ok().and_then(|l| l.passages().first().copied()) else {
        return Ok(None);
    };
    match bible.and_then(|t| t.verse_map.as_deref()) {
        None => Ok(Some(native)),
        Some(map) => standard_passage(map, native)
            .map(Some)
            .ok_or_else(|| failed(format!("“{text}” has no Standard equivalent"))),
    }
}

/// A Passage in a Bible's own numbering → the Standard Passage covering every Verse it
/// maps to, from the earliest to the latest. A map may reorder Verses (a Bible's 1:16 the
/// Standard 1:17 and its 1:17 the Standard 1:16), so the ends are not simply where its first
/// and last Verses map. Unmapped Verses inside it add nothing; Verses outside the map count
/// as they are. `None` when its first Verse is Unmapped.
fn standard_passage(map: &VerseMap, native: Passage) -> Option<Passage> {
    let ToStandard::Verse(_) = to_standard(Some(map), native.start()) else {
        return None;
    };
    let covered = map
        .entries()
        .iter()
        .filter(|e| {
            let last = Verse::new(e.verse.book, e.verse.chapter, e.last);
            e.verse <= native.end() && native.start() <= last
        })
        .filter_map(|e| e.standard);
    let outside = [native.start(), native.end()]
        .into_iter()
        .filter(|&v| map.entry(v).is_none())
        .map(Passage::verse);
    let all = || covered.clone().chain(outside.clone());
    let start = all().map(|p| p.start()).min()?;
    let end = all().map(|p| p.end()).max()?;
    Passage::new(start, end).ok()
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

/// Dropped `text`, dragged out of Tab `from`. A Tab that looks words up or searches takes the
/// text as typed; only elsewhere is it read as a Passage (`as_passage`), so a reference with no
/// Standard equivalent can still be searched for.
fn drop_text(
    s: &mut Session,
    text: &str,
    from: Option<TabId>,
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
    }
    let passage = as_passage(s, text, from)?;
    if let DropAt::Tab(tab) = at
        && s.workspace.tab(tab).map(|t| t.header_box()) == Some(HeaderBox::Reference)
        && follows_passages(s, tab)
    {
        let passage =
            passage.ok_or_else(|| failed(format!("“{text}” is not a Bible reference")))?;
        return drop_passage(s, passage, bible, at);
    }
    if let Some(passage) = passage {
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
