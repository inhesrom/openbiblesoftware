//! Collections (#153): named sets of Resources the user saves in the Library Tool, to choose
//! as a Library Search scope, as the Passage Guide's Commentaries filter and as a Library
//! filter.
//!
//! - A Collection holds Resources **chosen by hand** and, optionally, **rules**
//!   ([`CollectionRule`]: Resource type, language, Resource tag, licence, title contains).
//!   Its members are the Resources chosen by hand plus every library Resource the rules
//!   match. Rules are checked against the library each time, so a rule-based Collection
//!   takes in a Resource as soon as it is downloaded or imported.
//! - Rules of different kinds must all match; several rules of one kind match when any does
//!   (`type:lexicon type:dictionary language:greek` is every Greek Lexicon or Dictionary).
//! - **Resource tags** are the user's own labels on Resources (About Tab), for `tag:` rules.
//! - Both live in the User database, as the `library.collections` and `library.tags`
//!   preferences (JSON), like Copy presets.

use std::collections::BTreeMap;

use obs_model::{Manifest, ResourceType};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::library::{Library, language_name};
use crate::session::Session;
use crate::workspace::{Effect, TabKind, Tool};

/// One rule of a rule-based Collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionRule {
    /// `type:commentary`
    ResourceType(ResourceType),
    /// `language:greek` or `language:grc`: the language's name or its BCP 47 tag.
    Language(String),
    /// `tag:older`: a Resource tag the user gave the Resource.
    Tag(String),
    /// `licence:cc-by`: part of the Resource's SPDX licence, any case.
    Licence(String),
    /// `title:henry`: part of the Resource's title or abbreviation, any case.
    TitleContains(String),
}

impl CollectionRule {
    fn field(&self) -> &'static str {
        match self {
            CollectionRule::ResourceType(_) => "type",
            CollectionRule::Language(_) => "language",
            CollectionRule::Tag(_) => "tag",
            CollectionRule::Licence(_) => "licence",
            CollectionRule::TitleContains(_) => "title",
        }
    }

    /// Whether the rule matches a Resource with these Resource tags.
    pub fn matches(&self, m: &Manifest, tags: &[String]) -> bool {
        let has = |hay: &str, needle: &str| hay.to_lowercase().contains(&needle.to_lowercase());
        match self {
            CollectionRule::ResourceType(ty) => m.resource_type == *ty,
            CollectionRule::Language(l) => {
                let tag = m.language.split('-').next().unwrap_or(&m.language);
                l.eq_ignore_ascii_case(tag)
                    || l.eq_ignore_ascii_case(&m.language)
                    || l.eq_ignore_ascii_case(language_name(&m.language))
            }
            CollectionRule::Tag(t) => tags.iter().any(|x| x.eq_ignore_ascii_case(t)),
            CollectionRule::Licence(l) => has(&m.license, l),
            CollectionRule::TitleContains(t) => has(&m.title, t) || has(&m.abbreviation, t),
        }
    }

    /// The rule as typed: `type:commentary`, `title:"Matthew Henry"`.
    pub fn text(&self) -> String {
        let value = match self {
            CollectionRule::ResourceType(ty) => type_word(*ty).to_string(),
            CollectionRule::Language(v)
            | CollectionRule::Tag(v)
            | CollectionRule::Licence(v)
            | CollectionRule::TitleContains(v) => {
                if v.contains(char::is_whitespace) {
                    format!("\"{v}\"")
                } else {
                    v.clone()
                }
            }
        };
        format!("{}:{value}", self.field())
    }
}

/// A Resource type as a `type:` rule writes it.
fn type_word(ty: ResourceType) -> &'static str {
    match ty {
        ResourceType::Bible => "bible",
        ResourceType::Commentary => "commentary",
        ResourceType::Lexicon => "lexicon",
        ResourceType::Dictionary => "dictionary",
        ResourceType::CrossReferenceSet => "cross-reference-set",
        ResourceType::PeopleAndPlaces => "people-and-places",
    }
}

/// A Resource type from a `type:` rule's value (singular or plural, any case).
fn type_from_word(word: &str) -> Option<ResourceType> {
    let w = word.trim().to_lowercase().replace(['_', ' '], "-");
    Some(match w.as_str() {
        "bible" | "bibles" => ResourceType::Bible,
        "commentary" | "commentaries" => ResourceType::Commentary,
        "lexicon" | "lexicons" => ResourceType::Lexicon,
        "dictionary" | "dictionaries" => ResourceType::Dictionary,
        "cross-reference-set" | "cross-reference-sets" | "cross-references" => {
            ResourceType::CrossReferenceSet
        }
        "people-and-places" | "people-&-places" | "people-and-places-index" => {
            ResourceType::PeopleAndPlaces
        }
        _ => return None,
    })
}

