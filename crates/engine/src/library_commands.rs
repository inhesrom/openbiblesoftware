//! Library commands (#46): Preferred order (move a Resource within its type), the About Tab,
//! and removing a Resource. The Library Tab's drag handle, Move up/down, Open / About /
//! Remove… all run these through [`Session::run`].

use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::library::LibraryEntry;
use crate::session::Session;
use crate::workspace::{Effect, OpenRequest, PanelId, TabKind};

const ID: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::String,
    required: true,
    help: "Resource id from the library",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "library.move",
        label: "Move in Preferred order",
        shortcut: None,
        params: &[
            ID,
            ParamSpec {
                name: "to",
                kind: ParamKind::Integer,
                required: true,
                help: "New position within its Resource type; 0 is first (★)",
            },
        ],
        enabled: always,
        run: move_to,
    },
    CommandSpec {
        id: "library.move_up",
        label: "Move up",
        shortcut: None,
        params: &[ID],
        enabled: always,
        run: |s, p| step(s, p, true),
    },
    CommandSpec {
        id: "library.move_down",
        label: "Move down",
        shortcut: None,
        params: &[ID],
        enabled: always,
        run: |s, p| step(s, p, false),
    },
    CommandSpec {
        id: "resource.about",
        label: "About",
        shortcut: None,
        params: &[
            ID,
            ParamSpec {
                name: "panel",
                kind: ParamKind::Integer,
                required: false,
                help: "Panel to open it in",
            },
            ParamSpec {
                name: "split",
                kind: ParamKind::Boolean,
                required: false,
                help: "Open in a new Panel split off to the right (Ctrl/Cmd held)",
            },
        ],
        enabled: always,
        run: about,
    },
    CommandSpec {
        id: "library.remove",
        label: "Remove…",
        shortcut: None,
        params: &[
            ID,
            ParamSpec {
                name: "confirm",
                kind: ParamKind::Boolean,
                required: false,
                help: "The user confirmed; without it the command only asks",
            },
        ],
        enabled: always,
        run: remove,
    },
];

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// The resource id parameter, checked against the library.
fn resource<'a>(
    command: &'static str,
    s: &Session,
    p: &Params<'a>,
) -> Result<&'a str, CommandError> {
    let id = p.str("id").unwrap_or_default();
    entry(command, s, id)?;
    Ok(id)
}

/// The library entry of Resource `id`.
fn entry<'s>(
    command: &'static str,
    s: &'s Session,
    id: &str,
) -> Result<&'s LibraryEntry, CommandError> {
    s.library
        .get(id)
        .ok_or_else(|| failed(command, format!("{id} is not in the library")))
}

fn moved(s: &mut Session, id: &str, to: usize) -> Value {
    let at = s.library.move_to(id, to);
    if let Err(e) = s.save_preferred_order() {
        s.workspace.push_effect(Effect::Message(format!(
            "Could not save the Preferred order: {e}"
        )));
    }
    json!({ "id": id, "position": at })
}

fn move_to(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "library.move";
    let id = resource(CMD, s, &p)?;
    let to = p
        .i64("to")
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| failed(CMD, "`to` must be 0 or more"))?;
    Ok(moved(s, id, to))
}

fn step(s: &mut Session, p: Params<'_>, up: bool) -> Result<Value, CommandError> {
    let cmd = if up {
        "library.move_up"
    } else {
        "library.move_down"
    };
    let id = resource(cmd, s, &p)?;
    let at = s.library.position_in_type(id).unwrap_or(0);
    let to = if up { at.saturating_sub(1) } else { at + 1 };
    Ok(moved(s, id, to))
}

fn about(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "resource.about";
    let kind = TabKind::About(resource(CMD, s, &p)?.to_string());
    let title = crate::workspace_commands::title_of(s, &kind);
    let req = OpenRequest {
        panel: p
            .i64("panel")
            .and_then(|n| u64::try_from(n).ok())
            .map(PanelId),
        split: p.bool("split").unwrap_or(false),
    };
    let tab = s.workspace.open(kind, title, req);
    Ok(json!({ "tab": tab.0 }))
}

/// "1 Annotation refers" / "3 Annotations refer" / "No Annotations refer".
pub fn annotations_phrase(n: u64) -> String {
    match n {
        0 => "No Annotations refer".to_string(),
        1 => "1 Annotation refers".to_string(),
        n => format!("{n} Annotations refer"),
    }
}

fn remove(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "library.remove";
    let id = p.str("id").unwrap_or_default().to_string();
    let entry = entry(CMD, s, &id)?.clone();
    let annotations = s.annotations_referring(&id);
    let in_dir = s.library.holds_file(&entry.path);
    if !p.bool("confirm").unwrap_or(false) {
        let file = entry
            .path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        let what = if in_dir {
            format!("Its file {file} is deleted from your library.")
        } else {
            format!(
                "It is taken off the list; {file} itself is not in the library folder and stays where it is."
            )
        };
        let kept = if annotations == 0 {
            String::new()
        } else {
            "; they are kept, and come back if you import it again".to_string()
        };
        let message = format!(
            "Remove {} ({}) from your library? {what} {} to it{kept}.",
            entry.manifest.title,
            entry.manifest.abbreviation,
            annotations_phrase(annotations),
        );
        s.workspace.push_effect(Effect::ConfirmRemove {
            resource_id: id.clone(),
            title: format!("Remove {}", entry.manifest.abbreviation),
            message: message.clone(),
        });
        return Ok(json!({ "confirm": message, "annotations": annotations }));
    }
    if in_dir
        && let Err(e) = std::fs::remove_file(&entry.path)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        return Err(failed(CMD, format!("{}: {e}", entry.path.display())));
    }
    let open: Vec<_> = s
        .workspace
        .tabs()
        .filter(|t| matches!(&t.kind, TabKind::Resource(r) | TabKind::About(r) | TabKind::ImportReport(r) if *r == id))
        .map(|t| t.id)
        .collect();
    for t in open {
        s.workspace.close(t);
    }
    s.library.remove(&id);
    s.imports.reports.remove(&id);
    if let Some(user) = &s.user {
        let _ = user.set_preference(&format!("import-report/{id}"), "null");
    }
    if let Err(e) = s.save_preferred_order() {
        s.workspace.push_effect(Effect::Message(format!(
            "Could not save the Preferred order: {e}"
        )));
    }
    s.workspace.push_effect(Effect::Removed(id.clone()));
    s.workspace.push_effect(Effect::Message(format!(
        "Removed {}",
        entry.manifest.abbreviation
    )));
    Ok(json!({ "removed": id, "deleted_file": in_dir }))
}
