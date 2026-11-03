//! Workspace commands: opening Tabs, per-Tab navigation, layout, View and Help. Every menu
//! item, + entry and header button runs one of these through [`Session::run`].

use obs_scripture::{RefStyle, Standard, Verse, Versification, format_passage, parse_reference};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::workspace::{Effect, Guide, OpenRequest, PanelId, TabId, TabKind, Tool};

/// A launcher menu: its title and items, in order.
#[derive(Debug, Clone, Copy)]
pub struct Menu {
    pub title: &'static str,
    pub items: &'static [MenuItem],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    /// Run this command with no parameters.
    Command(&'static str),
    /// One `resource.open` item per library Resource, grouped by type.
    Resources,
    Separator,
}

use MenuItem::{Command as C, Resources, Separator};

/// The top menu bar (#18 rule 3). A Search field sits to the right of it.
pub const MENUS: &[Menu] = &[
    Menu {
        title: "Library",
        items: &[C("library.open"), C("library.import"), Separator, Resources],
    },
    Menu {
        title: "Guides",
        items: &[C("guide.passage_guide"), C("guide.word_study")],
    },
    Menu {
        title: "Tools",
        items: &[
            C("tool.search"),
            C("tool.text_comparison"),
            C("tool.notes"),
            C("tool.word_info"),
            C("tool.cross_references"),
        ],
    },
    Menu {
        title: "Layouts",
        items: &[C("layout.save"), Separator, C("layout.reset")],
    },
    Menu {
        title: "View",
        items: &[
            C("view.text_larger"),
            C("view.text_smaller"),
            C("view.verse_numbers"),
            Separator,
            C("view.dark_page"),
        ],
    },
    Menu {
        title: "Help",
        items: &[C("help.about"), C("help.credits"), C("app.commands")],
    },
];

const PANEL: ParamSpec = ParamSpec {
    name: "panel",
    kind: ParamKind::Integer,
    required: false,
    help: "Panel whose + was used",
};
const SPLIT: ParamSpec = ParamSpec {
    name: "split",
    kind: ParamKind::Boolean,
    required: false,
    help: "Open in a new Panel split off to the right (Ctrl/Cmd held)",
};
const TAB: ParamSpec = ParamSpec {
    name: "tab",
    kind: ParamKind::Integer,
    required: true,
    help: "The Tab",
};
const OPEN: &[ParamSpec] = &[PANEL, SPLIT];

fn never(_: &Session) -> bool {
    false
}

const fn open_cmd(
    id: &'static str,
    label: &'static str,
    shortcut: Option<&'static str>,
    run: fn(&mut Session, Params<'_>) -> Result<Value, CommandError>,
) -> CommandSpec {
    CommandSpec {
        id,
        label,
        shortcut,
        params: OPEN,
        enabled: always,
        run,
    }
}

const fn simple(
    id: &'static str,
    label: &'static str,
    shortcut: Option<&'static str>,
    enabled: fn(&Session) -> bool,
    run: fn(&mut Session, Params<'_>) -> Result<Value, CommandError>,
) -> CommandSpec {
    CommandSpec {
        id,
        label,
        shortcut,
        params: &[],
        enabled,
        run,
    }
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    open_cmd("library.open", "Library", Some("Ctrl+Shift+L"), |s, p| {
        open(s, p, TabKind::Library)
    }),
    simple("library.import", "Import…", None, never, |_, _| {
        Ok(Value::Null)
    }),
    CommandSpec {
        id: "resource.open",
        label: "Open Resource",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "id",
                kind: ParamKind::String,
                required: true,
                help: "Resource id from the library",
            },
            PANEL,
            SPLIT,
        ],
        enabled: always,
        run: open_resource,
    },
    open_cmd("guide.passage_guide", "Passage Guide", None, |s, p| {
        open(s, p, TabKind::Guide(Guide::PassageGuide))
    }),
    open_cmd("guide.word_study", "Word Study", None, |s, p| {
        open(s, p, TabKind::Guide(Guide::WordStudy))
    }),
    open_cmd("tool.search", "Search", Some("Ctrl+Shift+F"), |s, p| {
        open(s, p, TabKind::Tool(Tool::Search))
    }),
    open_cmd("tool.text_comparison", "Text Comparison", None, |s, p| {
        open(s, p, TabKind::Tool(Tool::TextComparison))
    }),
    open_cmd("tool.notes", "Notes", None, |s, p| {
        open(s, p, TabKind::Tool(Tool::Notes))
    }),
    open_cmd(
        "tool.word_info",
        "Word Info",
        Some("Ctrl+Shift+I"),
        |s, p| open(s, p, TabKind::Tool(Tool::WordInfo)),
    ),
    open_cmd("tool.cross_references", "Cross-references", None, |s, p| {
        open(s, p, TabKind::Tool(Tool::CrossReferences))
    }),
    simple("layout.save", "Save layout…", None, never, |_, _| {
        Ok(Value::Null)
    }),
    simple("layout.reset", "Reset layout", None, always, |s, _| {
        s.workspace.push_effect(Effect::ResetLayout);
        Ok(Value::Null)
    }),
    simple(
        "view.text_larger",
        "Larger text",
        Some("Ctrl+Plus"),
        always,
        |s, _| {
            s.text.size = (s.text.size + 1.0).min(40.0);
            Ok(json!({ "size": s.text.size }))
        },
    ),
    simple(
        "view.text_smaller",
        "Smaller text",
        Some("Ctrl+Minus"),
        always,
        |s, _| {
            s.text.size = (s.text.size - 1.0).max(10.0);
            Ok(json!({ "size": s.text.size }))
        },
    ),
    simple(
        "view.verse_numbers",
        "Verse numbers",
        None,
        always,
        |s, _| {
            s.text.verse_numbers = !s.text.verse_numbers;
            Ok(json!({ "on": s.text.verse_numbers }))
        },
    ),
    simple("view.dark_page", "Dark page", None, always, |s, _| {
        s.dark_page = !s.dark_page;
        Ok(json!({ "on": s.dark_page }))
    }),
    simple("help.about", "About Open Bible", None, always, about),
    simple("help.credits", "Credits", None, always, credits),
    CommandSpec {
        id: "tab.go",
        label: "Go to reference",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "text",
                kind: ParamKind::String,
                required: true,
                help: "Reference, read in the Tab's own numbering (`Jn 3:16`, `2:4`, `5`)",
            },
        ],
        enabled: always,
        run: go,
    },
    CommandSpec {
        id: "tab.back",
        label: "Back",
        shortcut: Some("Alt+Left"),
        params: &[TAB],
        enabled: always,
        run: |s, p| history(s, p, true),
    },
    CommandSpec {
        id: "tab.forward",
        label: "Forward",
        shortcut: Some("Alt+Right"),
        params: &[TAB],
        enabled: always,
        run: |s, p| history(s, p, false),
    },
    CommandSpec {
        id: "tab.close",
        label: "Close Tab",
        shortcut: Some("Ctrl+W"),
        params: &[TAB],
        enabled: always,
        run: |s, p| {
            let tab = tab_param("tab.close", s, &p)?;
            Ok(json!({ "closed": s.workspace.close(tab) }))
        },
    },
    CommandSpec {
        id: "tab.link",
        label: "Link set",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "set",
                kind: ParamKind::String,
                required: true,
                help: "Link set letter (A, B, C, D), or empty for none",
            },
        ],
        enabled: always,
        run: link,
    },
    CommandSpec {
        id: "search.run",
        label: "Search",
        shortcut: None,
        params: &[ParamSpec {
            name: "text",
            kind: ParamKind::String,
            required: true,
            help: "What to search for",
        }],
        enabled: always,
        run: search,
    },
];

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn request(p: &Params<'_>) -> OpenRequest {
    OpenRequest {
        panel: p
            .i64("panel")
            .and_then(|n| u64::try_from(n).ok())
            .map(PanelId),
        split: p.bool("split").unwrap_or(false),
    }
}

