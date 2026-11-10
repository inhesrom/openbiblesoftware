//! A Resource's own Text size (#292): an absolute size in the readable range (12–40 pt) that
//! overrides the global Text size for every Tab of that Resource. A Resource without one
//! follows the global Text size, and the global Larger text, Smaller text and Reset never move
//! one it has.
//!
//! The sizes are kept in the User database (preference [`RESOURCE_TEXT_SIZES_PREF`], a JSON
//! object of resource id → points), not in a Layout, so they follow the Resource across Layouts
//! and the Last session.
//!
//! The commands act on a Tab's **Text size Resource** ([`Session::text_size_resource`]): a
//! Resource Tab's Resource, or in a Text Comparison or Search Tab, which shows several, the one
//! last clicked into ([`crate::workspace::Workspace::clicked_into`]), else its first row's.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::appearance::{TEXT_SIZE_MAX, TEXT_SIZE_MIN, TEXT_SIZE_STEP};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::workspace::{Effect, TabId, TabKind, Tool};

/// Preference key of the Resources' own Text sizes.
pub(crate) const RESOURCE_TEXT_SIZES_PREF: &str = "view.resource_text_sizes";

/// Each Resource's own Text size, in points, by resource id. A Resource not here follows the
/// global Text size.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResourceTextSizes {
    sizes: BTreeMap<String, f32>,
}

impl ResourceTextSizes {
    /// The Resource's own size, if it has one.
    pub fn get(&self, resource: &str) -> Option<f32> {
        self.sizes.get(resource).copied()
    }

    /// Every Resource with its own size, by resource id.
    pub fn iter(&self) -> impl Iterator<Item = (&str, f32)> {
        self.sizes.iter().map(|(id, size)| (id.as_str(), *size))
    }

    pub fn is_empty(&self) -> bool {
        self.sizes.is_empty()
    }

    /// The stored record: resource id → points.
    pub(crate) fn to_json(&self) -> Value {
        Value::Object(
            (self.sizes.iter())
                .map(|(id, size)| (id.clone(), json!(size)))
                .collect(),
        )
    }

    /// Read a stored record. A size outside the readable range is brought into it; a value
    /// that is not a number is left out (the Resource follows the global size).
    pub(crate) fn from_json(v: &Value) -> Self {
        let sizes = (v.as_object().into_iter().flatten())
            .filter_map(|(id, size)| Some((id.clone(), clamp(size.as_f64()? as f32))))
            .collect();
        Self { sizes }
    }
}

/// A size in the readable range, in whole points (the step of every Text size control).
fn clamp(size: f32) -> f32 {
    size.round().clamp(TEXT_SIZE_MIN, TEXT_SIZE_MAX)
}

impl Session {
    /// The Text size Resource `resource` is drawn at: its own size, else the global one.
    pub fn text_size_of(&self, resource: &str) -> f32 {
        self.resource_text_sizes
            .get(resource)
            .unwrap_or(self.text.size)
    }

    /// The Text settings Resource `resource` is drawn with: the global ones at its size.
    pub fn text_settings_of(&self, resource: &str) -> obs_layout::TextSettings {
        obs_layout::TextSettings {
            size: self.text_size_of(resource),
            ..self.text
        }
    }

    /// Each Resource's own Text size.
    pub fn resource_text_sizes(&self) -> &ResourceTextSizes {
        &self.resource_text_sizes
    }

