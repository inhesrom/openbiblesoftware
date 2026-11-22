//! Sync in the Session (plan S29): without a Sync folder nothing syncs or shows; two Sessions
//! on their own User databases converge through one folder and list each other; an edit makes
//! a sync due 30 seconds on; a restore on a syncing device syncs first, keeps this device's
//! Sync folder (whatever the Backup held) and waits for Catch up or Replace, both of which
//! work; Stop syncing gives a new id and records nothing more; a Sync conflict's copy is in
//! Needs attention with its four actions, each one undo step that the other device takes
//! too; held ops are listed with why; a folder that is not there and one a newer app set up
//! say so. A Note editor's save after a merge changed its Note keeps both texts, the typed one
//! as a Sync conflict's copy; a copy whose Note was deleted on another device offers Keep this
//! text and Let it go. Off the UI thread (S29b): no call of the Sync folder is on it; edits
//! made while a sync runs are kept and go out with the next; choosing a folder and Stop
//! syncing wait for the sync running, "Syncing…" meanwhile; a worker that panics is a failed
//! sync; a restore whose sync first does not finish goes ahead, says so, and that sync writes
//! nothing after. From S29b's review: Stop syncing or Choose folder waiting for a sync when a
//! restore starts applies (before the restore when the sync ends in its wait, after it when
//! the restore abandons it); Stop syncing abandons a sync that does not end in its wait, and
//! stops; an edit made while a snapshot is due skips the snapshot, not the sync; a worker
//! syncs on a fixed clock the Session's thread runs on (the gallery's).

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
        let root =
            std::env::temp_dir().join(format!("obs-sync-session-{name}-{}", std::process::id()));
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
        s.run(
            "sync.choose_folder",
            &json!({ "path": self.folder().to_string_lossy() }),
        )
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

/// Sync now, as Sync now does: the sync starts on its worker at the next frame, "Syncing…"
/// until a frame takes its result.
fn sync(s: &mut Session) {
    s.run("sync.now", &json!({})).unwrap();
    assert!(s.sync_tick(now()).is_some());
    assert!(
        s.sync_panel(now()).syncing,
        "a requested sync starts at once"
    );
    assert_eq!(s.sync_panel(now()).status, texts::SYNCING);
    settle(s);
}

/// The frames until the sync running (or a restore's choice) has ended and been taken.
fn settle(s: &mut Session) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while s.sync_panel(now()).syncing {
        assert!(std::time::Instant::now() < deadline, "the sync never ended");
        std::thread::sleep(std::time::Duration::from_millis(1));
        s.sync_tick(now());
    }
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
    s.commit(
        "New Note",
        vec![Change::insert(Row::Annotation(note.clone()))],
    )
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
    assert!(
        !store(&s).records_ops().unwrap(),
        "nothing is recorded for a Sync folder"
    );
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
    assert_eq!(
        panel.folder.as_deref(),
        Some(&*dir.folder().to_string_lossy())
    );
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
    assert_eq!(
        keys,
        ["sync.now"],
        "Sync now can have a key once this device syncs"
    );
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
    a.sync_tick(t + 30_000);
    assert!(a.sync_panel(now()).syncing, "due 30 seconds after the edit");
    settle(&mut a);
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
    a.run(
        "sync.choose_folder",
        &json!({ "path": dir.folder().to_string_lossy() }),
    )
    .unwrap();
    sync(&mut a);
    // Not synced yet when the restore comes: the restore syncs it first.
    a.create_notebook("Written just before").unwrap();
    assert!(store(&a).has_pending_ops().unwrap());
    restore(&mut a, &backup);
    assert_eq!(
        a.sync_folder(),
        Some(dir.folder().as_path()),
        "the folder is kept"
    );
    assert!(a.sync_restore_pending());
    assert!(a.is_enabled("sync.restore_choice"));
    assert_eq!(a.sync_panel(now()).status, texts::RESTORE_TITLE);
    assert_eq!(
        a.sync_tick(now()),
        None,
        "nothing syncs until the user answers"
    );
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
    assert_eq!(a.sync_panel(now()).status, texts::SYNCING);
    settle(&mut a);
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
fn the_restore_prompt_comes_back_after_a_restart_until_it_is_answered() {
    let dir = Dir::new("pending-restart");
    let mut a = dir.syncing("a");
    a.create_notebook("Backed up").unwrap();
    sync(&mut a);
    let backup = dir.0.join("a.obsbackup");
    back_up(&mut a, &backup);
    restore(&mut a, &backup);
    assert!(a.sync_restore_pending());
    // Quit without answering: the next launch asks again, and syncs nothing until then.
    drop(a);
    let mut a = dir.session("a");
    assert_eq!(a.sync_folder(), Some(dir.folder().as_path()));
    assert!(a.sync_restore_pending(), "asked again after a restart");
    assert_eq!(a.sync_panel(now()).status, texts::RESTORE_TITLE);
    assert_eq!(a.sync_tick(now()), None);
    a.run("sync.restore_choice", &json!({ "choice": "catch_up" }))
        .unwrap();
    settle(&mut a);
    drop(a);
    let a = dir.session("a");
    assert!(!a.sync_restore_pending(), "answered once, asked no more");
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
    settle(&mut a);
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
    assert_eq!(
        a.sync_folder(),
        None,
        "this device stopped syncing; the Backup does not restart it"
    );
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
    a.run(
        "sync.choose_folder",
        &json!({ "path": dir.folder().to_string_lossy() }),
    )
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
    assert_eq!(cards[0].conflict_actions, crate::conflict_actions(true));
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
    // The other device takes it: this version in place, no copy.
    sync(holder);
    sync(other);
    assert_eq!(body(other, &n.id), copy.body());
    assert!(other.sync_conflicts().is_empty());
    holder.undo().unwrap();
    assert_eq!(body(holder, &n.id), in_place.body());
    sync(holder);
    sync(other);
    assert_eq!(body(other, &n.id), in_place.body());

    run(holder, ConflictChoice::KeepOther);
    assert!(holder.annotation(&copy.id).is_err());
    assert_eq!(body(holder, &n.id), in_place.body());
    holder.undo().unwrap();

    run(holder, ConflictChoice::KeepBoth);
    assert_eq!(holder.annotation(&copy.id).unwrap().conflict_of, None);
    assert!(
        holder.note_cards("").unwrap().is_empty(),
        "nothing needs attention"
    );
    // The other device takes it: both texts, as two ordinary Notes.
    sync(holder);
    sync(other);
    assert_eq!(body(other, &n.id), in_place.body());
    assert_eq!(body(other, &copy.id), copy.body());
    assert!(other.sync_conflicts().is_empty());
    holder.undo().unwrap();
    assert_eq!(holder.note_cards("").unwrap().len(), 1);

    run(holder, ConflictChoice::Combine);
    assert_eq!(
        body(holder, &n.id),
        format!("{}\n\n{}", in_place.body(), copy.body())
    );
    assert!(holder.annotation(&copy.id).is_err());
    assert_eq!(
        holder.notes.reveal,
        Some((n.id.clone(), true)),
        "opened to edit"
    );
    // The other device takes the resolution like any edit.
    sync(holder);
    sync(other);
    assert!(other.sync_conflicts().is_empty());
    assert_eq!(body(other, &n.id), body(holder, &n.id));
}