fn title_of(s: &Session, kind: &TabKind) -> String {
    match kind {
        TabKind::Library => "Library".into(),
        TabKind::Resource(id) => s
            .library
            .get(id)
            .map_or_else(|| id.clone(), |e| e.manifest.abbreviation.clone()),
        TabKind::Guide(g) => g.title().into(),
        TabKind::Tool(t) => t.title().into(),
    }
}

fn open(s: &mut Session, p: Params<'_>, kind: TabKind) -> Result<Value, CommandError> {
    let title = title_of(s, &kind);
    let tab = s.workspace.open(kind, title, request(&p));
    Ok(json!({ "tab": tab.0 }))
}

fn open_resource(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let id = p.str("id").unwrap_or_default();
    if s.library.get(id).is_none() {
        return Err(failed(
            "resource.open",
            format!("{id} is not in the library"),
        ));
    }
    open(s, p, TabKind::Resource(id.to_string()))
}

fn tab_param(command: &'static str, s: &Session, p: &Params<'_>) -> Result<TabId, CommandError> {
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .ok_or_else(|| failed(command, "no tab"))?;
    if s.workspace.tab(tab).is_none() {
        return Err(failed(command, format!("no Tab {}", tab.0)));
    }
    Ok(tab)
}

fn go(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "tab.go";
    let tab = tab_param(ID, s, &p)?;
    let text = p.str("text").unwrap_or_default();
    let Some(t) = s.workspace.tab(tab) else {
        return Err(failed(ID, "no tab"));
    };
    let context = t.history.current();
    let (start, own) = match &t.versification {
        Some(v) => {
            let list = parse_reference(text, v.as_ref(), context)
                .map_err(|e| failed(ID, e.kind.to_string()))?;
            let first = list.passages().first().copied();
            let start = first.map(|p| p.start());
            if let Some(v0) = start
                && !v.contains(v0)
            {
                return Err(failed(ID, format!("{v0} is not in this Bible")));
            }
            (
                start,
                first.map(|p| format_passage(p, v.as_ref(), RefStyle::Long)),
            )
        }
        None => {
            let list = parse_reference(text, &Standard, context)
                .map_err(|e| failed(ID, e.kind.to_string()))?;
            let first = list.passages().first().copied();
            (
                first.map(|p| p.start()),
                first.map(|p| format_passage(p, &Standard, RefStyle::Long)),
            )
        }
    };
    let Some(verse) = start else {
        return Err(failed(ID, "no reference"));
    };
    s.workspace.navigate(tab, verse);
    Ok(json!({ "tab": tab.0, "verse": verse.osis(), "text": own }))
}