/// Read rules as typed: `field:value` pairs separated by spaces, a value with spaces in
/// double quotes. Fields: `type`, `language` (`lang`), `tag`, `licence` (`license`), `title`.
pub fn parse_rules(text: &str) -> Result<Vec<CollectionRule>, String> {
    let mut rules = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let Some((field, after)) = rest.split_once(':') else {
            return Err(format!(
                "“{rest}” is not a rule: write field:value, e.g. type:commentary"
            ));
        };
        let field = field.trim().to_lowercase();
        let after = after.trim_start();
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted
                .find('"')
                .ok_or_else(|| format!("{field}: the quoted value has no closing \""))?;
            (&quoted[..end], &quoted[end + 1..])
        } else {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            (&after[..end], &after[end..])
        };
        let value = value.trim();
        if value.is_empty() {
            return Err(format!("{field}: needs a value"));
        }
        rules.push(match field.as_str() {
            "type" => CollectionRule::ResourceType(type_from_word(value).ok_or_else(|| {
                format!(
                    "type:{value} is not a Resource type (bible, commentary, lexicon, \
                     dictionary, cross-reference-set, people-and-places)"
                )
            })?),
            "language" | "lang" => CollectionRule::Language(value.to_string()),
            "tag" => CollectionRule::Tag(value.to_string()),
            "licence" | "license" => CollectionRule::Licence(value.to_string()),
            "title" => CollectionRule::TitleContains(value.to_string()),
            other => {
                return Err(format!(
                    "“{other}” is not a rule field (type, language, tag, licence, title)"
                ));
            }
        });
        rest = next.trim_start();
    }
    Ok(rules)
}

/// Rules as typed, separated by spaces.
pub fn rules_text(rules: &[CollectionRule]) -> String {
    rules
        .iter()
        .map(CollectionRule::text)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A **Collection**: a named set of Resources, chosen by hand and/or by rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub name: String,
    /// Empty for a Collection made only by hand.
    #[serde(default)]
    pub rules: Vec<CollectionRule>,
    /// Resource ids chosen by hand (they may name Resources not in the library right now).
    #[serde(default)]
    pub resources: Vec<String>,
}

impl Collection {
    /// Whether a Resource with these Resource tags is in the Collection: chosen by hand, or
    /// matched by its rules (each kind of rule by at least one of its rules).
    pub fn includes(&self, m: &Manifest, tags: &[String]) -> bool {
        if self.resources.contains(&m.resource_id) {
            return true;
        }
        if self.rules.is_empty() {
            return false;
        }
        let mut fields: Vec<&str> = self.rules.iter().map(CollectionRule::field).collect();
        fields.dedup();
        fields.iter().all(|f| {
            self.rules
                .iter()
                .filter(|r| r.field() == *f)
                .any(|r| r.matches(m, tags))
        })
    }
}

/// The Collections and Resource tags, as the library keeps them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollectionSet {
    pub collections: Vec<Collection>,
    /// Resource id → its Resource tags.
    pub tags: BTreeMap<String, Vec<String>>,
}

impl Library {
    /// The saved Collections, in the order made.
    pub fn collections(&self) -> &[Collection] {
        &self.collection_set().collections
    }

    /// A Collection by name (any case).
    pub fn collection(&self, name: &str) -> Option<&Collection> {
        self.collections()
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name.trim()))
    }

    /// A Resource's Resource tags.
    pub fn tags_of(&self, resource_id: &str) -> &[String] {
        self.collection_set()
            .tags
            .get(resource_id)
            .map_or(&[], Vec::as_slice)
    }

    /// Whether a library Resource is in the Collection `name` (false for no such Collection).
    pub fn in_collection(&self, name: &str, resource_id: &str) -> bool {
        match (self.collection(name), self.get(resource_id)) {
            (Some(c), Some(e)) => c.includes(&e.manifest, self.tags_of(resource_id)),
            _ => false,
        }
    }

    /// The library Resources in the Collection `name`, by Resource type in Preferred order.
    pub fn collection_members(&self, name: &str) -> Vec<String> {
        self.entries()
            .iter()
            .filter(|e| self.in_collection(name, &e.manifest.resource_id))
            .map(|e| e.manifest.resource_id.clone())
            .collect()
    }
}

