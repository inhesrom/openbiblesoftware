//! Reading layout (#209): how Resource text flows on the page. Paragraphs or One verse per
//! line, Large chapter numbers, Section headings, the Reading column width and Justified text.
//!
//! Each choice has a default, kept with the Text settings in the User database and changed in
//! Settings (`view.text_flow`, `view.large_chapter_numbers`, `view.section_headings`,
//! `view.column_width`, `view.justified`). A Tab may set any of them for itself
//! (`tab.reading_layout`, View → Reading layout); what it has not set follows the default.
//! A Tab's own choices are saved in Layouts and the Last session.

use obs_layout::{ColumnWidth, ReadingLayout, TextFlow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::workspace::{TabId, TabKind};

/// The choices of Paragraphs or One verse per line: (flow, id, label).
pub const TEXT_FLOWS: [(TextFlow, &str, &str); 2] = [
    (TextFlow::Paragraphs, "paragraphs", "Paragraphs"),
    (
        TextFlow::VersePerLine,
        "verse_per_line",
        "One verse per line",
    ),
];

/// The Reading column widths: (width, id, label).
pub const COLUMN_WIDTHS: [(ColumnWidth, &str, &str); 4] = [
    (ColumnWidth::Narrow, "narrow", "Narrow"),
    (ColumnWidth::Medium, "medium", "Medium"),
    (ColumnWidth::Wide, "wide", "Wide"),
    (ColumnWidth::Full, "full", "Full width"),
];

pub fn text_flow_id(flow: TextFlow) -> &'static str {
    TEXT_FLOWS
        .iter()
        .find(|c| c.0 == flow)
        .map_or("paragraphs", |c| c.1)
}

pub fn column_width_id(column: ColumnWidth) -> &'static str {
    COLUMN_WIDTHS
        .iter()
        .find(|c| c.0 == column)
        .map_or("medium", |c| c.1)
}

fn text_flow_from_id(id: &str) -> Option<TextFlow> {
    TEXT_FLOWS.iter().find(|c| c.1 == id).map(|c| c.0)
}

fn column_width_from_id(id: &str) -> Option<ColumnWidth> {
    COLUMN_WIDTHS.iter().find(|c| c.1 == id).map(|c| c.0)
}

/// A Tab's own Reading layout choices; each one it has not made follows the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabReadingLayout {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<TextFlow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub large_chapter_numbers: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_headings: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<ColumnWidth>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub justified: Option<bool>,
}

impl TabReadingLayout {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The layout the Tab shows: its own choices over `defaults`.
    pub fn over(&self, defaults: ReadingLayout) -> ReadingLayout {
        ReadingLayout {
            flow: self.flow.unwrap_or(defaults.flow),
            large_chapter_numbers: self
                .large_chapter_numbers
                .unwrap_or(defaults.large_chapter_numbers),
            section_headings: self.section_headings.unwrap_or(defaults.section_headings),
            column: self.column.unwrap_or(defaults.column),
            justified: self.justified.unwrap_or(defaults.justified),
        }
    }
}

impl Session {
    /// The Reading layout Tab `tab` shows: its own choices over the defaults in Settings.
    pub fn reading_layout(&self, tab: TabId) -> ReadingLayout {
        self.workspace.tab(tab).map_or(self.text.layout, |t| {
            t.reading_layout.over(self.text.layout)
        })
    }
}

