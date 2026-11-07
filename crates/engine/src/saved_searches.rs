//! Saved searches and the Search history (#179).
//!
//! - A **Saved search** is a [`SavedSearch`] (mode, query and Search scope: what the Search
//!   Tool's fields held) kept under a name the user gives it. Names ignore case: saving under a
//!   name already used replaces that Saved search.
//! - The **Search history** keeps every search run in a Search Tab, newest first, at most
//!   [`SEARCH_HISTORY_LIMIT`]; running one again moves it to the top. It can be cleared. It is
//!   not History (the Tool listing Visits), though a search also makes a Visit there.
//!
//! Running either opens the Search Tool with every field as kept (even "none": no Bibles
//! chosen means the ★ Bible, not the reused Tab's choice) and searches. A Search scope naming a
//! Resource or Collection that is gone still runs, over what is left, and the Search Tab says
//! what was left out ([`crate::bible_search::SearchTab::notice`]).
//!
//! Both live in the User database, as the `search.saved` and `search.history` preferences
//! (JSON), like the Collections and Copy presets.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bible_search::{SearchMode, SearchTab, failed, tab_param};
use crate::command::{CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::favorites::SavedSearch;
use crate::session::Session;
use crate::workspace::{Effect, OpenRequest, PanelId, TabId};

/// How many searches the Search history keeps.
pub const SEARCH_HISTORY_LIMIT: usize = 50;

const SAVED_PREF: &str = "search.saved";
const HISTORY_PREF: &str = "search.history";

/// A Saved search: a search kept under a name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamedSearch {
    pub name: String,
    pub search: SavedSearch,
}

/// One search in the Search history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchHistoryEntry {
    pub search: SavedSearch,
    /// When it last ran (Unix seconds).
    pub at: i64,
}

impl SavedSearch {
    /// How a Saved search or history row describes it: `strong:G26 · Bible · New Testament`.
    pub fn summary(&self) -> String {
        let scope = match self.mode {
            SearchMode::Bible if self.scope.trim().is_empty() => {
                crate::bible_search::WHOLE_BIBLE.to_string()
            }
            SearchMode::Bible | SearchMode::Notes => self.scope.trim().to_string(),
            SearchMode::Library => String::new(),
        };
        let mut out = format!("{} · {}", self.query.trim(), self.mode.label());
        if !scope.is_empty() {
            out.push_str(" · ");
            out.push_str(&scope);
        }
        out
    }
}

/// The query and Scope fields as typed in a Search Tab (not yet run), for
/// [`Session::save_search`]; `None` keeps the last search's.
#[derive(Debug, Clone, Copy, Default)]
pub struct TypedFields<'a> {
    pub query: Option<&'a str>,
    pub scope: Option<&'a str>,
}

/// The Saved searches and Search history as the User database has them.
#[derive(Debug, Default)]
pub struct SavedSearches {
    /// By name, ignoring case.
    saved: Vec<NamedSearch>,
    /// Newest first.
    history: Vec<SearchHistoryEntry>,
}

impl Session {
    /// Read the Saved searches and the Search history from the User database.
    pub(crate) fn load_saved_searches(&mut self) {
        let read = |key: &str| {
            self.user
                .as_ref()
                .and_then(|u| u.preference(key).ok().flatten())
        };
        let mut saved: Vec<NamedSearch> = read(SAVED_PREF)
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        sort_by_name(&mut saved);
        let mut history: Vec<SearchHistoryEntry> = read(HISTORY_PREF)
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        history.truncate(SEARCH_HISTORY_LIMIT);
        self.saved_searches = SavedSearches { saved, history };
    }

    /// The Saved searches, by name.
    pub fn saved_searches(&self) -> &[NamedSearch] {
        &self.saved_searches.saved
    }

