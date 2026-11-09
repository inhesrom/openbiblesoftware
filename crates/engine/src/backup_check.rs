//! What a Backup must hold readable before a restore may replace the User database (#258).
//!
//! The store checks the database itself (`obs_store`'s `backup_store.rs`: every table,
//! column and row readable, no row pointing at one that is not there). What the engine keeps
//! *inside* rows is its own: a Layout's Workspace snapshot, a Favorite's or a Visit's
//! location, and the settings it keeps as JSON ([`JSON_SETTINGS`]). The app reads each of
//! those leniently at launch: an unreadable one is left out or back at its default. So a
//! restore checks them first, and a Backup holding one the app could not read is refused,
//! naming it, rather than silently losing it after the live data is gone.
//!
//! Plain-text settings (a Log level, the last Notebook used) are not checked: any text is
//! one, and a value the app does not know is its default.

use serde::de::DeserializeOwned;

use crate::favorites::TabLocation;
use crate::persist::{UserData, WorkspaceSnapshot};

/// A setting the app keeps as JSON in the User database.
pub(crate) struct JsonSetting {
    /// Its preference key; one ending in `/` or `.` is the start of one key per Guide,
    /// search mode or Resource.
    pub key: &'static str,
    /// What the user calls it.
    pub what: &'static str,
    /// Whether the app can read a value of it.
    pub reads: fn(&str) -> bool,
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
fn reads<T: DeserializeOwned>(json: &str) -> bool {
    serde_json::from_str::<T>(json).is_ok()
}

/// Every setting the app keeps as JSON, with the type its loader reads it as.
pub(crate) const JSON_SETTINGS: &[JsonSetting] = &[
    JsonSetting {
        key: crate::session::VIEW_PREF,
        what: "View settings (Text size, Theme, fonts)",
        reads: reads::<serde_json::Value>,
    },
    JsonSetting {
        key: crate::toolbar_settings::TOOLBAR_PREF,
        what: "Toolbar",
        reads: reads::<serde_json::Value>,
    },
    JsonSetting {
        key: crate::key_bindings::KEY_BINDINGS_PREF,
        what: "Keyboard shortcuts",
        reads: reads::<serde_json::Value>,
    },
    JsonSetting {
        key: crate::home::HOME_PREF,
        what: "Home",
        reads: reads::<crate::home::Stored>,
    },
    JsonSetting {
        key: crate::guide::ARRANGEMENT_PREF_PREFIX,
        what: "Guide arrangement",
        reads: reads::<crate::guide::Stored>,
    },
    JsonSetting {
        key: crate::saved_searches::SAVED_PREF,
        what: "Saved searches",
        reads: reads::<Vec<crate::saved_searches::NamedSearch>>,
    },
    JsonSetting {
        key: crate::saved_searches::HISTORY_PREF,
        what: "Search history",
        reads: reads::<Vec<crate::saved_searches::SearchHistoryEntry>>,
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
        reads: reads::<Option<crate::import::ImportRecord>>,
    },
    JsonSetting {
        key: crate::notes_export::ATTRIBUTION_PREF_PREFIX,
        what: "kept Attribution",
        reads: reads::<crate::notes_export::KeptAttribution>,
    },
];

/// Everything in `data` (a Backup, read into memory) the app could not read once it is the
/// User database, one line each: "the Layout “Broken”", "the setting `search.saved` (Saved
/// searches)". Empty when it can all be read.
pub(crate) fn unreadable_records(data: &dyn UserData) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for name in data.layouts()? {
        let readable = data
            .layout(&name)?
            .is_some_and(|json| WorkspaceSnapshot::from_json(&json).is_some());
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
    for (key, value) in data.preferences()? {
        if let Some(s) = JSON_SETTINGS.iter().find(|s| s.names(&key))
            && !(s.reads)(&value)
        {
            out.push(format!("the setting `{key}` ({})", s.what));
        }
    }
    Ok(out)
}
