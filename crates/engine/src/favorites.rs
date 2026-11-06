//! Favorites and History (#155).
//!
//! A [`TabLocation`] is where a Tab is: its Tab kind and its Current location (a Passage in
//! the Tab's own numbering, an entry Key, or a Search). A Favorite keeps one in a Favorites
//! folder; History keeps a Visit for every Tab opened or jumped, across all Tabs, grouped by
//! day. Both live in the User database; opening either brings back that Tab kind at that
//! location.

use obs_model::{Key, ResourceType};
use obs_scripture::{Passage, RefStyle, Standard, Verse, format_passage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::bible_search::{SearchMode, SearchRequestFields, SearchTab};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::session::Session;
use crate::workspace::{
    Effect, Guide, HeaderBox, OpenRequest, PanelId, PendingVisit, Tab, TabId, TabKind, Tool,
    VisitedAt,
};

/// A Search as a Favorite or Visit keeps it: what the Search Tool's fields held.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSearch {
    pub mode: SearchMode,
    pub query: String,
    /// The Scope field (empty: the Whole Bible).
    #[serde(default)]
    pub scope: String,
    /// The Bibles chosen (none: the ★ Bible).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bibles: Option<Vec<String>>,
    /// Library mode's Resource types and single Resources.
    #[serde(default)]
    pub types: Vec<ResourceType>,
    #[serde(default)]
    pub resources: Vec<String>,
    /// Library mode's Collections (#153).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<String>,
    /// Notes mode's Notebook (none: all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notebook: Option<String>,
}

impl SavedSearch {
    /// A Search Tab's fields, unless its query is empty.
    pub(crate) fn of(st: &SearchTab) -> Option<Self> {
        (!st.query.trim().is_empty()).then(|| Self {
            mode: st.mode,
            query: st.query.clone(),
            scope: st.scope.clone(),
            bibles: st.bibles.clone(),
            types: st.library.types.clone(),
            resources: st.library.resources.clone(),
            collections: st.library.collections.clone(),
            notebook: st.notebook.clone(),
        })
    }
}

/// Where a Tab is: its kind and Current location. What a Favorite or a Visit opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabLocation {
    pub kind: TabKind,
    /// A Passage location, in the Tab's own numbering (a Bible's own, else the Standard).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verse: Option<Verse>,
    /// An entry Key: a Lexicon's or Dictionary's entry, or a Word Study's Strong's number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<Key>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<SavedSearch>,
}

impl TabLocation {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Option<Self> {
        serde_json::from_str(s).ok()
    }
}

/// A saved Current location, in a Favorites folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favorite {
    pub id: i64,
    /// Its Favorites folder; empty when it is in none.
    pub folder: String,
    pub label: String,
    pub location: TabLocation,
}

/// One entry in History: where a Tab was opened or jumped to, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub id: i64,
    pub label: String,
    pub location: TabLocation,
    /// The local day, `YYYY-MM-DD`, and how many days ago it was (0: today).
    pub day: String,
    pub days_ago: i64,
    /// The local time of day, `HH:MM`.
    pub time: String,
}

/// One day of History, newest Visit first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryDay {
    /// "Today", "Yesterday", or the date ("Thursday 5 November 2026").
    pub heading: String,
    pub visits: Vec<Visit>,
}

/// What [`Session`] keeps of Favorites and History: the User database's, cached so that
/// drawing a frame never reads it.
#[derive(Debug, Default)]
pub struct FavoritesAndHistory {
    favorites: Vec<Favorite>,
    history: Vec<Visit>,
    /// The last location History recorded, so returning to it records nothing.
    last_visit: Option<TabLocation>,
    /// The local day History's day headings were worked out on (`YYYY-MM-DD`).
    history_day: Option<String>,
    /// When writing a Visit last failed (Unix seconds), until one is written again: the
    /// queued Visits wait [`HISTORY_RETRY_SECS`] before the next try.
    history_failed_at: Option<i64>,
}

/// How long History waits after a failed write before trying its queued Visits again.
const HISTORY_RETRY_SECS: i64 = 5;

impl Session {
    /// Load Favorites and History from the User database.
    pub(crate) fn load_favorites_and_history(&mut self) {
        self.reload_favorites();
        self.reload_history();
        self.places.last_visit = self.places.history.first().map(|v| v.location.clone());
    }