fn history(s: &mut Session, p: Params<'_>, back: bool) -> Result<Value, CommandError> {
    let id = if back { "tab.back" } else { "tab.forward" };
    let tab = tab_param(id, s, &p)?;
    let to: Option<Verse> = if back {
        s.workspace.back(tab)
    } else {
        s.workspace.forward(tab)
    };
    Ok(json!({ "verse": to.map(|v| v.osis()) }))
}

fn link(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "tab.link";
    let tab = tab_param(ID, s, &p)?;
    let set = p.str("set").unwrap_or_default().trim().to_uppercase();
    let link = match set.chars().next() {
        None => None,
        Some(c) if set.len() == 1 && crate::link::LINK_SETS.contains(&c) => Some(c),
        Some(_) => return Err(failed(ID, format!("no Link set {set}"))),
    };
    let has_chip = s.workspace.tab(tab).is_some_and(|t| t.kind.has_link_chip());
    if !has_chip && link.is_some() {
        return Err(failed(ID, "this Tab cannot be linked"));
    }
    s.workspace.set_link(tab, link);
    Ok(json!({ "tab": tab.0, "set": link.map(String::from) }))
}

fn search(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let text = p.str("text").unwrap_or_default().to_string();
    let title = Tool::Search.title().to_string();
    let tab = s
        .workspace
        .open(TabKind::Tool(Tool::Search), title, OpenRequest::default());
    s.workspace.push_effect(Effect::Message(format!(
        "Search for “{text}” arrives with the Search Tool (#59)"
    )));
    Ok(json!({ "tab": tab.0 }))
}

fn about(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    let body = format!(
        "Open Bible Software {}\n\nA Bible study app for open texts.\nLicensed MIT OR Apache-2.0.",
        env!("CARGO_PKG_VERSION")
    );
    s.workspace.push_effect(Effect::Show {
        title: "About Open Bible".into(),
        body,
    });
    Ok(Value::Null)
}

/// App-level attribution for data built into the app (#18 constraint for #20).
pub const CREDITS: &str = "\
Gentium Plus 6.200, © SIL International, SIL Open Font License 1.1.
Ezra SIL 2.51, © SIL International, SIL Open Font License 1.1.
TVTMS (Translators Versification Traditions with Methodology for Standardisation), \
STEP Bible (www.STEPBible.org), CC BY 4.0; reformatted for verse maps.

Each Resource's own Attribution is shown in its About view.";

fn credits(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    s.workspace.push_effect(Effect::Show {
        title: "Credits".into(),
        body: CREDITS.into(),
    });
    Ok(Value::Null)
}