    /// The Resource whose Text size Tab `tab`'s size controls change: a Resource Tab's
    /// Resource; in a Text Comparison or Search Tab the Resource last clicked into while it is
    /// still shown, else the first one shown. `None` for other Tabs, and while a Text
    /// Comparison or Search Tab shows none.
    pub fn text_size_resource(&self, tab: TabId) -> Option<String> {
        let t = self.workspace.tab(tab)?;
        let shown: Vec<String> = match &t.kind {
            TabKind::Resource(id) => return Some(id.clone()),
            TabKind::Tool(Tool::TextComparison) => t.comparison.bibles(&self.library),
            TabKind::Tool(Tool::Search) => match (&t.search.results, &t.search.library_results) {
                (Some(r), _) => r.bibles.iter().map(|b| b.id.clone()).collect(),
                (None, Some(r)) => r.groups.iter().map(|g| g.id.clone()).collect(),
                (None, None) => Vec::new(),
            },
            _ => return None,
        };
        (t.clicked_resource.as_ref())
            .filter(|id| shown.contains(id))
            .or(shown.first())
            .cloned()
    }

    /// What the Text size controls call a Resource: its abbreviation (its id when it is not in
    /// the library).
    pub fn text_size_name(&self, resource: &str) -> String {
        self.library
            .get(resource)
            .map_or_else(|| resource.to_string(), |e| e.manifest.abbreviation.clone())
    }

    /// Load the Resources' own Text sizes from the User database (none when it has none).
    pub(crate) fn load_resource_text_sizes(&mut self) {
        self.resource_text_sizes = (self.user.as_ref())
            .and_then(|u| u.preference(RESOURCE_TEXT_SIZES_PREF).ok().flatten())
            .and_then(|v| serde_json::from_str::<Value>(&v).ok())
            .map(|v| ResourceTextSizes::from_json(&v))
            .unwrap_or_default();
    }

    /// Every Resource follows the global Text size again (Settings' Reset all), saved in one
    /// write: a failed save leaves every own size as it was.
    pub(crate) fn clear_resource_text_sizes(
        &mut self,
        command: &'static str,
    ) -> Result<(), CommandError> {
        let before = std::mem::take(&mut self.resource_text_sizes);
        if let Err(e) = self.save_resource_text_sizes(command) {
            self.resource_text_sizes = before;
            return Err(e);
        }
        Ok(())
    }

    fn save_resource_text_sizes(&self, command: &'static str) -> Result<(), CommandError> {
        let Some(user) = &self.user else {
            return Ok(());
        };
        let v = self.resource_text_sizes.to_json().to_string();
        user.set_preference(RESOURCE_TEXT_SIZES_PREF, &v)
            .map_err(|e| failed(command, format!("saving the Text size: {e}")))
    }