    fn reload_favorites(&mut self) {
        let rows = self
            .user
            .as_ref()
            .and_then(|u| u.favorites().ok())
            .unwrap_or_default();
        self.places.favorites = rows
            .into_iter()
            .filter_map(|r| {
                Some(Favorite {
                    id: r.id,
                    folder: r.folder,
                    label: r.label,
                    location: TabLocation::from_json(&r.location)?,
                })
            })
            .collect();
    }

    fn reload_history(&mut self) {
        self.reload_history_as_of(unix_now());
    }

    /// Load History with its day headings as of `at` (Unix seconds).
    fn reload_history_as_of(&mut self, at: i64) {
        let Some(user) = &self.user else {
            return;
        };
        let rows = user.history_as_of(at).unwrap_or_default();
        self.places.history_day = user.local_day(at).ok();
        self.places.history = rows
            .into_iter()
            .filter_map(|r| {
                Some(Visit {
                    id: r.id,
                    label: r.label,
                    location: TabLocation::from_json(&r.location)?,
                    day: r.day,
                    days_ago: r.days_ago,
                    time: r.time,
                })
            })
            .collect();
    }

    /// Every Favorite: those in no folder first, then each Favorites folder by name; in the
    /// order added within each.
    pub fn favorites(&self) -> &[Favorite] {
        &self.places.favorites
    }

    /// The Favorites folders, by name.
    pub fn favorite_folders(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for f in &self.places.favorites {
            if !f.folder.is_empty() && !out.contains(&f.folder) {
                out.push(f.folder.clone());
            }
        }
        out
    }

