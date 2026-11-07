//! Key bindings and Help → Keyboard shortcuts (#177).
//!
//! Every binding the user can change is a command a key can run (one that needs no parameter
//! but its Tab) or one of the Selection keys the Shell binds itself on the reading-pane
//! selection (Ctrl+Shift+N and Alt+1…9, #62, #181; the Highlight keys were Ctrl+1…9 until
//! those went to the Panels, #215). Each has a default key, its command's
//! [`CommandSpec::shortcut`]; the user's own keys override it by command id and are kept in
//! the User database (`key_bindings`), applying at once.
//!
//! Keys are written in egui notation (`Ctrl+Shift+N`, `Alt+Left`, `Ctrl+Plus`): Ctrl, Shift and
//! Alt, then one key name. Ctrl means Cmd on macOS. Ctrl+Plus is also pressed as Ctrl+= (#137),
//! so the two are the same key here.
//!
//! - `help.keyboard_shortcuts` opens the Keyboard shortcuts Tab.
//! - `keys.set {command, key, swap}` binds a key. A key another binding has is refused, naming
//!   it, unless `swap` is true: then that binding takes this one's old key. A swap never moves
//!   a reserved key (Esc, Unpin Word Info's default) off the binding it is the default of.
//! - `keys.clear {command}` leaves a binding without a key.
//! - `keys.reset {command, swap}` restores the default key (refused like `keys.set` when
//!   another binding has it now), and `keys.reset_all` every default.
//!
//! Esc, Enter, and Tab without Ctrl are never bound (they close, confirm and move the keyboard
//! focus; Alt+Tab is the system's), and a key needs Ctrl or Alt unless it is a function key: a
//! key alone or with Shift types text.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::highlight_styles::STYLE_SLOTS;
use crate::session::Session;
use crate::workspace_commands::{MENUS, MenuItem};

/// The User database preference holding the user's own keys.
pub(crate) const KEY_BINDINGS_PREF: &str = "key_bindings";

/// What a Selection key does to the reading-pane selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionAction {
    Note,
    /// Highlight with the active Highlight palette's style at this place (0 first, #181).
    Highlight(usize),
}

/// A key the Shell binds itself on the reading-pane selection (#62): not a registered command,
/// since only the Shell knows the selection, but listed and rebindable by its id like one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionKey {
    pub id: &'static str,
    pub label: &'static str,
    pub default: &'static str,
    pub action: SelectionAction,
}

const fn highlight_key(
    id: &'static str,
    label: &'static str,
    default: &'static str,
    slot: usize,
) -> SelectionKey {
    SelectionKey {
        id,
        label,
        default,
        action: SelectionAction::Highlight(slot),
    }
}

/// The Selection keys: Note, then the first nine styles of the active Highlight palette, in
/// its order (#181).
pub const SELECTION_KEYS: [SelectionKey; 1 + STYLE_SLOTS] = [
    SelectionKey {
        id: "selection.note",
        label: "Note on the selection",
        default: "Ctrl+Shift+N",
        action: SelectionAction::Note,
    },
    highlight_key(
        "selection.highlight_1",
        "Highlight with the palette's style 1",
        "Alt+1",
        0,
    ),
    highlight_key(
        "selection.highlight_2",
        "Highlight with the palette's style 2",
        "Alt+2",
        1,
    ),
    highlight_key(
        "selection.highlight_3",
        "Highlight with the palette's style 3",
        "Alt+3",
        2,
    ),
    highlight_key(
        "selection.highlight_4",
        "Highlight with the palette's style 4",
        "Alt+4",
        3,
    ),
    highlight_key(
        "selection.highlight_5",
        "Highlight with the palette's style 5",
        "Alt+5",
        4,
    ),
    highlight_key(
        "selection.highlight_6",
        "Highlight with the palette's style 6",
        "Alt+6",
        5,
    ),
    highlight_key(
        "selection.highlight_7",
        "Highlight with the palette's style 7",
        "Alt+7",
        6,
    ),
    highlight_key(
        "selection.highlight_8",
        "Highlight with the palette's style 8",
        "Alt+8",
        7,
    ),
    highlight_key(
        "selection.highlight_9",
        "Highlight with the palette's style 9",
        "Alt+9",
        8,
    ),
];

