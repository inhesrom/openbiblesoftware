//! Saving and restoring the Workspace (#37): the Last session and named Layouts are
//! [`WorkspaceSnapshot`]s, stored as JSON in the User database through [`UserData`].

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use obs_scripture::Verse;
use obs_store::{
    Annotation, BackupContents, BackupFile, Change, FavoriteRow, HighlightPalette, HighlightStyle,
    Notebook, PlanReadingRow, ReadingPlanRow, VisitRow,
};

use crate::floating_windows::WindowSnapshot;
use crate::workspace::{RecentlyClosed, TabKind, Workspace};

/// Snapshot format version; bump when the shape changes incompatibly.
pub const SNAPSHOT_VERSION: u32 = 1;

/// How Panels are arranged: a tree of rows and columns with Panels (Tabs containers) as
/// leaves. Shares are relative sizes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayoutNode {
    Panel {
        panel: u64,
        tabs: Vec<u64>,
        active: Option<u64>,
    },
    Row {
        children: Vec<(LayoutNode, f32)>,
    },
    Column {
        children: Vec<(LayoutNode, f32)>,
    },
}

impl LayoutNode {
    /// Keep only the Tabs in `keep`; Panels left empty disappear, and so do rows and columns
    /// left empty.
    pub fn retain_tabs(self, keep: &BTreeSet<u64>) -> Option<LayoutNode> {
        match self {
            LayoutNode::Panel {
                panel,
                tabs,
                active,
            } => {
                let tabs: Vec<u64> = tabs.into_iter().filter(|t| keep.contains(t)).collect();
                if tabs.is_empty() {
                    return None;
                }
                let active = active
                    .filter(|a| tabs.contains(a))
                    .or(tabs.first().copied());
                Some(LayoutNode::Panel {
                    panel,
                    tabs,
                    active,
                })
            }
            LayoutNode::Row { children } => {
                let c = retain_children(children, keep);
                (!c.is_empty()).then_some(LayoutNode::Row { children: c })
            }
            LayoutNode::Column { children } => {
                let c = retain_children(children, keep);
                (!c.is_empty()).then_some(LayoutNode::Column { children: c })
            }
        }
    }

    /// Every Panel: its id, Tabs and active Tab.
    pub fn panels(&self) -> Vec<(u64, Vec<u64>, Option<u64>)> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<(u64, Vec<u64>, Option<u64>)>) {
        match self {
            LayoutNode::Panel {
                panel,
                tabs,
                active,
            } => out.push((*panel, tabs.clone(), *active)),
            LayoutNode::Row { children } | LayoutNode::Column { children } => {
                for (c, _) in children {
                    c.collect(out);
                }
            }
        }
    }
}

fn retain_children(
    children: Vec<(LayoutNode, f32)>,
    keep: &BTreeSet<u64>,
) -> Vec<(LayoutNode, f32)> {
    children
        .into_iter()
        .filter_map(|(c, share)| c.retain_tabs(keep).map(|c| (c, share)))
        .collect()
}

/// One Tab as saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabSnapshot {
    pub id: u64,
    pub kind: TabKind,
    pub title: String,
    /// The Current location, in the Tab's own numbering.
    pub location: Option<Verse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_location: Option<obs_model::Key>,
    pub link: Option<char>,
    /// A Bible Tab's interlinear lines (#143).
    #[serde(
        default,
        skip_serializing_if = "crate::interlinear::InterlinearLines::is_default"
    )]
    pub interlinear: crate::interlinear::InterlinearLines,
    /// A Resource Tab shows its Contents pane (#176).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub contents: bool,
    /// Where a closed Resource Tab's view was scrolled, for Reopen closed Tab (#214). Only
    /// Recently closed Tabs have one ([`TabSnapshot::closing`]); Layouts and the Last
    /// session's open Tabs do not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll: Option<ScrollPosition>,
    /// The user hid the Tab's Footnote markers (#208).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub footnote_markers_hidden: bool,
    /// A Search Tab's search: its mode and fields (#259). Restored, it runs again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<crate::favorites::SavedSearch>,
    /// A Text Comparison's choices: Bibles, base, range and Differences (#259).
    #[serde(
        default,
        skip_serializing_if = "crate::compare::Comparison::is_default"
    )]
    pub comparison: crate::compare::Comparison,
}