/// Real time passes, so the next edit's clock is later than the last one's.
fn later() {
    std::thread::sleep(std::time::Duration::from_millis(5));
}

/// A Note editor's save, as the Notes Tool's card makes it: `body` typed over `base`.
fn save_typed(s: &mut Session, id: &str, body: &str, base: &str, continues: bool) -> Annotation {
    let out = s
        .run(
            "annotation.edit",
            &json!({ "id": id, "body": body, "base": base, "continue": continues }),
        )
        .unwrap();
    s.annotation(out["annotation"]["id"].as_str().unwrap())
        .unwrap()
}

#[test]
fn an_editors_save_after_a_merge_changed_its_note_keeps_both_texts() {
    let dir = Dir::new("edit-merged");
    let mut a = dir.syncing("a");
    let mut b = dir.syncing("b");
    let n = add_note(&mut a, "first");
    sync(&mut a);
    sync(&mut b);
    // A's editor opens on "first"; B's edit reaches A before A's editor saves.
    b.edit_note(&n.id, "by B").unwrap();
    sync(&mut b);
    sync(&mut a);
    assert_eq!(a.annotation(&n.id).unwrap().body(), "by B");
    let copy = save_typed(&mut a, &n.id, "first, typed on A", "first", false);
    assert_ne!(copy.id, n.id, "the typed text is not written over B's");
    assert_eq!(copy.conflict_of.as_deref(), Some(n.id.as_str()));
    assert_eq!(copy.body(), "first, typed on A");
    assert_eq!(
        (copy.anchor.clone(), copy.notebook_id.clone()),
        (n.anchor.clone(), n.notebook_id.clone())
    );
    assert_eq!(a.annotation(&n.id).unwrap().body(), "by B");
    // The editor goes on in the copy; its saves are one undo step.
    let more = save_typed(
        &mut a,
        &copy.id,
        "first, typed on A, and more",
        "first, typed on A",
        true,
    );
    assert_eq!(more.id, copy.id);
    assert_eq!(more.body(), "first, typed on A, and more");
    assert_eq!(a.sync_conflicts().len(), 1);
    a.notes.filter = NotesFilter::NeedsAttention;
    let cards = a.note_cards("").unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].conflict_actions, crate::conflict_actions(true));
    // B gets both texts.
    sync(&mut a);
    sync(&mut b);
    assert_eq!(b.annotation(&n.id).unwrap().body(), "by B");
    assert_eq!(
        b.annotation(&copy.id).unwrap().body(),
        "first, typed on A, and more"
    );
    a.undo().unwrap();
    assert!(
        a.annotation(&copy.id).is_err(),
        "one undo takes the pass back"
    );
    assert_eq!(a.annotation(&n.id).unwrap().body(), "by B");
}

#[test]
fn an_editors_save_goes_into_the_copy_a_merge_made_of_its_text() {
    let dir = Dir::new("edit-merged-copy");
    let mut a = dir.syncing("a");
    let mut b = dir.syncing("b");
    let n = add_note(&mut a, "first");
    sync(&mut a);
    sync(&mut b);
    // A's editor saved once; B edits later, before seeing it, and wins.
    save_typed(&mut a, &n.id, "saved on A", "first", false);
    later();
    b.edit_note(&n.id, "by B").unwrap();
    sync(&mut b);
    sync(&mut a);
    assert_eq!(a.annotation(&n.id).unwrap().body(), "by B");
    let merged = copy_of(&a);
    assert_eq!(merged.body(), "saved on A");
    let saved = save_typed(&mut a, &n.id, "saved on A, then more", "saved on A", true);
    assert_eq!(
        saved.id, merged.id,
        "the merge's copy takes the text: one copy"
    );
    assert_eq!(saved.body(), "saved on A, then more");
    assert_eq!(a.sync_conflicts().len(), 1);
    assert_eq!(a.annotation(&n.id).unwrap().body(), "by B");
    // Without a merge, the save is an ordinary edit.
    let m = add_note(&mut a, "plain");
    let edited = save_typed(&mut a, &m.id, "plain, edited", "plain", false);
    assert_eq!(edited.id, m.id);
    assert_eq!(edited.body(), "plain, edited");
    assert_eq!(a.sync_conflicts().len(), 1);
}

