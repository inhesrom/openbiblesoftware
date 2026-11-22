//! Every word Settings → Sync, the status indicator, the restore prompt and Needs attention
//! show of Sync (plan S29; the owner approved them through the supervisor). The desktop shell
//! and the Mobile app (S30) lay these out and add none of their own, so both say the same.
//! Compiles everywhere.

use obs_store::annotation::{Change, Row};

/// The Settings section, and the setting the search finds.
pub const SECTION: &str = "Sync";

/// What Sync is, before a Sync folder is chosen.
pub const ONBOARDING: &str = "Keep your Notes, Highlights, Reading plans and other study data in \
     step on your computers, iPhone and iPad, through a folder in your own cloud storage. Each \
     device adds only its own files there; nothing is sent anywhere else.";

/// Which folders work on `platform` (`macos`, `windows`, `linux`, …), and to keep the folder
/// downloaded.
pub fn folder_guidance(platform: &str) -> &'static str {
    match platform {
        "macos" => {
            "To sync with an iPhone or iPad, choose a folder in iCloud Drive. Between computers, \
             any folder Dropbox, OneDrive, Google Drive (mirrored) or Syncthing keeps on this Mac \
             works. Keep the folder downloaded."
        }
        "windows" => {
            "Between computers, choose a folder Dropbox, OneDrive, Google Drive (mirrored) or \
             Syncthing keeps on this computer. iCloud for Windows can reach an iPhone or iPad, \
             but is not tested. Keep the folder available offline."
        }
        _ => {
            "Between computers, choose a folder Dropbox, Google Drive (mirrored) or Syncthing \
             keeps on this computer. With an iPhone or iPad, Syncthing on both may work, but is \
             not tested."
        }
    }
}

pub const CHOOSE_FOLDER: &str = "Choose Sync folder…";
pub const FOLDER_LABEL: &str = "Sync folder";
pub const DEVICE_NAME_LABEL: &str = "This device's name";
pub const DEVICE_NAME_HINT: &str = "Your other devices show this name.";
pub const DEVICE_NAME_PLACEHOLDER: &str = "Name this device";
pub const SYNC_NOW: &str = "Sync now";
pub const SYNCING: &str = "Syncing…";
pub const NOT_SYNCED_YET: &str = "Not synced yet";
pub const UNREACHABLE: &str =
    "Could not reach the Sync folder. It is tried again in a few minutes.";
pub const DEVICES_LABEL: &str = "Synced devices";
pub const UNNAMED_DEVICE: &str = "Unnamed device";
pub const THIS_DEVICE: &str = "this device";
pub const NEEDS_NEWER_APP: &str =
    "Uses a newer version of Open Bible: update this app to sync with it.";
pub const STOP_SYNCING: &str = "Stop syncing";
pub const STOP_CONFIRM: &str =
    "Stop syncing? This computer keeps its data; the Sync folder keeps its files.";
pub const FOLDER_NEWER: &str = "A newer version of Open Bible set up this Sync folder. Update the \
     app to sync through it.";

/// The prompt after a Backup is restored on a syncing device (ADR 0022 rule 7).
pub const RESTORE_TITLE: &str = "Restored from a Backup";
pub const RESTORE_BODY: &str = "This computer syncs with your other devices. Catch up keeps the \
     restored data and adds what changed on your other devices since the Backup. Replace makes \
     the restored data what every device has. Changes made on other devices since the Backup are \
     replaced too, but a Note's text that would be lost is kept as a Sync conflict.";
pub const CATCH_UP: &str = "Catch up";
pub const REPLACE: &str = "Replace";

/// A Sync conflict's actions in Needs attention (each one undo step).
pub const KEEP_THIS: &str = "Keep this version";
pub const KEEP_OTHER: &str = "Keep the other";
pub const KEEP_BOTH: &str = "Keep both";
pub const COMBINE: &str = "Combine";
/// A Sync conflict's actions when another device deleted the Note in place.
pub const KEEP_THIS_TEXT: &str = "Keep this text";
pub const LET_IT_GO: &str = "Let it go";
/// What a Sync conflict's copy says when another device deleted the Note in place.
pub const CONFLICT_IN_PLACE_GONE: &str = "Sync conflict: this text was changed here while the \
     Note was deleted on another device. Keep this text or let it go.";

/// Needs attention's hover text: with a Sync folder chosen it lists Sync conflicts too.
pub fn needs_attention_hover(syncing: bool) -> &'static str {
    if syncing {
        "Annotations whose text was not found after their Resource changed, and Sync conflicts"
    } else {
        "Annotations whose text was not found after their Resource changed"
    }
}

/// The heading of the held ops in Needs attention.
pub const CHANGES_WAITING: &str = "Changes waiting";

