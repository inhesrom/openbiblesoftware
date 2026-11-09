//! What a Backup must hold readable before a restore may replace the User database (#258).
//!
//! The store checks the database itself (`obs_store`'s `backup_store.rs`: every table,
//! column and row readable, no row pointing at one that is not there). What the engine keeps
//! *inside* rows is its own: a Layout's Workspace snapshot, a Favorite's or a Visit's
//! location, a Reading plan's start day and each of its Readings' Passages and day, and the
//! settings it keeps as JSON ([`JSON_SETTINGS`]). The app reads each of
//! those leniently at launch: an unreadable one is left out or back at its default, and so is
//! an unreadable field of a setting read field by field (a View setting, a Keyboard
//! shortcut, an Update check flag, whether the First-run tour was shown), a Custom theme from a later version, a View Text size or Line spacing beyond what the app
//! allows (it would be clamped), a Layout holding two Tabs with one id (restoring it would
//! keep one of them), a Search history longer than the app keeps (its oldest
//! entries would be dropped), a repeated or unknown card of Home or section of a Guide
//! arrangement, and an Import report filed under another Resource's key. So a restore checks
//! them first, and a Backup holding one the app would not keep is refused, naming it, rather
//! than silently losing it after the live data is gone.
//!
//! Plain-text settings (a Log level, the last Notebook used) are not checked: any text is
//! one, and a value the app does not know is its default.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::appearance::{
    FontSlot, InterfaceSize, LINE_SPACING_MAX, LINE_SPACING_MIN, PageTone, TEXT_SIZE_MAX, Theme,
    VERSE_NUMBER_STYLES,
};
use crate::favorites::TabLocation;
use crate::persist::{UserData, WorkspaceSnapshot};
use crate::reading_plans::{Reading, ReadingPlan};
use crate::toolbar::Launcher;
use crate::toolbar_settings::{LauncherLabels, ToolbarPosition};

/// A setting the app keeps as JSON in the User database.
pub(crate) struct JsonSetting {
    /// Its preference key; one ending in `/` or `.` is the start of one key per Guide,
    /// search mode or Resource.
    pub key: &'static str,
    /// What the user calls it.
    pub what: &'static str,
    /// Whether the app keeps all of a value of it, given the key it is under (a Guide
    /// arrangement is one Guide's, an Import report one Resource's) and the value.
    pub reads: fn(&str, &str) -> bool,
}

impl JsonSetting {
    /// Whether `key` is this setting's key (or one it starts).
    pub(crate) fn names(&self, key: &str) -> bool {
        if self.key.ends_with(['/', '.']) {
            key.starts_with(self.key)
        } else {
            key == self.key
        }
    }
}

/// Whether `json` reads as a `T`, as the setting's loader reads it.
fn reads<T: DeserializeOwned>(_key: &str, json: &str) -> bool {
    serde_json::from_str::<T>(json).is_ok()
}

/// A field check: whether the loader reads this present value rather than leaving it at
/// its default.
type FieldCheck = fn(&Value) -> bool;

/// Whether `json` is an object each of whose `fields` that is present reads. The settings
/// read field by field (View, Toolbar, Update check) leave a field they cannot read at its
/// default, so a Backup holding one must be refused here. Fields the app does not know are
/// ignored on load and here.
fn object_with(json: &str, fields: &[(&str, FieldCheck)]) -> bool {
    let Ok(Value::Object(v)) = serde_json::from_str::<Value>(json) else {
        return false;
    };
    fields
        .iter()
        .all(|(name, reads)| v.get(*name).is_none_or(reads))
}

fn is_bool(v: &Value) -> bool {
    v.is_boolean()
}

fn is_number(v: &Value) -> bool {
    v.is_number()
}

/// The View settings, as `appearance::apply_json` reads them, with the records from #37 (a
/// `dark_page` flag and a boolean `verse_numbers`). A Text size above the maximum or a Line
/// spacing outside its range would be clamped, so is refused; a Text size below the minimum
/// is read, since the app resets it with a notice (#241).
fn reads_view(_key: &str, json: &str) -> bool {
    object_with(
        json,
        &[
            ("theme", |v| v.as_str().and_then(Theme::from_id).is_some()),
            ("page_tone", |v| {
                v.as_str().and_then(PageTone::from_id).is_some()
            }),
            ("dark_page", is_bool),
            ("interface_size", |v| {
                v.as_str().and_then(InterfaceSize::from_id).is_some()
            }),
            ("text_size", |v| {
                v.as_f64().is_some_and(|s| s as f32 <= TEXT_SIZE_MAX)
            }),
            ("text_size_default", is_number),
            ("line_spacing", |v| {
                v.as_f64()
                    .is_some_and(|s| (LINE_SPACING_MIN..=LINE_SPACING_MAX).contains(&(s as f32)))
            }),
            ("red_letter", is_bool),
            ("highlights", is_bool),
            ("reduce_motion", is_bool),
            ("verse_numbers", |v| match v {
                Value::String(s) => VERSE_NUMBER_STYLES.iter().any(|c| c.1 == s),
                Value::Bool(_) => true,
                _ => false,
            }),
            ("fonts", reads_fonts),
            // Written as `null` only if the layout could not be written.
            ("reading_layout", |v| {
                v.is_null()
                    || serde_json::from_value::<obs_layout::ReadingLayout>(v.clone()).is_ok()
            }),
        ],
    )
}

