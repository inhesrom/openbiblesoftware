//! Sync in the Session (plan S29): without a Sync folder nothing syncs or shows; two Sessions
//! on their own User databases converge through one folder and list each other; an edit makes
//! a sync due 30 seconds on; a restore on a syncing device syncs first, keeps this device's
//! Sync folder (whatever the Backup held) and waits for Catch up or Replace, both of which
//! work; Stop syncing gives a new id and records nothing more; a Sync conflict's copy is in
//! Needs attention with its four actions, each one undo step that the other device takes
//! too; held ops are listed with why; a folder that is not there and one a newer app set up
//! say so.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_store::annotation::new_id;
use obs_store::{Annotation, Change, Content, Row};
use serde_json::json;

use crate::notes::NotesFilter;
use crate::sync::texts;
use crate::{ConflictChoice, JobMode, Session};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-sync-session-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("folder")).unwrap();
        Self(root)
    }

    fn folder(&self) -> PathBuf {
        self.0.join("folder")
    }

    /// A Session on its own User database `name`.
    fn session(&self, name: &str) -> Session {
        let mut s = Session::new(JobMode::Inline);
        let (store, _) = obs_store::UserStore::open(&self.0.join(name).join("user.db")).unwrap();
        s.attach_user(Box::new(store));
        s
    }

    /// A Session on its own User database `name`, syncing through the folder.
    fn syncing(&self, name: &str) -> Session {
        let mut s = self.session(name);
        s.run("sync.choose_folder", &json!({ "path": self.folder().to_string_lossy() }))
            .unwrap();
        s
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn now() -> i64 {
    obs_store::clock::now_ms()
}

/// Sync now, as Sync now does: "Syncing…" for a frame, then the sync.
fn sync(s: &mut Session) {
    s.run("sync.now", &json!({})).unwrap();
    assert_eq!(s.sync_tick(now()), Some(0), "a requested sync is due at once");
    assert!(s.sync_panel(now()).syncing);
    assert_eq!(s.sync_panel(now()).status, texts::SYNCING);
    s.sync_tick(now());
    assert!(!s.sync_panel(now()).syncing);
}

fn store(s: &Session) -> &obs_store::UserStore {
    s.user.as_ref().unwrap().user_store().unwrap()
}

fn notebook_names(s: &Session) -> Vec<String> {
    let mut names: Vec<String> = s.notebooks().unwrap().into_iter().map(|n| n.name).collect();
    names.sort();
    names
}

/// A Note with no anchor in the default Notebook.
fn add_note(s: &mut Session, body: &str) -> Annotation {
    let note = Annotation {
        id: new_id(),
        notebook_id: s.default_notebook().unwrap().id,
        content: Content::Note { body: body.into() },
        anchor: None,
        created_at: 1,
        modified_at: 1,
        conflict_of: None,
    };
    s.commit("New Note", vec![Change::insert(Row::Annotation(note.clone()))])
        .unwrap();
    note
}

fn back_up(s: &mut Session, path: &Path) {
    s.run("data.back_up", &json!({ "path": path.to_string_lossy() }))
        .unwrap();
}

fn restore(s: &mut Session, path: &Path) {
    s.run("data.restore", &json!({ "path": path.to_string_lossy() }))
        .unwrap();
}

#[test]
fn without_a_sync_folder_nothing_syncs_and_nothing_shows() {
    let dir = Dir::new("none");
    let mut s = dir.session("a");
    s.create_notebook("Study").unwrap();
    assert_eq!(s.sync_tick(now()), None);
    assert_eq!(s.sync_indicator(now()), None);
    assert!(!s.sync_restore_pending());
    let panel = s.sync_panel(now());
    assert_eq!(panel.folder, None);
    assert!(panel.devices.is_empty());
    assert!(!s.is_enabled("sync.now") && !s.is_enabled("sync.stop"));
    assert!(
        !(s.key_bindings().iter()).any(|b| b.id.starts_with("sync.")),
        "Help → Keyboard shortcuts lists nothing of Sync"
    );
    assert!(!store(&s).records_ops().unwrap(), "nothing is recorded for a Sync folder");
    assert!(std::fs::read_dir(dir.folder()).unwrap().next().is_none());
    // A restore leaves it so.
    let backup = dir.0.join("b.obsbackup");
    back_up(&mut s, &backup);
    restore(&mut s, &backup);
    assert_eq!(s.sync_folder(), None);
    assert!(!s.sync_restore_pending());
}

#[test]
fn two_sessions_converge_through_one_folder_and_list_each_other() {
    let dir = Dir::new("converge");
    let mut a = dir.syncing("a");
    let mut b = dir.syncing("b");
    a.run("sync.device_name", &json!({ "name": " Study Mac " }))
        .unwrap();
    a.create_notebook("From A").unwrap();
    sync(&mut a);
    sync(&mut b);
    assert!(notebook_names(&b).contains(&"From A".to_string()));
    b.create_notebook("From B").unwrap();
    sync(&mut b);
    sync(&mut a);
    assert_eq!(notebook_names(&a), notebook_names(&b));
    assert!(notebook_names(&a).contains(&"From B".to_string()));

    let panel = a.sync_panel(now());
    assert_eq!(panel.folder.as_deref(), Some(&*dir.folder().to_string_lossy()));
    assert_eq!(panel.device_name, "Study Mac");
    assert_eq!(panel.status, "Last synced just now");
    assert!(!panel.warn);
    let platform = texts::platform_name(crate::sync_session::PLATFORM);
    let lines: Vec<&str> = panel.devices.iter().map(|d| d.line.as_str()).collect();
    assert_eq!(
        lines,
        [
            format!("Study Mac · {platform} · last seen just now (this device)"),
            format!("Unnamed device · {platform} · last seen just now"),
        ]
    );
    assert!(panel.devices[0].this_device && !panel.devices[1].this_device);
    let keys: Vec<&str> = (a.key_bindings().iter())
        .map(|b| b.id)
        .filter(|id| id.starts_with("sync."))
        .collect();
    assert_eq!(keys, ["sync.now"], "Sync now can have a key once this device syncs");
    let indicator = a.sync_indicator(now()).unwrap();
    assert_eq!(indicator.text, "Last synced just now");
    assert!(!indicator.warn);
    // Seen from B five minutes on.
    let later = b.sync_panel(now() + 5 * 60_000);
    assert_eq!(later.status, "Last synced 5 minutes ago");
    assert_eq!(
        later.devices[1].line,
        format!("Study Mac · {platform} · last seen 5 minutes ago")
    );
}

#[test]
fn an_edit_makes_a_sync_due_thirty_seconds_on_and_the_period_is_five_minutes() {
    let dir = Dir::new("schedule");
    let mut a = dir.syncing("a");
    sync(&mut a);
    let t = now();
    let idle = a.sync_tick(t).unwrap();
    assert!(idle > 4 * 60_000, "the next is the period's: {idle}");
    a.create_notebook("Edited").unwrap();
    let due = a.sync_tick(t).unwrap();
    assert!(due <= 30_000 && due > 20_000, "{due}");
    assert_eq!(a.sync_tick(t + 30_000), Some(0), "due 30 seconds after the edit");
    a.sync_tick(t + 30_000);
    assert!(!store(&a).has_pending_ops().unwrap(), "the edit went out");
}

#[test]
fn a_restore_on_a_syncing_device_syncs_first_keeps_its_folder_and_catching_up_works() {
    let dir = Dir::new("catch-up");
    // A Backup from before this device synced: it holds no Sync folder.
    let mut a = dir.session("a");
    a.create_notebook("Before sync").unwrap();
    let backup = dir.0.join("before.obsbackup");
    back_up(&mut a, &backup);
    a.run("sync.choose_folder", &json!({ "path": dir.folder().to_string_lossy() }))
        .unwrap();
    sync(&mut a);
    // Not synced yet when the restore comes: the restore syncs it first.
    a.create_notebook("Written just before").unwrap();
    assert!(store(&a).has_pending_ops().unwrap());
    restore(&mut a, &backup);
    assert_eq!(a.sync_folder(), Some(dir.folder().as_path()), "the folder is kept");
    assert!(a.sync_restore_pending());
    assert!(a.is_enabled("sync.restore_choice"));
    assert_eq!(a.sync_panel(now()).status, texts::RESTORE_TITLE);
    assert_eq!(a.sync_tick(now()), None, "nothing syncs until the user answers");
    assert!(!notebook_names(&a).contains(&"Written just before".to_string()));
    let mut b = dir.syncing("b");
    sync(&mut b);
    assert!(
        notebook_names(&b).contains(&"Written just before".to_string()),
        "the edit made before the restore reached the folder"
    );

    a.run("sync.restore_choice", &json!({ "choice": "catch_up" }))
        .unwrap();
    assert!(!a.sync_restore_pending());
    assert!(
        notebook_names(&a).contains(&"Written just before".to_string()),
        "catching up brought it back"
    );
    assert_eq!(a.sync_panel(now()).status, "Last synced just now");
    // A restart asks nothing more.
    drop(a);
    let a = dir.session("a");
    assert!(!a.sync_restore_pending());
    assert_eq!(a.sync_folder(), Some(dir.folder().as_path()));
}

#[test]
fn replace_makes_the_restored_data_what_every_device_has() {
    let dir = Dir::new("replace");
    let mut a = dir.syncing("a");
    let mut b = dir.syncing("b");
    a.create_notebook("Kept").unwrap();
    sync(&mut a);
    sync(&mut b);
    let backup = dir.0.join("a.obsbackup");
    back_up(&mut a, &backup);
    let kept = (a.notebooks().unwrap().into_iter())
        .find(|n| n.name == "Kept")
        .unwrap();
    a.rename_notebook(&kept.id, "Renamed").unwrap();
    sync(&mut a);
    sync(&mut b);
    assert!(notebook_names(&b).contains(&"Renamed".to_string()));
    // Restored the Backup from a device that stopped syncing since: its folder is not taken.
    restore(&mut a, &backup);
    assert!(a.sync_restore_pending());
    a.run("sync.restore_choice", &json!({ "choice": "replace" }))
        .unwrap();
    assert!(!a.sync_restore_pending());
    assert!(notebook_names(&a).contains(&"Kept".to_string()));
    sync(&mut b);
    assert!(notebook_names(&b).contains(&"Kept".to_string()));
    assert!(!notebook_names(&b).contains(&"Renamed".to_string()));
    assert_eq!(notebook_names(&a), notebook_names(&b));
}

#[test]
fn a_restore_never_takes_the_backups_sync_folder() {
    let dir = Dir::new("folder-kept");
    let mut a = dir.syncing("a");
    sync(&mut a);
    let backup = dir.0.join("a.obsbackup");
    back_up(&mut a, &backup);
    a.run("sync.stop", &json!({ "confirm": true })).unwrap();
    restore(&mut a, &backup);
    assert_eq!(a.sync_folder(), None, "this device stopped syncing; the Backup does not restart it");
    assert!(!a.sync_restore_pending());
    assert_eq!(a.sync_tick(now()), None);
}

#[test]
fn stop_syncing_records_nothing_more_and_choosing_again_joins_as_a_new_device() {
    let dir = Dir::new("stop");
    let mut a = dir.syncing("a");
    sync(&mut a);
    let old = store(&a).device_id().unwrap();
    a.create_notebook("Waiting").unwrap();
    assert!(store(&a).has_pending_ops().unwrap());
    a.run("sync.stop", &json!({ "confirm": true })).unwrap();
    assert_eq!(a.sync_folder(), None);
    assert_eq!(a.sync_indicator(now()), None);
    assert_ne!(store(&a).device_id().unwrap(), old);
    assert!(!store(&a).records_ops().unwrap());
    assert!(!store(&a).has_pending_ops().unwrap());
    a.create_notebook("Unsynced").unwrap();
    assert!(!store(&a).has_pending_ops().unwrap(), "nothing is recorded");
    // Its files stay; choosing the folder again joins as a new device and sends everything.
    a.run("sync.choose_folder", &json!({ "path": dir.folder().to_string_lossy() }))
        .unwrap();
    sync(&mut a);
    assert_eq!(a.sync_panel(now()).devices.len(), 2);
    let mut b = dir.syncing("b");
    sync(&mut b);
    for name in ["Waiting", "Unsynced"] {
        assert!(notebook_names(&b).contains(&name.to_string()), "{name}");
    }
}

/// Two Sessions with a Sync conflict: the Note `n` edited on both before either synced.
fn conflict(dir: &Dir) -> (Session, Session, Annotation) {
    let mut a = dir.syncing("a");
    let mut b = dir.syncing("b");
    let n = add_note(&mut a, "first");
    sync(&mut a);
    sync(&mut b);
    b.edit_note(&n.id, "by B").unwrap();
    a.edit_note(&n.id, "by A").unwrap();
    sync(&mut a);
    sync(&mut b);
    sync(&mut a);
    (a, b, n)
}

fn copy_of(s: &Session) -> Annotation {
    let copies = s.sync_conflicts();
    assert_eq!(copies.len(), 1, "{copies:?}");
    copies[0].clone()
}

#[test]
fn a_sync_conflicts_copy_is_in_needs_attention_with_its_four_actions() {
    let dir = Dir::new("conflict");
    let (mut a, mut b, n) = conflict(&dir);
    // The device whose text lost holds the copy.
    let (holder, other) = if a.sync_conflicts().is_empty() {
        (&mut b, &mut a)
    } else {
        (&mut a, &mut b)
    };
    let copy = copy_of(holder);
    assert_eq!(copy.conflict_of.as_deref(), Some(n.id.as_str()));
    let in_place = holder.annotation(&n.id).unwrap();
    assert_ne!(in_place.body(), copy.body());
    holder.notes.filter = NotesFilter::NeedsAttention;
    let cards = holder.note_cards("").unwrap();
    assert_eq!(cards.len(), 1);
    assert!(cards[0].is_sync_conflict() && cards[0].needs_attention());
    assert_eq!(
        cards[0].attention.as_deref(),
        Some(
            "Sync conflict: this text and the Note's at No anchor were changed on two devices. \
             Keep one, combine them or keep both."
        )
    );
    assert_eq!(holder.needs_attention_count(), 1);

    let body = |s: &Session, id: &str| s.annotation(id).unwrap().body().to_string();
    let run = |s: &mut Session, keep: ConflictChoice| {
        s.run(
            "sync.resolve_conflict",
            &json!({ "id": copy.id, "keep": keep.id() }),
        )
        .unwrap();
    };
    run(holder, ConflictChoice::KeepThis);
    assert_eq!(body(holder, &n.id), copy.body());
    assert!(holder.annotation(&copy.id).is_err());
    assert_eq!(holder.undo_history().undo_label(), Some(texts::KEEP_THIS));
    holder.undo().unwrap();
    assert_eq!(body(holder, &n.id), in_place.body());

    run(holder, ConflictChoice::KeepOther);
    assert!(holder.annotation(&copy.id).is_err());
    assert_eq!(body(holder, &n.id), in_place.body());
    holder.undo().unwrap();

    run(holder, ConflictChoice::KeepBoth);
    assert_eq!(holder.annotation(&copy.id).unwrap().conflict_of, None);
    assert!(holder.note_cards("").unwrap().is_empty(), "nothing needs attention");
    holder.undo().unwrap();
    assert_eq!(holder.note_cards("").unwrap().len(), 1);

    run(holder, ConflictChoice::Combine);
    assert_eq!(
        body(holder, &n.id),
        format!("{}\n\n{}", in_place.body(), copy.body())
    );
    assert!(holder.annotation(&copy.id).is_err());
    assert_eq!(holder.notes.reveal, Some((n.id.clone(), true)), "opened to edit");
    // The other device takes the resolution like any edit.
    sync(holder);
    sync(other);
    assert!(other.sync_conflicts().is_empty());
    assert_eq!(body(other, &n.id), body(holder, &n.id));
}

#[test]
fn held_ops_are_listed_in_needs_attention_with_why() {
    let dir = Dir::new("held");
    let mut a = dir.syncing("a");
    sync(&mut a);
    let later = new_id();
    let op = crate::sync::ops::Op {
        device: later.clone(),
        clock: obs_store::Hlc {
            wall_ms: u64::try_from(now()).unwrap(),
            counter: 0,
            device_id: later.clone(),
        },
        before: None,
        after: Some(Row::Preference(obs_store::SyncedPreference {
            key: "study.later_app_setting".into(),
            value: "1".into(),
        })),
        before_clock: None,
        absent: Vec::new(),
    };
    let path = dir.folder().join(crate::sync::ops::op_file_path(&later, 1));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, crate::sync::ops::encode_op_file(&[op]).unwrap()).unwrap();
    store(&a)
        .execute_unchecked(&format!(
            "INSERT INTO applied_up_to (device_id, seq) VALUES ('{later}', 0)"
        ))
        .unwrap();
    sync(&mut a);
    let lines = a.sync_held_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with(
            "A change from Unnamed device to the setting `study.later_app_setting` is waiting: "
        ),
        "{}",
        lines[0]
    );
    assert!(lines[0].ends_with(". It is tried again at every sync."), "{}", lines[0]);
    assert_eq!(a.needs_attention_count(), 1);
}

