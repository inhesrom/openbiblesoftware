//! Sync on the desktop (plan S29) through the headless harness: without a Sync folder nothing
//! of Sync shows but Settings → Sync's Choose Sync folder…; two windows on their own User
//! databases choose one folder there (the native folder dialog injected) and converge; after
//! a Backup is restored on a syncing window its prompt shows, and Catch up and Replace both
//! work; a Sync conflict's copy is in Needs attention with its four actions and the changes
//! held back are listed. The gallery's Sync surfaces are drawn by [`sync_tabs`] and
//! [`sync_dialogs`]. `UPDATE_PREVIEWS=1` writes `sync-*.ppm` to `target/previews/`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use obs_engine::sync::texts;
use obs_store::{Annotation, Change, Row};

use super::*;
use crate::shell::backups_tab::DataPick;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-sync-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("folder")).unwrap();
    dir
}

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// Frames until `done` holds.
fn until(h: &mut Harness, what: &str, done: impl Fn(&Harness) -> bool) {
    for _ in 0..300 {
        if done(h) {
            h.frames(2);
            return;
        }
        h.paced_frame();
    }
    panic!("never: {what}; on screen: {:?}", h.texts());
}

fn shows(h: &Harness, text: &str) -> bool {
    h.find(text).is_some()
}

fn now() -> i64 {
    obs_store::clock::now_ms()
}

/// Settings, showing Sync only (as the status indicator opens it).
fn settings_on_sync(h: &mut Harness) -> obs_engine::TabId {
    let tab = h.shell.run("app.settings", &json!({})).unwrap()["tab"]
        .as_u64()
        .unwrap();
    let tab = obs_engine::TabId(tab);
    h.shell.st.tabs.entry(tab).or_default().filter = texts::SECTION.to_string();
    h.frames(3);
    tab
}

/// A window whose folder dialog answers `folder`, syncing as a Mac.
fn window(db: &Path, folder: &Path) -> Harness {
    let mut h = Harness::launch(Some(db));
    h.shell.st.session.set_sync_platform("macos");
    let folder = folder.to_path_buf();
    h.shell.st.backups_ui.dialog = Arc::new(move |pick, _| {
        assert_eq!(pick, DataPick::SyncFolder);
        Some(folder.clone())
    });
    h
}

/// Choose the folder in Settings → Sync and wait for the first sync.
fn choose_folder(h: &mut Harness) {
    let tab = settings_on_sync(h);
    h.click_text(texts::CHOOSE_FOLDER);
    until(h, "the first sync", |h| {
        h.shell
            .st
            .session
            .sync_panel(now())
            .status
            .starts_with("Last synced")
    });
    h.shell.run("tab.close", &json!({ "tab": tab.0 }));
    h.frames(2);
}

/// Sync now, from Settings → Sync.
fn sync_now(h: &mut Harness) {
    let tab = settings_on_sync(h);
    h.click_text(texts::SYNC_NOW);
    h.frames(4);
    assert!(!h.shell.st.session.sync_panel(now()).syncing);
    h.shell.run("tab.close", &json!({ "tab": tab.0 }));
    h.frames(2);
}

fn note_texts(h: &Harness) -> Vec<String> {
    let mut texts: Vec<String> = (h.shell.st.session.annotations(None).unwrap().into_iter())
        .filter(|a| a.is_note())
        .map(|a| a.body().to_string())
        .collect();
    texts.sort();
    texts
}

fn indicator_shown(h: &Harness) -> bool {
    (h.texts().iter()).any(|t| *t == crate::icons::Icon::Sync.marker())
}

