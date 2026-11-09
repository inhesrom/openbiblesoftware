//! The Custom theme and the Theme editor (#224): the chrome's colour roles as the user set
//! them, starting from a built-in theme.
//!
//! - A [`CustomTheme`] is a built-in base (Light, Dark, High contrast light or dark) and the
//!   colour roles the user changed; the others stay the base's. The base's colours, and the
//!   contrast warnings the editor shows, belong to the UI, which owns the colours.
//! - Every edit makes the Custom theme the app's Theme, so the whole window previews it as
//!   the user edits, and is saved at once in the User database (`custom-theme`).
//! - **Export** writes it as a small JSON file ([`ThemeFile`]); **Import** replaces the
//!   Custom theme with one and uses it.
//!
//! Commands: `view.theme_editor` (open the window), `theme.start_from {base}`,
//! `theme.set_color {role, color}`, `theme.export {path}` and `theme.import {path | text}`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::appearance::Theme;
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::highlight_styles::Rgb;
use crate::session::Session;
use crate::workspace::Effect;

/// The User database preference holding the Custom theme, as a [`ThemeFile`].
pub const CUSTOM_THEME_PREF: &str = "custom-theme";

/// `format` of an exported theme file.
pub const THEME_FILE_FORMAT: &str = "openbible-theme";

/// The newest theme file `version` this build reads.
pub const THEME_FILE_VERSION: u32 = 1;

/// A colour role of the chrome: what the Theme editor changes. The ids are stable; they are
/// the keys of a theme file's `colors`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ThemeRole {
    Shell,
    Surface,
    Field,
    Rule,
    Ink,
    Muted,
    Lapis,
    Verdigris,
    Violet,
    Gilt,
    Selection,
    Hover,
    HoverRule,
    Warn,
    Unlinked,
}

impl ThemeRole {
    /// Every role, with its id and its name in the Theme editor, in the editor's order.
    pub const ALL: [(ThemeRole, &'static str, &'static str); 15] = [
        (ThemeRole::Shell, "shell", "Window background"),
        (ThemeRole::Surface, "surface", "Panel background"),
        (ThemeRole::Field, "field", "Field background"),
        (ThemeRole::Rule, "rule", "Borders and rules"),
        (ThemeRole::Ink, "ink", "Text"),
        (ThemeRole::Muted, "muted", "Secondary text"),
        (ThemeRole::Lapis, "lapis", "Links, focus and Link set A"),
        (
            ThemeRole::Verdigris,
            "verdigris",
            "Study accent and Link set B",
        ),
        (ThemeRole::Violet, "violet", "Link set C"),
        (ThemeRole::Gilt, "gilt", "Stars and Link set D"),
        (ThemeRole::Selection, "selection", "Selection"),
        (ThemeRole::Hover, "hover", "Hover"),
        (ThemeRole::HoverRule, "hover_rule", "Hover border"),
        (ThemeRole::Warn, "warn", "Warnings"),
        (ThemeRole::Unlinked, "unlinked", "Unlinked Tab chip"),
    ];

    pub fn id(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|r| r.0 == self)
            .map_or("ink", |r| r.1)
    }

    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|r| r.0 == self)
            .map_or("Text", |r| r.2)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().find(|r| r.1 == id).map(|r| r.0)
    }
}

/// The user's own theme: a built-in base and the roles changed from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomTheme {
    /// One of [`Theme::BUILT_IN`].
    pub base: Theme,
    /// The roles the user changed; the rest are the base's.
    pub colors: BTreeMap<ThemeRole, Rgb>,
}

impl Default for CustomTheme {
    fn default() -> Self {
        Self {
            base: Theme::Light,
            colors: BTreeMap::new(),
        }
    }
}

/// A Custom theme as exported: a small JSON file another user (or another computer) imports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeFile {
    pub format: String,
    pub version: u32,
    /// The id of the built-in theme it starts from (`light`, `high_contrast_dark` …).
    pub base: String,
    /// Role id → `#rrggbb`, for the roles changed from the base.
    #[serde(default)]
    pub colors: BTreeMap<String, Rgb>,
}

impl CustomTheme {
    /// The theme as a file's contents.
    pub fn to_file(&self) -> ThemeFile {
        ThemeFile {
            format: THEME_FILE_FORMAT.into(),
            version: THEME_FILE_VERSION,
            base: self.base.id().into(),
            colors: (self.colors.iter())
                .map(|(role, c)| (role.id().to_string(), *c))
                .collect(),
        }
    }

    /// The theme a file holds, or why it holds none.
    pub fn from_file(file: &ThemeFile) -> Result<Self, String> {
        if file.format != THEME_FILE_FORMAT {
            return Err(format!(
                "This is not a theme file (its format is “{}”)",
                file.format
            ));
        }
        if file.version > THEME_FILE_VERSION {
            return Err(format!(
                "This theme file is version {}; this version of the app reads up to {}",
                file.version, THEME_FILE_VERSION
            ));
        }
        let base = built_in(&file.base)?;
        let mut colors = BTreeMap::new();
        for (id, c) in &file.colors {
            let role = ThemeRole::from_id(id)
                .ok_or_else(|| format!("“{id}” is not a colour role of a theme"))?;
            colors.insert(role, *c);
        }
        Ok(Self { base, colors })
    }
}

/// The built-in theme with id `id`, which a Custom theme can start from.
fn built_in(id: &str) -> Result<Theme, String> {
    Theme::from_id(id)
        .filter(|t| Theme::BUILT_IN.contains(t))
        .ok_or_else(|| {
            format!(
                "“{id}” is not a theme to start from (light, dark, high_contrast_light or \
                 high_contrast_dark)"
            )
        })
}

