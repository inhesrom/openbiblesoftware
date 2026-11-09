//! The trial restore (#258): what a restore of a Backup would lose, found by loading it.
//!
//! The store checks the database itself (`obs_store`'s `backup_store.rs`: every table,
//! column and row readable, no row pointing at one that is not there). What the engine keeps
//! *inside* rows (Layouts, Favorite and Visit locations, Reading plans' days and Passages, and
//! the settings it keeps as JSON or text) only its loaders read, and they read leniently: an
//! unreadable record is left out, an unreadable or unknown field is left at its default, a
//! value out of range is clamped, a repeated entry is kept once. So rather than check each
//! kind of record on its own (which drifts from the loaders), a restore first loads the
//! Backup with the loaders themselves ([`Session::trial_restore`]):
//!
//! 1. What the Backup holds is read first ([`StoredRecords`]), since loading writes. Then a
//!    copy of it, in memory, is attached to a Session of its own with this
//!    Session's library, exactly as a launch on it would ([`Session::attach_user`]), every
//!    Layout is restored with `layout.restore`, and what is read on demand (recent searches,
//!    Command box entries, Word forms, kept Attributions) is read.
//! 2. What that Session then holds is compared with what the Backup holds
//!    ([`Session::unkept_records`]): every Favorite, Visit, Reading plan and Reading must be
//!    there, each location as stored; every Layout must be read whole (its stored JSON
//!    against what was read of it) and restored whole (what was read against the Workspace
//!    restored), but the Tabs of Resources not in the library and what names a Tab or Panel
//!    the Layout does not have, which a launch leaves out too; and each setting the loaders
//!    read must be
//!    what the app would write back for it now, holding every value the stored one holds
//!    ([`holds`]). A record from an older version is first put in today's shape by the
//!    loader's own upgrade (`appearance::upgrade_record`, `KeyBindings::upgrade_record`).
//! 3. A panic while loading (a record no loader expected) is caught and refuses the Backup
//!    as damaged.
//!
//! A Backup with anything the trial did not keep is refused, naming each, before a Safety
//! backup is taken or anything is replaced. Two settings are not compared: the last Notebook
//! used and the active Highlight palette only name another record, and fall back to the
//! default when it is gone, as at launch. A setting no loader reads (a Guide arrangement of a
//! Guide this version does not have) is restored as it is; nothing reads or overwrites it.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

use serde::Serialize;
use serde_json::{Value, json};

use crate::persist::{UserData, WorkspaceSnapshot};
use crate::session::{JobMode, Session};

/// One setting as a Session holds it after loading.
pub(crate) struct HeldSetting {
    /// Its preference key.
    pub key: String,
    /// What the user calls it.
    pub what: &'static str,
    /// What the app would write for it now; `None` when the Session holds none of it, so a
    /// stored one was left out.
    pub value: Option<String>,
}

fn held(key: impl Into<String>, what: &'static str, value: Option<String>) -> HeldSetting {
    HeldSetting {
        key: key.into(),
        what,
        value,
    }
}

fn written(v: &impl Serialize) -> Option<String> {
    serde_json::to_string(v).ok()
}

/// What a User database holds that the trial restore compares, read before anything loads
/// it: loading writes to it (a Text size too small to read is reset, the Attributions of the
/// library's Resources are confirmed).
pub(crate) struct StoredRecords {
    preferences: BTreeMap<String, String>,
    favorites: Vec<obs_store::FavoriteRow>,
    visits: Vec<obs_store::VisitRow>,
    plans: Vec<(obs_store::ReadingPlanRow, Vec<obs_store::PlanReadingRow>)>,
    layouts: Vec<(String, Option<String>)>,
}

impl StoredRecords {
    pub(crate) fn read(user: &dyn UserData) -> Result<Self, String> {
        let mut plans = Vec::new();
        for plan in user.reading_plans()? {
            let readings = user.plan_readings(plan.id)?;
            plans.push((plan, readings));
        }
        let mut layouts = Vec::new();
        for name in user.layouts()? {
            let json = user.layout(&name)?;
            layouts.push((name, json));
        }
        Ok(Self {
            preferences: user.preferences()?.into_iter().collect(),
            favorites: user.favorites()?,
            visits: user
                .history_as_of(i64::try_from(crate::get_bibles::now_secs()).unwrap_or(0))?,
            plans,
            layouts,
        })
    }
}