/// The ids the five Highlight Selection keys had before palettes (#177), and the slot each
/// became: keys the user set for them carry over.
const RENAMED_SELECTION_KEYS: [(&str, &str); 5] = [
    ("selection.highlight_yellow", "selection.highlight_1"),
    ("selection.highlight_green", "selection.highlight_2"),
    ("selection.highlight_blue", "selection.highlight_3"),
    ("selection.highlight_pink", "selection.highlight_4"),
    ("selection.highlight_red_underline", "selection.highlight_5"),
];

/// The group of the Selection keys in the Keyboard shortcuts Tab.
pub const SELECTION_GROUP: &str = "Selection";
/// The group of commands in no menu.
pub const OTHER_GROUP: &str = "Other commands";

/// One binding as the Keyboard shortcuts Tab lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    /// The command id, or a [`SelectionKey`]'s id.
    pub id: &'static str,
    pub label: &'static str,
    /// The menu it is in, [`SELECTION_GROUP`] or [`OTHER_GROUP`].
    pub group: &'static str,
    pub default: Option<&'static str>,
    /// The key it has now (the default unless the user changed it).
    pub key: Option<String>,
}

impl KeyBinding {
    /// Whether the user changed it from its default.
    pub fn is_custom(&self) -> bool {
        self.key.as_deref() != self.default
    }
}

/// A key other than the binding's own that already has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyConflict {
    pub id: &'static str,
    pub label: &'static str,
    pub key: String,
}

/// The user's own keys, by command id: `Some(key)`, or `None` for no key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyBindings {
    overrides: BTreeMap<String, Option<String>>,
}

impl KeyBindings {
    fn to_json(&self) -> Value {
        let map: Map<String, Value> = (self.overrides.iter())
            .map(|(id, key)| (id.clone(), key.clone().map_or(Value::Null, Value::String)))
            .collect();
        Value::Object(map)
    }

    /// The keys of a saved record; a key that no longer reads is dropped (its default
    /// applies). Keys saved under a Selection key's old id apply to its new one.
    fn from_json(v: &Value) -> Self {
        let mut overrides = BTreeMap::new();
        for (id, key) in v.as_object().into_iter().flatten() {
            let id = &RENAMED_SELECTION_KEYS
                .iter()
                .find(|(old, _)| old == id)
                .map_or_else(|| id.clone(), |(_, new)| (*new).to_string());
            match key {
                Value::Null => {
                    overrides.insert(id.clone(), None);
                }
                Value::String(k) => {
                    if let Ok(k) = canonical_key(k) {
                        overrides.insert(id.clone(), Some(k));
                    }
                }
                _ => {}
            }
        }
        Self { overrides }
    }
}

/// Modifiers in the order keys are written.
const MODIFIERS: [&str; 3] = ["Ctrl", "Shift", "Alt"];

/// Key names (egui's), with the other names the same key is written as.
const KEY_NAMES: &[(&str, &[&str])] = &[
    ("Up", &["ArrowUp"]),
    ("Down", &["ArrowDown"]),
    ("Left", &["ArrowLeft"]),
    ("Right", &["ArrowRight"]),
    ("Escape", &["Esc"]),
    ("Tab", &[]),
    ("Backspace", &[]),
    ("Enter", &["Return"]),
    ("Insert", &[]),
    ("Delete", &[]),
    ("Home", &[]),
    ("End", &[]),
    ("PageUp", &[]),
    ("PageDown", &[]),
    ("Space", &[" "]),
    ("Colon", &[":"]),
    ("Comma", &[","]),
    ("Minus", &["-"]),
    ("Period", &["."]),
    ("Plus", &[]),
    ("Equals", &["=", "Equal"]),
    ("Semicolon", &[";"]),
    ("Backslash", &["\\"]),
    ("Slash", &["/"]),
    ("Pipe", &["|"]),
    ("Questionmark", &["?"]),
    ("Exclamationmark", &["!"]),
    ("OpenBracket", &["["]),
    ("CloseBracket", &["]"]),
    ("OpenCurlyBracket", &["{"]),
    ("CloseCurlyBracket", &["}"]),
    ("Backtick", &["`"]),
    ("Quote", &["'"]),
];

/// The key name `name` is written as, if it is a key: a letter, a digit, F1–F35 or one of
/// [`KEY_NAMES`].
fn key_name(name: &str) -> Option<String> {
    if let [c] = name.as_bytes()
        && c.is_ascii_alphanumeric()
    {
        return Some(char::from(*c).to_ascii_uppercase().to_string());
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u8>().ok())
        && (1..=35).contains(&n)
        && !name.starts_with("F0")
    {
        return Some(name.to_string());
    }
    KEY_NAMES
        .iter()
        .find(|(k, aliases)| *k == name || aliases.contains(&name))
        .map(|(k, _)| (*k).to_string())
}