#[test]
fn without_a_sync_folder_only_choose_sync_folder_shows() {
    let dir = temp("none");
    let mut h = Harness::launch(Some(&dir.join("a/user.db")));
    h.frames(3);
    assert!(!indicator_shown(&h), "no status indicator: {:?}", h.texts());
    preview(&h, "sync-no-folder-kittest");
    settings_on_sync(&mut h);
    preview(&h, "sync-settings-no-folder-kittest");
    assert!(shows(&h, texts::CHOOSE_FOLDER));
    assert!(shows(&h, texts::ONBOARDING));
    assert!(!shows(&h, texts::SYNC_NOW) && !shows(&h, texts::DEVICES_LABEL));
    assert_eq!(h.shell.st.session.sync_folder(), None);
}

#[test]
fn two_windows_converge_through_one_folder_and_the_restore_prompt_takes_both_answers() {
    let dir = temp("two");
    let folder = dir.join("folder");
    let mut a = window(&dir.join("a/user.db"), &folder);
    let mut b = window(&dir.join("b/user.db"), &folder);
    a.shell
        .run("sync.device_name", &json!({ "name": "Study Mac" }));
    choose_folder(&mut a);
    choose_folder(&mut b);
    assert!(indicator_shown(&a), "{:?}", a.texts());
    // The indicator opens Settings showing Sync.
    a.click_text(&crate::icons::Icon::Sync.marker());
    a.frames(3);
    assert!(shows(&a, texts::DEVICES_LABEL), "{:?}", a.texts());
    assert!(!shows(&a, "TEXT AND FONTS"), "only Sync: {:?}", a.texts());
    let settings = a.tab_id("Settings");
    a.shell.run("tab.close", &json!({ "tab": settings.0 }));
    a.shell.run(
        "annotation.note",
        &json!({ "passage": "Gen 1:1", "body": "In the beginning" }),
    );
    sync_now(&mut a);
    sync_now(&mut b);
    assert_eq!(note_texts(&b), ["In the beginning"]);
    settings_on_sync(&mut b);
    assert!(shows(&b, texts::DEVICES_LABEL));
    assert!(
        shows(&b, "Study Mac · macOS · last seen just now"),
        "{:?}",
        b.texts()
    );
    preview(&b, "sync-settings-kittest");

    // A Backup, then an edit both windows have, then the Backup restored on A.
    let backup = dir.join("a.obsbackup");
    a.shell
        .run("data.back_up", &json!({ "path": backup.to_string_lossy() }));
    let note = a.shell.st.session.annotations(None).unwrap().remove(0);
    a.shell.run(
        "annotation.edit",
        &json!({ "id": note.id, "body": "In the beginning, edited" }),
    );
    sync_now(&mut a);
    sync_now(&mut b);
    assert_eq!(note_texts(&b), ["In the beginning, edited"]);
    a.shell
        .run("data.restore", &json!({ "path": backup.to_string_lossy() }));
    until(&mut a, "the restore's prompt", |h| {
        shows(h, texts::RESTORE_TITLE)
    });
    assert!(shows(&a, texts::RESTORE_BODY) && shows(&a, texts::REPLACE));
    preview(&a, "sync-restore-prompt-kittest");
    assert_eq!(note_texts(&a), ["In the beginning"]);
    a.click_text(texts::CATCH_UP);
    until(&mut a, "the prompt to go", |h| {
        !shows(h, texts::RESTORE_TITLE)
    });
    assert_eq!(
        note_texts(&a),
        ["In the beginning, edited"],
        "caught up with the edit made since the Backup"
    );

    // Again, and Replace: the Backup's text everywhere.
    a.shell
        .run("data.restore", &json!({ "path": backup.to_string_lossy() }));
    until(&mut a, "the restore's prompt", |h| {
        shows(h, texts::RESTORE_TITLE)
    });
    a.click_text(texts::REPLACE);
    until(&mut a, "the prompt to go", |h| {
        !shows(h, texts::RESTORE_TITLE)
    });
    assert_eq!(note_texts(&a), ["In the beginning"]);
    sync_now(&mut b);
    assert_eq!(
        note_texts(&b),
        ["In the beginning"],
        "B took the restored data"
    );
    // The restores kept A's Sync folder.
    assert_eq!(a.shell.st.session.sync_folder(), Some(folder.as_path()));
}