impl Session {
    /// Load `backup` (a Backup read into memory) into a Session of its own, as a launch on it
    /// would with this Session's library, and list what the app would not keep of it
    /// ([`Session::unkept_records`]). The trial loads a copy, so `backup` is not changed, and
    /// this Session is not either. A panic while loading refuses the Backup as damaged.
    pub(crate) fn trial_restore(&self, backup: &dyn UserData) -> Result<Vec<String>, String> {
        let stored = StoredRecords::read(backup)?;
        let copy = backup.copy_in_memory()?;
        let library = self.library.clone();
        let tried = catch_unwind(AssertUnwindSafe(move || {
            let mut trial = Session::new(JobMode::Inline);
            trial.library = library;
            trial.attach_user(copy);
            trial.unkept_records(&stored)
        }));
        // Loading applied the Backup's Log level to the process: put this Session's back.
        crate::diagnostics::set_log_level(self.log_level());
        tried.map_err(|panic| {
            format!(
                "the app stopped reading it: {}",
                crate::diagnostics::panic_message(&*panic)
            )
        })
    }

    /// What this Session, having loaded a User database that held `stored`, did not keep of
    /// it, one line each: "the Layout “Broken”", "the Favorite “John 3:16”", "2 History
    /// visits", "the Reading plan “Gospel”", "Reading 3 of the Reading plan “Gospel”", "the
    /// setting `view` (View settings)". Empty when it kept everything. Restores every Layout,
    /// so the Workspace is not as it was.
    pub(crate) fn unkept_records(&mut self, stored: &StoredRecords) -> Vec<String> {
        let mut out = Vec::new();
        for f in &stored.favorites {
            let kept = self
                .favorites()
                .iter()
                .find(|h| h.id == f.id)
                .is_some_and(|h| keeps("", &f.location, &h.location.to_json()));
            if !kept {
                out.push(format!("the Favorite “{}”", f.label));
            }
        }
        let lost_visits = stored
            .visits
            .iter()
            .filter(|v| {
                !self
                    .history()
                    .iter()
                    .find(|h| h.id == v.id)
                    .is_some_and(|h| keeps("", &v.location, &h.location.to_json()))
            })
            .count();
        match lost_visits {
            0 => {}
            1 => out.push("1 History visit".to_string()),
            n => out.push(format!("{n} History visits")),
        }
        for (plan, readings) in &stored.plans {
            let Some(held) = self.reading_plan(plan.id) else {
                out.push(format!("the Reading plan “{}”", plan.name));
                continue;
            };
            for r in readings {
                if !held.readings.iter().any(|h| h.position == r.position) {
                    out.push(format!(
                        "Reading {} of the Reading plan “{}”",
                        u64::from(r.position) + 1,
                        plan.name
                    ));
                }
            }
        }
        for (name, json) in &stored.layouts {
            if !self.keeps_layout(name, json.as_deref()) {
                out.push(format!("the Layout “{name}”"));
            }
        }
        for h in self.held_settings(&stored.preferences) {
            let Some(value) = stored.preferences.get(&h.key) else {
                continue;
            };
            let kept = match &h.value {
                Some(now) => keeps(&h.key, value, now),
                // Nothing held: kept only if the stored value said nothing either (`null`
                // over the Import report of a removed Resource).
                None => serde_json::from_str::<Value>(value).is_ok_and(|v| carries_nothing(&v)),
            };
            if !kept {
                out.push(format!("the setting `{}` ({})", h.key, h.what));
            }
        }
        out
    }

    /// Whether `layout.restore` keeps all of the Layout `name` (its snapshot `json`) as stored:
    /// every value of the stored JSON is read (none unknown to this version, none read as
    /// another), and the Workspace it restores holds all of what was read: every Tab, Panel,
    /// share, Floating window and remembered Panel. Two exceptions, as at launch: the Tabs of
    /// Resources not in the library are left out ([`WorkspaceSnapshot::without_missing`]), and
    /// a focused Tab, remembered Panel or last Panel that names one the Layout does not have
    /// falls back ([`without_dangling`]).
    fn keeps_layout(&mut self, name: &str, json: Option<&str>) -> bool {
        let Some(json) = json else {
            return false;
        };
        let (Ok(raw), Some(read)) = (
            serde_json::from_str::<Value>(json),
            WorkspaceSnapshot::from_json(json),
        ) else {
            return false;
        };
        if !serde_json::to_value(&read).is_ok_and(|v| holds(&raw, &v)) {
            return false;
        }
        let expected = without_dangling(read.without_missing(|id| self.library.get(id).is_some()));
        if self
            .run("layout.restore", &json!({ "name": name }))
            .is_err()
        {
            return false;
        }
        let restored = self.workspace.snapshot();
        match (
            serde_json::to_value(&expected),
            serde_json::to_value(&restored),
        ) {
            (Ok(expected), Ok(restored)) => restored_whole(expected, restored),
            _ => false,
        }
    }