#[test]
fn a_sync_conflicts_copy_whose_note_was_deleted_elsewhere_keeps_this_text_or_lets_it_go() {
    let dir = Dir::new("conflict-gone");
    let mut a = dir.syncing("a");
    let mut b = dir.syncing("b");
    let mut c = dir.syncing("c");
    let n = add_note(&mut a, "first");
    sync(&mut a);
    sync(&mut b);
    sync(&mut c);
    // A changes the Note; B, not having seen that, changes it later; C, having B's text
    // only, deletes it.
    a.edit_note(&n.id, "kept on A").unwrap();
    later();
    b.edit_note(&n.id, "by B").unwrap();
    sync(&mut b);
    sync(&mut c);
    c.delete_annotation(&n.id).unwrap();
    sync(&mut c);
    sync(&mut a);
    assert!(a.annotation(&n.id).is_err(), "the delete won");
    let copy = copy_of(&a);
    assert_eq!(copy.body(), "kept on A");
    a.notes.filter = NotesFilter::NeedsAttention;
    let cards = a.note_cards("").unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(
        cards[0].attention.as_deref(),
        Some(texts::CONFLICT_IN_PLACE_GONE)
    );
    assert_eq!(
        cards[0].conflict_actions,
        [
            (ConflictChoice::KeepBoth, texts::KEEP_THIS_TEXT),
            (ConflictChoice::KeepOther, texts::LET_IT_GO),
        ]
    );
    // An editor open on the deleted Note saves into the copy.
    let saved = save_typed(&mut a, &n.id, "kept on A, typed on", "kept on A", false);
    assert_eq!(saved.id, copy.id);
    a.undo().unwrap();
    assert_eq!(a.annotation(&copy.id).unwrap().body(), "kept on A");

    let run = |s: &mut Session, keep: ConflictChoice| {
        s.run(
            "sync.resolve_conflict",
            &json!({ "id": copy.id, "keep": keep.id() }),
        )
        .unwrap();
    };
    // Keep this text: an ordinary Note.
    run(&mut a, ConflictChoice::KeepBoth);
    assert_eq!(a.undo_history().undo_label(), Some(texts::KEEP_THIS_TEXT));
    let kept = a.annotation(&copy.id).unwrap();
    assert_eq!(
        (kept.conflict_of.as_deref(), kept.body()),
        (None, "kept on A")
    );
    assert!(
        a.note_cards("").unwrap().is_empty(),
        "nothing needs attention"
    );
    a.undo().unwrap();
    // Let it go: the copy is deleted, and undo brings it back.
    run(&mut a, ConflictChoice::KeepOther);
    assert_eq!(a.undo_history().undo_label(), Some(texts::LET_IT_GO));
    assert!(a.annotation(&copy.id).is_err());
    a.undo().unwrap();
    assert_eq!(a.annotation(&copy.id).unwrap().body(), "kept on A");
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
    assert!(
        lines[0].ends_with(". It is tried again at every sync."),
        "{}",
        lines[0]
    );
    assert_eq!(a.needs_attention_count(), 1);
}

#[test]
fn a_folder_not_there_or_set_up_by_a_newer_app_says_so() {
    let dir = Dir::new("problems");
    let mut a = dir.syncing("a");
    std::fs::create_dir_all(dir.folder().join(crate::sync::SYNC_ROOT)).unwrap();
    std::fs::write(
        dir.folder().join(crate::sync::SYNC_JSON),
        b"{\"format\":99}\n",
    )
    .unwrap();
    sync(&mut a);
    let panel = a.sync_panel(now());
    assert_eq!(panel.folder_newer, Some(texts::FOLDER_NEWER));
    assert_eq!(panel.status, texts::FOLDER_NEWER);
    assert!(panel.warn && a.sync_indicator(now()).unwrap().warn);

    let gone = dir.0.join("gone");
    std::fs::create_dir_all(&gone).unwrap();
    a.run(
        "sync.choose_folder",
        &json!({ "path": gone.to_string_lossy() }),
    )
    .unwrap();
    std::fs::remove_dir_all(&gone).unwrap();
    sync(&mut a);
    let panel = a.sync_panel(now());
    assert_eq!(panel.status, texts::UNREACHABLE);
    assert!(panel.warn);
    assert!(panel.details.unwrap().contains("is not there"));
    assert!(!gone.exists(), "nothing is made where the folder was");
    let t = now();
    assert!(
        a.sync_tick(t).unwrap() > 0,
        "tried again later, not every frame"
    );
}

/// The op files of every device in `folder`.
fn op_files(folder: &Path) -> usize {
    let devices = folder.join(crate::sync::DEVICES_DIR);
    (std::fs::read_dir(devices).unwrap().filter_map(Result::ok))
        .filter_map(|d| std::fs::read_dir(d.path().join("ops")).ok())
        .map(|files| files.count())
        .sum()
}

/// What the frames did while a sync ran on its worker: each frame's sync work on the UI
/// thread (`sync_tick`: starting it, looking for its result, then taking it and reloading
/// the Synced data it merged; and the status the shell draws), the frame after the result's
/// (the views reading again what moved `annotations_revision`) and, when the frames edited,
/// how long each edit took.
struct Frames {
    ticks: Vec<std::time::Duration>,
    after: std::time::Duration,
    edits: Vec<std::time::Duration>,
}