/// The Note at Gen 1:1 and a Sync conflict's copy of it, written as the merge writes one.
fn conflicted_note(h: &mut Harness) -> (Annotation, Annotation) {
    h.shell.run(
        "annotation.note",
        &json!({ "passage": "Gen 1:1", "body": "Beginnings: the heavens first." }),
    );
    let note = (h.shell.st.session.annotations(None).unwrap().into_iter())
        .find(|a| a.is_note())
        .unwrap();
    let copy = Annotation {
        id: obs_store::annotation::new_id(),
        content: obs_engine::Content::Note {
            body: "Beginnings: written on the iPhone.".into(),
        },
        conflict_of: Some(note.id.clone()),
        ..note.clone()
    };
    let user = h.shell.st.session.user.as_ref().unwrap();
    user.apply_changes(&[Change::insert(Row::Annotation(copy.clone()))])
        .unwrap();
    (note, copy)
}

/// A change another device sent that this one holds back: a setting a later app added.
fn held_change(h: &mut Harness, folder: &Path) {
    let later = obs_store::annotation::new_id();
    let op = obs_engine::sync::ops::Op {
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
    let path = folder.join(obs_engine::sync::ops::op_file_path(&later, 1));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, obs_engine::sync::ops::encode_op_file(&[op]).unwrap()).unwrap();
    let store = h
        .shell
        .st
        .session
        .user
        .as_ref()
        .unwrap()
        .user_store()
        .unwrap();
    store
        .execute_unchecked(&format!(
            "INSERT INTO applied_up_to (device_id, seq) VALUES ('{later}', 0)"
        ))
        .unwrap();
    h.shell.run("sync.now", &json!({}));
    h.frames(4);
}

fn needs_attention(h: &mut Harness) {
    h.shell.run("tool.notes", &json!({}));
    h.shell
        .run("notes.notebook", &json!({ "value": "attention" }));
    h.frames(5);
}

#[test]
fn a_sync_conflict_and_a_change_waiting_are_in_needs_attention() {
    let dir = temp("conflict");
    let folder = dir.join("folder");
    let mut h = window(&dir.join("a/user.db"), &folder);
    choose_folder(&mut h);
    let (note, copy) = conflicted_note(&mut h);
    held_change(&mut h, &folder);
    needs_attention(&mut h);
    assert!(shows(&h, "Needs attention"));
    assert!(
        (h.texts().iter()).any(|t| t.starts_with("Sync conflict: this text and the Note's at ")),
        "{:?}",
        h.texts()
    );
    assert!(shows(&h, &texts::CHANGES_WAITING.to_uppercase()));
    assert!(
        (h.texts().iter()).any(|t| t.starts_with(
            "A change from Unnamed device to the setting `study.later_app_setting` is waiting"
        )),
        "{:?}",
        h.texts()
    );
    for label in [
        texts::KEEP_THIS,
        texts::KEEP_OTHER,
        texts::KEEP_BOTH,
        texts::COMBINE,
    ] {
        assert!(shows(&h, label), "{label}: {:?}", h.texts());
    }
    preview(&h, "sync-needs-attention-kittest");
    h.click_text(texts::KEEP_THIS);
    h.frames(3);
    let body = |h: &Harness, id: &str| {
        h.shell
            .st
            .session
            .annotation(id)
            .unwrap()
            .body()
            .to_string()
    };
    assert_eq!(body(&h, &note.id), copy.body());
    assert!(h.shell.st.session.annotation(&copy.id).is_err());
    assert!(!shows(&h, texts::KEEP_THIS), "the conflict is resolved");
    // One undo step brings it back.
    h.shell.run("edit.undo", &json!({}));
    h.frames(3);
    assert!(shows(&h, texts::KEEP_BOTH));
    h.click_text(texts::KEEP_BOTH);
    h.frames(3);
    assert_eq!(
        h.shell.st.session.annotation(&copy.id).unwrap().conflict_of,
        None
    );
    assert!(!shows(&h, texts::KEEP_BOTH));
}

