//! The command registry: every user action is a [`CommandSpec`] dispatched through
//! [`Session::run`](crate::Session::run). Menus, shortcuts, the command palette and
//! `obs-cli run` all use it (#12 rule 9).

use std::fmt;

use serde_json::{Map, Value};

use crate::session::Session;

/// Longest string parameter accepted, in bytes. Stops hostile input before a command sees it.
pub const MAX_STRING_PARAM: usize = 1 << 20;

/// The type of one command parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    String,
    Integer,
    Boolean,
}

impl ParamKind {
    pub fn name(self) -> &'static str {
        match self {
            ParamKind::String => "string",
            ParamKind::Integer => "integer",
            ParamKind::Boolean => "boolean",
        }
    }
}

/// One parameter a command accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamSpec {
    pub name: &'static str,
    pub kind: ParamKind,
    pub required: bool,
    pub help: &'static str,
}

/// Why a command did not run or did not succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    UnknownCommand(String),
    DuplicateCommand(&'static str),
    Disabled(&'static str),
    InvalidParams {
        command: &'static str,
        message: String,
    },
    Failed {
        command: &'static str,
        message: String,
    },
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::UnknownCommand(id) => write!(f, "unknown command `{id}`"),
            CommandError::DuplicateCommand(id) => write!(f, "command `{id}` is registered twice"),
            CommandError::Disabled(id) => write!(f, "command `{id}` is not available right now"),
            CommandError::InvalidParams { command, message } => {
                write!(f, "`{command}`: {message}")
            }
            CommandError::Failed { command, message } => write!(f, "`{command}` failed: {message}"),
        }
    }
}

impl std::error::Error for CommandError {}

/// Validated parameters handed to a command's `run`.
#[derive(Debug, Clone, Copy)]
pub struct Params<'a> {
    map: &'a Map<String, Value>,
}

impl<'a> Params<'a> {
    pub fn str(&self, name: &str) -> Option<&'a str> {
        self.map.get(name).and_then(Value::as_str)
    }

    pub fn i64(&self, name: &str) -> Option<i64> {
        self.map.get(name).and_then(Value::as_i64)
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        self.map.get(name).and_then(Value::as_bool)
    }
}

/// A registered command. All fields are `'static` or plain function pointers, so a spec is
/// cheap to copy out of the registry before it runs against a mutable [`Session`].
#[derive(Clone, Copy)]
pub struct CommandSpec {
    /// Stable id, `area.action`, e.g. `reference.parse`. Never renamed once shipped.
    pub id: &'static str,
    pub label: &'static str,
    /// Default shortcut in egui notation, e.g. `Ctrl+Shift+N`.
    pub shortcut: Option<&'static str>,
    pub params: &'static [ParamSpec],
    pub enabled: fn(&Session) -> bool,
    pub run: fn(&mut Session, Params<'_>) -> Result<Value, CommandError>,
}

impl fmt::Debug for CommandSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandSpec")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("shortcut", &self.shortcut)
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

/// `enabled` for commands that are always available.
pub fn always(_: &Session) -> bool {
    true
}

impl CommandSpec {
    fn invalid(&self, message: impl Into<String>) -> CommandError {
        CommandError::InvalidParams {
            command: self.id,
            message: message.into(),
        }
    }

    /// Check `params` against the spec: an object (or null for none), no unknown names,
    /// required names present, values of the declared kind, strings within
    /// [`MAX_STRING_PARAM`].
    pub fn validate<'a>(&self, params: &'a Value) -> Result<Params<'a>, CommandError> {
        static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
        let map = match params {
            Value::Null => EMPTY.get_or_init(Map::new),
            Value::Object(map) => map,
            other => {
                return Err(self.invalid(format!(
                    "parameters must be an object, not {}",
                    json_kind(other)
                )));
            }
        };
        for name in map.keys() {
            if !self.params.iter().any(|p| p.name == name) {
                return Err(self.invalid(format!("unknown parameter `{name}`")));
            }
        }
        for spec in self.params {
            match map.get(spec.name) {
                None | Some(Value::Null) if spec.required => {
                    return Err(self.invalid(format!("missing parameter `{}`", spec.name)));
                }
                None | Some(Value::Null) => {}
                Some(value) => {
                    let ok = match spec.kind {
                        ParamKind::String => value.as_str().is_some(),
                        ParamKind::Integer => value.as_i64().is_some(),
                        ParamKind::Boolean => value.is_boolean(),
                    };
                    if !ok {
                        return Err(self.invalid(format!(
                            "parameter `{}` must be {}, not {}",
                            spec.name,
                            spec.kind.name(),
                            json_kind(value)
                        )));
                    }
                    if value.as_str().is_some_and(|s| s.len() > MAX_STRING_PARAM) {
                        return Err(self.invalid(format!(
                            "parameter `{}` is longer than {MAX_STRING_PARAM} bytes",
                            spec.name
                        )));
                    }
                }
            }
        }
        Ok(Params { map })
    }

    /// Build a parameter object from `name=value` pairs (as typed on a command line),
    /// converting each value to its declared kind.
    pub fn params_from_pairs(&self, pairs: &[(&str, &str)]) -> Result<Value, CommandError> {
        let mut map = Map::new();
        for &(name, raw) in pairs {
            let Some(spec) = self.params.iter().find(|p| p.name == name) else {
                return Err(self.invalid(format!("unknown parameter `{name}`")));
            };
            let value = match spec.kind {
                ParamKind::String => Value::String(raw.to_string()),
                ParamKind::Integer => raw
                    .parse::<i64>()
                    .map(Value::from)
                    .map_err(|_| self.invalid(format!("`{name}` must be an integer")))?,
                ParamKind::Boolean => match raw {
                    "true" | "yes" | "1" => Value::Bool(true),
                    "false" | "no" | "0" => Value::Bool(false),
                    _ => return Err(self.invalid(format!("`{name}` must be true or false"))),
                },
            };
            map.insert(name.to_string(), value);
        }
        Ok(Value::Object(map))
    }
}

fn json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// All commands, in registration order.
#[derive(Debug, Default, Clone)]
pub struct CommandRegistry {
    commands: Vec<CommandSpec>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a command; ids must be unique.
    pub fn register(&mut self, spec: CommandSpec) -> Result<(), CommandError> {
        if self.get(spec.id).is_some() {
            return Err(CommandError::DuplicateCommand(spec.id));
        }
        self.commands.push(spec);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&CommandSpec> {
        self.commands.iter().find(|c| c.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &CommandSpec> {
        self.commands.iter()
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}