    /// Every setting the loaders read, with what the app would write for it now. `stored`
    /// names the kept Attributions, which are read on demand by key.
    pub(crate) fn held_settings(&mut self, stored: &BTreeMap<String, String>) -> Vec<HeldSetting> {
        let view = crate::appearance::to_json(&self.appearance, &self.text, &self.fonts);
        let collections = self.library.collection_set();
        let mut out = vec![
            held(
                crate::session::VIEW_PREF,
                "View settings",
                Some(view.to_string()),
            ),
            held(
                crate::custom_theme::CUSTOM_THEME_PREF,
                "Custom theme",
                written(&self.custom_theme.to_file()),
            ),
            held(
                crate::toolbar_settings::TOOLBAR_PREF,
                "Toolbar",
                Some(self.toolbar.to_json().to_string()),
            ),
            held(
                crate::appearance::PAGE_TONE_HINT_PREF,
                "Page tone hint",
                self.page_tone_hinted.then(|| "shown".to_string()),
            ),
            held(crate::home::HOME_PREF, "Home", Some(self.home.to_json())),
            held(
                crate::copy_verses::OPTIONS_PREF,
                "Copy verses options",
                written(&self.copy_verses.options),
            ),
            held(
                crate::copy_verses::PRESETS_PREF,
                "Copy presets",
                written(&self.copy_verses.presets),
            ),
            held(
                crate::collections::COLLECTIONS_PREF,
                "Collections",
                written(&collections.collections),
            ),
            held(
                crate::collections::TAGS_PREF,
                "Resource tags",
                written(&collections.tags),
            ),
            held(
                crate::visual_filters::VISUAL_FILTERS_PREF,
                "Visual filters",
                written(&self.visual_filters.list),
            ),
            held(
                crate::visual_filters::VISUAL_FILTERS_NEXT_ID_PREF,
                "Visual filters",
                Some(self.visual_filters.next_id.to_string()),
            ),
            held(
                crate::saved_searches::SAVED_PREF,
                "Saved searches",
                written(&self.saved_searches()),
            ),
            held(
                crate::saved_searches::HISTORY_PREF,
                "Search history",
                written(&self.search_history()),
            ),
            held(
                crate::key_bindings::KEY_BINDINGS_PREF,
                "Keyboard shortcuts",
                Some(self.keys.to_json().to_string()),
            ),
            held(
                crate::diagnostics::LOG_LEVEL_PREF,
                "Log level",
                Some(self.log_level().id().to_string()),
            ),
            held(
                crate::updates::UPDATES_PREF,
                "Update check settings",
                Some(self.updates().to_json().to_string()),
            ),
            held(
                crate::tour::TOUR_PREF,
                "First-run tour",
                Some(json!({ "shown": self.tour().shown() }).to_string()),
            ),
            held(
                crate::starter::STATE_PREF,
                "Starter library",
                self.starter.state().and_then(written),
            ),
        ];
        for guide in crate::Guide::ALL {
            out.push(held(
                crate::guide::pref(guide),
                "Guide arrangement",
                Some(self.guides.arrangement(guide).to_json()),
            ));
        }
        for entry in self.library.entries() {
            let id = &entry.manifest.resource_id;
            out.push(held(
                crate::import::pref(id),
                "Import report",
                self.imports.reports.get(id).and_then(written),
            ));
        }
        for (key, value) in stored {
            if key.starts_with(crate::notes_export::ATTRIBUTION_PREF_PREFIX) {
                out.push(held(
                    key.clone(),
                    "kept Attribution",
                    crate::notes_export::KeptAttribution::read(value).and_then(|k| written(&k)),
                ));
            }
        }
        // Read on demand.
        for mode in crate::SearchMode::ALL {
            out.push(held(
                mode.preference_key(),
                "recent searches",
                written(&self.recent_searches(mode)),
            ));
        }
        out.push(held(
            crate::command_box::RECENT_PREF,
            "recent Command box entries",
            written(&self.command_box_recent()),
        ));
        let word_forms = if self.search_word_forms() {
            "true"
        } else {
            "false"
        };
        out.push(held(
            crate::bible_search::WORD_FORMS_KEY,
            "Word forms",
            Some(word_forms.to_string()),
        ));
        out
    }
}

