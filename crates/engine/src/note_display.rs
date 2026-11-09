//! Note display (#210): how one Resource Tab shows the Notes in its text. **Icons** (the
//! default) draws each Note's marker in the text; **Margin** also sets the Notes as cards in a
//! column beside the text, each aligned to its Anchor; **Hidden** draws no Notes (Highlights
//! still show). It is the Tab's own choice (View → Note display, `view.note_display`), kept
//! with the Tab in Layouts and the Last session, like its Footnote markers (#208).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::session::Session;
use crate::workspace::{TabId, TabKind};

/// How a Resource Tab shows its Notes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoteDisplay {
    /// Note cards in a column beside the text, each beside its Note's marker. A Tab too
    /// narrow for the column shows Icons.
    Margin,
    /// A marker in the text for each Note (the default).
    #[default]
    Icons,
    /// No Notes in the text.
    Hidden,
}

impl NoteDisplay {
    /// The choices: (display, id, label).
    pub const ALL: [(NoteDisplay, &'static str, &'static str); 3] = [
        (NoteDisplay::Margin, "margin", "Margin"),
        (NoteDisplay::Icons, "icons", "Icons"),
        (NoteDisplay::Hidden, "hidden", "Hidden"),
    ];

    pub fn id(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|c| c.0 == self)
            .map_or("icons", |c| c.1)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().find(|c| c.1 == id).map(|c| c.0)
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The current choice of `view.note_display`: the focused Tab's (Icons without one), for the
/// View menu.
pub(crate) fn choices(
    s: &Session,
    command: &str,
) -> Option<(Vec<(&'static str, &'static str)>, &'static str)> {
    (command == "view.note_display").then(|| {
        let shown = (s.workspace.focused())
            .and_then(|t| s.workspace.tab(t))
            .map_or(NoteDisplay::default(), |t| t.note_display);
        (
            NoteDisplay::ALL.iter().map(|c| (c.1, c.2)).collect(),
            shown.id(),
        )
    })
}

fn failed(message: impl ToString) -> CommandError {
    CommandError::Failed {
        command: "view.note_display",
        message: message.to_string(),
    }
}

/// `view.note_display`: set how one Resource Tab shows its Notes.
fn set_note_display(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .or_else(|| s.workspace.focused())
        .ok_or_else(|| failed("no Tab"))?;
    let value = p.str("value").unwrap_or_default();
    let display = NoteDisplay::from_id(value)
        .ok_or_else(|| failed(format!("“{value}” is not margin, icons or hidden")))?;
    let t = s
        .workspace
        .tab_mut(tab)
        .ok_or_else(|| failed(format!("no Tab {}", tab.0)))?;
    if !matches!(t.kind, TabKind::Resource(_)) {
        return Err(failed("only a Resource Tab shows Notes in its text"));
    }
    t.note_display = display;
    Ok(json!({ "tab": tab.0, "value": display.id() }))
}

fn focused_resource_tab(s: &Session) -> bool {
    s.workspace
        .focused()
        .and_then(|t| s.workspace.tab(t))
        .is_some_and(|t| matches!(t.kind, TabKind::Resource(_)))
}

pub(crate) const COMMANDS: &[CommandSpec] = &[CommandSpec {
    id: "view.note_display",
    label: "Note display",
    shortcut: None,
    params: &[
        ParamSpec {
            name: "tab",
            kind: ParamKind::Integer,
            required: false,
            help: "The Resource Tab (default: the focused Tab)",
        },
        ParamSpec {
            name: "value",
            kind: ParamKind::String,
            required: true,
            help: "margin (Note cards beside the text), icons (markers in it) or hidden",
        },
    ],
    enabled: focused_resource_tab,
    run: set_note_display,
}];