impl TabSnapshot {
    /// Tab `t` as a Layout or the Last session saves it.
    pub fn of(t: &crate::workspace::Tab) -> Self {
        TabSnapshot {
            id: t.id.0,
            kind: t.kind.clone(),
            title: t.title.clone(),
            location: t.history.current(),
            entry_location: t.entry_history.current(),
            link: t.link,
            interlinear: t.interlinear.clone(),
            contents: t.contents,
            scroll: None,
            footnote_markers_hidden: !t.footnote_markers,
            search: (t.kind == TabKind::Tool(crate::workspace::Tool::Search))
                .then(|| crate::favorites::SavedSearch::fields(&t.search))
                .filter(|f| *f != crate::favorites::SavedSearch::default()),
            comparison: t.comparison.clone(),
        }
    }

    /// Tab `t` as Recently closed keeps it, closed by the user now: scrolled as its view is.
    pub fn closing(t: &crate::workspace::Tab) -> Self {
        TabSnapshot {
            scroll: t.scroll.clone(),
            ..Self::of(t)
        }
    }
}

/// A place in a Resource Tab's text finer than its Current location: the text unit at the
/// top of its view, a UTF-8 byte offset into it, and how far, in points, the view is scrolled
/// past the top of the line holding that byte (#214).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScrollPosition {
    pub unit: obs_model::UnitId,
    pub byte: usize,
    pub into: f32,
}

/// The Workspace as saved: Tabs, the arrangement of Panels, Link sets, focus and each Tab
/// kind's remembered Panel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub version: u32,
    pub tabs: Vec<TabSnapshot>,
    pub layout: Option<LayoutNode>,
    pub focused: Option<u64>,
    /// Tab kind slot (as text) → Panel id.
    pub remembered: Vec<(String, u64)>,
    pub last_reading_panel: Option<u64>,
    pub last_other_panel: Option<u64>,
    /// Floating windows (#152), each with its own Panels and where it was on screen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<WindowSnapshot>,
    /// Recently closed Tabs and Panels (#214): kept with the Last session, also when no Tab
    /// is open, never with a Layout.
    #[serde(default, skip_serializing_if = "RecentlyClosed::is_default")]
    pub recently_closed: RecentlyClosed,
}

impl WorkspaceSnapshot {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parse a saved snapshot; `None` if it is unreadable or from an unknown version.
    pub fn from_json(s: &str) -> Option<Self> {
        let snap: Self = serde_json::from_str(s).ok()?;
        (snap.version == SNAPSHOT_VERSION).then_some(snap)
    }

    /// Drop Tabs whose Resource is no longer in the library.
    pub fn without_missing(mut self, has: impl Fn(&str) -> bool) -> Self {
        let kept = |t: &TabSnapshot| t.kind.resource_id().is_none_or(&has);
        self.tabs.retain(kept);
        let keep: BTreeSet<u64> = self.tabs.iter().map(|t| t.id).collect();
        self.layout = self.layout.and_then(|l| l.retain_tabs(&keep));
        self.windows = std::mem::take(&mut self.windows)
            .into_iter()
            .filter_map(|w| {
                Some(WindowSnapshot {
                    layout: w.layout.retain_tabs(&keep)?,
                    ..w
                })
            })
            .collect();
        if self.focused.is_some_and(|f| !keep.contains(&f)) {
            self.focused = None;
        }
        let closed = &mut self.recently_closed.closed;
        for c in closed.iter_mut() {
            c.tabs.retain(kept);
        }
        closed.retain(|c| !c.tabs.is_empty());
        self
    }
}