impl Frames {
    fn report(&self, what: &str) -> String {
        let ms = |d: &std::time::Duration| d.as_secs_f64() * 1000.0;
        let mut ticks: Vec<f64> = self.ticks.iter().map(ms).collect();
        ticks.sort_by(f64::total_cmp);
        let buckets = [1.0, 2.0, 4.0, 8.0, 16.0, f64::INFINITY];
        let mut counts = [0usize; 6];
        for t in &ticks {
            counts[buckets.iter().position(|b| t < b).unwrap_or(5)] += 1;
        }
        let last = self.ticks.last().map_or(0.0, ms);
        let mut line = format!(
            "{what}: {} frames, sync work a frame max {:.2} ms (the result's frame {last:.2} ms, \
             the views' reads the frame after {:.2} ms), p50 {:.3} ms; histogram <1 {} | 1-2 {} \
             | 2-4 {} | 4-8 {} | 8-16 {} | >=16 {}",
            ticks.len(),
            ticks.last().copied().unwrap_or(0.0),
            ms(&self.after),
            ticks.get(ticks.len() / 2).copied().unwrap_or(0.0),
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            counts[4],
            counts[5],
        );
        if !self.edits.is_empty() {
            let mut edits: Vec<f64> = self.edits.iter().map(ms).collect();
            edits.sort_by(f64::total_cmp);
            line.push_str(&format!(
                "; an edit each frame ({}): max {:.2} ms, p50 {:.2} ms",
                edits.len(),
                edits.last().copied().unwrap_or(0.0),
                edits.get(edits.len() / 2).copied().unwrap_or(0.0),
            ));
        }
        line
    }

    fn max_tick(&self) -> std::time::Duration {
        self.ticks.iter().max().copied().unwrap_or_default()
    }
}

/// Sync now, as the app's frames run it (one each 16 ms) until the sync's result is taken,
/// timing each frame's sync work and then what the views read again at the frame after
/// (`MarkCache` and the Notes Tool's cards, with Needs attention's count, once the reload
/// moved `annotations_revision`), and with `edit` making a Notebook at every frame while it
/// runs.
fn frames_of_a_sync(s: &mut Session, edit: bool) -> Frames {
    let frame = std::time::Duration::from_millis(16);
    let mut frames = Frames {
        ticks: Vec::new(),
        after: std::time::Duration::ZERO,
        edits: Vec::new(),
    };
    s.run("sync.now", &json!({})).unwrap();
    loop {
        let start = std::time::Instant::now();
        s.sync_tick(now());
        // What the shell asks every frame besides.
        let syncing = s.sync_panel(now()).syncing;
        let _ = s.sync_indicator(now());
        frames.ticks.push(start.elapsed());
        if !syncing {
            let start = std::time::Instant::now();
            let _ = s.annotations_in_text().unwrap();
            let _ = s.notebooks().unwrap();
            let _ = s.highlight_styles().unwrap();
            let _ = s.note_cards("").unwrap();
            let _ = s.needs_attention_count();
            frames.after = start.elapsed();
            return frames;
        }
        if edit {
            let at = std::time::Instant::now();
            s.create_notebook(&format!("Edit {}", new_id())).unwrap();
            frames.edits.push(at.elapsed());
        }
        // Spun, not slept: a sleep in a test process can wake many milliseconds late (timer
        // coalescing), and the frames would be too few to see a stall.
        while start.elapsed() < frame {
            std::hint::spin_loop();
        }
    }
}

/// A device makes `notes` Notes over `files` op files (one sync each), then a new device
/// joins, editing at every frame: its frames, and how long its sync took (ms).
fn joined_by_a_new_device(notes: usize, files: usize) -> (Frames, i64) {
    let dir = Dir::new(&format!("measure-{notes}-{files}"));
    let mut l = dir.syncing("library");
    sync(&mut l);
    let notebook = l.default_notebook().unwrap().id;
    for file in 0..files {
        let changes: Vec<Change> = (file * notes / files..(file + 1) * notes / files)
            .map(|i| {
                Change::insert(Row::Annotation(Annotation {
                    id: new_id(),
                    notebook_id: notebook.clone(),
                    content: Content::Note {
                        body: format!("Note {i}: a sentence or two of study."),
                    },
                    anchor: None,
                    created_at: 1,
                    modified_at: 1,
                    conflict_of: None,
                }))
            })
            .collect();
        store(&l).apply_changes(&changes).unwrap();
        sync(&mut l);
    }
    let mut n = dir.syncing("new");
    let frames = frames_of_a_sync(&mut n, true);
    assert_eq!(
        (n.annotations(None).unwrap().into_iter())
            .filter(|a| a.is_note())
            .count(),
        notes
    );
    (frames, n.last_sync_duration_ms().unwrap())
}

