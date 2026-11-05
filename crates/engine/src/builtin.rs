//! Built-in commands available in every Session. Later tickets add theirs here or in their
//! own modules' `register` functions.

use obs_scripture::{RefStyle, Standard, format_passage, format_passage_list, parse_reference};
use serde_json::{Value, json};

use crate::command::{
    CommandError, CommandRegistry, CommandSpec, ParamKind, ParamSpec, Params, always,
};
use crate::session::Session;

const TEXT: ParamSpec = ParamSpec {
    name: "text",
    kind: ParamKind::String,
    required: true,
    help: "Text to work on",
};

const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "app.version",
        label: "About this build",
        shortcut: None,
        params: &[],
        enabled: always,
        run: version,
    },
    CommandSpec {
        id: "app.commands",
        label: "List commands",
        shortcut: Some("Ctrl+Shift+P"),
        params: &[],
        enabled: always,
        run: list_commands,
    },
    CommandSpec {
        id: "reference.parse",
        label: "Parse a Bible reference",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "text",
                kind: ParamKind::String,
                required: true,
                help: "Bible reference, e.g. `Jn 3:16-18; Rom 5:8`",
            },
            ParamSpec {
                name: "context",
                kind: ParamKind::String,
                required: false,
                help: "Verse that relative forms (`2:4`, `5`) are read against",
            },
            ParamSpec {
                name: "long",
                kind: ParamKind::Boolean,
                required: false,
                help: "Use full book names in the output",
            },
        ],
        enabled: always,
        run: parse_ref,
    },
    CommandSpec {
        id: "text.normalize",
        label: "Normalize text for search",
        shortcut: None,
        params: &[TEXT],
        enabled: always,
        run: normalize,
    },
    CommandSpec {
        id: "morph.expand",
        label: "Explain a morphology code",
        shortcut: None,
        params: &[ParamSpec {
            name: "code",
            kind: ParamKind::String,
            required: true,
            help: "STEPBible Hebrew or Greek code, e.g. `HVqp3ms` or `V-AAI-3S`",
        }],
        enabled: always,
        run: morph,
    },
    CommandSpec {
        id: "text.transliterate",
        label: "Transliterate Greek or Hebrew",
        shortcut: None,
        params: &[TEXT],
        enabled: always,
        run: transliterate,
    },
];

/// A registry holding every built-in command.
pub fn registry() -> CommandRegistry {
    let mut r = CommandRegistry::new();
    for spec in COMMANDS
        .iter()
        .chain(crate::workspace_commands::COMMANDS)
        .chain(crate::library_commands::COMMANDS)
        .chain(crate::import::COMMANDS)
        .chain(crate::get_bibles::COMMANDS)
        .chain(crate::compare::COMMANDS)
        .chain(crate::annotations::COMMANDS)
        .chain(crate::notes::COMMANDS)
        .chain(crate::search::COMMANDS)
        .chain(crate::guide::COMMANDS)
        .chain(crate::bible_search::COMMANDS)
        .chain(crate::word_study::COMMANDS)
        .chain(crate::library_search::COMMANDS)
        .chain(crate::word_sections::COMMANDS)
    {
        // Ids in COMMANDS are unique (checked by a test), so registration cannot fail.
        let _ = r.register(*spec);
    }
    r
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn version(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Ok(json!({ "version": env!("CARGO_PKG_VERSION") }))
}

fn list_commands(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    let list: Vec<Value> = s
        .commands()
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "label": c.label,
                "shortcut": c.shortcut,
                "params": c.params.iter().map(|p| json!({
                    "name": p.name,
                    "kind": p.kind.name(),
                    "required": p.required,
                    "help": p.help,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Value::Array(list))
}

fn parse_ref(_: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "reference.parse";
    let text = p.str("text").unwrap_or_default();
    let context = match p.str("context") {
        None => None,
        Some(c) => {
            let list = parse_reference(c, &Standard, None)
                .map_err(|e| failed(ID, format!("context: {e}")))?;
            list.passages().first().map(|p| p.start())
        }
    };
    let style = if p.bool("long").unwrap_or(false) {
        RefStyle::Long
    } else {
        RefStyle::Short
    };
    let list = parse_reference(text, &Standard, context).map_err(|e| CommandError::Failed {
        command: ID,
        message: format!("{} at bytes {}..{}", e.kind, e.span.start, e.span.end),
    })?;
    let passages: Vec<Value> = list
        .iter()
        .map(|p| {
            json!({
                "text": format_passage(*p, &Standard, style),
                "start": p.start().osis(),
                "end": p.end().osis(),
            })
        })
        .collect();
    Ok(json!({
        "text": format_passage_list(&list, &Standard, style),
        "passages": passages,
    }))
}

fn normalize(_: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let text = p.str("text").unwrap_or_default();
    let tokens: Vec<Value> = obs_model::tokenize(text)
        .into_iter()
        .map(|t| json!({ "text": t.text, "start": t.range.start, "end": t.range.end }))
        .collect();
    Ok(json!({ "normalized": obs_model::normalize(text), "tokens": tokens }))
}

fn morph(_: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let code = p.str("code").unwrap_or_default();
    let m = obs_model::morph::parse(code).map_err(|e| failed("morph.expand", e))?;
    Ok(json!({
        "code": m.code(),
        "short": m.short(),
        "long": m.long(),
        "part_of_speech": m.pos().map(obs_model::PartOfSpeech::name),
        "function_word": m.is_function_word(),
    }))
}

fn transliterate(_: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let text = p.str("text").unwrap_or_default();
    Ok(json!({ "transliteration": obs_model::transliterate(text) }))
}