/// Whether the Workspace `restored` (as JSON) holds all of the snapshot `expected` it was
/// restored from ([`holds`]). Its arrangement may also hold more: `Workspace::restore` gives
/// the Tabs the stored arrangement does not place (all of them, when it has none) a Panel of
/// their own on the right, in the stored Row or in a new Row holding the stored arrangement
/// first, and may then remember that Panel or make it the last one.
fn restored_whole(mut expected: Value, mut restored: Value) -> bool {
    let (Some(e), Some(r)) = (expected.as_object_mut(), restored.as_object_mut()) else {
        return false;
    };
    let stored = e.remove("layout").unwrap_or(Value::Null);
    let now = r.remove("layout").unwrap_or(Value::Null);
    // A Tab or Panel named by none is not one to keep: the restore may name one it made.
    e.retain(|_, v| !v.is_null());
    let beside = || {
        now["kind"] == "row"
            && now["children"]
                .as_array()
                .and_then(|c| c.first())
                .is_some_and(|first| holds(&stored, &first[0]))
    };
    holds(&expected, &restored) && (stored.is_null() || holds(&stored, &now) || beside())
}

/// `snap` without what names a Tab or Panel it does not have, which `Workspace::restore`
/// drops, as the app falls back without it: the focused Tab, remembered Panels (by Tab kind)
/// and the last reading and other Panel. The app writes remembered Panels it has since
/// closed. A remembered Panel's slot that is not one this version knows is kept, so it counts
/// as lost.
fn without_dangling(mut snap: WorkspaceSnapshot) -> WorkspaceSnapshot {
    let mut panels: Vec<u64> = snap
        .layout
        .iter()
        .chain(snap.windows.iter().map(|w| &w.layout))
        .flat_map(|l| l.panels())
        .map(|(panel, _, _)| panel)
        .collect();
    panels.sort_unstable();
    let has_panel = |p: &u64| panels.binary_search(p).is_ok();
    snap.remembered.retain(|(_, panel)| has_panel(panel));
    snap.last_reading_panel = snap.last_reading_panel.filter(has_panel);
    snap.last_other_panel = snap.last_other_panel.filter(has_panel);
    let tabs: Vec<u64> = snap.tabs.iter().map(|t| t.id).collect();
    snap.focused = snap.focused.filter(|f| tabs.contains(f));
    snap
}

/// Whether `now`, what the app would write for the setting `key` it read, keeps everything
/// `stored` holds. JSON is compared as JSON ([`holds`]), after the loader's upgrade of an
/// older record; other text as text.
pub(crate) fn keeps(key: &str, stored: &str, now: &str) -> bool {
    match (
        serde_json::from_str::<Value>(stored),
        serde_json::from_str::<Value>(now),
    ) {
        (Ok(stored), Ok(now)) => holds(&upgraded(key, &stored), &now),
        _ => stored == now,
    }
}

/// `stored` in the shape the app writes today, as the loader of `key` reads it.
fn upgraded(key: &str, stored: &Value) -> Value {
    match key {
        crate::session::VIEW_PREF => crate::appearance::upgrade_record(stored),
        crate::key_bindings::KEY_BINDINGS_PREF => {
            crate::key_bindings::KeyBindings::upgrade_record(stored)
        }
        _ => stored.clone(),
    }
}

/// Whether `now` holds every value `stored` does: each field of an object (one missing from
/// `now` only if it carries nothing, [`carries_nothing`], as fields at their default are left
/// out when written), each item of a list (in any order, each matched once, so a repeated
/// item kept once does not count as kept), and the same number, text or flag. A number is
/// the same when it is equal as written or as the app keeps it (`f32`). `now` may hold more:
/// fields and items the app adds as defaults.
pub(crate) fn holds(stored: &Value, now: &Value) -> bool {
    match (stored, now) {
        (Value::Number(a), Value::Number(b)) => {
            a == b
                || a.as_f64()
                    .zip(b.as_f64())
                    .is_some_and(|(a, b)| a == b || (a as f32) == (b as f32))
        }
        (Value::Array(a), Value::Array(b)) => {
            let mut free: Vec<&Value> = b.iter().collect();
            a.iter()
                .all(|x| match free.iter().position(|y| holds(x, y)) {
                    Some(i) => {
                        free.remove(i);
                        true
                    }
                    None => false,
                })
        }
        (Value::Object(a), Value::Object(b)) => a.iter().all(|(k, v)| match b.get(k) {
            Some(w) => holds(v, w),
            None => carries_nothing(v),
        }),
        _ => stored == now,
    }
}

/// Whether a value says nothing a writer leaving out defaults would write: `null`, `false`,
/// `0`, empty text, an empty list, or an object of such values.
fn carries_nothing(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.values().all(carries_nothing),
    }
}