/// How sync holds the UI thread now that it runs on its worker (plan S29b; S29 measured it on
/// the UI thread, 136–141 ms for the catch-up): frame by frame, a device catching up with
/// 1,000 op files another wrote while it was away, once with no edits and once with an edit
/// at every frame (how long a user's edit waits for the worker's merge transactions); a new
/// device joining that folder; and a new device joining a folder whose other device holds
/// 10,000 Notes (the merge's biggest transactions: a file of 10,000 ops, and a snapshot).
/// `cargo test -p obs-engine --lib measure_ -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn measure_syncs_off_the_ui_thread() {
    let dir = Dir::new("measure");
    let mut a = dir.syncing("a");
    let mut e = dir.syncing("e");
    let mut b = dir.syncing("b");
    sync(&mut a);
    sync(&mut e);
    sync(&mut b);
    sync(&mut a);
    sync(&mut e);
    let at = std::time::Instant::now();
    a.create_notebook("Before").unwrap();
    let edit_alone = at.elapsed();
    sync(&mut a);
    let one_edit = a.last_sync_duration_ms().unwrap();
    for i in 0..1000 {
        b.create_notebook(&format!("Notebook {i}")).unwrap();
        sync(&mut b);
    }
    let files = op_files(&dir.folder());
    let caught_up = frames_of_a_sync(&mut a, false);
    let caught_up_ms = a.last_sync_duration_ms().unwrap();
    assert!(notebook_names(&a).contains(&"Notebook 999".to_string()));
    let edited = frames_of_a_sync(&mut e, true);
    assert!(notebook_names(&e).contains(&"Notebook 999".to_string()));
    let nothing_new = frames_of_a_sync(&mut a, false);
    let mut c = dir.syncing("c");
    let joined = frames_of_a_sync(&mut c, true);
    let joined_ms = c.last_sync_duration_ms().unwrap();
    assert!(notebook_names(&c).contains(&"Notebook 999".to_string()));

    // A large library: 10,000 Notes on one device, then a new device joins, once with them
    // in one op file (one merge transaction of 10,000 ops) and once with them in that
    // device's snapshot (one transaction too).
    let in_one_file = joined_by_a_new_device(10_000, 1);
    let in_a_snapshot = joined_by_a_new_device(10_000, 101);

    eprintln!(
        "sync off the UI thread, local folder ({files} op files), dev profile:\n\
         - one edit's sync {one_edit} ms; an edit with no sync running {:.2} ms\n\
         - {} (the sync {caught_up_ms} ms)\n\
         - {}\n\
         - {}\n\
         - {} (the sync {joined_ms} ms)\n\
         - {} (the sync {} ms)\n\
         - {} (the sync {} ms)",
        edit_alone.as_secs_f64() * 1000.0,
        caught_up.report("catching up with 1,000 op files"),
        edited.report("catching up with 1,000 op files, editing"),
        nothing_new.report("nothing new"),
        joined.report("a new device joining, editing"),
        in_one_file
            .0
            .report("a new device joining 10,000 Notes in one op file, editing"),
        in_one_file.1,
        in_a_snapshot
            .0
            .report("a new device joining 10,000 Notes in a snapshot, editing"),
        in_a_snapshot.1,
    );
    assert!(
        caught_up.max_tick() < std::time::Duration::from_millis(16),
        "a frame stalled: {:?}",
        caught_up.max_tick()
    );
}

/// Which call of the Sync folder to hold: by its name (`list`, `read`, `ensure_available`
/// …) and path.
type HoldWhen = Box<dyn Fn(&str, &str) -> bool + Send>;

/// The Sync folder on disk as the worker sees it, watched: which threads call it, and it
/// can hold the next call of a kind (the read of an op file) until the test lets it go, or
/// panic at a list.
struct WatchedFolder {
    fs: crate::sync::FsSyncFolder,
    threads: std::sync::Mutex<Vec<std::thread::ThreadId>>,
    /// Held: the call, the signal that it is reached, and the switch that lets it go on.
    #[allow(clippy::type_complexity)]
    hold: std::sync::Mutex<
        Option<(
            HoldWhen,
            std::sync::mpsc::Sender<()>,
            std::sync::mpsc::Receiver<()>,
        )>,
    >,
    panics: std::sync::atomic::AtomicBool,
}

impl WatchedFolder {
    fn new(path: &Path) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            fs: crate::sync::FsSyncFolder::new(path),
            threads: Default::default(),
            hold: Default::default(),
            panics: Default::default(),
        })
    }

    /// Hold the next call `when` picks: returns the signal that it is reached and the switch
    /// that lets it go on.
    fn hold_next(
        &self,
        when: impl Fn(&str, &str) -> bool + Send + 'static,
    ) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
        let (reached, on_reached) = std::sync::mpsc::channel();
        let (go, on_go) = std::sync::mpsc::channel();
        *self.hold.lock().unwrap() = Some((Box::new(when), reached, on_go));
        (on_reached, go)
    }

    /// Hold the next read of another device's op file than `own`'s (a merge's).
    fn hold_next_op_read(
        &self,
        own: &Session,
    ) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
        let own = store(own).device_id().unwrap();
        self.hold_next(move |call, path| {
            call == "read" && path.contains("/ops/") && !path.contains(own.as_str())
        })
    }

    /// Note the call `call` of `path`, holding it when it is the one held.
    fn note(&self, call: &str, path: &str) {
        self.threads
            .lock()
            .unwrap()
            .push(std::thread::current().id());
        let mut hold = self.hold.lock().unwrap();
        let held = match &*hold {
            Some((when, _, _)) if when(call, path) => hold.take(),
            _ => None,
        };
        drop(hold);
        if let Some((_, reached, go)) = held {
            reached.send(()).unwrap();
            go.recv().unwrap();
        }
    }

    fn calls_on(&self, thread: std::thread::ThreadId) -> usize {
        (self.threads.lock().unwrap().iter())
            .filter(|t| **t == thread)
            .count()
    }
}

impl crate::sync::SyncFolder for WatchedFolder {
    fn list(&self, dir: &str) -> Result<Vec<String>, crate::sync::FolderError> {
        self.note("list", dir);
        if self.panics.load(std::sync::atomic::Ordering::SeqCst) {
            panic!("the provider failed");
        }
        self.fs.list(dir)
    }

    fn read(&self, path: &str) -> Result<Vec<u8>, crate::sync::FolderError> {
        self.note("read", path);
        self.fs.read(path)
    }