/// Every key name [`canonical_key`] accepts, as written: A–Z, 0–9, F1–F35 and [`KEY_NAMES`].
pub fn key_names() -> Vec<String> {
    let letters = ('A'..='Z').chain('0'..='9').map(String::from);
    let function = (1..=35).map(|n| format!("F{n}"));
    letters
        .chain(function)
        .chain(KEY_NAMES.iter().map(|(k, _)| (*k).to_string()))
        .collect()
}

/// `key` written the one way: modifiers as Ctrl, Shift, Alt in that order, then the key's
/// name. Plus drops Shift (it is Shift+= on most keyboards). An error says what is wrong.
pub fn canonical_key(key: &str) -> Result<String, String> {
    let mut held = [false; 3];
    let mut name = None;
    // `+` separates parts, so the Plus key is only ever written `Plus`.
    for part in key.trim().split('+') {
        let part = part.trim();
        if let Some(i) = MODIFIERS.iter().position(|m| m.eq_ignore_ascii_case(part)) {
            held[i] = true;
        } else if name.is_some() {
            return Err(format!("“{key}” has more than one key"));
        } else {
            name = Some(key_name(part).ok_or_else(|| format!("“{part}” is not a key"))?);
        }
    }
    let name = name.ok_or_else(|| format!("“{key}” has no key, only modifiers"))?;
    if name == "Plus" {
        held[1] = false;
    }
    let mut out: Vec<&str> = (MODIFIERS.iter().zip(held))
        .filter(|(_, on)| *on)
        .map(|(m, _)| *m)
        .collect();
    out.push(&name);
    Ok(out.join("+"))
}

/// Why `key` (canonical) cannot be bound, if it cannot.
pub fn not_bindable(key: &str) -> Option<String> {
    let (mods, name) = key.rsplit_once('+').unwrap_or(("", key));
    match name {
        "Escape" => return Some("Esc is kept for closing and cancelling".into()),
        "Enter" => return Some("Enter is kept for text fields and buttons".into()),
        // Ctrl+Tab is the Tab switcher's (#215); Alt+Tab is the system's.
        "Tab" if !mods.split('+').any(|m| m == "Ctrl") => {
            return Some("Tab is kept for moving between controls".into());
        }
        _ => {}
    }
    let function = name.len() > 1 && name.starts_with('F') && name[1..].parse::<u8>().is_ok();
    let ctrl_or_alt = mods.split('+').any(|m| m == "Ctrl" || m == "Alt");
    if !ctrl_or_alt && !function {
        return Some(format!(
            "{key} is kept for typing and moving: add Ctrl or Alt (a function key needs neither)"
        ));
    }
    None
}

/// The key reports a canonical key is pressed as: itself, and Ctrl+Plus also as Ctrl+= (#137).
fn pressed_as(key: &str) -> Vec<String> {
    let mut out = vec![key.to_string()];
    if let Some(mods) = key.strip_suffix("Plus") {
        out.push(format!("{mods}Equals"));
    }
    out
}

/// Whether two keys are pressed the same way.
fn same_key(a: &str, b: &str) -> bool {
    let (Ok(a), Ok(b)) = (canonical_key(a), canonical_key(b)) else {
        return false;
    };
    let b = pressed_as(&b);
    pressed_as(&a).iter().any(|k| b.contains(k))
}

/// Whether `spec` can run from a key: it needs no parameter but its Tab.
fn runs_from_a_key(spec: &CommandSpec) -> bool {
    spec.params.iter().all(|p| !p.required || p.name == "tab")
}

/// The menu `id` is in, if any.
fn menu_of(id: &str) -> Option<&'static str> {
    MENUS
        .iter()
        .find(|m| (m.items.iter()).any(|i| matches!(i, MenuItem::Command(c) if *c == id)))
        .map(|m| m.title)
}

/// The groups of the Keyboard shortcuts Tab, in order.
pub fn key_binding_groups() -> Vec<&'static str> {
    (MENUS.iter().map(|m| m.title))
        .chain([SELECTION_GROUP, OTHER_GROUP])
        .collect()
}

impl Session {
    /// The key `id` (a command or a [`SelectionKey`]) has now: the user's own, else its
    /// default.
    pub fn shortcut(&self, id: &str) -> Option<&str> {
        if let Some(own) = self.keys.overrides.get(id) {
            return own.as_deref();
        }
        self.default_shortcut(id)
    }