/// Where the Workspace and preferences are kept: the User database on the desktop.
pub trait UserData: std::fmt::Debug {
    fn preference(&self, key: &str) -> Result<Option<String>, String>;
    fn set_preference(&self, key: &str, value: &str) -> Result<(), String>;
    fn last_session(&self) -> Result<Option<String>, String>;
    fn save_last_session(&self, snapshot: &str) -> Result<(), String>;
    fn layouts(&self) -> Result<Vec<String>, String>;
    fn layout(&self, name: &str) -> Result<Option<String>, String>;
    fn save_layout(&self, name: &str, snapshot: &str) -> Result<(), String>;
    fn rename_layout(&self, from: &str, to: &str) -> Result<bool, String>;
    fn delete_layout(&self, name: &str) -> Result<bool, String>;
    /// The saved Preferred order: resource ids, best first.
    fn preferred_order(&self) -> Result<Vec<String>, String>;
    fn save_preferred_order(&self, ids: &[String]) -> Result<(), String>;
    /// The last known `(resource_id, abbreviation, title)` of every Resource seen, so a
    /// removed Resource's Annotations still name it (#65).
    fn resource_names(&self) -> Result<Vec<(String, String, String)>, String>;
    /// Remember Resources' abbreviations and titles (others are kept).
    fn save_resource_names(&self, names: &[(String, String, String)]) -> Result<(), String>;

    // Favorites and History (#155); see `favorites.rs`.

    /// Every Favorite, by folder (none first) and then in the order added.
    fn favorites(&self) -> Result<Vec<FavoriteRow>, String>;
    /// Add a Favorite at the end of `folder` (empty: none); returns its id.
    fn add_favorite(&self, folder: &str, label: &str, location: &str) -> Result<i64, String>;
    fn remove_favorite(&self, id: i64) -> Result<bool, String>;
    fn move_favorite(&self, id: i64, folder: &str) -> Result<bool, String>;
    /// Add a Visit made now to History (which forgets Visits past its cap).
    fn record_visit(&self, label: &str, location: &str) -> Result<(), String>;
    /// History, newest first, with its days counted to the local day of `at` (Unix seconds).
    fn history_as_of(&self, at: i64) -> Result<Vec<VisitRow>, String>;
    /// The local day of `at` (Unix seconds), `YYYY-MM-DD`.
    fn local_day(&self, at: i64) -> Result<String, String>;
    fn clear_history(&self) -> Result<(), String>;

    // Reading plans (#147); see `reading_plans.rs`.

    /// Every Reading plan, oldest first.
    fn reading_plans(&self) -> Result<Vec<ReadingPlanRow>, String>;
    /// A Reading plan's Readings, in order.
    fn plan_readings(&self, plan_id: i64) -> Result<Vec<PlanReadingRow>, String>;
    /// Add a Reading plan with its Readings, in order; returns its id.
    fn add_reading_plan(
        &self,
        plan: &ReadingPlanRow,
        readings: &[PlanReadingRow],
    ) -> Result<i64, String>;
    /// Mark a Reading read or unread; false if the plan has no such Reading.
    fn set_reading_read(&self, plan_id: i64, position: u32, read: bool) -> Result<bool, String>;
    /// Move Readings to other days: `(position, day)` pairs.
    fn move_readings(&self, plan_id: i64, days: &[(u32, String)]) -> Result<(), String>;
    fn remove_reading_plan(&self, id: i64) -> Result<bool, String>;

    // Annotations (#61). Every edit is [`UserData::apply_changes`]; see `annotations.rs`.