    fn write_new(&self, path: &str, bytes: &[u8]) -> Result<(), crate::sync::FolderError> {
        self.note("write_new", path);
        self.fs.write_new(path, bytes)
    }

    fn remove(&self, path: &str) -> Result<(), crate::sync::FolderError> {
        self.note("remove", path);
        self.fs.remove(path)
    }

    fn ensure_available(&self, path: &str) -> Result<bool, crate::sync::FolderError> {
        self.note("ensure_available", path);
        self.fs.ensure_available(path)
    }
}

/// A Session syncing through `dir`'s folder, watched.
fn watched(dir: &Dir, name: &str) -> (Session, std::sync::Arc<WatchedFolder>) {
    let mut s = dir.syncing(name);
    let folder = WatchedFolder::new(&dir.folder());
    s.set_sync_test_folder(folder.clone());
    (s, folder)
}

/// Every file in `folder`, by path.
fn folder_files(folder: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(folder, &mut out);
    out.sort();
    out
}

/// Wait until the worker syncing through `folder` has ended: only the test and the Session
/// hold it then.
fn worker_ends(folder: &std::sync::Arc<WatchedFolder>) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::sync::Arc::strong_count(folder) > 2 {
        assert!(
            std::time::Instant::now() < deadline,
            "the worker never ended"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// What a sync could change of a User database.
fn sync_rows(s: &Session) -> String {
    let store = store(s);
    format!(
        "{:?} {:?} {:?} {:?} {:?} {:?} {:?} {:?}",
        notebook_names(s),
        store.device_id().unwrap(),
        store.own_device_ids().unwrap(),
        store.merged_devices().unwrap(),
        store.last_reserved_op_file().unwrap(),
        store.has_pending_ops().unwrap(),
        store.records_ops().unwrap(),
        store.restore_choice_pending().unwrap(),
    )
}

#[test]
fn every_read_and_write_of_the_sync_folder_is_off_the_ui_thread() {
    let dir = Dir::new("off-ui");
    let ui = std::thread::current().id();
    let (mut a, folder) = watched(&dir, "a");
    let mut b = dir.syncing("b");
    a.create_notebook("From A").unwrap();
    sync(&mut a);
    sync(&mut b);
    b.create_notebook("From B").unwrap();
    sync(&mut b);
    sync(&mut a);
    assert!(notebook_names(&a).contains(&"From B".to_string()));
    assert_eq!(
        a.sync_panel(now()).devices.len(),
        2,
        "the devices were read"
    );
    // A restore syncs first, then the prompt's Catch up syncs.
    let backup = dir.0.join("a.obsbackup");
    back_up(&mut a, &backup);
    restore(&mut a, &backup);
    a.run("sync.restore_choice", &json!({ "choice": "catch_up" }))
        .unwrap();
    settle(&mut a);
    assert!(!a.sync_restore_pending());
    assert!(folder.threads.lock().unwrap().len() > 20);
    assert_eq!(folder.calls_on(ui), 0, "none on the UI thread");
}

#[test]
fn edits_made_while_a_sync_runs_are_kept_and_go_out_with_the_next() {
    let dir = Dir::new("edit-during");
    let (mut a, folder) = watched(&dir, "a");
    let mut b = dir.syncing("b");
    sync(&mut a);
    sync(&mut b);
    for i in 0..20 {
        b.create_notebook(&format!("From B {i}")).unwrap();
        sync(&mut b);
    }
    let (reached, go) = folder.hold_next_op_read(&a);
    a.run("sync.now", &json!({})).unwrap();
    a.sync_tick(now());
    reached
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    // The worker is merging B's files: the user goes on.
    assert!(a.sync_panel(now()).syncing);
    assert_eq!(a.sync_tick(now()), Some(crate::sync_session::SYNC_POLL_MS));
    a.create_notebook("Made while syncing").unwrap();
    let note = add_note(&mut a, "Written while syncing");
    go.send(()).unwrap();
    // More edits while it merges.
    a.create_notebook("Made while merging").unwrap();
    settle(&mut a);
    let names = notebook_names(&a);
    for name in ["From B 19", "Made while syncing", "Made while merging"] {
        assert!(names.contains(&name.to_string()), "{name} in {names:?}");
    }
    sync(&mut a);
    sync(&mut b);
    let names = notebook_names(&b);
    for name in ["Made while syncing", "Made while merging"] {
        assert!(names.contains(&name.to_string()), "{name} reached B");
    }
    assert!(
        b.annotations(None)
            .unwrap()
            .iter()
            .any(|n| n.id == note.id && n.body() == "Written while syncing")
    );
}

#[test]
fn choosing_a_folder_and_stop_syncing_wait_for_the_sync_running_showing_syncing() {
    let dir = Dir::new("deferred");
    let (mut a, folder) = watched(&dir, "a");
    let mut b = dir.syncing("b");
    sync(&mut a);
    b.create_notebook("From B").unwrap();
    sync(&mut b);
    let old = store(&a).device_id().unwrap();

    let (reached, go) = folder.hold_next_op_read(&a);
    a.run("sync.now", &json!({})).unwrap();
    a.sync_tick(now());
    reached
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    a.run("sync.stop", &json!({ "confirm": true })).unwrap();
    assert_eq!(a.sync_folder(), Some(dir.folder().as_path()), "not yet");
    assert!(a.sync_panel(now()).syncing);
    assert_eq!(a.sync_indicator(now()).unwrap().text, texts::SYNCING);
    go.send(()).unwrap();
    settle(&mut a);
    assert_eq!(a.sync_folder(), None, "stopped once the sync ended");
    assert_ne!(store(&a).device_id().unwrap(), old);
    assert!(notebook_names(&a).contains(&"From B".to_string()));

    // Choosing a folder while a sync runs (from a script) chooses it once that sync ends.
    b.create_notebook("From B again").unwrap();
    sync(&mut b);
    let other = dir.0.join("other");
    std::fs::create_dir_all(&other).unwrap();
    a.run(
        "sync.choose_folder",
        &json!({ "path": dir.folder().to_string_lossy() }),
    )
    .unwrap();
    let (reached, go) = folder.hold_next_op_read(&a);
    a.sync_tick(now());
    reached
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    a.run(
        "sync.choose_folder",
        &json!({ "path": other.to_string_lossy() }),
    )
    .unwrap();
    assert_eq!(a.sync_folder(), Some(dir.folder().as_path()));
    assert!(a.sync_panel(now()).syncing);
    go.send(()).unwrap();
    settle(&mut a);
    assert_eq!(a.sync_folder(), Some(other.as_path()));
    assert_eq!(a.sync_panel(now()).status, "Last synced just now");
}

#[test]
fn a_sync_whose_worker_panics_fails_and_the_next_one_syncs() {
    let dir = Dir::new("panics");
    let (mut a, folder) = watched(&dir, "a");
    sync(&mut a);
    folder
        .panics
        .store(true, std::sync::atomic::Ordering::SeqCst);
    sync(&mut a);
    let panel = a.sync_panel(now());
    assert_eq!(panel.status, texts::UNREACHABLE);
    assert!(panel.warn);
    assert!(
        panel
            .details
            .unwrap()
            .contains("stopped before it finished"),
        "the closed channel is a failure"
    );
    folder
        .panics
        .store(false, std::sync::atomic::Ordering::SeqCst);
    sync(&mut a);
    assert_eq!(a.sync_panel(now()).status, "Last synced just now");
}

#[test]
fn a_restore_goes_ahead_when_its_sync_first_does_not_finish_and_that_sync_writes_nothing_after() {
    let dir = Dir::new("abandoned");
    let (mut a, folder) = watched(&dir, "a");
    let mut b = dir.syncing("b");
    sync(&mut a);
    sync(&mut b);
    let backup = dir.0.join("a.obsbackup");
    back_up(&mut a, &backup);
    a.create_notebook("Not written yet").unwrap();
    b.create_notebook("From B").unwrap();
    sync(&mut b);
    // The sync before the restore writes A's op file, then is held reading B's: an iCloud
    // download that does not come.
    a.set_restore_sync_wait(std::time::Duration::from_millis(300));
    let (reached, go) = folder.hold_next_op_read(&a);
    let started = std::time::Instant::now();
    restore(&mut a, &backup);
    let waited = started.elapsed();
    reached.try_recv().unwrap();
    assert!(
        waited >= std::time::Duration::from_millis(300)
            && waited < std::time::Duration::from_secs(20),
        "{waited:?}"
    );
    assert!(a.sync_restore_pending());
    let panel = a.sync_panel(now());
    assert_eq!(panel.status, texts::RESTORE_TITLE);
    assert_eq!(
        panel.details.as_deref(),
        Some(texts::RESTORE_SYNC_UNFINISHED),
        "it says the sync before the restore did not finish"
    );
    assert!(!notebook_names(&a).contains(&"Not written yet".to_string()));
    let restored = sync_rows(&a);
    let files = folder_files(&dir.folder());
    // The held read comes back: the abandoned sync reads and writes the folder no more, and
    // the merge of B's file it read is refused (its connection's commits roll back): either
    // alone keeps the restored data as it is; this fails without both.
    go.send(()).unwrap();
    worker_ends(&folder);
    assert_eq!(
        sync_rows(&a),
        restored,
        "nothing written into the restored data"
    );
    assert!(!notebook_names(&a).contains(&"From B".to_string()));
    assert_eq!(
        folder_files(&dir.folder()),
        files,
        "nothing written in the folder"
    );
    // The edit made before the restore reached the folder before it was held.
    sync(&mut b);
    assert!(notebook_names(&b).contains(&"Not written yet".to_string()));
    // Catch up syncs as after any restore.
    a.run("sync.restore_choice", &json!({ "choice": "catch_up" }))
        .unwrap();
    settle(&mut a);
    assert!(notebook_names(&a).contains(&"From B".to_string()));
    assert_eq!(a.sync_panel(now()).status, "Last synced just now");
}

/// A restore of A's Backup while `ask` (Stop syncing, or choosing `other`) waits for a sync
/// held reading B's op file: let go during the restore's wait (`let_go`), so what waited
/// applies before the restore, or never, so the restore abandons that sync.
fn restore_while_waiting(name: &str, ask: &str, let_go: bool) -> (Dir, Session, PathBuf) {
    let dir = Dir::new(name);
    let (mut a, folder) = watched(&dir, "a");
    let mut b = dir.syncing("b");
    sync(&mut a);
    sync(&mut b);
    let backup = dir.0.join("a.obsbackup");
    back_up(&mut a, &backup);
    b.create_notebook("From B").unwrap();
    sync(&mut b);
    let other = dir.0.join("other");
    std::fs::create_dir_all(&other).unwrap();
    a.set_restore_sync_wait(std::time::Duration::from_millis(if let_go {
        30_000
    } else {
        300
    }));
    let (reached, go) = folder.hold_next_op_read(&a);
    a.run("sync.now", &json!({})).unwrap();
    a.sync_tick(now());
    reached
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    match ask {
        "stop" => a.run("sync.stop", &json!({ "confirm": true })).unwrap(),
        _ => a
            .run(
                "sync.choose_folder",
                &json!({ "path": other.to_string_lossy() }),
            )
            .unwrap(),
    };
    assert!(a.sync_panel(now()).syncing, "it waits for the sync");
    let letting_go = let_go.then(|| {
        let go = go.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            go.send(()).unwrap();
        })
    });
    restore(&mut a, &backup);
    if let Some(thread) = letting_go {
        thread.join().unwrap();
    } else {
        go.send(()).unwrap();
    }
    worker_ends(&folder);
    settle(&mut a);
    (dir, a, other)
}

