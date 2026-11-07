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
    /// "Favorites": a submenu of the Favorites, each Favorites folder a submenu of its own;
    /// each runs `favorite.open` (#155).
    Favorites,
    /// "Recently closed": a submenu of Recently closed, newest first, each running
    /// `tab.reopen_closed` with its id (#214).
    RecentlyClosed,
    /// A submenu of exclusive choices: one `command` run with `{ "value": id }` per choice,
    /// the current one checked.
    Choice {
        title: &'static str,
        command: &'static str,
    },
    Separator,
}

use MenuItem::{Command as C, Layouts, Resources, Separator};

/// The top menu bar (#18 rule 3). The Command box (#139) sits to the right of it. Library,
/// Guides, Tools and Layouts are drawn as toolbar Launchers (#140, [`crate::toolbar`]) whose
/// panes offer these items.
/// The Back up & restore Tab's title (#217).
pub const BACKUPS_TITLE: &str = "Back up & restore";

pub const MENUS: &[Menu] = &[
    Menu {
        title: "Library",
        items: &[
            C("library.open"),
            C("library.import"),
            C("library.get_resources"),
            C("library.get_bibles"),
            C("library.starter"),
            MenuItem::Favorites,
            Separator,
            Resources,
        ],
    },
    Menu {
        title: "Guides",
        items: &[
            C("guide.passage_guide"),
            C("guide.exegetical_guide"),
            C("guide.word_study"),
            C("guide.factbook"),
            C("guide.topic_guide"),
        ],
    },
    Menu {
        title: "Tools",
        items: &[
            C("tool.search"),
            C("tool.text_comparison"),
            C("tool.notes"),
            C("tool.word_info"),
            C("tool.cross_references"),
            C("tool.favorites"),
            C("tool.history"),
            C("tool.reading_plans"),
            C("tool.atlas"),
            C("tool.timeline"),
            Separator,
            C("tool.copy_verses"),
            C("data.backups"),
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
            C("app.settings"),
            Separator,
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
            C("view.footnote_markers"),
            C("view.fonts"),
            Separator,
            C("view.highlights"),
            C("view.highlight_styles"),
            MenuItem::Notebooks,
            Separator,
            MenuItem::Choice {
                title: "Toolbar position",
                command: "view.toolbar_position",
            },
            C("view.toolbar_auto_hide"),
            C("view.toolbar_customize"),
        ],
    },
    Menu {
        title: "Window",
        items: &[
            C("tab.reopen_closed"),
            MenuItem::RecentlyClosed,
            Separator,
            C("panel.close"),
        ],
    },
    Menu {
        title: "Help",
        items: &[
            C("help.about"),
            C("help.credits"),
            C("help.keyboard_shortcuts"),
            C("app.commands"),
        ],
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
/// Passage-keyed Guides and Tools can open at a Passage (from the right-click menu, #142).
const AT_PASSAGE: &[ParamSpec] = &[
    PANEL,
    SPLIT,
    ParamSpec {
        name: "passage",
        kind: ParamKind::String,
        required: false,
        help: "Bible reference in the Standard versification, e.g. `Jn 3:16`: a Tab of this kind in the focused Tab's Link set moves there, else a new one opens there",
    },
    ParamSpec {
        name: "new_tab",
        kind: ParamKind::Boolean,
        required: false,
        help: "With `passage`: always open a new Tab (Ctrl/Cmd-click)",
    },
];
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
    if let Some(toolbar) = crate::toolbar_settings::choices(s, command) {
        return toolbar;
    }
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
        params: &[
            PANEL,
            SPLIT,
            crate::get_bibles::TYPE,
            crate::get_bibles::LICENCE,
        ],
        enabled: always,
        run: |s, p| {
            let failed = |message| CommandError::Failed {
                command: "library.get_resources",
                message,
            };
            let filter = crate::get_bibles::type_filter_from_id(p.str("type").unwrap_or_default())
                .map_err(failed)?;
            // The licence filter is kept unless asked for (#134).
            if let Some(licence) = p.str("licence") {
                let licence = crate::licence::LicenceFilter::from_id(licence).map_err(failed)?;
                s.get_bibles.set_licence_filter(licence);
            }
            open_get_resources(s, p, filter)
        },
    },
    CommandSpec {
        id: "library.get_bibles",
        label: "Get Bibles…",
        shortcut: None,
        params: OPEN,
        enabled: always,
        run: |s, p| {
            open_get_resources(
                s,
                p,
                crate::get_bibles::ResourceFilter::Type(obs_model::ResourceType::Bible),
            )
        },
    },
    CommandSpec {
        id: "library.starter",
        label: "Starter library…",
        shortcut: None,
        params: OPEN,
        enabled: always,
        run: |s, p| {
            let out = open(s, p, TabKind::StarterLibrary)?;
            // Without a bundle, the rows come from the latest release's manifest.
            if s.starter.manifest().is_none() {
                s.check_starter_release();
            }
            Ok(out)
        },
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
    CommandSpec {
        id: "guide.passage_guide",
        label: "Passage Guide",
        shortcut: None,
        params: AT_PASSAGE,
        enabled: always,
        run: |s, p| open_passage_keyed_guide(s, p, Guide::PassageGuide, "guide.passage_guide"),
    },
    CommandSpec {
        id: "guide.exegetical_guide",
        label: "Exegetical Guide",
        shortcut: None,
        params: AT_PASSAGE,
        enabled: always,
        run: |s, p| {
            open_passage_keyed_guide(s, p, Guide::ExegeticalGuide, "guide.exegetical_guide")
        },
    },
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
    CommandSpec {
        id: "tool.text_comparison",
        label: "Text Comparison",
        shortcut: None,
        params: AT_PASSAGE,
        enabled: always,
        run: |s, p| {
            const ID: &str = "tool.text_comparison";
            let kind = TabKind::Tool(Tool::TextComparison);
            match open_at_passage(s, &p, kind.clone(), ID)? {
                Some(out) => Ok(out),
                None => open(s, p, kind),
            }
        },
    },
    open_cmd("tool.notes", "Notes", None, |s, p| {
        open(s, p, TabKind::Tool(Tool::Notes))
    }),
    open_cmd(
        "tool.word_info",
        "Word Info",
        Some("Ctrl+Shift+I"),
        |s, p| open(s, p, TabKind::Tool(Tool::WordInfo)),
    ),
    CommandSpec {
        id: "tool.cross_references",
        label: "Cross-references",
        shortcut: None,
        params: AT_PASSAGE,
        enabled: always,
        run: |s, p| {
            const ID: &str = "tool.cross_references";
            let kind = TabKind::Tool(Tool::CrossReferences);
            match open_at_passage(s, &p, kind.clone(), ID)? {
                Some(out) => Ok(out),
                None => open(s, p, kind),
            }
        },
    },
    CommandSpec {
        id: "tool.atlas",
        label: "Atlas",
        shortcut: None,
        params: AT_PASSAGE,
        enabled: always,
        run: |s, p| {
            const ID: &str = "tool.atlas";
            let kind = TabKind::Tool(Tool::Atlas);
            match open_at_passage(s, &p, kind.clone(), ID)? {
                Some(out) => Ok(out),
                None => open(s, p, kind),
            }
        },
    },
    CommandSpec {
        id: "tool.timeline",
        label: "Timeline",
        shortcut: None,
        params: AT_PASSAGE,
        enabled: always,
        run: |s, p| {
            const ID: &str = "tool.timeline";
            let kind = TabKind::Tool(Tool::Timeline);
            match open_at_passage(s, &p, kind.clone(), ID)? {
                Some(out) => Ok(out),
                None => open(s, p, kind),
            }
        },
    },
    open_cmd("tool.favorites", "Favorites", None, |s, p| {
        open(s, p, TabKind::Tool(Tool::Favorites))
    }),
    open_cmd("tool.history", "History", None, |s, p| {
        open(s, p, TabKind::Tool(Tool::History))
    }),
    open_cmd("tool.reading_plans", "Reading plans", None, |s, p| {
        open(s, p, TabKind::Tool(Tool::ReadingPlans))
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
        label: "Word Study",
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
    CommandSpec {
        id: "view.footnote_markers",
        label: "Footnote markers",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "tab",
                kind: ParamKind::Integer,
                required: false,
                help: "The Resource Tab (default: the focused Tab)",
            },
            ParamSpec {
                name: "on",
                kind: ParamKind::Boolean,
                required: false,
                help: "Show (true) or hide (false) the Tab's Footnote markers; without it, toggle them",
            },
        ],
        enabled: focused_resource_tab,
        run: set_footnote_markers,
    },
    simple("view.fonts", "Fonts…", None, always, |s, _| {
        s.workspace.push_effect(Effect::ShowFonts);
        Ok(Value::Null)
    }),
    simple(
        "view.highlight_styles",
        "Highlight styles…",
        None,
        |s| s.user.is_some(),
        |s, _| {
            s.workspace.push_effect(Effect::ShowHighlightStyles);
            Ok(Value::Null)
        },
    ),
    CommandSpec {
        id: "view.font",
        label: "Choose a font",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "slot",
                kind: ParamKind::String,
                required: true,
                help: "latin, greek, hebrew or interface",
            },
            ParamSpec {
                name: "family",
                kind: ParamKind::String,
                required: false,
                help: "Font family installed on this computer; empty or missing for the bundled font",
            },
            ParamSpec {
                name: "style",
                kind: ParamKind::String,
                required: false,
                help: "Style of the family (default Regular)",
            },
        ],
        enabled: always,
        run: set_font,
    },
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
    // Ctrl+, (Cmd+, on macOS), where desktop apps keep their settings.
    open_cmd("app.settings", "Settings", Some("Ctrl+Comma"), |s, p| {
        open(s, p, TabKind::Settings)
    }),
    open_cmd(
        "help.keyboard_shortcuts",
        "Keyboard shortcuts",
        None,
        |s, p| open(s, p, TabKind::KeyboardShortcuts),
    ),
    open_cmd("data.backups", "Back up & restore…", None, |s, p| {
        open(s, p, TabKind::Backups)
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
    // Parallel resources (#151): the Tab shows another Resource of its type, at the same place.
    CommandSpec {
        id: "tab.parallel",
        label: "Switch to parallel resource",
        shortcut: None,
        params: &[
            TAB,
            ParamSpec {
                name: "id",
                kind: ParamKind::String,
                required: true,
                help: "Resource id of the same Resource type as the Tab's",
            },
        ],
        enabled: always,
        run: switch_parallel,
    },
    CommandSpec {
        id: "tab.parallel_previous",
        label: "Previous parallel resource",
        shortcut: Some("Ctrl+["),
        params: &[TAB],
        enabled: focused_can_switch,
        run: |s, p| cycle_parallel(s, p, false),
    },
    CommandSpec {
        id: "tab.parallel_next",
        label: "Next parallel resource",
        shortcut: Some("Ctrl+]"),
        params: &[TAB],
        enabled: focused_can_switch,
        run: |s, p| cycle_parallel(s, p, true),
    },
    CommandSpec {
        id: "tab.close",
        label: "Close Tab",
        shortcut: Some("Ctrl+W"),
        params: &[TAB],
        enabled: always,
        run: |s, p| {
            let tab = tab_param("tab.close", s, &p)?;
            Ok(json!({ "closed": s.workspace.user_closed(tab) }))
        },
    },
    // Recently closed (#214).
    CommandSpec {
        id: "tab.reopen_closed",
        label: "Reopen closed Tab",
        shortcut: Some("Ctrl+Shift+T"),
        params: &[ParamSpec {
            name: "closed",
            kind: ParamKind::Integer,
            required: false,
            help: "Which Recently closed entry, by id (default: the newest)",
        }],
        enabled: |s| s.workspace.recently_closed().next().is_some(),
        run: reopen_closed,
    },
    CommandSpec {
        id: "panel.close",
        label: "Close Panel",
        shortcut: None,
        params: &[ParamSpec {
            name: "tab",
            kind: ParamKind::Integer,
            required: false,
            help: "A Tab in the Panel (default: the focused Tab)",
        }],
        enabled: |s| s.workspace.focused().is_some(),
        run: close_panel,
    },
    // Find in Resource (#175): the Find bar is the UI's; these open it and step through it.
    CommandSpec {
        id: "tab.find",
        label: "Find in Resource",
        shortcut: Some("Ctrl+F"),
        params: &[
            TAB,
            ParamSpec {
                name: "text",
                kind: ParamKind::String,
                required: false,
                help: "What to find (default: the Tab's selection, else what it found last)",
            },
        ],
        enabled: focused_can_find,
        run: |s, p| {
            let tab = findable_tab("tab.find", s, &p)?;
            let text = p.str("text").map(str::to_string);
            s.workspace.push_effect(Effect::Find { tab, text });
            Ok(json!({}))
        },
    },
    CommandSpec {
        id: "tab.find_next",
        label: "Find next",
        shortcut: None,
        params: &[TAB],
        enabled: focused_can_find,
        run: |s, p| find_step(s, p, "tab.find_next", true),
    },
    CommandSpec {
        id: "tab.find_previous",
        label: "Find previous",
        shortcut: None,
        params: &[TAB],
        enabled: focused_can_find,
        run: |s, p| find_step(s, p, "tab.find_previous", false),
    },
    CommandSpec {
        id: "tab.float",
        label: "Float in new window",
        shortcut: None,
        params: &[TAB],
        enabled: always,
        run: |s, p| {
            let tab = tab_param("tab.float", s, &p)?;
            let window = s
                .workspace
                .float(tab, None)
                .ok_or_else(|| failed("tab.float", "no such Tab"))?;
            Ok(json!({ "tab": tab.0, "window": window.0 }))
        },
    },
    CommandSpec {
        id: "tab.dock",
        label: "Move to main window",
        shortcut: None,
        params: &[TAB],
        enabled: always,
        run: |s, p| {
            let tab = tab_param("tab.dock", s, &p)?;
            if !s.workspace.dock(tab) {
                return Err(failed("tab.dock", "this Tab is in the main window"));
            }
            Ok(json!({ "tab": tab.0 }))
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
        TabKind::StarterLibrary => "Starter library".into(),
        TabKind::Credits => "Credits".into(),
        TabKind::Home => "Home".into(),
        TabKind::KeyboardShortcuts => "Keyboard shortcuts".into(),
        TabKind::Settings => "Settings".into(),
        TabKind::Backups => BACKUPS_TITLE.into(),
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

/// The Get Resources Tab with its filter set.
fn open_get_resources(
    s: &mut Session,
    p: Params<'_>,
    filter: crate::get_bibles::ResourceFilter,
) -> Result<Value, CommandError> {
    s.get_bibles.set_filter(filter);
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
    let tab = open_tab(s, kind, request(&p));
    Ok(json!({ "tab": tab.0 }))
}

/// Open a Tab of `kind` (a Resource Tab follows the Key schemes of its manifest).
pub(crate) fn open_tab(s: &mut Session, kind: TabKind, req: OpenRequest) -> TabId {
    let title = title_of(s, &kind);
    let schemes = match &kind {
        TabKind::Resource(id) => s
            .library
            .get(id)
            .map(|e| e.manifest.key_schemes.clone())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let tab = s.workspace.open(kind, title, req);
    if let Some(t) = s.workspace.tab_mut(tab)
        && !schemes.is_empty()
    {
        t.key_schemes = schemes;
    }
    tab
}

/// A Passage Guide or Exegetical Guide starts where its Link set is; unlinked, at the
/// Passage the focused Tab shows (as a study app opens a Guide on the active reference).
fn open_passage_keyed_guide(
    s: &mut Session,
    p: Params<'_>,
    guide: Guide,
    id: &'static str,
) -> Result<Value, CommandError> {
    if let Some(out) = open_at_passage(s, &p, TabKind::Guide(guide), id)? {
        return Ok(out);
    }
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
    let out = open(s, p, TabKind::Guide(guide))?;
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

/// With a `passage` param: show it in a Tab of `kind`. The most recent such Tab in the focused
/// Tab's Link set (or, from an unlinked Tab, an unlinked one) moves there; with `new_tab` or
/// `split`, or when there is none, a new one opens (joining the focused Tab's Link set). The
/// Link set follows, as after any jump. `None` without the param.
fn open_at_passage(
    s: &mut Session,
    p: &Params<'_>,
    kind: TabKind,
    id: &'static str,
) -> Result<Option<Value>, CommandError> {
    let Some(text) = p.str("passage") else {
        return Ok(None);
    };
    let list =
        parse_reference(text, &Standard, None).map_err(|e| failed(id, e.kind.to_string()))?;
    let passage = list
        .passages()
        .first()
        .copied()
        .ok_or_else(|| failed(id, "no reference"))?;
    let verse = passage.start();
    let opener = s.workspace.opener_link();
    let new_tab = p.bool("new_tab").unwrap_or(false) || p.bool("split").unwrap_or(false);
    let existing = (!new_tab)
        .then(|| {
            s.workspace
                .most_recent(|t| t.kind == kind && t.link == opener)
        })
        .flatten();
    let tab = match existing {
        Some(tab) => {
            s.workspace.focus(tab);
            tab
        }
        None => {
            let title = title_of(s, &kind);
            s.workspace.open(kind, title, request(p))
        }
    };
    s.workspace.navigate_standard(tab, verse);
    // A chapter or range stays whole (`eg Ps 23`, `pg Jn 3:16-18`).
    if let Some(t) = s.workspace.tab_mut(tab) {
        t.passage = Some(passage);
    }
    Ok(Some(json!({
        "tab": tab.0,
        "verse": verse.osis(),
        "passage": format_passage(passage, &Standard, RefStyle::Long),
    })))
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

impl Session {
    /// Recently closed, newest first, as Window → Recently closed lists it (#214): each
    /// entry's id and how to name it, a Tab by where it was ("John 3:16 · KJV") or its title,
    /// a closed Panel by its Tabs' titles.
    pub fn recently_closed_labels(&self) -> Vec<(u64, String)> {
        self.workspace
            .recently_closed()
            .map(|c| {
                let label = match c.tabs.as_slice() {
                    [one] => {
                        let mut tab =
                            crate::workspace::Tab::from_snapshot(TabId(one.id), one.clone());
                        // A Lexicon or Dictionary is named by its Key, as its manifest says.
                        if let Some(e) =
                            (tab.kind.resource_id()).and_then(|id| self.library.get(id))
                            && !e.manifest.key_schemes.is_empty()
                        {
                            tab.key_schemes = e.manifest.key_schemes.clone();
                        }
                        self.location_label(&tab)
                            .unwrap_or_else(|| one.title.clone())
                    }
                    tabs => {
                        let titles: Vec<&str> = tabs.iter().map(|t| t.title.as_str()).collect();
                        format!("Panel: {}", titles.join(", "))
                    }
                };
                (c.id, label)
            })
            .collect()
    }
}

fn reopen_closed(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "tab.reopen_closed";
    let id = match p.i64("closed") {
        Some(n) => Some(u64::try_from(n).map_err(|_| failed(ID, format!("no closing {n}")))?),
        None => None,
    };
    let tabs = s.workspace.reopen_closed(id).ok_or_else(|| match id {
        Some(n) => failed(ID, format!("no closing {n}")),
        None => failed(ID, "nothing to reopen"),
    })?;
    Ok(json!({ "tabs": tabs.iter().map(|t| t.0).collect::<Vec<_>>() }))
}

fn close_panel(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "panel.close";
    let tab = match p.i64("tab") {
        Some(_) => tab_param(ID, s, &p)?,
        None => s
            .workspace
            .focused()
            .ok_or_else(|| failed(ID, "no Tab is focused"))?,
    };
    let panel = s
        .workspace
        .panel_of(tab)
        .ok_or_else(|| failed(ID, format!("Tab {} is in no Panel", tab.0)))?;
    Ok(json!({ "closed": s.workspace.close_panel(panel) }))
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
    if t.header_box() == crate::HeaderBox::Name {
        return crate::factbook::go(s, tab, text);
    }
    if t.header_box() == crate::HeaderBox::Topic {
        return crate::topic_guide::go(s, tab, text);
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
    // A Guide or Cross-references Tab shows the typed chapter or range whole.
    if let Some(t) = s.workspace.tab_mut(tab)
        && t.versification.is_none()
    {
        t.passage = passage;
    }
    Ok(json!({ "tab": tab.0, "verse": verse.osis(), "text": own }))
}

fn history(s: &mut Session, p: Params<'_>, back: bool) -> Result<Value, CommandError> {
    let id = if back { "tab.back" } else { "tab.forward" };
    let tab = tab_param(id, s, &p)?;
    let shown = s.workspace.tab(tab).map(|t| t.kind.clone());
    let mut out = if s.workspace.tab(tab).is_some_and(|t| t.keeps_entries()) {
        s.stop_choosing(tab);
        s.stop_choosing_factbook(tab);
        s.stop_choosing_topic(tab);
        json!({ "key": s.workspace.entry_history(tab, back) })
    } else {
        let to: Option<Verse> = if back {
            s.workspace.back(tab)
        } else {
            s.workspace.forward(tab)
        };
        json!({ "verse": to.map(|v| v.osis()) })
    };
    // Back or Forward across a switch between parallel resources (#151).
    if let Some(TabKind::Resource(now)) = s.workspace.tab(tab).map(|t| t.kind.clone())
        && shown != Some(TabKind::Resource(now.clone()))
    {
        resource_shown(s, tab);
        out["id"] = json!(now);
    }
    Ok(out)
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

/// `view.font`: choose the font of one slot by family and style (#138). The UI loads it and
/// lays text out again; a missing font falls back to the bundled one.
fn set_font(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    use crate::appearance::{FontChoice, FontSlot};
    let id = p.str("slot").unwrap_or_default();
    let slot = FontSlot::from_id(id)
        .ok_or_else(|| failed("view.font", format!("“{id}” is not a font slot")))?;
    let family = p.str("family").unwrap_or_default().trim();
    let choice = (!family.is_empty()).then(|| FontChoice {
        family: family.to_string(),
        style: p
            .str("style")
            .map(str::trim)
            .filter(|st| !st.is_empty())
            .unwrap_or("Regular")
            .to_string(),
        missing_noticed: false,
    });
    let out = match &choice {
        Some(c) => json!({ "slot": slot.id(), "family": c.family, "style": c.style }),
        None => json!({ "slot": slot.id(), "family": null }),
    };
    s.fonts.set(slot, choice);
    view_saved(s, out)
}

/// Whether the focused Tab shows a Resource (Footnote markers are per Resource Tab, #208).
fn focused_resource_tab(s: &Session) -> bool {
    s.workspace
        .focused()
        .and_then(|t| s.workspace.tab(t))
        .is_some_and(|t| matches!(t.kind, TabKind::Resource(_)))
}

/// `view.footnote_markers`: show or hide one Resource Tab's Footnote markers (#208).
fn set_footnote_markers(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "view.footnote_markers";
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .or_else(|| s.workspace.focused())
        .ok_or_else(|| failed(ID, "no Tab"))?;
    let t = s
        .workspace
        .tab_mut(tab)
        .ok_or_else(|| failed(ID, format!("no Tab {}", tab.0)))?;
    if !matches!(t.kind, TabKind::Resource(_)) {
        return Err(failed(ID, "only a Resource Tab has Footnote markers"));
    }
    t.footnote_markers = p.bool("on").unwrap_or(!t.footnote_markers);
    Ok(json!({ "tab": tab.0, "on": t.footnote_markers }))
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
    // Recently closed is the session's, never a Layout's (#214).
    let snapshot = crate::persist::WorkspaceSnapshot {
        recently_closed: Default::default(),
        ..s.workspace.snapshot()
    };
    let json = snapshot.to_json();
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
    // Where the closing Tabs went is History before they are replaced.
    s.record_history();
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

/// A Resource Tab's parallel resources (#151): every Resource of its Resource type in the
/// library, its own included, in Preferred order. Empty for other Tabs.
pub fn parallel_resources(s: &Session, tab: TabId) -> Vec<&crate::LibraryEntry> {
    let Some(TabKind::Resource(id)) = s.workspace.tab(tab).map(|t| &t.kind) else {
        return Vec::new();
    };
    let Some(ty) = s.library.get(id).map(|e| e.manifest.resource_type) else {
        return Vec::new();
    };
    s.library.of_type(ty).collect()
}

/// The parallel-resource keys are left to other uses unless the focused Tab is a Resource Tab
/// (with no focused Tab, as from `obs-cli`, they run and report what is wrong).
/// Ctrl+F is for the focused Tab's Find bar; elsewhere the key is left alone.
fn focused_can_find(s: &Session) -> bool {
    s.workspace
        .focused()
        .and_then(|t| s.workspace.tab(t))
        .is_some_and(|t| t.kind.has_find_bar())
}

fn findable_tab(command: &'static str, s: &Session, p: &Params<'_>) -> Result<TabId, CommandError> {
    let tab = tab_param(command, s, p)?;
    match s.workspace.tab(tab) {
        Some(t) if t.kind.has_find_bar() => Ok(tab),
        _ => Err(failed(
            command,
            "only a Resource or Text Comparison Tab has a Find bar",
        )),
    }
}

fn find_step(
    s: &mut Session,
    p: Params<'_>,
    command: &'static str,
    forward: bool,
) -> Result<Value, CommandError> {
    let tab = findable_tab(command, s, &p)?;
    s.workspace.push_effect(Effect::FindStep { tab, forward });
    Ok(json!({}))
}

fn focused_can_switch(s: &Session) -> bool {
    s.workspace
        .focused()
        .and_then(|t| s.workspace.tab(t))
        .is_none_or(|t| matches!(t.kind, TabKind::Resource(_)))
}

/// `tab.parallel`: show the Resource `id` in the Tab, at the Tab's Current location.
fn switch_parallel(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "tab.parallel";
    let tab = tab_param(ID, s, &p)?;
    let to = p.str("id").unwrap_or_default();
    let parallel = parallel_resources(s, tab);
    if parallel.is_empty() {
        return Err(failed(ID, "this Tab does not show a library Resource"));
    }
    if !parallel.iter().any(|e| e.manifest.resource_id == to) {
        let ty = crate::library_search::type_plural(parallel[0].manifest.resource_type);
        return Err(failed(
            ID,
            format!("{to} is not one of the {ty} in the library"),
        ));
    }
    switch_to(s, tab, to.to_string())
}

/// `tab.parallel_previous` / `tab.parallel_next`: the Resource before or after the Tab's in
/// Preferred order, wrapping around.
fn cycle_parallel(s: &mut Session, p: Params<'_>, forward: bool) -> Result<Value, CommandError> {
    let id = if forward {
        "tab.parallel_next"
    } else {
        "tab.parallel_previous"
    };
    let tab = tab_param(id, s, &p)?;
    let Some(TabKind::Resource(current)) = s.workspace.tab(tab).map(|t| t.kind.clone()) else {
        return Err(failed(id, "this Tab does not show a Resource"));
    };
    let parallel: Vec<String> = parallel_resources(s, tab)
        .iter()
        .map(|e| e.manifest.resource_id.clone())
        .collect();
    let Some(at) = parallel.iter().position(|r| *r == current) else {
        return Err(failed(id, format!("{current} is not in the library")));
    };
    if parallel.len() < 2 {
        let ty = s.library.get(&current).map(|e| e.manifest.resource_type);
        let what = ty.map_or("Resources", crate::library_search::type_plural);
        return Err(failed(
            id,
            format!("There are no other {what} in the library"),
        ));
    }
    let n = parallel.len();
    let next = if forward {
        (at + 1) % n
    } else {
        (at + n - 1) % n
    };
    switch_to(s, tab, parallel[next].clone())
}

fn switch_to(s: &mut Session, tab: TabId, to: String) -> Result<Value, CommandError> {
    s.stop_choosing(tab);
    if s.workspace.switch_resource(tab, to.clone()) {
        resource_shown(s, tab);
    }
    Ok(json!({ "tab": tab.0, "id": to }))
}

/// A Resource Tab now shows another Resource: its title and Key schemes are that one's.
fn resource_shown(s: &mut Session, tab: TabId) {
    let Some(kind) = s.workspace.tab(tab).map(|t| t.kind.clone()) else {
        return;
    };
    let title = title_of(s, &kind);
    let schemes = match &kind {
        TabKind::Resource(id) => s
            .library
            .get(id)
            .map(|e| e.manifest.key_schemes.clone())
            .unwrap_or_default(),
        _ => return,
    };
    if let Some(t) = s.workspace.tab_mut(tab) {
        t.title = title;
        if !schemes.is_empty() {
            t.key_schemes = schemes;
        }
    }
}