    /// The Saved searches whose name or query contains `text` (ignoring case), by name: the
    /// Command box's suggestions.
    pub fn saved_searches_matching(&self, text: &str) -> Vec<&NamedSearch> {
        let needle = text.trim().to_lowercase();
        self.saved_searches
            .saved
            .iter()
            .filter(|n| {
                needle.is_empty()
                    || n.name.to_lowercase().contains(&needle)
                    || n.search.query.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// The Search history, newest first.
    pub fn search_history(&self) -> &[SearchHistoryEntry] {
        &self.saved_searches.history
    }

    /// Save what a Search Tab's fields hold under `name`, replacing a Saved search of that
    /// name (any case). `typed` is the query and Scope fields as the user sees them, typed
    /// but not yet run; without it, the last search's.
    pub fn save_search(
        &mut self,
        tab: TabId,
        name: &str,
        typed: TypedFields<'_>,
    ) -> Result<NamedSearch, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Give the search a name".into());
        }
        let search = self
            .search_tab(tab)
            .map(|st| {
                let mut st = st.clone();
                if let Some(query) = typed.query {
                    st.query = query.to_string();
                }
                if let Some(scope) = typed.scope {
                    st.scope = scope.to_string();
                }
                st
            })
            .as_ref()
            .and_then(SavedSearch::of)
            .ok_or("Type a search to save")?;
        let named = NamedSearch {
            name: name.to_string(),
            search,
        };
        let mut saved = self.saved_searches.saved.clone();
        saved.retain(|n| !same_name(&n.name, name));
        saved.push(named.clone());
        sort_by_name(&mut saved);
        self.write_pref(SAVED_PREF, &saved, "the Saved searches")?;
        self.saved_searches.saved = saved;
        Ok(named)
    }

    /// Delete a Saved search (any case); false if there is none of that name.
    pub fn delete_saved_search(&mut self, name: &str) -> Result<bool, String> {
        let mut saved = self.saved_searches.saved.clone();
        let before = saved.len();
        saved.retain(|n| !same_name(&n.name, name));
        if saved.len() == before {
            return Ok(false);
        }
        self.write_pref(SAVED_PREF, &saved, "the Saved searches")?;
        self.saved_searches.saved = saved;
        Ok(true)
    }

    /// Run a Saved search (any case) in Search Tab `tab`, else the most recently active one,
    /// else a new one.
    pub fn open_saved_search(
        &mut self,
        name: &str,
        tab: Option<TabId>,
        req: OpenRequest,
    ) -> Result<TabId, String> {
        let search = self
            .saved_searches
            .saved
            .iter()
            .find(|n| same_name(&n.name, name))
            .map(|n| n.search.clone())
            .ok_or_else(|| format!("No Saved search “{}”", name.trim()))?;
        Ok(self.open_search_in(&search, tab, req))
    }

    /// Run the `index`th search of the Search history (0: the newest) again, in Search Tab
    /// `tab` as [`Self::open_saved_search`] does.
    pub fn open_search_history(
        &mut self,
        index: usize,
        tab: Option<TabId>,
        req: OpenRequest,
    ) -> Result<TabId, String> {
        let search = self
            .saved_searches
            .history
            .get(index)
            .map(|e| e.search.clone())
            .ok_or_else(|| format!("No search {index} in the Search history"))?;
        Ok(self.open_search_in(&search, tab, req))
    }

    /// Forget the Search history.
    pub fn clear_search_history(&mut self) -> Result<(), String> {
        self.write_pref::<SearchHistoryEntry>(HISTORY_PREF, &[], "the Search history")?;
        self.saved_searches.history.clear();
        Ok(())
    }

    /// Put a search that just ran at the top of the Search history.
    pub(crate) fn record_search_history(&mut self, st: &SearchTab) {
        let Some(search) = SavedSearch::of(st) else {
            return;
        };
        let mut history = self.saved_searches.history.clone();
        history.retain(|e| e.search != search);
        history.insert(
            0,
            SearchHistoryEntry {
                search,
                at: i64::try_from(crate::get_bibles::now_secs()).unwrap_or(i64::MAX),
            },
        );
        history.truncate(SEARCH_HISTORY_LIMIT);
        if let Err(e) = self.write_pref(HISTORY_PREF, &history, "the Search history") {
            self.workspace.push_effect(Effect::Message(e));
        }
        // Kept for this session even when it could not be written.
        self.saved_searches.history = history;
    }

    /// What a search's Search scope chooses beyond its summary: the Bibles (Bible mode), the
    /// Resource types, Collections and Resources (Library mode) or the Notebook (Notes mode).
    /// The Saved search and history rows' tooltip, so that two rows alike in their summary
    /// can be told apart.
    pub fn search_scope_details(&self, search: &SavedSearch) -> String {
        match search.mode {
            SearchMode::Bible => match &search.bibles {
                None => "Bibles: the first in Preferred order".to_string(),
                Some(ids) if ids.is_empty() => "Bibles: none".to_string(),
                Some(ids) => {
                    let names: Vec<String> = ids.iter().map(|id| self.resource_label(id)).collect();
                    format!("Bibles: {}", names.join(", "))
                }
            },
            SearchMode::Library => {
                let mut parts: Vec<String> = search
                    .types
                    .iter()
                    .map(|ty| crate::library_search::type_plural(*ty).to_string())
                    .collect();
                parts.extend(
                    search
                        .collections
                        .iter()
                        .map(|c| format!("Collection “{c}”")),
                );
                parts.extend(search.resources.iter().map(|id| self.resource_label(id)));
                if parts.is_empty() {
                    "Resources: none".to_string()
                } else {
                    format!("Resources: {}", parts.join(", "))
                }
            }
            SearchMode::Notes => match &search.notebook {
                None => "All Notebooks".to_string(),
                Some(id) => {
                    let name = self
                        .notebooks()
                        .ok()
                        .and_then(|all| all.into_iter().find(|n| n.id == *id).map(|n| n.name));
                    name.map_or_else(
                        || "Notebook: one that no longer exists".to_string(),
                        |name| format!("Notebook: {name}"),
                    )
                }
            },
        }
    }

    /// A Resource's abbreviation: from the library, else as the User database remembers it
    /// (removed since), else its id.
    fn resource_label(&self, id: &str) -> String {
        if let Some(e) = self.library.get(id) {
            return e.manifest.abbreviation.clone();
        }
        self.user
            .as_ref()
            .and_then(|u| u.resource_names().ok())
            .and_then(|names| {
                names
                    .into_iter()
                    .find(|(rid, _, _)| rid == id)
                    .map(|(_, abbreviation, _)| abbreviation)
            })
            .unwrap_or_else(|| id.to_string())
    }

    /// What a search's Search scope names that is gone, as the Search Tab's notice: Bibles
    /// and Resources no longer in the library, Collections and a Notebook that no longer
    /// exist. Only the parts its mode searches count.
    pub(crate) fn gone_from_scope(&self, search: &SavedSearch) -> Option<String> {
        let names = self
            .user
            .as_ref()
            .and_then(|u| u.resource_names().ok())
            .unwrap_or_default();
        let name_of = |id: &str| {
            names.iter().find(|(rid, _, _)| rid == id).map_or_else(
                || id.to_string(),
                |(_, abbreviation, _)| abbreviation.clone(),
            )
        };
        let mut parts: Vec<String> = Vec::new();
        let gone: Vec<String> = match search.mode {
            SearchMode::Bible => search.bibles.iter().flatten().cloned().collect(),
            SearchMode::Library => search.resources.clone(),
            SearchMode::Notes => Vec::new(),
        }
        .into_iter()
        .filter(|id| self.library.get(id).is_none())
        .map(|id| name_of(&id))
        .collect();
        if !gone.is_empty() {
            let verb = if gone.len() == 1 { "is" } else { "are" };
            parts.push(format!(
                "{} {verb} no longer in your library",
                and_list(&gone)
            ));
        }
        if search.mode == SearchMode::Library {
            let gone: Vec<String> = search
                .collections
                .iter()
                .filter(|c| self.library.collection(c).is_none())
                .map(|c| format!("“{c}”"))
                .collect();
            if !gone.is_empty() {
                let (noun, verb) = if gone.len() == 1 {
                    ("Collection", "no longer exists")
                } else {
                    ("Collections", "no longer exist")
                };
                parts.push(format!("the {noun} {} {verb}", and_list(&gone)));
            }
        }
        if search.mode == SearchMode::Notes
            && let Some(id) = &search.notebook
            && self
                .notebooks()
                .is_ok_and(|all| !all.iter().any(|n| n.id == *id))
        {
            parts.push("its Notebook no longer exists, so every Notebook was searched".into());
        }
        if parts.is_empty() {
            return None;
        }
        Some(format!(
            "Searched without what is gone: {}.",
            parts.join("; ")
        ))
    }

    fn write_pref<T: Serialize>(&self, key: &str, value: &[T], what: &str) -> Result<(), String> {
        let Some(user) = &self.user else {
            return Ok(());
        };
        let json = serde_json::to_string(value).map_err(|e| e.to_string())?;
        user.set_preference(key, &json)
            .map_err(|e| format!("Could not save {what}: {e}"))
    }
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

fn sort_by_name(saved: &mut [NamedSearch]) {
    saved.sort_by_key(|n| n.name.to_lowercase());
}

/// `A`, `A and B`, `A, B and C`.
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

// -------------------------------------------------------------------------------- commands

const NAME: ParamSpec = ParamSpec {
    name: "name",
    kind: ParamKind::String,
    required: true,
    help: "The Saved search's name (any case)",
};
const IN_TAB: ParamSpec = ParamSpec {
    name: "tab",
    kind: ParamKind::Integer,
    required: false,
    help: "The Search Tab to run in (default: the most recently active one, else a new one)",
};
const PANEL: ParamSpec = ParamSpec {
    name: "panel",
    kind: ParamKind::Integer,
    required: false,
    help: "Panel to open a new Search Tab in",
};
const SPLIT: ParamSpec = ParamSpec {
    name: "split",
    kind: ParamKind::Boolean,
    required: false,
    help: "Open in a new Tab split off to the right (Ctrl/Cmd held)",
};

fn request(p: &Params<'_>) -> OpenRequest {
    OpenRequest {
        panel: p
            .i64("panel")
            .and_then(|n| u64::try_from(n).ok())
            .map(PanelId),
        split: p.bool("split").unwrap_or(false),
    }
}

fn in_tab(p: &Params<'_>) -> Option<TabId> {
    p.i64("tab").and_then(|n| u64::try_from(n).ok()).map(TabId)
}

fn opened(s: &Session, tab: TabId) -> Value {
    let st = s.search_tab(tab);
    json!({
        "tab": tab.0,
        "error": crate::bible_search::error_json(st.and_then(|st| st.error.as_ref())),
        "notice": st.and_then(|st| st.notice.clone()),
    })
}

fn search_json(search: &SavedSearch) -> Value {
    serde_json::to_value(search).unwrap_or(Value::Null)
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "search.save",
        label: "Save search…",
        shortcut: None,
        params: &[
            crate::bible_search::TAB,
            ParamSpec {
                name: "name",
                kind: ParamKind::String,
                required: true,
                help: "A name for it (an existing name, any case, is replaced)",
            },
            ParamSpec {
                name: "text",
                kind: ParamKind::String,
                required: false,
                help: "The query as typed (default: the last search's)",
            },
            ParamSpec {
                name: "scope",
                kind: ParamKind::String,
                required: false,
                help: "The Scope field as typed (default: the last search's)",
            },
        ],
        enabled: always,
        run: |s, p| {
            const ID: &str = "search.save";
            let tab = tab_param(ID, s, &p)?;
            let typed = TypedFields {
                query: p.str("text"),
                scope: p.str("scope"),
            };
            let named = s
                .save_search(tab, p.str("name").unwrap_or_default(), typed)
                .map_err(|e| failed(ID, e))?;
            s.workspace.push_effect(Effect::Message(format!(
                "Saved the search “{}”",
                named.name
            )));
            Ok(json!({ "name": named.name, "search": search_json(&named.search) }))
        },
    },
    CommandSpec {
        id: "search.open_saved",
        label: "Run Saved search",
        shortcut: None,
        params: &[NAME, IN_TAB, PANEL, SPLIT],
        enabled: always,
        run: |s, p| {
            const ID: &str = "search.open_saved";
            let tab = s
                .open_saved_search(p.str("name").unwrap_or_default(), in_tab(&p), request(&p))
                .map_err(|e| failed(ID, e))?;
            Ok(opened(s, tab))
        },
    },
    CommandSpec {
        id: "search.delete_saved",
        label: "Delete Saved search",
        shortcut: None,
        params: &[NAME],
        enabled: always,
        run: |s, p| {
            const ID: &str = "search.delete_saved";
            let name = p.str("name").unwrap_or_default();
            if !s.delete_saved_search(name).map_err(|e| failed(ID, e))? {
                return Err(failed(ID, format!("No Saved search “{}”", name.trim())));
            }
            Ok(json!({ "deleted": name.trim() }))
        },
    },
    CommandSpec {
        id: "search.open_history",
        label: "Run from Search history",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "index",
                kind: ParamKind::Integer,
                required: true,
                help: "Which search (0: the newest)",
            },
            IN_TAB,
            PANEL,
            SPLIT,
        ],
        enabled: always,
        run: |s, p| {
            const ID: &str = "search.open_history";
            let index = p
                .i64("index")
                .and_then(|n| usize::try_from(n).ok())
                .ok_or_else(|| failed(ID, "no index"))?;
            let tab = s
                .open_search_history(index, in_tab(&p), request(&p))
                .map_err(|e| failed(ID, e))?;
            Ok(opened(s, tab))
        },
    },
    CommandSpec {
        id: "search.clear_history",
        label: "Clear Search history",
        shortcut: None,
        params: &[],
        enabled: |s| !s.saved_searches.history.is_empty(),
        run: |s, _| {
            s.clear_search_history()
                .map_err(|e| failed("search.clear_history", e))?;
            Ok(Value::Null)
        },
    },
    CommandSpec {
        id: "search.list_saved",
        label: "Saved searches and Search history",
        shortcut: None,
        params: &[],
        enabled: always,
        run: |s, _| {
            let saved: Vec<Value> = s
                .saved_searches()
                .iter()
                .map(|n| json!({ "name": n.name, "summary": n.search.summary(), "search": search_json(&n.search) }))
                .collect();
            let history: Vec<Value> = s
                .search_history()
                .iter()
                .map(|e| json!({ "at": e.at, "summary": e.search.summary(), "search": search_json(&e.search) }))
                .collect();
            Ok(json!({ "saved": saved, "history": history }))
        },
    },
];