// ---------------------------------------------------------------------------------- session

const COLLECTIONS_PREF: &str = "library.collections";
const TAGS_PREF: &str = "library.tags";

impl Session {
    /// Read the Collections and Resource tags from the User database.
    pub(crate) fn load_collections(&mut self) {
        let Some(user) = &self.user else {
            return;
        };
        let mut set = CollectionSet::default();
        if let Ok(Some(json)) = user.preference(COLLECTIONS_PREF)
            && let Ok(c) = serde_json::from_str(&json)
        {
            set.collections = c;
        }
        if let Ok(Some(json)) = user.preference(TAGS_PREF)
            && let Ok(t) = serde_json::from_str(&json)
        {
            set.tags = t;
        }
        self.library.set_collection_set(set);
    }

    fn save_collections(&mut self) {
        let Some(user) = &self.user else {
            return;
        };
        let set = self.library.collection_set();
        let collections = serde_json::to_string(&set.collections).unwrap_or_default();
        let tags = serde_json::to_string(&set.tags).unwrap_or_default();
        if let Err(e) = user
            .set_preference(COLLECTIONS_PREF, &collections)
            .and_then(|()| user.set_preference(TAGS_PREF, &tags))
        {
            self.workspace.push_effect(Effect::Message(format!(
                "Could not save the Collections: {e}"
            )));
        }
    }

    /// A Collection was renamed (`to: Some`) or deleted: follow it in Search Tabs' scopes and
    /// the Guide filters.
    fn collection_renamed(&mut self, from: &str, to: Option<&str>) {
        let tabs: Vec<_> = self
            .workspace
            .tabs()
            .filter(|t| t.kind == TabKind::Tool(Tool::Search))
            .map(|t| t.id)
            .collect();
        for tab in tabs {
            if let Some(st) = self.search_tab_mut(tab) {
                st.library.rename_collection(from, to);
            }
        }
        self.rename_guide_filters(from, to);
    }
}

fn collection_json(library: &Library, c: &Collection) -> Value {
    json!({
        "name": c.name,
        "rules": rules_text(&c.rules),
        "resources": c.resources,
        "members": library.collection_members(&c.name),
    })
}

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

/// A Collection's name from `name`: trimmed, not empty, no commas (scopes list them
/// comma-separated).
fn name_param(command: &'static str, p: &Params<'_>, field: &str) -> Result<String, CommandError> {
    let name = p.str(field).unwrap_or_default().trim().to_string();
    if name.is_empty() {
        return Err(failed(command, "a Collection needs a name"));
    }
    if name.contains(',') {
        return Err(failed(
            command,
            "a Collection's name cannot contain a comma",
        ));
    }
    Ok(name)
}

fn ids_param(p: &Params<'_>, field: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in p
        .str(field)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        if !out.iter().any(|o| o == id) {
            out.push(id.to_string());
        }
    }
    out
}

const NAME: ParamSpec = ParamSpec {
    name: "name",
    kind: ParamKind::String,
    required: true,
    help: "The Collection's name",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "collection.save",
        label: "Save a Collection",
        shortcut: None,
        params: &[
            NAME,
            ParamSpec {
                name: "rules",
                kind: ParamKind::String,
                required: false,
                help: "Rules, e.g. `type:commentary language:en tag:older licence:cc-by title:henry` (empty: by hand only)",
            },
            ParamSpec {
                name: "resources",
                kind: ParamKind::String,
                required: false,
                help: "Resource ids chosen by hand, comma-separated",
            },
            ParamSpec {
                name: "previous",
                kind: ParamKind::String,
                required: false,
                help: "The Collection's name before this edit, when renaming it",
            },
        ],
        enabled: always,
        run: run_save,
    },
    CommandSpec {
        id: "collection.toggle",
        label: "Add to or remove from a Collection",
        shortcut: None,
        params: &[
            NAME,
            ParamSpec {
                name: "id",
                kind: ParamKind::String,
                required: true,
                help: "Resource id: added by hand, or taken out if it was added by hand",
            },
        ],
        enabled: always,
        run: run_toggle,
    },
    CommandSpec {
        id: "collection.delete",
        label: "Delete a Collection",
        shortcut: None,
        params: &[NAME],
        enabled: always,
        run: run_delete,
    },
    CommandSpec {
        id: "collection.list",
        label: "List the Collections",
        shortcut: None,
        params: &[],
        enabled: always,
        run: |s, _| {
            let lib = &s.library;
            Ok(Value::Array(
                lib.collections()
                    .iter()
                    .map(|c| collection_json(lib, c))
                    .collect(),
            ))
        },
    },
    CommandSpec {
        id: "resource.tags",
        label: "Set a Resource's tags",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "id",
                kind: ParamKind::String,
                required: true,
                help: "Resource id from the library",
            },
            ParamSpec {
                name: "tags",
                kind: ParamKind::String,
                required: true,
                help: "Resource tags, comma-separated (empty: none)",
            },
        ],
        enabled: always,
        run: run_tags,
    },
];

