//! Where the toolbar sits and what it shows (#141): its Toolbar position (Top, Bottom, Left,
//! Right), Auto-hide, and the Launchers it shows, in what order, with icons, labels or both.
//! These are app preferences kept in the User database (`toolbar`), never part of a Layout.
//!
//! The UI (`crates/ui-egui/src/shell/toolbar_bar.rs`) draws the toolbar from
//! [`ToolbarSettings`] and changes it only through these commands:
//!
//! - `view.toolbar_position {value}` (top, bottom, left, right), in View → Toolbar position
//!   and the toolbar's right-click menu; dragging its grip runs it too.
//! - `view.toolbar_auto_hide`: Auto-hide on or off.
//! - `view.toolbar_customize`: open the Customize toolbar window.
//! - `toolbar.labels {value}` (both, icons, labels), `toolbar.show_launcher {launcher, shown}`,
//!   `toolbar.move_launcher {launcher, to}` and `toolbar.reset`, from that window.
//!
//! A hidden Launcher's command (`toolbar.library` …) still runs, from its shortcut, the Command
//! box and the command list, and drops its pane down from the toolbar.

use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::toolbar::Launcher;
use crate::workspace::Effect;

/// The User database preference holding [`ToolbarSettings`].
pub(crate) const TOOLBAR_PREF: &str = "toolbar";

/// The window edge the toolbar sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ToolbarPosition {
    #[default]
    Top,
    Bottom,
    Left,
    Right,
}

impl ToolbarPosition {
    /// Each position, its id and its label, in menu order.
    pub const ALL: [(ToolbarPosition, &'static str, &'static str); 4] = [
        (ToolbarPosition::Top, "top", "Top"),
        (ToolbarPosition::Bottom, "bottom", "Bottom"),
        (ToolbarPosition::Left, "left", "Left"),
        (ToolbarPosition::Right, "right", "Right"),
    ];

    pub fn id(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|t| t.0 == self)
            .map_or("top", |t| t.1)
    }

    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|t| t.0 == self)
            .map_or("Top", |t| t.2)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().find(|t| t.1 == id).map(|t| t.0)
    }

    /// Left and Right: the Launchers are stacked, icons only.
    pub fn is_vertical(self) -> bool {
        matches!(self, ToolbarPosition::Left | ToolbarPosition::Right)
    }
}

/// What a Launcher button shows (Customize toolbar…). On a vertical toolbar it is always
/// icons only, with the label in the tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LauncherLabels {
    #[default]
    IconsAndLabels,
    IconsOnly,
    LabelsOnly,
}

impl LauncherLabels {
    pub const ALL: [(LauncherLabels, &'static str, &'static str); 3] = [
        (LauncherLabels::IconsAndLabels, "both", "Icons and labels"),
        (LauncherLabels::IconsOnly, "icons", "Icons only"),
        (LauncherLabels::LabelsOnly, "labels", "Labels only"),
    ];

    pub fn id(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|t| t.0 == self)
            .map_or("both", |t| t.1)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().find(|t| t.1 == id).map(|t| t.0)
    }
}

/// The toolbar's preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolbarSettings {
    pub position: ToolbarPosition,
    /// The toolbar is out of the way until the pointer reaches its edge or Alt is held.
    pub auto_hide: bool,
    pub labels: LauncherLabels,
    /// Every Launcher once, in toolbar order, each with whether it is shown.
    launchers: Vec<(Launcher, bool)>,
}

impl Default for ToolbarSettings {
    fn default() -> Self {
        Self {
            position: ToolbarPosition::default(),
            auto_hide: false,
            labels: LauncherLabels::default(),
            launchers: Launcher::ALL.iter().map(|l| (*l, true)).collect(),
        }
    }
}

impl ToolbarSettings {
    /// Every Launcher in toolbar order, with whether it is shown.
    pub fn launchers(&self) -> &[(Launcher, bool)] {
        &self.launchers
    }

    /// The shown Launchers, in toolbar order.
    pub fn shown(&self) -> Vec<Launcher> {
        (self.launchers.iter())
            .filter(|(_, on)| *on)
            .map(|(l, _)| *l)
            .collect()
    }

