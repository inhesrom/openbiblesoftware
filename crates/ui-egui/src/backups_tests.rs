//! Back up & restore (#217) through the headless harness: the Tab opened from the Tools menu,
//! Back up now, a deleted Notebook brought back by Restore from backup… after its preview,
//! Undo restore, the Safety backup in the kept list, and Export everything.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use obs_engine::BACKUPS_TITLE;

use super::*;
use crate::shell::backups_tab::DataPick;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-backups-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// Frames until `done` holds (the file dialog answers on its own thread).
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

fn dump(h: &Harness) -> String {
    h.shell
        .st
        .session
        .user
        .as_ref()
        .unwrap()
        .annotation_dump()
        .unwrap()
}

#[test]
fn a_deleted_notebook_comes_back_from_a_backup_and_undo_restore_takes_it_back() {
    let dir = folder("restore");
    let mut h = Harness::launch(Some(&dir.join("user.db")));
    let nb = h
        .shell
        .run("notebook.create", &json!({ "name": "Romans" }))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    for (passage, body) in [
        ("Rom 5:8", "while we were yet sinners"),
        ("Rom 8:28", "all things"),
    ] {
        h.shell.run(
            "annotation.note",
            &json!({ "passage": passage, "body": body, "notebook": nb }),
        );
    }
    let backed_up = dump(&h);

    // Tools → Back up & restore…
    h.frames(2);
    h.open_menu("File");
    preview(&h, "backups-file-menu-kittest");
    h.click_text("Back up & restore…");
    let tab = h.tab_id(BACKUPS_TITLE);
    assert_eq!(
        h.shell.st.session.workspace.tab(tab).unwrap().kind,
        TabKind::Backups
    );

    // Back up now… asks where, with a suggested name, and writes the file.
    let backup = dir.join("mine.obsbackup");
    let asked = Arc::new(Mutex::new(Vec::new()));
    let (to, log) = (backup.clone(), Arc::clone(&asked));
    h.shell.st.backups_ui.dialog = Arc::new(move |pick, name| {
        log.lock().unwrap().push((pick, name.to_string()));
        Some(to.clone())
    });
    h.click_text("Back up now…");
    until(&mut h, "backup written", |_| backup.is_file());
    {
        let asked = asked.lock().unwrap();
        assert_eq!(asked[0].0, DataPick::BackUp);
        assert!(
            asked[0].1.starts_with("Open Bible backup ") && asked[0].1.ends_with(".obsbackup"),
            "{:?}",
            asked[0]
        );
    }

    // Delete the Notebook with its Notes.
    h.shell.run(
        "notebook.delete",
        &json!({ "id": nb, "annotations": "delete" }),
    );
    let deleted = dump(&h);
    assert_ne!(deleted, backed_up);

    // Restore from backup… shows what will change first; nothing changes yet.
    h.frames(2);
    h.click_text("Restore from backup…");
    until(&mut h, "preview", |h| {
        h.find("Restore mine.obsbackup?").is_some()
    });
    assert_eq!(asked.lock().unwrap()[1].0, DataPick::Restore);
    assert!(
        h.find("Notebooks: brings back “Romans”").is_some(),
        "{:?}",
        h.texts()
    );
    assert!(h.find("After restoring").is_some());
    assert_eq!(dump(&h), deleted);
    preview(&h, "backups-restore-preview-kittest");

    h.click_text("Restore");
    assert_eq!(dump(&h), backed_up);
    assert_eq!(h.shell.st.session.annotations(None).unwrap().len(), 2);
    assert!(
        h.find("Restored from mine.obsbackup. A safety backup was taken first.")
            .is_some(),
        "{:?}",
        h.texts()
    );
    // The Safety backup is in the kept list.
    assert!(h.find("Safety backup").is_some());
    preview(&h, "backups-restored-kittest");

    // Undo restore brings back the data as it was just before.
    h.click_text("Undo restore");
    assert_eq!(dump(&h), deleted);
    assert!(h.find("Undo restore").is_none());

    // A kept Backup restores the same way: its Restore… opens the preview.
    let safety_rows = h
        .shell
        .st
        .session
        .kept_backups()
        .iter()
        .filter(|b| b.kind == obs_engine::backups::BackupKind::Safety)
        .count();
    assert_eq!(safety_rows, 2);
    let first_restore = h
        .shapes
        .iter()
        .filter(|(_, t)| t == "Restore…")
        .map(|(r, _)| r.center())
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    h.click(first_restore);
    assert!(
        h.texts().iter().any(|t| t.starts_with("Restore user-")),
        "{:?}",
        h.texts()
    );
    h.click_text("Cancel");
    assert!(!h.texts().iter().any(|t| t.starts_with("Restore user-")));
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_everything_writes_the_zip_where_the_dialog_says() {
    let dir = folder("export");
    let mut h = Harness::launch(Some(&dir.join("user.db")));
    h.shell.run(
        "annotation.note",
        &json!({ "passage": "Jn 3:16", "body": "love" }),
    );
    h.shell.run("data.backups", &json!({}));
    h.frames(3);
    let zip = dir.join("data.zip");
    let to = zip.clone();
    h.shell.st.backups_ui.dialog = Arc::new(move |pick, name| {
        assert_eq!(pick, DataPick::Export);
        assert!(name.ends_with(".zip"), "{name}");
        Some(to.clone())
    });
    preview(&h, "backups-tab-kittest");
    h.click_text("Export everything…");
    until(&mut h, "zip written", |_| zip.is_file());
    let bytes = std::fs::read(&zip).unwrap();
    assert!(bytes.starts_with(b"PK"));
    // A cancelled dialog does nothing.
    let files = || std::fs::read_dir(&dir).unwrap().count();
    let before = files();
    h.shell.st.backups_ui.dialog = Arc::new(|_, _| None);
    h.click_text("Back up now…");
    until(&mut h, "dialog answered", |h| {
        !h.shell.st.data_dialog_waiting()
    });
    assert_eq!(files(), before);
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #258: a Backup holding a Layout and a setting the app could not read is refused in the
/// preview, naming them, with no Restore offered and nothing changed.
#[test]
fn a_backup_with_an_unreadable_layout_is_refused_in_the_preview() {
    let dir = folder("refused");
    let mut h = Harness::launch(Some(&dir.join("user.db")));
    h.shell.run(
        "annotation.note",
        &json!({ "passage": "Jn 3:16", "body": "kept" }),
    );
    let good = dir.join("good.obsbackup");
    h.shell.st.session.back_up(&good).unwrap();
    // A copy damaged through the store, which keeps whatever text it is given.
    let copy = dir.join("damaging").join("user.db");
    std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
    std::fs::copy(&good, &copy).unwrap();
    let backup = dir.join("damaged.obsbackup");
    {
        let (store, _) = obs_store::UserStore::open(&copy).unwrap();
        store.save_layout("Broken", "not JSON").unwrap();
        store.set_preference("search.saved", "not JSON").unwrap();
        store.back_up(&backup).unwrap();
    }
    let before = dump(&h);
    h.shell.run("data.backups", &json!({}));
    h.frames(3);
    let to = backup.clone();
    h.shell.st.backups_ui.dialog = Arc::new(move |_, _| Some(to.clone()));
    h.click_text("Restore from backup…");
    until(&mut h, "the preview", |h| {
        h.texts()
            .iter()
            .any(|t| t.starts_with("damaged.obsbackup cannot be restored"))
    });
    preview(&h, "backups-restore-refused-kittest");
    let shown = h.texts().join("\n");
    for named in [
        "the Layout “Broken”",
        "the setting `search.saved` (Saved searches)",
        "Nothing was changed.",
    ] {
        assert!(shown.contains(named), "{named}: {shown}");
    }
    assert!(h.find("Restore").is_none(), "{shown}");
    assert_eq!(dump(&h), before);
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}