    fn default_shortcut(&self, id: &str) -> Option<&'static str> {
        if let Some(k) = SELECTION_KEYS.iter().find(|k| k.id == id) {
            return Some(k.default);
        }
        self.commands().get(id).and_then(|c| c.shortcut)
    }

    /// Every binding, grouped as [`key_binding_groups`] orders them; within a group, menu
    /// commands in menu order, then the rest in registration order.
    pub fn key_bindings(&self) -> Vec<KeyBinding> {
        let mut out: Vec<KeyBinding> = Vec::new();
        for m in MENUS {
            for item in m.items {
                if let MenuItem::Command(id) = item
                    && let Some(spec) = self.commands().get(id)
                    && runs_from_a_key(spec)
                    && out.iter().all(|b| b.id != spec.id)
                {
                    out.push(self.binding(spec.id, spec.label, m.title, spec.shortcut));
                }
            }
        }
        for k in &SELECTION_KEYS {
            out.push(self.binding(k.id, k.label, SELECTION_GROUP, Some(k.default)));
        }
        for spec in self.commands().iter() {
            if runs_from_a_key(spec) && menu_of(spec.id).is_none() {
                out.push(self.binding(spec.id, spec.label, OTHER_GROUP, spec.shortcut));
            }
        }
        out
    }

    fn binding(
        &self,
        id: &'static str,
        label: &'static str,
        group: &'static str,
        default: Option<&'static str>,
    ) -> KeyBinding {
        KeyBinding {
            id,
            label,
            group,
            default,
            key: self.shortcut(id).map(str::to_string),
        }
    }

    /// The binding other than `id` that has `key` now, if any.
    pub fn key_conflict(&self, id: &str, key: &str) -> Option<KeyConflict> {
        self.key_bindings()
            .into_iter()
            .filter(|b| b.id != id)
            .find_map(|b| {
                let k = b.key?;
                same_key(&k, key).then_some(KeyConflict {
                    id: b.id,
                    label: b.label,
                    key: k,
                })
            })
    }

    /// Each binding whose key another binding has too, with the first such other one. The
    /// commands refuse to make conflicts, but a default key that comes with a newer version
    /// can meet a key the user chose.
    pub fn key_conflicts(&self) -> BTreeMap<&'static str, KeyConflict> {
        let keyed: Vec<(KeyBinding, Vec<String>)> = (self.key_bindings().into_iter())
            .filter_map(|b| {
                let pressed = pressed_as(&canonical_key(b.key.as_deref()?).ok()?);
                Some((b, pressed))
            })
            .collect();
        let mut out = BTreeMap::new();
        for (b, pressed) in &keyed {
            let other =
                (keyed.iter()).find(|(o, p)| o.id != b.id && p.iter().any(|k| pressed.contains(k)));
            if let Some((o, _)) = other {
                out.insert(
                    b.id,
                    KeyConflict {
                        id: o.id,
                        label: o.label,
                        key: o.key.clone().unwrap_or_default(),
                    },
                );
            }
        }
        out
    }

    /// Save the user's own keys in the User database (none: kept for this run only).
    fn save_key_bindings(&self) -> Result<(), String> {
        let Some(user) = &self.user else {
            return Ok(());
        };
        user.set_preference(KEY_BINDINGS_PREF, &self.keys.to_json().to_string())
    }

    /// Read the user's own keys from the User database.
    pub(crate) fn load_key_bindings(&mut self) {
        self.keys = (self.user.as_ref())
            .and_then(|u| u.preference(KEY_BINDINGS_PREF).ok().flatten())
            .and_then(|v| serde_json::from_str::<Value>(&v).ok())
            .map(|v| KeyBindings::from_json(&v))
            .unwrap_or_default();
    }

    /// Give `id` the key `key` (none: no key), recorded as the user's own unless it is the
    /// default.
    fn put_key(&mut self, id: &str, key: Option<String>) {
        if key.as_deref() == self.default_shortcut(id) {
            self.keys.overrides.remove(id);
        } else {
            self.keys.overrides.insert(id.to_string(), key);
        }
    }

    /// Why `other` cannot take `id`'s key in a swap, if it cannot: a key kept for closing,
    /// confirming or typing (Esc, the default of Unpin Word Info, say) stays with the binding
    /// it is the default of.
    pub fn swap_refused(&self, id: &str, other: &str) -> Option<String> {
        let old = self.shortcut(id)?;
        if self.default_shortcut(other) == Some(old) {
            return None;
        }
        let why = not_bindable(&canonical_key(old).ok()?)?;
        let label = (self.key_bindings().into_iter())
            .find(|b| b.id == other)
            .map_or(other, |b| b.label);
        Some(format!("“{label}” cannot take {old}: {why}"))
    }

    /// Bind `key` (`None`: clear) to `id`; with `swap`, a binding that has the key takes `id`'s
    /// old key (refused when [`Self::swap_refused`]), else it refuses.
    fn bind(
        &mut self,
        command: &'static str,
        id: &str,
        key: Option<String>,
        swap: bool,
    ) -> Result<Value, CommandError> {
        let old = self.shortcut(id).map(str::to_string);
        let conflict = key.as_deref().and_then(|k| self.key_conflict(id, k));
        if let Some(c) = &conflict
            && !swap
        {
            return Err(failed(
                command,
                format!(
                    "{} is already the key of “{}”: swap the two keys or pick another",
                    key.as_deref().unwrap_or_default(),
                    c.label
                ),
            ));
        }
        if let Some(why) = (conflict.as_ref()).and_then(|c| self.swap_refused(id, c.id)) {
            return Err(failed(command, format!("{why}; pick another key")));
        }
        self.put_key(id, key.clone());
        if let Some(c) = &conflict {
            self.put_key(c.id, old.clone());
        }
        self.save_key_bindings()
            .map_err(|e| failed(command, format!("saving the keyboard shortcuts: {e}")))?;
        Ok(json!({
            "command": id,
            "key": key,
            "swapped": conflict.map(|c| json!({ "command": c.id, "key": old })),
        }))
    }
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// The binding id named by the `command` parameter.
fn binding_param(
    s: &Session,
    command: &'static str,
    p: &Params<'_>,
) -> Result<String, CommandError> {
    let id = p.str("command").unwrap_or_default();
    if s.key_bindings().iter().any(|b| b.id == id) {
        Ok(id.to_string())
    } else {
        Err(failed(
            command,
            format!("“{id}” is not a command that runs from a key"),
        ))
    }
}