    pub fn is_shown(&self, which: Launcher) -> bool {
        self.launchers.iter().any(|(l, on)| *l == which && *on)
    }

    pub fn set_shown(&mut self, which: Launcher, shown: bool) {
        if let Some(slot) = self.launchers.iter_mut().find(|(l, _)| *l == which) {
            slot.1 = shown;
        }
    }

    /// Move `which` to place `to` (0 = first), the others keeping their order.
    pub fn move_to(&mut self, which: Launcher, to: usize) {
        let Some(from) = self.launchers.iter().position(|(l, _)| *l == which) else {
            return;
        };
        let item = self.launchers.remove(from);
        let to = to.min(self.launchers.len());
        self.launchers.insert(to, item);
    }

    /// Whether the buttons draw their icon.
    pub fn shows_icons(&self) -> bool {
        self.position.is_vertical() || self.labels != LauncherLabels::LabelsOnly
    }

    /// Whether the buttons draw their label (else it is in the tooltip).
    pub fn shows_labels(&self) -> bool {
        !self.position.is_vertical() && self.labels != LauncherLabels::IconsOnly
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "position": self.position.id(),
            "auto_hide": self.auto_hide,
            "labels": self.labels.id(),
            "launchers": self.launchers.iter().map(|(l, on)| json!({
                "id": l.id(),
                "shown": on,
            })).collect::<Vec<_>>(),
        })
    }

    /// Settings from a saved record; anything unknown or missing keeps its default, and a
    /// Launcher the record lacks is shown at the end.
    pub(crate) fn from_json(v: &Value) -> Self {
        let mut out = Self::default();
        if let Some(p) = v["position"].as_str().and_then(ToolbarPosition::from_id) {
            out.position = p;
        }
        if let Some(b) = v["auto_hide"].as_bool() {
            out.auto_hide = b;
        }
        if let Some(l) = v["labels"].as_str().and_then(LauncherLabels::from_id) {
            out.labels = l;
        }
        if let Some(list) = v["launchers"].as_array() {
            let mut launchers: Vec<(Launcher, bool)> = Vec::new();
            for item in list {
                let Some(l) = item["id"].as_str().and_then(Launcher::from_id) else {
                    continue;
                };
                if launchers.iter().all(|(seen, _)| *seen != l) {
                    launchers.push((l, item["shown"].as_bool().unwrap_or(true)));
                }
            }
            for l in Launcher::ALL {
                if launchers.iter().all(|(seen, _)| *seen != l) {
                    launchers.push((l, true));
                }
            }
            out.launchers = launchers;
        }
        out
    }
}

impl Session {
    /// Save the toolbar's preferences in the User database (none: kept for this run only).
    pub(crate) fn save_toolbar_preferences(&self) -> Result<(), String> {
        let Some(user) = &self.user else {
            return Ok(());
        };
        user.set_preference(TOOLBAR_PREF, &self.toolbar.to_json().to_string())
    }
}