    /// Every Notebook, the default first.
    fn notebooks(&self) -> Result<Vec<Notebook>, String>;
    /// How many Annotations each Notebook holds (empty Notebooks left out).
    fn notebook_counts(&self) -> Result<Vec<(String, u64)>, String>;
    fn annotation(&self, id: &str) -> Result<Option<Annotation>, String>;
    /// Annotations in one Notebook (or all), oldest first.
    fn annotations(&self, notebook: Option<&str>) -> Result<Vec<Annotation>, String>;
    /// How many Annotations have a Text range in Resource `resource_id`.
    fn annotations_referring(&self, resource_id: &str) -> Result<u64, String>;
    /// Ids of Notes matching an FTS5 query over normalized words, newest first.
    fn search_notes(&self, fts_query: &str) -> Result<Vec<String>, String>;
    /// Apply row Changes in one transaction, checking each row is still its `before`. The
    /// error says why: undo tells a Conflict or Rejected list (the history no longer fits the
    /// database) from a failure that may pass (a busy or full disk).
    fn apply_changes(&self, changes: &[Change]) -> Result<(), obs_store::StoreError>;

    // Highlight styles and palettes (#181); see `highlight_styles.rs`. Edits are row Changes.

    /// Every Highlight palette, in order.
    fn highlight_palettes(&self) -> Result<Vec<HighlightPalette>, String>;
    /// Every Highlight style, by palette and then in order within it.
    fn highlight_styles(&self) -> Result<Vec<HighlightStyle>, String>;
    /// How many Highlights use each style (unused styles left out).
    fn highlight_style_uses(&self) -> Result<Vec<(String, u64)>, String>;
    /// The annotation tables as text, for tests comparing database states.
    #[cfg(any(test, feature = "test-support"))]
    fn annotation_dump(&self) -> Result<String, String>;

    // Backups (#217); see `backups.rs`.

    /// Write a Backup of everything to `dest`, replacing a file there (never the User
    /// database itself, see [`UserData::refuse_own_file`]).
    fn back_up(&self, dest: &Path) -> Result<(), String>;
    /// Refuse `dest` if writing there would overwrite the User database or SQLite's files
    /// beside it, by whatever path names them.
    fn refuse_own_file(&self, dest: &Path) -> Result<(), String>;
    /// The Backups the app keeps by itself (at launch, Safety backups, before an upgrade),
    /// newest first.
    fn backups(&self) -> Result<Vec<BackupFile>, String>;
    /// What the User database holds now.
    fn contents(&self) -> Result<BackupContents, String>;
    /// What the Backup at `path` holds, migrated to this build's format; the file is only read.
    fn backup_contents(&self, path: &Path) -> Result<BackupContents, String>;
    /// Replace everything with the Backup at `path`, after taking a Safety backup, whose path
    /// is returned. An unreadable or newer Backup changes nothing.
    fn restore(&mut self, path: &Path) -> Result<PathBuf, String>;
    /// Every saved preference, `(key, value)`, by key.
    fn preferences(&self) -> Result<Vec<(String, String)>, String>;
    /// The local date and time of `at` (Unix seconds), `YYYY-MM-DD HH:MM`.
    fn local_time(&self, at: i64) -> Result<String, String>;
}