impl Session {
    /// Read the Custom theme saved in the User database (the default when none is, or it is
    /// unreadable).
    pub(crate) fn load_custom_theme(&mut self) {
        self.custom_theme = (self.user.as_ref())
            .and_then(|u| u.preference(CUSTOM_THEME_PREF).ok().flatten())
            .and_then(|text| serde_json::from_str::<ThemeFile>(&text).ok())
            .and_then(|file| CustomTheme::from_file(&file).ok())
            .unwrap_or_default();
    }

    /// Make `theme` the Custom theme and the app's Theme, and save both unless `save` is
    /// false (a colour picker being dragged saves once, when let go). Each is changed in
    /// memory only once it is saved, so a failed write leaves the app as the database has it.
    fn use_custom_theme(
        &mut self,
        cmd: &'static str,
        theme: CustomTheme,
        save: bool,
    ) -> Result<Value, CommandError> {
        let file = theme.to_file();
        if save && let Some(user) = &self.user {
            let text = serde_json::to_string(&file).map_err(|e| failed(cmd, e))?;
            user.set_preference(CUSTOM_THEME_PREF, &text)
                .map_err(|e| failed(cmd, format!("saving the Custom theme: {e}")))?;
        }
        self.custom_theme = theme;
        let previous = std::mem::replace(&mut self.appearance.theme, Theme::Custom);
        if save && let Err(e) = self.save_view_preferences() {
            self.appearance.theme = previous;
            return Err(failed(cmd, format!("saving preferences: {e}")));
        }
        Ok(json!({ "theme": file }))
    }
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// Export and import read and write files the user picked, as the other exports do, only
/// with the User database open (always so in the app).
fn has_user(s: &Session) -> bool {
    s.user.is_some()
}

const PATH: ParamSpec = ParamSpec {
    name: "path",
    kind: ParamKind::String,
    required: false,
    help: "Write the JSON file here (otherwise only returned)",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "view.theme_editor",
        label: "Theme editor",
        shortcut: None,
        params: &[],
        enabled: always,
        run: |s, _| {
            s.workspace.push_effect(Effect::ShowThemeEditor);
            Ok(Value::Null)
        },
    },
    CommandSpec {
        id: "theme.start_from",
        label: "Start the Custom theme from a built-in theme",
        shortcut: None,
        params: &[ParamSpec {
            name: "base",
            kind: ParamKind::String,
            required: true,
            help: "light, dark, high_contrast_light or high_contrast_dark",
        }],
        enabled: always,
        run: run_start_from,
    },
    CommandSpec {
        id: "theme.set_color",
        label: "Set a colour of the Custom theme",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "role",
                kind: ParamKind::String,
                required: true,
                help: "The colour role (ink, shell, lapis …)",
            },
            ParamSpec {
                name: "color",
                kind: ParamKind::String,
                required: false,
                help: "#rrggbb; empty or absent puts the base theme's colour back",
            },
            ParamSpec {
                name: "save",
                kind: ParamKind::Boolean,
                required: false,
                help: "false: in memory only, while a colour picker is dragged (default true)",
            },
        ],
        enabled: always,
        run: run_set_color,
    },
    CommandSpec {
        id: "theme.export",
        label: "Export the Custom theme",
        shortcut: None,
        params: &[PATH],
        enabled: has_user,
        run: run_export,
    },
    CommandSpec {
        id: "theme.import",
        label: "Import a theme",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "path",
                kind: ParamKind::String,
                required: false,
                help: "A theme's JSON file",
            },
            ParamSpec {
                name: "text",
                kind: ParamKind::String,
                required: false,
                help: "The JSON itself, instead of a file",
            },
        ],
        enabled: has_user,
        run: run_import,
    },
];

fn run_start_from(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "theme.start_from";
    let base = built_in(p.str("base").unwrap_or_default()).map_err(|e| failed(CMD, e))?;
    s.use_custom_theme(
        CMD,
        CustomTheme {
            base,
            colors: BTreeMap::new(),
        },
        true,
    )
}

fn run_set_color(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "theme.set_color";
    let id = p.str("role").unwrap_or_default();
    let role = ThemeRole::from_id(id)
        .ok_or_else(|| failed(CMD, format!("“{id}” is not a colour role of a theme")))?;
    let mut theme = s.custom_theme.clone();
    match p.str("color").map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => {
            theme
                .colors
                .insert(role, c.parse::<Rgb>().map_err(|e| failed(CMD, e))?);
        }
        None => {
            theme.colors.remove(&role);
        }
    }
    s.use_custom_theme(CMD, theme, p.bool("save").unwrap_or(true))
}

fn run_export(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "theme.export";
    let text =
        serde_json::to_string_pretty(&s.custom_theme.to_file()).map_err(|e| failed(CMD, e))?;
    if let Some(path) = p.str("path") {
        s.write_export(std::path::Path::new(path), text.as_bytes())
            .map_err(|e| failed(CMD, e))?;
    }
    Ok(json!({ "text": text }))
}

fn run_import(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "theme.import";
    let text = match (p.str("text"), p.str("path")) {
        (Some(t), _) => t.to_string(),
        (None, Some(path)) => {
            std::fs::read_to_string(path).map_err(|e| failed(CMD, format!("{path}: {e}")))?
        }
        (None, None) => return Err(failed(CMD, "give a `path` or the `text` of a theme")),
    };
    let file: ThemeFile = serde_json::from_str(&text)
        .map_err(|e| failed(CMD, format!("This is not a theme file: {e}")))?;
    let theme = CustomTheme::from_file(&file).map_err(|e| failed(CMD, e))?;
    s.use_custom_theme(CMD, theme, true)
}