/// The choices of `view.toolbar_position` and `toolbar.labels`, for menus: (id, label), and the
/// current id.
pub(crate) fn choices(
    s: &Session,
    command: &str,
) -> Option<(Vec<(&'static str, &'static str)>, &'static str)> {
    match command {
        "view.toolbar_position" => Some((
            ToolbarPosition::ALL.iter().map(|t| (t.1, t.2)).collect(),
            s.toolbar.position.id(),
        )),
        "toolbar.labels" => Some((
            LauncherLabels::ALL.iter().map(|t| (t.1, t.2)).collect(),
            s.toolbar.labels.id(),
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

/// Save after a change; the change stays for this run even when it cannot be saved.
fn saved(s: &mut Session, id: &'static str, out: Value) -> Result<Value, CommandError> {
    s.save_toolbar_preferences()
        .map_err(|e| failed(id, format!("saving the toolbar preferences: {e}")))?;
    Ok(out)
}

fn launcher_param(id: &'static str, p: &Params<'_>) -> Result<Launcher, CommandError> {
    let name = p.str("launcher").unwrap_or_default();
    Launcher::from_id(name).ok_or_else(|| {
        failed(
            id,
            format!("“{name}” is not a Launcher (library, guides, tools or layouts)"),
        )
    })
}

fn set_position(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "view.toolbar_position";
    let v = p.str("value").unwrap_or_default();
    let position = ToolbarPosition::from_id(v)
        .ok_or_else(|| failed(ID, format!("“{v}” is not top, bottom, left or right")))?;
    s.toolbar.position = position;
    saved(s, ID, json!({ "value": position.id() }))
}

fn set_labels(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "toolbar.labels";
    let v = p.str("value").unwrap_or_default();
    let labels = LauncherLabels::from_id(v)
        .ok_or_else(|| failed(ID, format!("“{v}” is not both, icons or labels")))?;
    s.toolbar.labels = labels;
    saved(s, ID, json!({ "value": labels.id() }))
}

fn toggle_auto_hide(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    s.toolbar.auto_hide = !s.toolbar.auto_hide;
    let on = s.toolbar.auto_hide;
    saved(s, "view.toolbar_auto_hide", json!({ "on": on }))
}

fn show_launcher(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "toolbar.show_launcher";
    let which = launcher_param(ID, &p)?;
    let shown = p.bool("shown").unwrap_or(true);
    s.toolbar.set_shown(which, shown);
    saved(s, ID, json!({ "launcher": which.id(), "shown": shown }))
}

fn move_launcher(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "toolbar.move_launcher";
    let which = launcher_param(ID, &p)?;
    let to = p.i64("to").unwrap_or(0).max(0) as usize;
    s.toolbar.move_to(which, to);
    let order: Vec<&str> = s.toolbar.launchers().iter().map(|(l, _)| l.id()).collect();
    saved(s, ID, json!({ "order": order }))
}

fn reset(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    s.toolbar = ToolbarSettings::default();
    saved(s, "toolbar.reset", Value::Null)
}

const VALUE: ParamSpec = ParamSpec {
    name: "value",
    kind: ParamKind::String,
    required: true,
    help: "The choice's id (see `choices`)",
};

const LAUNCHER: ParamSpec = ParamSpec {
    name: "launcher",
    kind: ParamKind::String,
    required: true,
    help: "library, guides, tools or layouts",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "view.toolbar_position",
        label: "Toolbar position",
        shortcut: None,
        params: &[VALUE],
        enabled: always,
        run: set_position,
    },
    CommandSpec {
        id: "view.toolbar_auto_hide",
        label: "Auto-hide toolbar",
        shortcut: None,
        params: &[],
        enabled: always,
        run: toggle_auto_hide,
    },
    CommandSpec {
        id: "view.toolbar_customize",
        label: "Customize toolbar…",
        shortcut: None,
        params: &[],
        enabled: always,
        run: |s, _| {
            s.workspace.push_effect(Effect::CustomizeToolbar);
            Ok(Value::Null)
        },
    },
    CommandSpec {
        id: "toolbar.labels",
        label: "Toolbar labels",
        shortcut: None,
        params: &[VALUE],
        enabled: always,
        run: set_labels,
    },
    CommandSpec {
        id: "toolbar.show_launcher",
        label: "Show or hide a Launcher",
        shortcut: None,
        params: &[
            LAUNCHER,
            ParamSpec {
                name: "shown",
                kind: ParamKind::Boolean,
                required: false,
                help: "Show it (default) or hide it",
            },
        ],
        enabled: always,
        run: show_launcher,
    },
    CommandSpec {
        id: "toolbar.move_launcher",
        label: "Move a Launcher",
        shortcut: None,
        params: &[
            LAUNCHER,
            ParamSpec {
                name: "to",
                kind: ParamKind::Integer,
                required: true,
                help: "Its new place on the toolbar, 0 first",
            },
        ],
        enabled: always,
        run: move_launcher,
    },
    CommandSpec {
        id: "toolbar.reset",
        label: "Reset toolbar",
        shortcut: None,
        params: &[],
        enabled: always,
        run: reset,
    },
];