#[test]
fn stop_syncing_waiting_for_a_sync_when_a_restore_starts_stays_stopped() {
    for let_go in [true, false] {
        let name = format!("stop-restore-{let_go}");
        let (_dir, a, _) = restore_while_waiting(&name, "stop", let_go);
        assert_eq!(a.sync_folder(), None, "let go: {let_go}");
        assert!(!a.sync_restore_pending(), "let go: {let_go}");
        assert!(
            !store(&a).records_ops().unwrap(),
            "it records nothing more (let go: {let_go})"
        );
        assert!(a.sync_indicator(now()).is_none());
    }
}

#[test]
fn a_folder_chosen_while_a_sync_runs_when_a_restore_starts_stays_chosen() {
    for let_go in [true, false] {
        let name = format!("choose-restore-{let_go}");
        let (_dir, a, other) = restore_while_waiting(&name, "choose", let_go);
        assert_eq!(a.sync_folder(), Some(other.as_path()), "let go: {let_go}");
    }
}

#[test]
fn stop_syncing_abandons_a_sync_that_does_not_end_and_stops() {
    let dir = Dir::new("stop-abandons");
    let (mut a, folder) = watched(&dir, "a");
    let mut b = dir.syncing("b");
    sync(&mut a);
    sync(&mut b);
    // In B's op file, not its snapshot.
    b.create_notebook("From B").unwrap();
    sync(&mut b);
    let wait = std::time::Duration::from_millis(300);
    a.set_stop_sync_wait(wait);
    // A folder call that does not come back: a hung network disk.
    let (reached, go) = folder.hold_next_op_read(&a);
    a.run("sync.now", &json!({})).unwrap();
    a.sync_tick(now());
    reached
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    let pressed = std::time::Instant::now();
    a.run("sync.stop", &json!({ "confirm": true })).unwrap();
    a.sync_tick(now());
    if pressed.elapsed() < wait {
        assert!(a.sync_panel(now()).syncing, "it waits first");
        assert_eq!(a.sync_folder(), Some(dir.folder().as_path()));
    }
    std::thread::sleep(wait);
    assert_eq!(a.sync_tick(now()), None, "stopped: nothing more to sync");
    assert_eq!(a.sync_folder(), None);
    assert!(!a.sync_panel(now()).syncing);
    assert!(!store(&a).records_ops().unwrap());
    let stopped = sync_rows(&a);
    let files = folder_files(&dir.folder());
    // The held call comes back: the abandoned sync merges nothing and writes nothing.
    go.send(()).unwrap();
    worker_ends(&folder);
    assert_eq!(sync_rows(&a), stopped);
    assert!(!notebook_names(&a).contains(&"From B".to_string()));
    assert_eq!(folder_files(&dir.folder()), files);
}

