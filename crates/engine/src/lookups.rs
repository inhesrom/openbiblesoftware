//! Lookups from Resource text (#142): the right-click menu's "Open in preferred Lexicon" and
//! "Look up in Dictionary", as commands so the command palette reaches them too.
//!
//! - `lexicon.open {strongs}`: the first Lexicon in Preferred order with an entry for the
//!   Strong's number (an extended number falls back to its base number, as the store does).
//! - `dictionary.lookup {text}`: the first Dictionary in Preferred order with a Headword
//!   matching the text (compared after normalization, aliases included).
//!
//! Either moves an open Tab of that Resource to the entry, or opens one; with `new_tab`
//! (Ctrl/Cmd held) a new Tab always opens.

use obs_model::{Key, KeyEntry, ResourceType};
use obs_scripture::StrongsNumber;
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::library::LibraryEntry;
use crate::session::Session;
use crate::workspace::{TabId, TabKind};

const NEW_TAB: ParamSpec = ParamSpec {
    name: "new_tab",
    kind: ParamKind::Boolean,
    required: false,
    help: "Open a new Tab even if the Resource is open (Ctrl/Cmd-click)",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "lexicon.open",
        label: "Open in preferred Lexicon",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "strongs",
                kind: ParamKind::String,
                required: true,
                help: "Strong's number, e.g. G25 or H1254A",
            },
            NEW_TAB,
        ],
        enabled: always,
        run: open_lexicon,
    },
    CommandSpec {
        id: "dictionary.lookup",
        label: "Look up in Dictionary",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "text",
                kind: ParamKind::String,
                required: true,
                help: "A headword, e.g. Abraham",
            },
            NEW_TAB,
        ],
        enabled: always,
        run: look_up,
    },
];

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn open_lexicon(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "lexicon.open";
    let text = p.str("strongs").unwrap_or_default().trim();
    let number: StrongsNumber = text.parse().map_err(|e| failed(ID, e))?;
    let found = s
        .library
        .of_type(ResourceType::Lexicon)
        .find_map(|e| strongs_entry(e, number).map(|k| (e.manifest.resource_id.clone(), k)));
    let Some((id, key)) = found else {
        return Err(failed(ID, format!("No Lexicon has an entry for {number}")));
    };
    let tab = show_entry(s, &id, key.clone(), p.bool("new_tab").unwrap_or(false))?;
    Ok(json!({ "tab": tab.0, "resource": id, "key": key }))
}

fn look_up(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "dictionary.lookup";
    let text = p.str("text").unwrap_or_default().trim();
    if obs_model::tokenize(text).is_empty() {
        return Err(failed(ID, "Select a word or name to look up"));
    }
    let found = s
        .library
        .of_type(ResourceType::Dictionary)
        .find_map(|e| headword_entry(e, text).map(|k| (e.manifest.resource_id.clone(), k)));
    let Some((id, key)) = found else {
        return Err(failed(
            ID,
            format!("No Dictionary has an entry for “{text}”"),
        ));
    };
    let tab = show_entry(s, &id, key.clone(), p.bool("new_tab").unwrap_or(false))?;
    Ok(json!({ "tab": tab.0, "resource": id, "key": key }))
}

/// Move the most recent Tab of Resource `id` to `key`, or open one there.
fn show_entry(s: &mut Session, id: &str, key: Key, new_tab: bool) -> Result<TabId, CommandError> {
    let kind = TabKind::Resource(id.to_string());
    let open = (!new_tab)
        .then(|| s.workspace.most_recent(|t| t.kind == kind))
        .flatten();
    let tab = match open {
        Some(tab) => {
            s.workspace.focus(tab);
            tab
        }
        None => {
            let out = s.run("resource.open", &json!({ "id": id, "split": new_tab }))?;
            out["tab"]
                .as_u64()
                .map(TabId)
                .ok_or_else(|| failed("resource.open", "no Tab opened"))?
        }
    };
    s.workspace.navigate_entry(tab, key);
    Ok(tab)
}

/// The Key of `entry`'s entry for `number` (its base number when it has no extended one).
#[cfg(not(target_arch = "wasm32"))]
fn strongs_entry(entry: &LibraryEntry, number: StrongsNumber) -> Option<Key> {
    use obs_store::ResourceStore;
    let reader = obs_store::ObrReader::open(&entry.path).ok()?;
    first_key(reader.lookup_strongs(number).ok()?)
}

/// The Key of `entry`'s entry whose Headword is `text`.
#[cfg(not(target_arch = "wasm32"))]
fn headword_entry(entry: &LibraryEntry, text: &str) -> Option<Key> {
    use obs_store::ResourceStore;
    let reader = obs_store::ObrReader::open(&entry.path).ok()?;
    first_key(reader.lookup_key(&Key::Headword(text.into())).ok()?)
}

/// Without files (wasm), no Resource can be looked into.
#[cfg(target_arch = "wasm32")]
fn strongs_entry(_: &LibraryEntry, _: StrongsNumber) -> Option<Key> {
    None
}

#[cfg(target_arch = "wasm32")]
fn headword_entry(_: &LibraryEntry, _: &str) -> Option<Key> {
    None
}

/// The entry's own Key rather than an alias's, when both match.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn first_key(found: Vec<KeyEntry>) -> Option<Key> {
    found
        .iter()
        .find(|e| !e.alias)
        .or_else(|| found.first())
        .map(|e| e.key.clone())
}