    /// Give `resource` its own Text size (`None`: follow the global size again) and save it.
    /// Every Tab of the Resource lays its text out again at the same Current location.
    pub(crate) fn set_resource_text_size(
        &mut self,
        command: &'static str,
        resource: &str,
        size: Option<f32>,
    ) -> Result<Value, CommandError> {
        let before = self.resource_text_sizes.clone();
        match size {
            Some(size) => {
                (self.resource_text_sizes.sizes).insert(resource.to_string(), clamp(size));
            }
            None => {
                self.resource_text_sizes.sizes.remove(resource);
            }
        }
        if let Err(e) = self.save_resource_text_sizes(command) {
            self.resource_text_sizes = before;
            return Err(e);
        }
        let own = self.resource_text_sizes.get(resource).is_some();
        Ok(json!({ "resource": resource, "size": self.text_size_of(resource), "own": own }))
    }
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

const TAB: ParamSpec = ParamSpec {
    name: "tab",
    kind: ParamKind::Integer,
    required: false,
    help: "The Tab whose Resource it is (default: the focused Tab)",
};

const RESOURCE: ParamSpec = ParamSpec {
    name: "resource",
    kind: ParamKind::String,
    required: false,
    help: "The Resource's id (default: the Tab's Resource, or in a Text Comparison or Search \
           Tab the one last clicked into)",
};

/// The Resource a command acts on: `resource`, else the Text size Resource of `tab` (else of
/// the focused Tab).
fn target(s: &Session, command: &'static str, p: &Params<'_>) -> Result<String, CommandError> {
    if let Some(id) = p.str("resource").map(str::trim).filter(|id| !id.is_empty()) {
        return Ok(id.to_string());
    }
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .or_else(|| s.workspace.focused())
        .ok_or_else(|| failed(command, "no Tab"))?;
    if s.workspace.tab(tab).is_none() {
        return Err(failed(command, format!("no Tab {}", tab.0)));
    }
    s.text_size_resource(tab)
        .ok_or_else(|| failed(command, "this Tab shows no Resource"))
}

/// The focused Tab shows a Resource whose size the commands can change.
fn focused_has_text_size_resource(s: &Session) -> bool {
    (s.workspace.focused()).is_some_and(|t| s.text_size_resource(t).is_some())
}

/// A step up or down from the size the Resource is drawn at now, and the size shown briefly
/// ("KJV text 22 pt").
fn step(
    s: &mut Session,
    p: Params<'_>,
    command: &'static str,
    by: f32,
) -> Result<Value, CommandError> {
    let resource = target(s, command, &p)?;
    let now = s.text_size_of(&resource);
    // At the end of the range a Resource that follows the global size keeps following it:
    // an own size equal to it would change nothing the reader sees.
    if s.resource_text_sizes.get(&resource).is_none() && clamp(now + by) == now {
        announce(s, &resource);
        return Ok(json!({ "resource": resource, "size": now, "own": false }));
    }
    let out = s.set_resource_text_size(command, &resource, Some(now + by))?;
    announce(s, &resource);
    Ok(out)
}

fn announce(s: &mut Session, resource: &str) {
    let name = s.text_size_name(resource);
    let message = match s.resource_text_sizes.get(resource) {
        Some(size) => format!("{name} text {size} pt"),
        None => format!("{name} text follows the Text size ({} pt)", s.text.size),
    };
    s.workspace.push_effect(Effect::Message(message));
}

fn larger(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    step(s, p, "view.resource_text_larger", TEXT_SIZE_STEP)
}

fn smaller(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    step(s, p, "view.resource_text_smaller", -TEXT_SIZE_STEP)
}

fn reset(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "view.resource_text_reset";
    let resource = target(s, ID, &p)?;
    let out = s.set_resource_text_size(ID, &resource, None)?;
    announce(s, &resource);
    Ok(out)
}

/// `view.resource_text_size {value}`: the slider's form. Shows no message: the slider shows
/// the size beside it.
fn set_value(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "view.resource_text_size";
    let value = p
        .i64("value")
        .ok_or_else(|| failed(ID, "no value (points, 12–40)"))?;
    let resource = target(s, ID, &p)?;
    s.set_resource_text_size(ID, &resource, Some(value as f32))
}

// The keys are the global Text size keys with Alt added; like them, Ctrl+Alt+Plus is also
// pressed as Ctrl+Alt+= and with the numpad's + and −.
pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "view.resource_text_larger",
        label: "Larger text for this Resource",
        shortcut: Some("Ctrl+Alt+Plus"),
        params: &[TAB, RESOURCE],
        enabled: focused_has_text_size_resource,
        run: larger,
    },
    CommandSpec {
        id: "view.resource_text_smaller",
        label: "Smaller text for this Resource",
        shortcut: Some("Ctrl+Alt+Minus"),
        params: &[TAB, RESOURCE],
        enabled: focused_has_text_size_resource,
        run: smaller,
    },
    CommandSpec {
        id: "view.resource_text_reset",
        label: "Reset this Resource's Text size",
        shortcut: Some("Ctrl+Alt+0"),
        params: &[TAB, RESOURCE],
        enabled: focused_has_text_size_resource,
        run: reset,
    },
    CommandSpec {
        id: "view.resource_text_size",
        label: "Set this Resource's Text size",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "value",
                kind: ParamKind::Integer,
                required: true,
                help: "The size in points, 12–40",
            },
            TAB,
            RESOURCE,
        ],
        // The slider names its Tab, focused or not.
        enabled: always,
        run: set_value,
    },
];