#[test]
fn an_edit_made_while_a_snapshot_is_due_skips_the_snapshot_not_the_sync() {
    let dir = Dir::new("snapshot-edit");
    let (mut a, folder) = watched(&dir, "a");
    let mut b = dir.syncing("b");
    sync(&mut a);
    let id = store(&a).device_id().unwrap();
    // A's snapshot is damaged, so a snapshot is due at its next sync, after reading it.
    let snapshots = dir.folder().join(crate::sync::snapshot::snapshots_dir(&id));
    let damaged = std::fs::read_dir(&snapshots)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&damaged, b"damaged").unwrap();
    let mine = format!("{}/", crate::sync::snapshot::snapshots_dir(&id));
    let (reached, go) =
        folder.hold_next(move |call, path| call == "ensure_available" && path.starts_with(&mine));
    a.run("sync.now", &json!({})).unwrap();
    a.sync_tick(now());
    reached
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    // The worker reads its snapshot in `snapshot_due`: the user goes on.
    a.create_notebook("Made while a snapshot was due").unwrap();
    go.send(()).unwrap();
    settle(&mut a);
    let panel = a.sync_panel(now());
    assert_eq!(panel.status, "Last synced just now", "{:?}", panel.details);
    // The edit went out, and the snapshot was written after it.
    sync(&mut b);
    assert!(notebook_names(&b).contains(&"Made while a snapshot was due".to_string()));
    let (_, name) =
        crate::sync::snapshot::newest_snapshot(&crate::sync::FsSyncFolder::new(dir.folder()), &id)
            .unwrap()
            .unwrap();
    assert!(matches!(
        crate::sync::snapshot::read_snapshot(
            store(&a),
            &crate::sync::FsSyncFolder::new(dir.folder()),
            &id,
            &name
        )
        .unwrap(),
        crate::sync::snapshot::SnapshotRead::Whole(_)
    ));
}

#[test]
fn a_worker_syncs_on_the_clock_the_session_runs_on() {
    // The gallery's clock is fixed: the worker's device.json reads the same time.
    let at = 1_700_000_000_000;
    obs_store::clock::fix(Some(at));
    let dir = Dir::new("fixed-clock");
    let mut a = dir.syncing("a");
    sync(&mut a);
    assert_eq!(a.sync_panel(at).status, "Last synced just now");
    assert_eq!(a.sync_panel(at + 61_000).status, "Last synced 1 minute ago");
    obs_store::clock::fix(None);
}