// ---------------------------------------------------------------------------------------
// The gallery's Sync surfaces.

/// A Sync folder of the gallery's, the same path on every computer (tests run in the
/// crate's folder), emptied first.
fn gallery_folder(name: &str) -> PathBuf {
    let folder = PathBuf::from(format!("../../target/gallery/sync-{name}"));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

/// The harness's window syncing through `folder` as "Study Mac", a Mac.
fn gallery_syncing(h: &mut Harness, folder: &Path) {
    let s = &mut h.shell.st.session;
    s.set_sync_platform("macos");
    s.run("sync.device_name", &json!({ "name": "Study Mac" }))
        .unwrap();
    s.run(
        "sync.choose_folder",
        &json!({ "path": folder.to_string_lossy() }),
    )
    .unwrap();
    h.frames(4);
}

/// "Ian's iPhone" syncing through `folder` beside the harness's User database.
fn gallery_iphone(h: &Harness, folder: &Path) {
    let db = (h.shell.st.session.user.as_ref())
        .and_then(|u| u.user_store())
        .unwrap()
        .path()
        .parent()
        .unwrap()
        .join("iphone/user.db");
    let mut phone = Session::new(JobMode::Inline);
    phone.attach_user(Box::new(obs_store::UserStore::open(&db).unwrap().0));
    phone.set_sync_platform("ios");
    phone
        .run("sync.device_name", &json!({ "name": "Ian's iPhone" }))
        .unwrap();
    phone
        .run(
            "sync.choose_folder",
            &json!({ "path": folder.to_string_lossy() }),
        )
        .unwrap();
    phone.request_sync();
    phone.sync_tick(now());
    phone.sync_tick(now());
}

/// Settings → Sync before and after a folder is chosen (the Tabs section).
pub(super) fn sync_tabs() -> Vec<super::a11y_audit::Overlay> {
    vec![
        (
            "Settings, Sync before a folder is chosen",
            |h| {
                h.shell.st.session.set_sync_platform("macos");
                settings_on_sync(h);
            },
            |h| shows(h, texts::CHOOSE_FOLDER),
        ),
        (
            "Settings, Sync with two devices",
            |h| {
                let folder = gallery_folder("settings");
                gallery_iphone(h, &folder);
                gallery_syncing(h, &folder);
                settings_on_sync(h);
            },
            |h| {
                shows(h, texts::DEVICES_LABEL)
                    && shows(h, "Ian's iPhone · iOS · last seen just now")
            },
        ),
        (
            "Needs attention, a Sync conflict and a change waiting",
            |h| {
                let folder = gallery_folder("attention");
                gallery_syncing(h, &folder);
                conflicted_note(h);
                held_change(h, &folder);
                needs_attention(h);
            },
            |h| shows(h, texts::KEEP_BOTH) && shows(h, &texts::CHANGES_WAITING.to_uppercase()),
        ),
    ]
}

/// The prompt after a Backup is restored on a syncing device (the Dialogs section).
pub(super) fn sync_dialogs() -> Vec<super::a11y_audit::Overlay> {
    vec![(
        "Sync restore prompt",
        |h| {
            let folder = gallery_folder("restore");
            gallery_syncing(h, &folder);
            let backup = folder.with_extension("obsbackup");
            h.shell
                .run("data.back_up", &json!({ "path": backup.to_string_lossy() }));
            h.shell
                .run("data.restore", &json!({ "path": backup.to_string_lossy() }));
            h.frames(4);
        },
        |h| shows(h, texts::RESTORE_TITLE) && shows(h, texts::CATCH_UP),
    )]
}