    /// Favorites whose label or folder contains `text` (ignoring case), in [`Self::favorites`]
    /// order: the Command box's suggestions (#139).
    pub fn favorites_matching(&self, text: &str) -> Vec<&Favorite> {
        let needle = text.trim().to_lowercase();
        self.places
            .favorites
            .iter()
            .filter(|f| {
                needle.is_empty()
                    || f.label.to_lowercase().contains(&needle)
                    || f.folder.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// The Favorite of the Tab's Current location, if it has one (the header's ★).
    pub fn favorite_of(&self, tab: TabId) -> Option<&Favorite> {
        let (location, _) = self.location_of(tab)?;
        self.places
            .favorites
            .iter()
            .find(|f| f.location == location)
    }

    /// Work History's day headings out again if the local day has changed since they were
    /// ("Today" becomes "Yesterday" at midnight). Saving the Last session (at most once a
    /// second while the app runs) calls this.
    pub(crate) fn refresh_history_day(&mut self) {
        self.refresh_history_day_as_of(unix_now());
    }

    pub(crate) fn refresh_history_day_as_of(&mut self, at: i64) {
        let Some(user) = &self.user else {
            return;
        };
        let today = user.local_day(at).ok();
        if today.is_some() && today != self.places.history_day {
            self.reload_history_as_of(at);
        }
    }

    /// History, newest first.
    pub fn history(&self) -> &[Visit] {
        &self.places.history
    }

    /// History grouped by day, newest first, keeping the Visits whose label contains
    /// `filter` (ignoring case).
    pub fn history_days(&self, filter: &str) -> Vec<HistoryDay> {
        let needle = filter.trim().to_lowercase();
        let mut out: Vec<HistoryDay> = Vec::new();
        for v in &self.places.history {
            if !needle.is_empty() && !v.label.to_lowercase().contains(&needle) {
                continue;
            }
            let heading = day_heading(&v.day, v.days_ago);
            match out.last_mut() {
                Some(d) if d.heading == heading => d.visits.push(v.clone()),
                _ => out.push(HistoryDay {
                    heading,
                    visits: vec![v.clone()],
                }),
            }
        }
        out
    }

    /// Where a Tab is now, and how to name it: `None` for Tabs without a Current location
    /// (the Library, Notes, About …) or before they have one.
    pub fn location_of(&self, tab: TabId) -> Option<(TabLocation, String)> {
        self.location_at(self.workspace.tab(tab)?, None)
    }

    /// [`Self::location_of`] for Tab `t`, with its Passage location, entry Key and Search
    /// fields taken from `at` when given (where a jump went, read later).
    fn location_at(&self, t: &Tab, at: Option<&VisitedAt>) -> Option<(TabLocation, String)> {
        let (verse, entry_key) = match at {
            Some(at) => (at.verse, at.key.clone()),
            None => (t.history.current(), t.entry_history.current()),
        };
        let place = |verse: Verse| TabLocation {
            kind: t.kind.clone(),
            verse: Some(verse),
            key: None,
            search: None,
        };
        let entry = |key: Key| TabLocation {
            kind: t.kind.clone(),
            verse: None,
            key: Some(key),
            search: None,
        };
        let reference = |v: Verse| match &t.versification {
            Some(own) => format_passage(Passage::verse(v), own.as_ref(), RefStyle::Long),
            None => format_passage(Passage::verse(v), &Standard, RefStyle::Long),
        };
        match &t.kind {
            TabKind::Resource(id) => {
                let abbreviation = self
                    .library
                    .get(id)
                    .map_or(id.as_str(), |e| e.manifest.abbreviation.as_str());
                if matches!(t.header_box(), HeaderBox::Lexicon | HeaderBox::Headword) {
                    let key = entry_key.clone()?;
                    let label = format!("{key} · {abbreviation}");
                    Some((entry(key), label))
                } else {
                    let v = verse?;
                    Some((place(v), format!("{} · {abbreviation}", reference(v))))
                }
            }
            TabKind::Guide(Guide::WordStudy) => {
                let key = entry_key.clone()?;
                let label = format!("{key} · {}", Guide::WordStudy.title());
                Some((entry(key), label))
            }
            TabKind::Guide(Guide::Factbook) => {
                let key = entry_key.clone()?;
                let Key::Headword(id) = &key else {
                    return None;
                };
                let label = format!(
                    "{} · {}",
                    crate::factbook::label(self, id),
                    Guide::Factbook.title()
                );
                Some((entry(key), label))
            }
            TabKind::Guide(Guide::TopicGuide) => {
                let key = entry_key.clone()?;
                let label = format!("{key} · {}", Guide::TopicGuide.title());
                Some((entry(key), label))
            }
            TabKind::Guide(Guide::PassageGuide | Guide::ExegeticalGuide)
            | TabKind::Tool(Tool::TextComparison | Tool::CrossReferences) => {
                let v = verse?;
                Some((place(v), format!("{} · {}", reference(v), title(&t.kind))))
            }
            TabKind::Tool(Tool::Search) => {
                let search = match at {
                    Some(at) => at.search.clone()?,
                    None => SavedSearch::of(&t.search)?,
                };
                let label = format!(
                    "“{}” · {} {}",
                    search.query.trim(),
                    search.mode.label(),
                    Tool::Search.title()
                );
                Some((
                    TabLocation {
                        kind: t.kind.clone(),
                        verse: None,
                        key: None,
                        search: Some(search),
                    },
                    label,
                ))
            }
            _ => None,
        }
    }

    /// Bring back a Tab kind at a location: the most recently active Tab of that kind moves
    /// there (unless `req` splits), else a new one opens there.
    pub fn open_location(
        &mut self,
        location: &TabLocation,
        req: OpenRequest,
    ) -> Result<TabId, String> {
        let kind = location.kind.clone();
        if let TabKind::Resource(id) = &kind
            && self.library.get(id).is_none()
        {
            return Err(format!("{id} is no longer in your library"));
        }
        if let Some(search) = &location.search {
            return Ok(self.open_search(search, req));
        }
        if kind == TabKind::Guide(Guide::WordStudy) {
            let Some(Key::StrongsNumber(n)) = location.key else {
                return Err("This Word Study has no Strong's number".into());
            };
            return Ok(crate::word_study::open_at(self, n, req, false));
        }
        if kind == TabKind::Guide(Guide::Factbook) {
            let Some(Key::Headword(id)) = &location.key else {
                return Err("This Factbook has no person or place".into());
            };
            return Ok(self.open_factbook_at(id, req, false));
        }
        if kind == TabKind::Guide(Guide::TopicGuide) {
            let Some(Key::Headword(topic)) = &location.key else {
                return Err("This Topic Guide has no topic".into());
            };
            return Ok(self.open_topic_guide_at(topic, req, false));
        }
        let keyed = matches!(
            kind,
            TabKind::Resource(_)
                | TabKind::Guide(Guide::PassageGuide | Guide::ExegeticalGuide)
                | TabKind::Tool(Tool::TextComparison | Tool::CrossReferences)
        );
        if !keyed || (location.verse.is_none() && location.key.is_none()) {
            return Err(format!("{} has no location to open", title(&kind)));
        }
        let existing = (!req.split)
            .then(|| self.workspace.most_recent(|t| t.kind == kind))
            .flatten();
        if let Some(tab) = existing {
            self.workspace.focus(tab);
            if let Some(v) = location.verse {
                self.workspace.navigate(tab, v);
            }
            if let Some(key) = &location.key {
                self.workspace.navigate_entry(tab, key.clone());
            }
            return Ok(tab);
        }
        let tab = crate::workspace_commands::open_tab(self, kind.clone(), req);
        if kind.is_reading() {
            // The UI places a new Resource Tab at its own location once it has loaded it, then
            // jumps there so the Link set it joined follows (in its numbering, known only then).
            if let Some(t) = self.workspace.tab_mut(tab) {
                if let Some(v) = location.verse {
                    t.history.visit(v);
                }
                if let Some(key) = &location.key {
                    t.entry_history.visit(key.clone());
                }
            }
            self.workspace.push_effect(Effect::NavigateOpened {
                tab,
                verse: location.verse,
                key: location.key.clone(),
            });
        } else if let Some(v) = location.verse {
            self.workspace.navigate(tab, v);
        }
        Ok(tab)
    }

    /// Run a search as kept (a Favorite, Visit, Saved search or Search history entry) in the
    /// Search Tool. A Search scope naming something that is gone still runs, over what is
    /// left, with a notice on the Tab (#179).
    pub(crate) fn open_search(&mut self, search: &SavedSearch, req: OpenRequest) -> TabId {
        self.open_search_in(search, None, req)
    }

    /// [`Self::open_search`] in Search Tab `tab` when given (its sidebar ran it).
    pub(crate) fn open_search_in(
        &mut self,
        search: &SavedSearch,
        tab: Option<TabId>,
        req: OpenRequest,
    ) -> TabId {
        let tab = self.search_tab_for(tab, req);
        // Every field as saved, even "none": a reused Search Tab's own choices do not carry
        // over (no Bibles chosen means the ★ Bible, not this Tab's last choice).
        if let Some(st) = self.search_tab_mut(tab) {
            st.mode = search.mode;
            st.notebook = search.notebook.clone();
            st.bibles = search.bibles.clone();
        }
        self.run_search(
            tab,
            SearchRequestFields {
                query: search.query.clone(),
                scope: Some(search.scope.clone()),
                bibles: search.bibles.clone(),
                library: Some(crate::library_search::LibraryScope {
                    types: search.types.clone(),
                    collections: search.collections.clone(),
                    resources: search.resources.clone(),
                }),
            },
        );
        let notice = self.gone_from_scope(search);
        if let Some(st) = self.search_tab_mut(tab) {
            st.notice = notice;
            st.restored += 1;
        }
        tab
    }

    /// Record a Visit for every Tab opened or jumped since the last call: where a jump went,
    /// and where an opened Tab is now. The UI calls this once it has applied the engine's
    /// effects; a new Resource Tab the UI has not placed yet (no location until its text is
    /// laid out) stays queued until it has one, unless it jumps first; a Tab closed since
    /// is recorded where it was. When a write fails, that Visit and the ones after it stay
    /// queued, in order, and are tried again a few seconds later (the failure is shown
    /// once). Without a User database the Visits are dropped.
    pub fn record_history(&mut self) {
        self.record_history_as_of(unix_now());
    }

    /// [`Self::record_history`] at `now` (Unix seconds).
    pub(crate) fn record_history_as_of(&mut self, now: i64) {
        if self
            .places
            .history_failed_at
            .is_some_and(|failed| now < failed + HISTORY_RETRY_SECS)
        {
            return;
        }
        let visits = self.workspace.take_visits();
        if visits.is_empty() || self.user.is_none() {
            self.workspace.keep_visits(Vec::new());
            return;
        }
        let mut recorded = false;
        let mut kept: Vec<PendingVisit> = Vec::new();
        for (i, visit) in visits.iter().enumerate() {
            let opened = visit.at.is_none();
            if opened && visits[i + 1..].iter().any(|later| later.tab == visit.tab) {
                // Its later jump is where it went.
                continue;
            }
            let found = self
                .workspace
                .visited_tab(visit.tab)
                .and_then(|t| self.location_at(t, visit.at.as_ref()));
            let Some((location, label)) = found else {
                if opened
                    && self
                        .workspace
                        .tab(visit.tab)
                        .is_some_and(|t| t.kind.is_reading())
                {
                    kept.push(visit.clone());
                }
                continue;
            };
            if self.places.last_visit.as_ref() == Some(&location) {
                continue;
            }
            let Some(user) = &self.user else {
                return;
            };
            if let Err(e) = user.record_visit(&label, &location.to_json()) {
                if self.places.history_failed_at.is_none() {
                    self.workspace
                        .push_effect(Effect::Message(format!("Could not save History: {e}")));
                }
                self.places.history_failed_at = Some(now);
                kept.extend_from_slice(&visits[i..]);
                break;
            }
            self.places.history_failed_at = None;
            self.places.last_visit = Some(location);
            recorded = true;
        }
        self.workspace.keep_visits(kept);
        if recorded {
            self.reload_history();
        }
    }
}

fn unix_now() -> i64 {
    i64::try_from(crate::get_bibles::now_secs()).unwrap_or(i64::MAX)
}

fn title(kind: &TabKind) -> &'static str {
    match kind {
        TabKind::Guide(g) => g.title(),
        TabKind::Tool(t) => t.title(),
        TabKind::Resource(_) => "This Resource",
        _ => "This Tab",
    }
}

/// "Today", "Yesterday", or a `YYYY-MM-DD` day as "Thursday 5 November 2026".
pub fn day_heading(day: &str, days_ago: i64) -> String {
    match days_ago {
        0 => return "Today".into(),
        1 => return "Yesterday".into(),
        _ => {}
    }
    let mut parts = day.split('-').map(str::parse::<i64>);
    let (Some(Ok(y)), Some(Ok(m)), Some(Ok(d))) = (parts.next(), parts.next(), parts.next()) else {
        return day.to_string();
    };
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    const WEEKDAYS: [&str; 7] = [
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
    ];
    let Some(month) = usize::try_from(m - 1).ok().and_then(|i| MONTHS.get(i)) else {
        return day.to_string();
    };
    // Days since 1970-01-01 (a Thursday), by Howard Hinnant's days-from-civil.
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let weekday = usize::try_from(days.rem_euclid(7))
        .ok()
        .and_then(|i| WEEKDAYS.get(i))
        .copied()
        .unwrap_or_default();
    format!("{weekday} {d} {month} {y}")
}

// -------------------------------------------------------------------------------- commands

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn has_user(s: &Session) -> bool {
    s.user.is_some()
}

const ID: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::Integer,
    required: true,
    help: "The Favorite's id",
};
const VISIT: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::Integer,
    required: true,
    help: "The Visit's id (from History)",
};
const FOLDER: ParamSpec = ParamSpec {
    name: "folder",
    kind: ParamKind::String,
    required: false,
    help: "Favorites folder (a new name makes one; empty or missing: none)",
};
const PANEL: ParamSpec = ParamSpec {
    name: "panel",
    kind: ParamKind::Integer,
    required: false,
    help: "Panel to open a new Tab in",
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

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "favorite.add",
        label: "Add to Favorites",
        shortcut: Some("Ctrl+D"),
        params: &[
            ParamSpec {
                name: "tab",
                kind: ParamKind::Integer,
                required: true,
                help: "The Tab whose Current location to add",
            },
            FOLDER,
        ],
        enabled: has_user,
        run: favorite_add,
    },
    CommandSpec {
        id: "favorite.open",
        label: "Open Favorite",
        shortcut: None,
        params: &[ID, PANEL, SPLIT],
        enabled: always,
        run: |s, p| {
            const CMD: &str = "favorite.open";
            let id = p.i64("id").unwrap_or_default();
            let f = s
                .places
                .favorites
                .iter()
                .find(|f| f.id == id)
                .cloned()
                .ok_or_else(|| failed(CMD, format!("no Favorite {id}")))?;
            let tab = s
                .open_location(&f.location, request(&p))
                .map_err(|e| failed(CMD, e))?;
            Ok(json!({ "tab": tab.0, "label": f.label }))
        },
    },
    CommandSpec {
        id: "favorite.remove",
        label: "Remove from Favorites",
        shortcut: None,
        params: &[ID],
        enabled: has_user,
        run: |s, p| {
            const CMD: &str = "favorite.remove";
            let id = p.i64("id").unwrap_or_default();
            let user = s
                .user
                .as_ref()
                .ok_or_else(|| failed(CMD, "no User database"))?;
            if !user.remove_favorite(id).map_err(|e| failed(CMD, e))? {
                return Err(failed(CMD, format!("no Favorite {id}")));
            }
            s.reload_favorites();
            Ok(json!({ "removed": id }))
        },
    },
    CommandSpec {
        id: "favorite.move",
        label: "Move to folder",
        shortcut: None,
        params: &[ID, FOLDER],
        enabled: has_user,
        run: |s, p| {
            const CMD: &str = "favorite.move";
            let id = p.i64("id").unwrap_or_default();
            let folder = p.str("folder").unwrap_or_default().trim().to_string();
            let user = s
                .user
                .as_ref()
                .ok_or_else(|| failed(CMD, "no User database"))?;
            if !user
                .move_favorite(id, &folder)
                .map_err(|e| failed(CMD, e))?
            {
                return Err(failed(CMD, format!("no Favorite {id}")));
            }
            s.reload_favorites();
            Ok(json!({ "id": id, "folder": folder }))
        },
    },
    CommandSpec {
        id: "history.open",
        label: "Open from History",
        shortcut: None,
        params: &[VISIT, PANEL, SPLIT],
        enabled: always,
        run: |s, p| {
            const CMD: &str = "history.open";
            let id = p.i64("id").unwrap_or_default();
            let v = s
                .places
                .history
                .iter()
                .find(|v| v.id == id)
                .cloned()
                .ok_or_else(|| failed(CMD, format!("no Visit {id} in History")))?;
            let tab = s
                .open_location(&v.location, request(&p))
                .map_err(|e| failed(CMD, e))?;
            Ok(json!({ "tab": tab.0, "label": v.label }))
        },
    },
    CommandSpec {
        id: "history.clear",
        label: "Clear History",
        shortcut: None,
        params: &[],
        enabled: has_user,
        run: |s, _| {
            const CMD: &str = "history.clear";
            let user = s
                .user
                .as_ref()
                .ok_or_else(|| failed(CMD, "no User database"))?;
            user.clear_history().map_err(|e| failed(CMD, e))?;
            // Visits not yet recorded are History too.
            s.workspace.forget_visits();
            s.places.history_failed_at = None;
            s.places.history.clear();
            s.places.last_visit = None;
            Ok(Value::Null)
        },
    },
];

fn favorite_add(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const CMD: &str = "favorite.add";
    let tab = p
        .i64("tab")
        .and_then(|n| u64::try_from(n).ok())
        .map(TabId)
        .ok_or_else(|| failed(CMD, "no tab"))?;
    let (location, label) = s
        .location_of(tab)
        .ok_or_else(|| failed(CMD, "This Tab has no location to add to Favorites"))?;
    let folder = p.str("folder").unwrap_or_default().trim().to_string();
    if let Some(f) = s
        .places
        .favorites
        .iter()
        .find(|f| f.location == location && f.folder.eq_ignore_ascii_case(&folder))
    {
        return Ok(json!({ "id": f.id, "label": f.label, "added": false }));
    }
    let user = s
        .user
        .as_ref()
        .ok_or_else(|| failed(CMD, "no User database"))?;
    let id = user
        .add_favorite(&folder, &label, &location.to_json())
        .map_err(|e| failed(CMD, e))?;
    s.reload_favorites();
    let message = if folder.is_empty() {
        format!("Added {label} to Favorites")
    } else {
        format!("Added {label} to Favorites › {folder}")
    };
    s.workspace.push_effect(Effect::Message(message));
    Ok(json!({ "id": id, "label": label, "added": true }))
}