fn run_set(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "keys.set";
    let id = binding_param(s, ID, &p)?;
    let key = canonical_key(p.str("key").unwrap_or_default()).map_err(|e| failed(ID, e))?;
    if let Some(why) = not_bindable(&key) {
        return Err(failed(ID, why));
    }
    s.bind(ID, &id, Some(key), p.bool("swap").unwrap_or(false))
}

fn run_clear(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "keys.clear";
    let id = binding_param(s, ID, &p)?;
    s.bind(ID, &id, None, false)
}

fn run_reset(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "keys.reset";
    let id = binding_param(s, ID, &p)?;
    let default = s.default_shortcut(&id).map(str::to_string);
    s.bind(ID, &id, default, p.bool("swap").unwrap_or(false))
}

fn run_reset_all(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "keys.reset_all";
    s.keys = KeyBindings::default();
    s.save_key_bindings()
        .map_err(|e| failed(ID, format!("saving the keyboard shortcuts: {e}")))?;
    Ok(Value::Null)
}

const COMMAND: ParamSpec = ParamSpec {
    name: "command",
    kind: ParamKind::String,
    required: true,
    help: "The command id (or a Selection key's id, selection.note …)",
};

const SWAP: ParamSpec = ParamSpec {
    name: "swap",
    kind: ParamKind::Boolean,
    required: false,
    help: "When another command has the key, give it this command's old key",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "keys.set",
        label: "Set keyboard shortcut",
        shortcut: None,
        params: &[
            COMMAND,
            ParamSpec {
                name: "key",
                kind: ParamKind::String,
                required: true,
                help: "The key, e.g. Ctrl+Shift+N",
            },
            SWAP,
        ],
        enabled: always,
        run: run_set,
    },
    CommandSpec {
        id: "keys.clear",
        label: "Clear keyboard shortcut",
        shortcut: None,
        params: &[COMMAND],
        enabled: always,
        run: run_clear,
    },
    CommandSpec {
        id: "keys.reset",
        label: "Reset keyboard shortcut",
        shortcut: None,
        params: &[COMMAND, SWAP],
        enabled: always,
        run: run_reset,
    },
    CommandSpec {
        id: "keys.reset_all",
        label: "Reset all keyboard shortcuts",
        shortcut: None,
        params: &[],
        enabled: always,
        run: run_reset_all,
    },
];