#[test]
fn a_folder_not_there_or_set_up_by_a_newer_app_says_so() {
    let dir = Dir::new("problems");
    let mut a = dir.syncing("a");
    std::fs::create_dir_all(dir.folder().join(crate::sync::SYNC_ROOT)).unwrap();
    std::fs::write(dir.folder().join(crate::sync::SYNC_JSON), b"{\"format\":99}\n").unwrap();
    sync(&mut a);
    let panel = a.sync_panel(now());
    assert_eq!(panel.folder_newer, Some(texts::FOLDER_NEWER));
    assert_eq!(panel.status, texts::FOLDER_NEWER);
    assert!(panel.warn && a.sync_indicator(now()).unwrap().warn);

    let gone = dir.0.join("gone");
    std::fs::create_dir_all(&gone).unwrap();
    a.run("sync.choose_folder", &json!({ "path": gone.to_string_lossy() }))
        .unwrap();
    std::fs::remove_dir_all(&gone).unwrap();
    sync(&mut a);
    let panel = a.sync_panel(now());
    assert_eq!(panel.status, texts::UNREACHABLE);
    assert!(panel.warn);
    assert!(panel.details.unwrap().contains("is not there"));
    assert!(!gone.exists(), "nothing is made where the folder was");
    let t = now();
    assert!(a.sync_tick(t).unwrap() > 0, "tried again later, not every frame");
}