fn run_save(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "collection.save";
    let name = name_param(CMD, &p, "name")?;
    let rules = parse_rules(p.str("rules").unwrap_or_default()).map_err(|e| failed(CMD, e))?;
    let resources = ids_param(&p, "resources");
    if let Some(id) = resources.iter().find(|id| s.library.get(id).is_none()) {
        return Err(failed(CMD, format!("{id} is not in the library")));
    }
    let previous = p
        .str("previous")
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    let mut set = s.library.collection_set().clone();
    let replaces = previous.as_deref().unwrap_or(&name);
    let at = set
        .collections
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(replaces));
    if previous.is_some() && at.is_none() {
        return Err(failed(CMD, format!("no Collection named “{replaces}”")));
    }
    if set
        .collections
        .iter()
        .enumerate()
        .any(|(i, c)| Some(i) != at && c.name.eq_ignore_ascii_case(&name))
    {
        return Err(failed(
            CMD,
            format!("there is already a Collection named “{name}”"),
        ));
    }
    let collection = Collection {
        name: name.clone(),
        rules,
        resources,
    };
    let old = match at {
        Some(i) => Some(std::mem::replace(&mut set.collections[i], collection).name),
        None => {
            set.collections.push(collection);
            None
        }
    };
    s.library.set_collection_set(set);
    s.save_collections();
    if let Some(old) = old
        && old != name
    {
        s.collection_renamed(&old, Some(&name));
    }
    let lib = &s.library;
    Ok(lib
        .collection(&name)
        .map(|c| collection_json(lib, c))
        .unwrap_or_default())
}

fn run_toggle(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "collection.toggle";
    let name = name_param(CMD, &p, "name")?;
    let id = p.str("id").unwrap_or_default().trim().to_string();
    if s.library.get(&id).is_none() {
        return Err(failed(CMD, format!("{id} is not in the library")));
    }
    let mut set = s.library.collection_set().clone();
    let c = set
        .collections
        .iter_mut()
        .find(|c| c.name.eq_ignore_ascii_case(&name))
        .ok_or_else(|| failed(CMD, format!("no Collection named “{name}”")))?;
    if let Some(i) = c.resources.iter().position(|r| *r == id) {
        c.resources.remove(i);
    } else {
        c.resources.push(id);
    }
    s.library.set_collection_set(set);
    s.save_collections();
    let lib = &s.library;
    Ok(lib
        .collection(&name)
        .map(|c| collection_json(lib, c))
        .unwrap_or_default())
}

fn run_delete(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "collection.delete";
    let name = name_param(CMD, &p, "name")?;
    let mut set = s.library.collection_set().clone();
    let i = set
        .collections
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(&name))
        .ok_or_else(|| failed(CMD, format!("no Collection named “{name}”")))?;
    let gone = set.collections.remove(i);
    s.library.set_collection_set(set);
    s.save_collections();
    s.collection_renamed(&gone.name, None);
    Ok(json!({ "deleted": gone.name }))
}

fn run_tags(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "resource.tags";
    let id = p.str("id").unwrap_or_default().trim().to_string();
    if s.library.get(&id).is_none() {
        return Err(failed(CMD, format!("{id} is not in the library")));
    }
    let mut tags: Vec<String> = Vec::new();
    for t in p
        .str("tags")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            tags.push(t.to_string());
        }
    }
    let mut set = s.library.collection_set().clone();
    if tags.is_empty() {
        set.tags.remove(&id);
    } else {
        set.tags.insert(id.clone(), tags.clone());
    }
    s.library.set_collection_set(set);
    s.save_collections();
    Ok(json!({ "id": id, "tags": tags }))
}