/// The font choices of the View settings: per script slot, a family (blank: none chosen)
/// with its style and whether its absence was noticed.
fn reads_fonts(v: &Value) -> bool {
    let Some(fonts) = v.as_object() else {
        return false;
    };
    FontSlot::ALL.iter().all(|slot| {
        fonts.get(slot.id()).is_none_or(|c| {
            c.as_object().is_some_and(|c| {
                c.get("family").is_some_and(Value::is_string)
                    && c.get("style").is_none_or(Value::is_string)
                    && c.get("missing_noticed").is_none_or(is_bool)
            })
        })
    })
}

/// The Toolbar, as `ToolbarSettings::from_json` reads it: each Launcher once.
fn reads_toolbar(_key: &str, json: &str) -> bool {
    object_with(
        json,
        &[
            ("position", |v| {
                v.as_str().and_then(ToolbarPosition::from_id).is_some()
            }),
            ("auto_hide", is_bool),
            ("labels", |v| {
                v.as_str().and_then(LauncherLabels::from_id).is_some()
            }),
            ("launchers", |v| {
                let Some(list) = v.as_array() else {
                    return false;
                };
                let mut seen = Vec::new();
                list.iter().all(|item| {
                    let launcher = item["id"].as_str().and_then(Launcher::from_id);
                    let read = launcher.is_some_and(|l| !seen.contains(&l))
                        && item.get("shown").is_none_or(is_bool);
                    seen.extend(launcher);
                    read
                })
            }),
        ],
    )
}

/// The Keyboard shortcuts, as `KeyBindings::from_json` reads them: per command id, a key it
/// can read or `null` for no key. A Selection key's old id counts as its new one, so the
/// two together would leave one binding out.
fn reads_key_bindings(_key: &str, json: &str) -> bool {
    let Ok(Value::Object(keys)) = serde_json::from_str::<Value>(json) else {
        return false;
    };
    let mut ids = std::collections::BTreeSet::new();
    keys.iter().all(|(id, key)| {
        let read = match key {
            Value::Null => true,
            Value::String(k) => crate::key_bindings::canonical_key(k).is_ok(),
            _ => false,
        };
        read && ids.insert(crate::key_bindings::current_binding_id(id))
    })
}

/// The Update check settings, as `Session::load_update_settings` reads them.
fn reads_update_settings(_key: &str, json: &str) -> bool {
    object_with(
        json,
        &[
            ("auto_check", is_bool),
            ("prereleases", is_bool),
            ("explained", is_bool),
            // `null`: no release skipped.
            ("skipped", |v| v.is_null() || v.is_string()),
        ],
    )
}

/// Whether the First-run tour was shown, as `Session::load_tour` reads it (an unreadable one
/// would start the tour again).
fn reads_tour(_key: &str, json: &str) -> bool {
    object_with(json, &[("shown", is_bool)])
}

/// A Guide arrangement, as `Session::load_guide_arrangements` reads it: under the key of a
/// Guide, each of that Guide's sections once.
fn reads_guide_arrangement(key: &str, json: &str) -> bool {
    key.strip_prefix(crate::guide::ARRANGEMENT_PREF_PREFIX)
        .and_then(crate::Guide::from_id)
        .is_some_and(|guide| crate::guide::GuideArrangement::keeps_every_section(guide, json))
}

/// An Import report, as `Session::restore_import_reports` reads it: the report of the
/// Resource its key names. `null`: the Resource was removed.
fn reads_import_report(key: &str, json: &str) -> bool {
    match serde_json::from_str::<Option<crate::import::ImportRecord>>(json) {
        Ok(None) => true,
        Ok(Some(record)) => {
            key.strip_prefix(crate::import::REPORT_PREF_PREFIX) == Some(&record.resource_id)
        }
        Err(_) => false,
    }
}

