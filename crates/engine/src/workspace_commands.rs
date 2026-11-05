//! Workspace commands: opening Tabs, per-Tab navigation, layout, View and Help. Every menu
//! item, + entry and header button runs one of these through [`Session::run`].

use obs_scripture::{RefStyle, Standard, Verse, Versification, format_passage, parse_reference};
use serde_json::{Value, json};

use crate::appearance::{TEXT_SIZE_MAX, TEXT_SIZE_MIN, TEXT_SIZE_STEP};
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
    /// The saved Layouts, each with Restore, Rename… and Delete.
    Layouts,
    /// "Notebooks in text": a submenu with each Notebook's Show in text toggle
    /// (`notebook.show_in_text`).
    Notebooks,
    /// A submenu of exclusive choices: one `command` run with `{ "value": id }` per choice,
    /// the current one checked.
    Choice {
        title: &'static str,
        command: &'static str,
    },
    Separator,
}

use MenuItem::{Command as C, Layouts, Resources, Separator};

/// The top menu bar (#18 rule 3). A Search field sits to the right of it.
pub const MENUS: &[Menu] = &[
    Menu {
        title: "Library",
        items: &[
            C("library.open"),
            C("library.import"),
            C("library.get_resources"),
            C("library.get_bibles"),
            Separator,
            Resources,
        ],
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
        items: &[
            C("layout.save"),
            Separator,
            Layouts,
            Separator,
            C("layout.reset"),
        ],
    },
    Menu {
        title: "View",
        items: &[
            MenuItem::Choice {
                title: "Theme",
                command: "view.theme",
            },
            MenuItem::Choice {
                title: "Page tone",
                command: "view.page_tone",
            },
            MenuItem::Choice {
                title: "Interface size",
                command: "view.interface_size",
            },
            Separator,
            C("view.text_larger"),
            C("view.text_smaller"),
            C("view.text_reset"),
            MenuItem::Choice {
                title: "Line spacing",
                command: "view.line_spacing",
            },
            MenuItem::Choice {
                title: "Verse numbers",
                command: "view.verse_numbers",
            },
            C("view.red_letter"),
            Separator,
            C("view.highlights"),
            MenuItem::Notebooks,
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
const NAME: ParamSpec = ParamSpec {
    name: "name",
    kind: ParamKind::String,
    required: true,
    help: "Layout name",
};

fn has_user(s: &Session) -> bool {
    s.user.is_some()
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

const VALUE: &[ParamSpec] = &[ParamSpec {
    name: "value",
    kind: ParamKind::String,
    required: true,
    help: "The choice's id (see `choices`)",
}];

/// A command that sets one View choice by id (see [`choices`]).
const fn choice(id: &'static str, label: &'static str) -> CommandSpec {
    CommandSpec {
        id,
        label,
        shortcut: None,
        params: VALUE,
        enabled: always,
        run: run_choice,
    }
}

/// The choices of a `Choice` menu command: (id, label), and the current id.
pub fn choices(s: &Session, command: &str) -> (Vec<(&'static str, &'static str)>, &'static str) {
    use crate::appearance::*;
    match command {
        "view.theme" => (
            Theme::ALL.iter().map(|t| (t.1, t.2)).collect(),
            s.appearance.theme.id(),
        ),
        "view.page_tone" => (
            PageTone::ALL.iter().map(|t| (t.1, t.2)).collect(),
            s.appearance.page_tone.id(),
        ),
        "view.interface_size" => (
            InterfaceSize::ALL.iter().map(|t| (t.1, t.2)).collect(),
            s.appearance.interface_size.id(),
        ),
        "view.line_spacing" => (
            LINE_SPACINGS.iter().map(|c| (c.0, c.1)).collect(),
            line_spacing_id(s.text.line_spacing),
        ),
        "view.verse_numbers" => (
            VERSE_NUMBER_STYLES.iter().map(|c| (c.1, c.2)).collect(),
            verse_numbers_id(s.text.verse_numbers),
        ),
        _ => (Vec::new(), ""),
    }
}

fn run_choice(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    use crate::appearance::*;
    let v = p.str("value").unwrap_or_default();
    // Command functions are not told their own id; the Session logs it just before running.
    let id = s.last_command().unwrap_or("");
    let ok = match id {
        "view.theme" => Theme::from_id(v).map(|t| s.appearance.theme = t),
        "view.page_tone" => PageTone::from_id(v).map(|t| s.appearance.page_tone = t),
        "view.interface_size" => InterfaceSize::from_id(v).map(|t| s.appearance.interface_size = t),
        "view.line_spacing" => LINE_SPACINGS
            .iter()
            .find(|c| c.0 == v)
            .map(|c| s.text.line_spacing = c.2),
        "view.verse_numbers" => VERSE_NUMBER_STYLES
            .iter()
            .find(|c| c.1 == v)
            .map(|c| s.text.verse_numbers = c.0),
        _ => None,
    };
    let Some(()) = ok else {
        return Err(failed("view", format!("“{v}” is not a choice for {id}")));
    };
    view_saved(s, json!({ "value": v }))
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
    CommandSpec {
        id: "library.get_resources",
        label: "Get Resources…",
        shortcut: None,
        params: &[PANEL, SPLIT, crate::get_bibles::TYPE],
        enabled: always,
        run: |s, p| {
            let ty = crate::get_bibles::type_filter_from_id(p.str("type").unwrap_or_default())
                .map_err(|message| CommandError::Failed {
                    command: "library.get_resources",
                    message,
                })?;
            open_get_resources(s, p, ty)
        },
    },
    CommandSpec {
        id: "library.get_bibles",
        label: "Get Bibles…",
        shortcut: None,
        params: OPEN,
        enabled: always,
        run: |s, p| open_get_resources(s, p, Some(obs_model::ResourceType::Bible)),
    },
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
    open_cmd(
        "guide.passage_guide",
        "Passage Guide",
        None,
        open_passage_guide,
    ),
    CommandSpec {
        id: "guide.word_study",
        label: "Word Study",
        shortcut: None,
        params: &[
            PANEL,
            SPLIT,
            ParamSpec {
                name: "strongs",
                kind: ParamKind::String,
                required: false,
                help: "Strong's number to study, e.g. `G25` (Key words, #54)",
            },
            ParamSpec {
                name: "tab",
                kind: ParamKind::Integer,
                required: false,
                help: "An open Word Study Tab to move (and focus) instead of opening a new one",
            },
        ],
        enabled: always,
        run: open_word_study,
    },
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
    // Word Info (#50): the pinned panel follows hover again on Esc or the header's 📌; Copy
    // hands the UI the card text; Word Study › opens the shown word's Strong's number in a
    // Word Study (#55); Search this word searches its Bible for the word's Strong's number (#59).
    simple(
        "word_info.unpin",
        "Unpin Word Info",
        Some("Escape"),
        |s| s.word_info.pinned,
        |s, _| {
            s.word_info.unpin();
            Ok(Value::Null)
        },
    ),
    simple(
        "word_info.copy",
        "Copy",
        None,
        |s| s.word_info.shown.is_some(),
        |s, _| {
            let text = s
                .word_info
                .shown
                .as_ref()
                .map(crate::word_info::ShownWord::copy_text)
                .ok_or_else(|| failed("word_info.copy", "nothing is shown"))?;
            Ok(json!({ "text": text }))
        },
    ),
    CommandSpec {
        id: "word_info.word_study",
        label: "Word Study ›",
        shortcut: None,
        params: OPEN,
        enabled: |s| crate::word_study::word_info_number(s).is_some(),
        run: |s, p| {
            let number = crate::word_study::word_info_number(s)
                .ok_or_else(|| failed("word_info.word_study", "nothing is shown"))?;
            let tab = crate::word_study::open_at(s, number, request(&p), false);
            Ok(json!({ "tab": tab.0, "key": number.to_string() }))
        },
    },
    simple(
        "word_info.search",
        "Search this word",
        None,
        |s| s.word_info.shown.is_some(),
        word_info_search,
    ),
    CommandSpec {
        id: "layout.save",
        label: "Save layout as…",
        shortcut: None,
        params: &[ParamSpec {
            name: "name",
            kind: ParamKind::String,
            required: false,
            help: "Layout name (asks when missing); an existing Layout of that name is replaced",
        }],
        enabled: has_user,
        run: layout_save,
    },
    CommandSpec {
        id: "layout.restore",
        label: "Restore layout",
        shortcut: None,
        params: &[NAME],
        enabled: has_user,
        run: layout_restore,
    },
    CommandSpec {
        id: "layout.rename",
        label: "Rename layout…",
        shortcut: None,
        params: &[
            NAME,
            ParamSpec {
                name: "to",
                kind: ParamKind::String,
                required: false,
                help: "New name (asks when missing)",
            },
        ],
        enabled: has_user,
        run: layout_rename,
    },
    CommandSpec {
        id: "layout.delete",
        label: "Delete layout",
        shortcut: None,
        params: &[NAME],
        enabled: has_user,
        run: layout_delete,
    },
    simple("layout.reset", "Reset layout", None, always, |s, _| {
        s.workspace.push_effect(Effect::ResetLayout);
        Ok(Value::Null)
    }),
    // The keys also match Ctrl+= and Ctrl+Shift+= (the + key unshifted and shifted) and the
    // numpad's + and −; egui's own whole-window zoom keys are off (#137).
    simple(
        "view.text_larger",
        "Larger text",
        Some("Ctrl+Plus"),
        always,
        |s, _| set_text_size(s, s.text.size + TEXT_SIZE_STEP),
    ),
    simple(
        "view.text_smaller",
        "Smaller text",
        Some("Ctrl+Minus"),
        always,
        |s, _| set_text_size(s, s.text.size - TEXT_SIZE_STEP),
    ),
    simple(
        "view.text_reset",
        "Reset text size",
        Some("Ctrl+0"),
        always,
        |s, _| set_text_size(s, obs_layout::TextSettings::default().size),
    ),
    choice("view.theme", "Theme"),
    choice("view.page_tone", "Page tone"),
    choice("view.interface_size", "Interface size"),
    choice("view.line_spacing", "Line spacing"),
    choice("view.verse_numbers", "Verse numbers"),
    simple("view.red_letter", "Red letter", None, always, |s, _| {
        s.text.red_letter = !s.text.red_letter;
        view_saved(s, json!({ "on": s.text.red_letter }))
    }),
    simple(
        "view.highlights",
        "Show Highlights",
        None,
        always,
        |s, _| {
            s.appearance.show_highlights = !s.appearance.show_highlights;
            // What is drawn in text changed: views reload their Annotations.
            s.annotations_revision += 1;
            view_saved(s, json!({ "on": s.appearance.show_highlights }))
        },
    ),
    simple("help.about", "About Open Bible", None, always, about),
    open_cmd("help.credits", "Credits", None, |s, p| {
        open(s, p, TabKind::Credits)
    }),
    CommandSpec {
        id: "tab.go",
        label: "Go to Key",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "text",
                kind: ParamKind::String,
                required: true,
                help: "Bible reference in the Tab's numbering, or Strong's number, Lemma or headword",
            },
        ],
        enabled: always,
        run: go,
    },
    CommandSpec {
        id: "tab.previous_entry",
        label: "Previous entry",
        shortcut: None,
        params: &[TAB],
        enabled: always,
        run: |s, p| adjacent_entry(s, p, false),
    },
    CommandSpec {
        id: "tab.next_entry",
        label: "Next entry",
        shortcut: None,
        params: &[TAB],
        enabled: always,
        run: |s, p| adjacent_entry(s, p, true),
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

pub(crate) fn title_of(s: &Session, kind: &TabKind) -> String {
    match kind {
        TabKind::Library => "Library".into(),
        TabKind::ImportReport(_) => "Import report".into(),
        TabKind::GetBibles => GET_RESOURCES_TITLE.into(),
        TabKind::Credits => "Credits".into(),
        TabKind::Resource(id) => s
            .library
            .get(id)
            .map_or_else(|| id.clone(), |e| e.manifest.abbreviation.clone()),
        TabKind::About(id) => format!(
            "About · {}",
            s.library
                .get(id)
                .map_or(id.as_str(), |e| e.manifest.abbreviation.as_str())
        ),
        TabKind::Guide(g) => g.title().into(),
        TabKind::Tool(t) => t.title().into(),
    }
}

/// The Get Resources Tab with its type filter set to `ty` (All when `None`).
fn open_get_resources(
    s: &mut Session,
    p: Params<'_>,
    ty: Option<obs_model::ResourceType>,
) -> Result<Value, CommandError> {
    s.get_bibles.set_type_filter(ty);
    let out = open(s, p, TabKind::GetBibles)?;
    // A Layout saved before #132 still calls the Tab "Get Bibles".
    if let Some(tab) = out["tab"]
        .as_u64()
        .and_then(|t| s.workspace.tab_mut(TabId(t)))
    {
        tab.title = GET_RESOURCES_TITLE.into();
    }
    // Show the cached catalog at once; refresh it in the background when stale.
    if s.get_bibles.needs_refresh(crate::get_bibles::now_secs()) {
        s.refresh_resource_catalog();
    }
    Ok(out)
}

const GET_RESOURCES_TITLE: &str = "Get Resources";

fn open(s: &mut Session, p: Params<'_>, kind: TabKind) -> Result<Value, CommandError> {
    let title = title_of(s, &kind);
    let tab = s.workspace.open(kind, title, request(&p));
    Ok(json!({ "tab": tab.0 }))
}

/// A Passage Guide starts where its Link set is; unlinked, at the Passage the focused Tab
/// shows (like Logos opening a Guide on the active reference).
fn open_passage_guide(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let from = s
        .workspace
        .focused()
        .and_then(|f| s.workspace.tab(f))
        .filter(|t| t.key_schemes.contains(&obs_model::KeyScheme::Passage))
        .and_then(|t| {
            match crate::link::to_standard(t.verse_map.as_deref(), t.history.current()?) {
                crate::link::ToStandard::Verse(v) => Some(v),
                crate::link::ToStandard::Unmapped => None,
            }
        });
    let out = open(s, p, TabKind::Guide(Guide::PassageGuide))?;
    if let Some(v) = from
        && let Some(t) = out["tab"]
            .as_u64()
            .and_then(|id| s.workspace.tab_mut(TabId(id)))
        && t.history.current().is_none()
    {
        t.history.scrolled_to(v);
    }
    Ok(out)
}

/// A Word Study Tab. With `strongs` it is at that number (its Key, kept in the Tab's entry
/// history like a Lexicon's). Without, a new one starts at its Link set's Strong's number,
/// else at the word Word Info shows (like Logos opening a word study on the active word),
/// otherwise empty. With `tab`, that open Word Study Tab moves (and takes focus) instead.
fn open_word_study(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "guide.word_study";
    let number = p
        .str("strongs")
        .map(|t| {
            t.parse::<obs_scripture::StrongsNumber>()
                .map_err(|e| failed(ID, e.to_string()))
        })
        .transpose()?;
    let shown = crate::word_study::word_info_number(s);
    let mut out = match p.i64("tab") {
        Some(_) => {
            let tab = tab_param(ID, s, &p)?;
            if s.workspace.tab(tab).map(|t| &t.kind) != Some(&TabKind::Guide(Guide::WordStudy)) {
                return Err(failed(ID, format!("Tab {} is not a Word Study", tab.0)));
            }
            s.workspace.focus(tab);
            json!({ "tab": tab.0 })
        }
        None => open(s, p, TabKind::Guide(Guide::WordStudy))?,
    };
    let Some(tab) = out["tab"].as_u64().map(TabId) else {
        return Ok(out);
    };
    if let Some(n) = number {
        let key = obs_model::Key::StrongsNumber(n);
        s.stop_choosing(tab);
        s.workspace.navigate_entry(tab, key.clone());
        out["key"] = json!(key);
        return Ok(out);
    }
    let fresh = s
        .workspace
        .tab(tab)
        .is_some_and(|t| t.entry_history.current().is_none());
    if fresh {
        let key = s
            .workspace
            .set_entry_location(tab)
            .or_else(|| shown.map(obs_model::Key::StrongsNumber));
        if let Some(key) = key
            && let Some(t) = s.workspace.tab_mut(tab)
        {
            t.entry_history.scrolled_to(key);
        }
    }
    Ok(out)
}

fn open_resource(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let id = p.str("id").unwrap_or_default();
    if s.library.get(id).is_none() {
        return Err(failed(
            "resource.open",
            format!("{id} is not in the library"),
        ));
    }
    let schemes = s
        .library
        .get(id)
        .map(|e| e.manifest.key_schemes.clone())
        .unwrap_or_default();
    let out = open(s, p, TabKind::Resource(id.to_string()))?;
    if let Some(tab) = out["tab"]
        .as_u64()
        .and_then(|id| s.workspace.tab_mut(TabId(id)))
        && !schemes.is_empty()
    {
        tab.key_schemes = schemes;
    }
    Ok(out)
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
    if t.header_box() == crate::HeaderBox::Word {
        return crate::word_study::go(s, tab, text);
    }
    if matches!(
        t.header_box(),
        crate::HeaderBox::Lexicon | crate::HeaderBox::Headword
    ) {
        let key = t
            .navigation
            .as_ref()
            .and_then(|n| n.parse_entry(text))
            .map(|e| e.key.clone())
            .ok_or_else(|| failed(ID, format!("No entry for {text}")))?;
        s.workspace.navigate_entry(tab, key.clone());
        return Ok(json!({ "tab": tab.0, "key": key }));
    }
    let context = t.history.current();
    let (start, own, passage) = match &t.versification {
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
                first,
            )
        }
        None => {
            let list = parse_reference(text, &Standard, context)
                .map_err(|e| failed(ID, e.kind.to_string()))?;
            let first = list.passages().first().copied();
            (
                first.map(|p| p.start()),
                first.map(|p| format_passage(p, &Standard, RefStyle::Long)),
                first,
            )
        }
    };
    let Some(verse) = start else {
        return Err(failed(ID, "no reference"));
    };
    if let Some(nav) = &t.navigation
        && nav.section(verse).is_none()
    {
        return Err(failed(ID, "No earlier section in this Commentary"));
    }
    s.workspace.navigate(tab, verse);
    // A typed range replaces the chip; other jumps retain it until they leave its chapters.
    if let Some(t) = s.workspace.tab_mut(tab)
        && t.kind == TabKind::Tool(Tool::TextComparison)
        && let Some(range) = passage.filter(|p| crate::compare::narrows(*p))
    {
        t.comparison.range = Some(range);
    }
    Ok(json!({ "tab": tab.0, "verse": verse.osis(), "text": own }))
}

fn history(s: &mut Session, p: Params<'_>, back: bool) -> Result<Value, CommandError> {
    let id = if back { "tab.back" } else { "tab.forward" };
    let tab = tab_param(id, s, &p)?;
    if s.workspace.tab(tab).is_some_and(|t| {
        matches!(
            t.header_box(),
            crate::HeaderBox::Lexicon | crate::HeaderBox::Headword | crate::HeaderBox::Word
        )
    }) {
        s.stop_choosing(tab);
        return Ok(json!({ "key": s.workspace.entry_history(tab, back) }));
    }
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

/// Set the Text size (clamped), save it and show it briefly ("Text 18 pt"). The size is
/// shown only once saved, so a failed save's error toast is not replaced by it.
fn set_text_size(s: &mut Session, size: f32) -> Result<Value, CommandError> {
    s.text.size = size.clamp(TEXT_SIZE_MIN, TEXT_SIZE_MAX);
    let out = view_saved(s, json!({ "size": s.text.size }))?;
    s.workspace
        .push_effect(Effect::Message(format!("Text {} pt", s.text.size)));
    Ok(out)
}

fn view_saved(s: &mut Session, out: Value) -> Result<Value, CommandError> {
    s.save_view_preferences()
        .map_err(|e| failed("view", format!("saving preferences: {e}")))?;
    Ok(out)
}

fn user<'a>(
    s: &'a Session,
    id: &'static str,
) -> Result<&'a dyn crate::persist::UserData, CommandError> {
    s.user
        .as_deref()
        .ok_or_else(|| failed(id, "there is no User database"))
}

fn layout_save(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "layout.save";
    let Some(name) = p.str("name").map(str::trim).filter(|n| !n.is_empty()) else {
        s.workspace
            .push_effect(Effect::PromptLayoutName { rename: None });
        return Ok(Value::Null);
    };
    let json = s.workspace.snapshot().to_json();
    user(s, ID)?
        .save_layout(name, &json)
        .map_err(|e| failed(ID, e))?;
    s.workspace
        .push_effect(Effect::Message(format!("Saved layout “{name}”")));
    Ok(json!({ "name": name }))
}

fn layout_restore(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "layout.restore";
    let name = p.str("name").unwrap_or_default();
    let json = user(s, ID)?
        .layout(name)
        .map_err(|e| failed(ID, e))?
        .ok_or_else(|| failed(ID, format!("no layout “{name}”")))?;
    let snap = crate::persist::WorkspaceSnapshot::from_json(&json).ok_or_else(|| {
        failed(
            ID,
            format!("layout “{name}” cannot be read by this version"),
        )
    })?;
    let snap = snap.without_missing(|id| s.library.get(id).is_some());
    s.workspace.restore(snap);
    Ok(json!({ "name": name }))
}

fn layout_rename(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "layout.rename";
    let name = p.str("name").unwrap_or_default().to_string();
    let Some(to) = p.str("to").map(str::trim).filter(|n| !n.is_empty()) else {
        s.workspace
            .push_effect(Effect::PromptLayoutName { rename: Some(name) });
        return Ok(Value::Null);
    };
    let renamed = user(s, ID)?
        .rename_layout(&name, to)
        .map_err(|_| failed(ID, format!("there is already a layout “{to}”")))?;
    if !renamed {
        return Err(failed(ID, format!("no layout “{name}”")));
    }
    Ok(json!({ "name": to }))
}

fn layout_delete(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "layout.delete";
    let name = p.str("name").unwrap_or_default();
    if !user(s, ID)?
        .delete_layout(name)
        .map_err(|e| failed(ID, e))?
    {
        return Err(failed(ID, format!("no layout “{name}”")));
    }
    Ok(json!({ "deleted": name }))
}

/// Word Info's "Search this word": `strong:` and the main Strong's number, in the Bible the
/// word was read in (falling back to the Search Tab's own choice when that Bible is gone).
/// Always in Bible mode, whatever mode the Search Tab was left in.
fn word_info_search(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "word_info.search";
    let shown = s
        .word_info
        .shown
        .as_ref()
        .ok_or_else(|| failed(ID, "nothing is shown"))?;
    let query = format!("strong:{}", shown.strongs());
    let bibles = shown
        .bible
        .clone()
        .filter(|id| {
            s.library
                .get(id)
                .is_some_and(|e| e.manifest.resource_type == obs_model::ResourceType::Bible)
        })
        .map(|id| id.to_string());
    let mut params = json!({ "text": query, "mode": "bible" });
    if let Some(b) = bibles {
        params["bibles"] = json!(b);
    }
    s.run("search.run", &params)
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

fn adjacent_entry(s: &mut Session, p: Params<'_>, forward: bool) -> Result<Value, CommandError> {
    let id = if forward {
        "tab.next_entry"
    } else {
        "tab.previous_entry"
    };
    let tab = tab_param(id, s, &p)?;
    let key = s
        .workspace
        .tab(tab)
        .and_then(|t| {
            let current = t.entry_history.current()?;
            t.navigation
                .as_ref()?
                .adjacent(&current, forward)
                .map(|e| e.key.clone())
        })
        .ok_or_else(|| failed(id, "No adjacent entry"))?;
    s.workspace.navigate_entry(tab, key.clone());
    Ok(json!({ "key": key }))
}