/// "Last synced 2 minutes ago".
pub fn last_synced(ago: &str) -> String {
    format!("Last synced {ago}")
}

/// What a Sync conflict's copy says: the Note in place is at `reference`.
pub fn conflict_attention(reference: &str) -> String {
    format!(
        "Sync conflict: this text and the Note's at {reference} were changed on two devices. \
         Keep one, combine them or keep both."
    )
}

/// One held op in Needs attention: `device` sent it, `what` it changes, `why` it waits.
pub fn held_line(device: &str, what: &str, why: &str) -> String {
    format!("A change from {device} to {what} is waiting: {why}. It is tried again at every sync.")
}

/// What a held op changes, for [`held_line`]: "the Collection “Gospels”", "a Note".
pub fn held_thing(change: &Change) -> String {
    let Some(row) = change.after.as_ref().or(change.before.as_ref()) else {
        return "a row".to_string();
    };
    let named = |kind: &str, name: &str| format!("the {kind} “{name}”");
    match row {
        Row::Notebook(n) => named("Notebook", &n.name),
        Row::Annotation(a) if a.is_note() => "a Note".to_string(),
        Row::Annotation(_) => "a Highlight".to_string(),
        Row::HighlightStyle(s) => named("Highlight style", &s.name),
        Row::HighlightPalette(p) => named("Highlight palette", &p.name),
        Row::Favorite(_) => "a Favorite".to_string(),
        Row::ReadingPlan(p) => named("Reading plan", &p.name),
        Row::Reading(_) => "a Reading".to_string(),
        Row::Layout(l) => named("Layout", &l.name),
        Row::PreferredOrder(_) => "the Preferred order".to_string(),
        Row::Preference(p) => format!("the setting `{}`", p.key),
        Row::Collection(i) => named("Collection", &i.name),
        Row::CopyPreset(i) => named("Copy preset", &i.name),
        Row::SavedSearch(i) => named("Saved search", &i.name),
        Row::GuideSection(_) => "a Guide arrangement".to_string(),
        Row::ResourceTags(_) => "Resource tags".to_string(),
    }
}

/// The name of a Synced device's platform, as its `device.json` gives it.
pub fn platform_name(platform: &str) -> String {
    match platform {
        "macos" => "macOS".to_string(),
        "windows" => "Windows".to_string(),
        "linux" => "Linux".to_string(),
        "ios" => "iOS".to_string(),
        other => other.to_string(),
    }
}

/// One row of Synced devices: "Ian's iPhone · iOS · last seen 5 minutes ago".
pub fn device_line(name: &str, platform: &str, seen: Option<&str>) -> String {
    let name = if name.trim().is_empty() {
        UNNAMED_DEVICE
    } else {
        name.trim()
    };
    let mut line = name.to_string();
    if !platform.is_empty() {
        line.push_str(" · ");
        line.push_str(&platform_name(platform));
    }
    if let Some(seen) = seen {
        line.push_str(" · last seen ");
        line.push_str(seen);
    }
    line
}

/// How long before `now_ms` `then_ms` was: "just now", "2 minutes ago", "3 hours ago",
/// "4 days ago".
pub fn ago(now_ms: i64, then_ms: i64) -> String {
    let secs = now_ms.saturating_sub(then_ms).max(0) / 1000;
    let n = |count: i64, unit: &str| {
        if count == 1 {
            format!("1 {unit} ago")
        } else {
            format!("{count} {unit}s ago")
        }
    };
    match secs {
        0..60 => "just now".to_string(),
        60..3600 => n(secs / 60, "minute"),
        3600..86_400 => n(secs / 3600, "hour"),
        _ => n(secs / 86_400, "day"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_ago_read_as_words() {
        let now = 10_000_000;
        assert_eq!(ago(now, now - 5_000), "just now");
        assert_eq!(ago(now, now - 60_000), "1 minute ago");
        assert_eq!(ago(now, now - 125_000), "2 minutes ago");
        assert_eq!(ago(now, now - 3 * 3_600_000), "3 hours ago");
        assert_eq!(ago(now, now - 4 * 86_400_000), "4 days ago");
        assert_eq!(
            ago(now, now + 9_000),
            "just now",
            "a clock ahead is not in the future"
        );
    }

    #[test]
    fn a_device_row_names_it_its_platform_and_when_it_was_seen() {
        assert_eq!(
            device_line("Ian's iPhone", "ios", Some("5 minutes ago")),
            "Ian's iPhone · iOS · last seen 5 minutes ago"
        );
        assert_eq!(device_line(" ", "macos", None), "Unnamed device · macOS");
        assert_eq!(last_synced("2 minutes ago"), "Last synced 2 minutes ago");
    }
}