/// Every setting the app keeps as JSON, with how its loader reads it.
pub(crate) const JSON_SETTINGS: &[JsonSetting] = &[
    JsonSetting {
        key: crate::session::VIEW_PREF,
        what: "View settings (Text size, Theme, fonts)",
        reads: reads_view,
    },
    JsonSetting {
        key: crate::custom_theme::CUSTOM_THEME_PREF,
        what: "Custom theme",
        reads: |_, json| {
            serde_json::from_str::<crate::custom_theme::ThemeFile>(json)
                .is_ok_and(|f| crate::custom_theme::CustomTheme::from_file(&f).is_ok())
        },
    },
    JsonSetting {
        key: crate::toolbar_settings::TOOLBAR_PREF,
        what: "Toolbar",
        reads: reads_toolbar,
    },
    JsonSetting {
        key: crate::key_bindings::KEY_BINDINGS_PREF,
        what: "Keyboard shortcuts",
        reads: reads_key_bindings,
    },
    JsonSetting {
        key: crate::updates::UPDATES_PREF,
        what: "Update check settings",
        reads: reads_update_settings,
    },
    JsonSetting {
        key: crate::tour::TOUR_PREF,
        what: "First-run tour",
        reads: reads_tour,
    },
    JsonSetting {
        key: crate::home::HOME_PREF,
        what: "Home",
        reads: |_, json| crate::home::HomeSettings::keeps_every_card(json),
    },
    JsonSetting {
        key: crate::guide::ARRANGEMENT_PREF_PREFIX,
        what: "Guide arrangement",
        reads: reads_guide_arrangement,
    },
    JsonSetting {
        key: crate::saved_searches::SAVED_PREF,
        what: "Saved searches",
        reads: reads::<Vec<crate::saved_searches::NamedSearch>>,
    },
    JsonSetting {
        key: crate::saved_searches::HISTORY_PREF,
        what: "Search history",
        // The loader keeps the newest `SEARCH_HISTORY_LIMIT` entries.
        reads: |_, json| {
            serde_json::from_str::<Vec<crate::saved_searches::SearchHistoryEntry>>(json)
                .is_ok_and(|h| h.len() <= crate::saved_searches::SEARCH_HISTORY_LIMIT)
        },
    },
    JsonSetting {
        key: crate::bible_search::RECENT_PREF_PREFIX,
        what: "recent searches",
        reads: reads::<Vec<String>>,
    },
    JsonSetting {
        key: crate::command_box::RECENT_PREF,
        what: "recent Command box entries",
        reads: reads::<Vec<String>>,
    },
    JsonSetting {
        key: crate::copy_verses::OPTIONS_PREF,
        what: "Copy verses options",
        reads: reads::<crate::copy_verses::CopyOptions>,
    },
    JsonSetting {
        key: crate::copy_verses::PRESETS_PREF,
        what: "Copy presets",
        reads: reads::<Vec<crate::copy_verses::CopyPreset>>,
    },
    JsonSetting {
        key: crate::collections::COLLECTIONS_PREF,
        what: "Collections",
        reads: reads::<Vec<crate::collections::Collection>>,
    },
    JsonSetting {
        key: crate::collections::TAGS_PREF,
        what: "Resource tags",
        reads: reads::<std::collections::BTreeMap<String, Vec<String>>>,
    },
    JsonSetting {
        key: crate::visual_filters::VISUAL_FILTERS_PREF,
        what: "Visual filters",
        reads: reads::<Vec<crate::visual_filters::VisualFilter>>,
    },
    JsonSetting {
        key: crate::starter::STATE_PREF,
        what: "Starter library",
        reads: reads::<crate::starter::StarterState>,
    },
    JsonSetting {
        // Removing a Resource writes `null` over its report.
        key: crate::import::REPORT_PREF_PREFIX,
        what: "Import report",
        reads: reads_import_report,
    },
    JsonSetting {
        key: crate::notes_export::ATTRIBUTION_PREF_PREFIX,
        what: "kept Attribution",
        reads: reads::<crate::notes_export::KeptAttribution>,
    },
];

/// Whether every Tab of a Layout has its own id: restoring the Layout keeps one Tab per id,
/// so a second Tab with the same id would be dropped.
fn holds_each_tab_once(snap: &WorkspaceSnapshot) -> bool {
    let mut ids = std::collections::BTreeSet::new();
    snap.tabs.iter().all(|t| ids.insert(t.id))
}

/// Everything in `data` (a Backup, read into memory) the app could not read once it is the
/// User database, one line each: "the Layout “Broken”", "the setting `search.saved` (Saved
/// searches)". Empty when it can all be read.
pub(crate) fn unreadable_records(data: &dyn UserData) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for name in data.layouts()? {
        let readable = data
            .layout(&name)?
            .and_then(|json| WorkspaceSnapshot::from_json(&json))
            .is_some_and(|snap| holds_each_tab_once(&snap));
        if !readable {
            out.push(format!("the Layout “{name}”"));
        }
    }
    for f in data.favorites()? {
        if TabLocation::from_json(&f.location).is_none() {
            out.push(format!("the Favorite “{}”", f.label));
        }
    }
    let visits = data
        .history_as_of(0)?
        .into_iter()
        .filter(|v| TabLocation::from_json(&v.location).is_none())
        .count();
    match visits {
        0 => {}
        1 => out.push("1 History visit".to_string()),
        n => out.push(format!("{n} History visits")),
    }
    for plan in data.reading_plans()? {
        if ReadingPlan::start_of(&plan).is_none() {
            out.push(format!("the Reading plan “{}”", plan.name));
            continue;
        }
        for r in data.plan_readings(plan.id)? {
            if Reading::from_row(&r).is_none() {
                out.push(format!(
                    "Reading {} of the Reading plan “{}”",
                    r.position + 1,
                    plan.name
                ));
            }
        }
    }
    for (key, value) in data.preferences()? {
        if let Some(s) = JSON_SETTINGS.iter().find(|s| s.names(&key))
            && !(s.reads)(&key, &value)
        {
            out.push(format!("the setting `{key}` ({})", s.what));
        }
    }
    Ok(out)
}