/// The current choice of a Reading layout `Choice` command, for menus and Settings.
pub(crate) fn choices(
    s: &Session,
    command: &str,
) -> Option<(Vec<(&'static str, &'static str)>, &'static str)> {
    match command {
        "view.text_flow" => Some((
            TEXT_FLOWS.iter().map(|c| (c.1, c.2)).collect(),
            text_flow_id(s.text.layout.flow),
        )),
        "view.column_width" => Some((
            COLUMN_WIDTHS.iter().map(|c| (c.1, c.2)).collect(),
            column_width_id(s.text.layout.column),
        )),
        _ => None,
    }
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn saved(s: &mut Session, command: &'static str, out: Value) -> Result<Value, CommandError> {
    s.save_view_preferences()
        .map_err(|e| failed(command, format!("saving preferences: {e}")))?;
    Ok(out)
}

/// A default's toggle: `on` sets it, without it the default flips.
fn toggle_default(
    s: &mut Session,
    p: Params<'_>,
    command: &'static str,
    field: fn(&mut ReadingLayout) -> &mut bool,
) -> Result<Value, CommandError> {
    let value = field(&mut s.text.layout);
    *value = p.bool("on").unwrap_or(!*value);
    let on = *value;
    saved(s, command, json!({ "on": on }))
}

/// `tab.reading_layout`: set a Tab's own Reading layout choices, or go back to the defaults.
fn set_tab_layout(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "tab.reading_layout";
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .ok_or_else(|| failed(ID, "no tab"))?;
    let flow = p
        .str("flow")
        .map(|id| text_flow_from_id(id).ok_or_else(|| failed(ID, format!("“{id}” is not a flow"))))
        .transpose()?;
    let column = p
        .str("column")
        .map(|id| {
            column_width_from_id(id)
                .ok_or_else(|| failed(ID, format!("“{id}” is not a column width")))
        })
        .transpose()?;
    let Some(t) = s.workspace.tab_mut(tab) else {
        return Err(failed(ID, format!("no Tab {}", tab.0)));
    };
    if !matches!(t.kind, TabKind::Resource(_)) {
        return Err(failed(ID, "this Tab does not show a Resource"));
    }
    let own = &mut t.reading_layout;
    if p.bool("reset") == Some(true) {
        *own = TabReadingLayout::default();
    }
    if let Some(flow) = flow {
        own.flow = Some(flow);
    }
    if let Some(column) = column {
        own.column = Some(column);
    }
    if let Some(on) = p.bool("large_chapter_numbers") {
        own.large_chapter_numbers = Some(on);
    }
    if let Some(on) = p.bool("section_headings") {
        own.section_headings = Some(on);
    }
    if let Some(on) = p.bool("justified") {
        own.justified = Some(on);
    }
    let shown = s.reading_layout(tab);
    Ok(json!({
        "flow": text_flow_id(shown.flow),
        "large_chapter_numbers": shown.large_chapter_numbers,
        "section_headings": shown.section_headings,
        "column": column_width_id(shown.column),
        "justified": shown.justified,
    }))
}

const VALUE: &[ParamSpec] = &[ParamSpec {
    name: "value",
    kind: ParamKind::String,
    required: true,
    help: "The choice's id (see `choices`)",
}];

const ON: &[ParamSpec] = &[ParamSpec {
    name: "on",
    kind: ParamKind::Boolean,
    required: false,
    help: "Turn it on (true) or off (false); without it, toggle",
}];

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "tab.reading_layout",
        label: "Reading layout",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "tab",
                kind: ParamKind::Integer,
                required: true,
                help: "The Resource Tab",
            },
            ParamSpec {
                name: "flow",
                kind: ParamKind::String,
                required: false,
                help: "paragraphs or verse_per_line",
            },
            ParamSpec {
                name: "large_chapter_numbers",
                kind: ParamKind::Boolean,
                required: false,
                help: "A drop numeral at each chapter start",
            },
            ParamSpec {
                name: "section_headings",
                kind: ParamKind::Boolean,
                required: false,
                help: "Show (true) or hide (false) section headings",
            },
            ParamSpec {
                name: "column",
                kind: ParamKind::String,
                required: false,
                help: "Reading column width: narrow, medium, wide or full",
            },
            ParamSpec {
                name: "justified",
                kind: ParamKind::Boolean,
                required: false,
                help: "Justified text, English hyphenated",
            },
            ParamSpec {
                name: "reset",
                kind: ParamKind::Boolean,
                required: false,
                help: "Forget the Tab's own choices first: it follows the defaults in Settings",
            },
        ],
        enabled: always,
        run: set_tab_layout,
    },
    CommandSpec {
        id: "view.text_flow",
        label: "Verses",
        shortcut: None,
        params: VALUE,
        enabled: always,
        run: |s, p| {
            const ID: &str = "view.text_flow";
            let v = p.str("value").unwrap_or_default();
            let flow = text_flow_from_id(v)
                .ok_or_else(|| failed(ID, format!("“{v}” is not a choice for {ID}")))?;
            s.text.layout.flow = flow;
            saved(s, ID, json!({ "value": v }))
        },
    },
    CommandSpec {
        id: "view.column_width",
        label: "Reading column width",
        shortcut: None,
        params: VALUE,
        enabled: always,
        run: |s, p| {
            const ID: &str = "view.column_width";
            let v = p.str("value").unwrap_or_default();
            let column = column_width_from_id(v)
                .ok_or_else(|| failed(ID, format!("“{v}” is not a choice for {ID}")))?;
            s.text.layout.column = column;
            saved(s, ID, json!({ "value": v }))
        },
    },
    CommandSpec {
        id: "view.large_chapter_numbers",
        label: "Large chapter numbers",
        shortcut: None,
        params: ON,
        enabled: always,
        run: |s, p| {
            toggle_default(s, p, "view.large_chapter_numbers", |l| {
                &mut l.large_chapter_numbers
            })
        },
    },
    CommandSpec {
        id: "view.section_headings",
        label: "Section headings",
        shortcut: None,
        params: ON,
        enabled: always,
        run: |s, p| toggle_default(s, p, "view.section_headings", |l| &mut l.section_headings),
    },
    CommandSpec {
        id: "view.justified",
        label: "Justified text",
        shortcut: None,
        params: ON,
        enabled: always,
        run: |s, p| toggle_default(s, p, "view.justified", |l| &mut l.justified),
    },
];