#[cfg(not(target_arch = "wasm32"))]
impl UserData for obs_store::UserStore {
    fn preference(&self, key: &str) -> Result<Option<String>, String> {
        obs_store::UserStore::preference(self, key).map_err(|e| e.to_string())
    }
    fn set_preference(&self, key: &str, value: &str) -> Result<(), String> {
        obs_store::UserStore::set_preference(self, key, value).map_err(|e| e.to_string())
    }
    fn last_session(&self) -> Result<Option<String>, String> {
        obs_store::UserStore::last_session(self).map_err(|e| e.to_string())
    }
    fn save_last_session(&self, snapshot: &str) -> Result<(), String> {
        obs_store::UserStore::save_last_session(self, snapshot).map_err(|e| e.to_string())
    }
    fn layouts(&self) -> Result<Vec<String>, String> {
        obs_store::UserStore::layouts(self).map_err(|e| e.to_string())
    }
    fn layout(&self, name: &str) -> Result<Option<String>, String> {
        obs_store::UserStore::layout(self, name).map_err(|e| e.to_string())
    }
    fn save_layout(&self, name: &str, snapshot: &str) -> Result<(), String> {
        obs_store::UserStore::save_layout(self, name, snapshot).map_err(|e| e.to_string())
    }
    fn rename_layout(&self, from: &str, to: &str) -> Result<bool, String> {
        obs_store::UserStore::rename_layout(self, from, to).map_err(|e| e.to_string())
    }
    fn delete_layout(&self, name: &str) -> Result<bool, String> {
        obs_store::UserStore::delete_layout(self, name).map_err(|e| e.to_string())
    }
    fn preferred_order(&self) -> Result<Vec<String>, String> {
        obs_store::UserStore::preferred_order(self).map_err(|e| e.to_string())
    }
    fn save_preferred_order(&self, ids: &[String]) -> Result<(), String> {
        obs_store::UserStore::save_preferred_order(self, ids).map_err(|e| e.to_string())
    }
    fn resource_names(&self) -> Result<Vec<(String, String, String)>, String> {
        obs_store::UserStore::resource_names(self).map_err(|e| e.to_string())
    }
    fn save_resource_names(&self, names: &[(String, String, String)]) -> Result<(), String> {
        obs_store::UserStore::save_resource_names(self, names).map_err(|e| e.to_string())
    }
    fn favorites(&self) -> Result<Vec<FavoriteRow>, String> {
        obs_store::UserStore::favorites(self).map_err(|e| e.to_string())
    }
    fn add_favorite(&self, folder: &str, label: &str, location: &str) -> Result<i64, String> {
        obs_store::UserStore::add_favorite(self, folder, label, location).map_err(|e| e.to_string())
    }
    fn remove_favorite(&self, id: i64) -> Result<bool, String> {
        obs_store::UserStore::remove_favorite(self, id).map_err(|e| e.to_string())
    }
    fn move_favorite(&self, id: i64, folder: &str) -> Result<bool, String> {
        obs_store::UserStore::move_favorite(self, id, folder).map_err(|e| e.to_string())
    }
    fn record_visit(&self, label: &str, location: &str) -> Result<(), String> {
        obs_store::UserStore::record_visit(self, label, location).map_err(|e| e.to_string())
    }
    fn history_as_of(&self, at: i64) -> Result<Vec<VisitRow>, String> {
        obs_store::UserStore::history_as_of(self, at).map_err(|e| e.to_string())
    }
    fn local_day(&self, at: i64) -> Result<String, String> {
        obs_store::UserStore::local_day(self, at).map_err(|e| e.to_string())
    }
    fn clear_history(&self) -> Result<(), String> {
        obs_store::UserStore::clear_history(self).map_err(|e| e.to_string())
    }
    fn reading_plans(&self) -> Result<Vec<ReadingPlanRow>, String> {
        obs_store::UserStore::reading_plans(self).map_err(|e| e.to_string())
    }
    fn plan_readings(&self, plan_id: i64) -> Result<Vec<PlanReadingRow>, String> {
        obs_store::UserStore::plan_readings(self, plan_id).map_err(|e| e.to_string())
    }
    fn add_reading_plan(
        &self,
        plan: &ReadingPlanRow,
        readings: &[PlanReadingRow],
    ) -> Result<i64, String> {
        obs_store::UserStore::add_reading_plan(self, plan, readings).map_err(|e| e.to_string())
    }
    fn set_reading_read(&self, plan_id: i64, position: u32, read: bool) -> Result<bool, String> {
        obs_store::UserStore::set_reading_read(self, plan_id, position, read)
            .map_err(|e| e.to_string())
    }
    fn move_readings(&self, plan_id: i64, days: &[(u32, String)]) -> Result<(), String> {
        obs_store::UserStore::move_readings(self, plan_id, days).map_err(|e| e.to_string())
    }
    fn remove_reading_plan(&self, id: i64) -> Result<bool, String> {
        obs_store::UserStore::remove_reading_plan(self, id).map_err(|e| e.to_string())
    }
    fn notebooks(&self) -> Result<Vec<Notebook>, String> {
        obs_store::UserStore::notebooks(self).map_err(|e| e.to_string())
    }
    fn notebook_counts(&self) -> Result<Vec<(String, u64)>, String> {
        obs_store::UserStore::notebook_counts(self).map_err(|e| e.to_string())
    }
    fn annotation(&self, id: &str) -> Result<Option<Annotation>, String> {
        obs_store::UserStore::annotation(self, id).map_err(|e| e.to_string())
    }
    fn annotations(&self, notebook: Option<&str>) -> Result<Vec<Annotation>, String> {
        obs_store::UserStore::annotations(self, notebook).map_err(|e| e.to_string())
    }
    fn annotations_referring(&self, resource_id: &str) -> Result<u64, String> {
        obs_store::UserStore::annotations_referring(self, resource_id).map_err(|e| e.to_string())
    }
    fn search_notes(&self, fts_query: &str) -> Result<Vec<String>, String> {
        obs_store::UserStore::search_notes(self, fts_query).map_err(|e| e.to_string())
    }
    fn apply_changes(&self, changes: &[Change]) -> Result<(), obs_store::StoreError> {
        obs_store::UserStore::apply_changes(self, changes)
    }
    fn highlight_palettes(&self) -> Result<Vec<HighlightPalette>, String> {
        obs_store::UserStore::highlight_palettes(self).map_err(|e| e.to_string())
    }
    fn highlight_styles(&self) -> Result<Vec<HighlightStyle>, String> {
        obs_store::UserStore::highlight_styles(self).map_err(|e| e.to_string())
    }
    fn highlight_style_uses(&self) -> Result<Vec<(String, u64)>, String> {
        obs_store::UserStore::highlight_style_uses(self).map_err(|e| e.to_string())
    }
    #[cfg(any(test, feature = "test-support"))]
    fn annotation_dump(&self) -> Result<String, String> {
        obs_store::UserStore::annotation_dump(self).map_err(|e| e.to_string())
    }
    fn back_up(&self, dest: &Path) -> Result<(), String> {
        obs_store::UserStore::back_up(self, dest).map_err(|e| e.to_string())
    }
    fn refuse_own_file(&self, dest: &Path) -> Result<(), String> {
        obs_store::UserStore::refuse_own_file(self, dest).map_err(|e| e.to_string())
    }
    fn backups(&self) -> Result<Vec<BackupFile>, String> {
        obs_store::UserStore::backups(self).map_err(|e| e.to_string())
    }
    fn contents(&self) -> Result<BackupContents, String> {
        obs_store::UserStore::contents(self).map_err(|e| e.to_string())
    }
    fn backup_contents(&self, path: &Path) -> Result<BackupContents, String> {
        obs_store::UserStore::backup_contents(path).map_err(|e| e.to_string())
    }
    fn restore(&mut self, path: &Path) -> Result<PathBuf, String> {
        obs_store::UserStore::restore(self, path).map_err(|e| e.to_string())
    }
    fn preferences(&self) -> Result<Vec<(String, String)>, String> {
        obs_store::UserStore::preferences(self).map_err(|e| e.to_string())
    }
    fn local_time(&self, at: i64) -> Result<String, String> {
        obs_store::UserStore::local_time(self, at).map_err(|e| e.to_string())
    }
}

impl Workspace {
    /// The Workspace as it is now.
    pub fn snapshot(&self) -> WorkspaceSnapshot {
        let live: BTreeSet<u64> = self.tabs().map(|t| t.id.0).collect();
        WorkspaceSnapshot {
            version: SNAPSHOT_VERSION,
            tabs: self.tabs().map(TabSnapshot::of).collect(),
            layout: self.arrangement.clone(),
            focused: self.focused().map(|t| t.0),
            remembered: self.remembered_slots(),
            last_reading_panel: self.last_panels().0,
            last_other_panel: self.last_panels().1,
            windows: self
                .floating
                .iter()
                .filter_map(|w| {
                    Some(WindowSnapshot {
                        id: w.id.0,
                        layout: w.arrangement.clone()?.retain_tabs(&live)?,
                        placement: w.placement,
                    })
                })
                .collect(),
            recently_closed: self.closed_snapshot(),
        }
    }
}
