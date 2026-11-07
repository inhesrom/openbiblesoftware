//! Back up, restore and export all user data (#217): Back up now, the restore preview, restore
//! with its Safety backup and Undo restore, the Session reloading what it keeps of the User
//! database, and Export everything.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Read as _;
use std::path::PathBuf;

use obs_store::BackupKind;
use serde_json::{Value, json};

use crate::testing::bibles::{kjv, web, write_library};
use crate::*;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-backups-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_library(&root.join("library"), &[kjv(), web()]);
        Self { root }
    }

    fn session(&self) -> Session {
        let mut s = Session::new(JobMode::Inline);
        assert!(s.library.scan(&self.root.join("library")).is_empty());
        let (store, _) = obs_store::UserStore::open(&self.root.join("user.db")).unwrap();
        s.attach_user(Box::new(store));
        s
    }

    fn file(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// A Session launched on a copy of `backup`: what a restore of it must leave.
    fn launched_on(&self, backup: &std::path::Path) -> Session {
        let db = self.root.join("launched").join("user.db");
        let _ = std::fs::remove_dir_all(db.parent().unwrap());
        std::fs::create_dir_all(db.parent().unwrap()).unwrap();
        std::fs::copy(backup, &db).unwrap();
        let mut s = Session::new(JobMode::Inline);
        assert!(s.library.scan(&self.root.join("library")).is_empty());
        let (store, _) = obs_store::UserStore::open(&db).unwrap();
        s.attach_user(Box::new(store));
        s
    }
}

/// The preferences two Sessions must agree on after a restore.
fn assert_same_preferences(restored: &mut Session, launched: &mut Session) {
    assert_eq!(restored.text, launched.text);
    assert_eq!(restored.appearance, launched.appearance);
    assert_eq!(restored.fonts, launched.fonts);
    assert_eq!(restored.toolbar, launched.toolbar);
    assert_eq!(restored.copy_verses, launched.copy_verses);
    assert_eq!(restored.home_settings(), launched.home_settings());
    assert_eq!(restored.key_bindings(), launched.key_bindings());
    for g in Guide::ALL {
        assert_eq!(
            restored.guides.arrangement(g),
            launched.guides.arrangement(g),
            "{g:?}"
        );
    }
    for setting in crate::settings::SETTINGS {
        assert_eq!(
            restored.setting_is_default(setting.id),
            launched.setting_is_default(setting.id),
            "{}",
            setting.id
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn dump(s: &Session) -> String {
    s.user.as_ref().unwrap().annotation_dump().unwrap()
}

fn path(p: &std::path::Path) -> Value {
    json!({ "path": p.to_string_lossy() })
}

/// A "Romans" Notebook with two Notes and a Highlight in KJV's Matthew; returns its id.
fn romans(s: &mut Session) -> String {
    let id = s
        .run("notebook.create", &json!({ "name": "Romans" }))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    for (passage, body) in [
        ("Rom 5:8", "while we were yet sinners"),
        ("Rom 8:28", "all things"),
    ] {
        s.run(
            "annotation.note",
            &json!({ "passage": passage, "body": body, "notebook": id }),
        )
        .unwrap();
    }
    let unit = kjv()
        .units
        .into_iter()
        .find(|u| u.id.as_str() == "MAT")
        .unwrap();
    let at = unit.text.find("neighbour").unwrap();
    s.run(
        "annotation.highlight",
        &json!({
            "resource": "kjv", "start_unit": "MAT", "start": at, "end": at + 9,
            "style": "green", "notebook": id,
        }),
    )
    .unwrap();
    id
}

/// #217 acceptance: backing up, deleting a Notebook and restoring brings back every
/// Annotation; the Safety backup allows undoing the restore.
#[test]
fn backing_up_deleting_a_notebook_and_restoring_brings_back_every_annotation() {
    let f = Fixture::new("acceptance");
    let mut s = f.session();
    let nb = romans(&mut s);
    let default = s.default_notebook().unwrap().id;
    s.run(
        "annotation.note",
        &json!({ "body": "unanchored thought", "notebook": default }),
    )
    .unwrap();
    let all = s.annotations(None).unwrap();
    assert_eq!(all.len(), 4);
    let backed_up = dump(&s);
    let backup = f.file("mine.obsbackup");
    s.run("data.back_up", &path(&backup)).unwrap();
    assert!(backup.is_file());

    // Delete the Notebook and everything in it.
    let out = s
        .run(
            "notebook.delete",
            &json!({ "id": nb, "annotations": "delete" }),
        )
        .unwrap();
    assert_eq!(out["deleted_annotations"], 3);
    let deleted = dump(&s);
    assert_eq!(s.annotations(None).unwrap().len(), 1);

    // The preview says what comes back.
    let preview = s.restore_preview(&backup).unwrap();
    let line = |what: &str| preview.lines.iter().find(|l| l.what == what).unwrap();
    assert_eq!((line("Notebooks").now, line("Notebooks").after), (1, 2));
    assert_eq!(line("Notebooks").detail, "brings back “Romans”");
    assert_eq!((line("Notes").now, line("Notes").after), (1, 3));
    assert_eq!((line("Highlights").now, line("Highlights").after), (0, 1));
    assert!(preview.changes_anything());
    assert!(!preview.older_format);
    // Previewing changed nothing.
    assert_eq!(dump(&s), deleted);

    let before = s.annotations_revision();
    let out = s.run("data.restore", &path(&backup)).unwrap();
    let safety = PathBuf::from(out["safety_backup"].as_str().unwrap());
    assert_eq!(dump(&s), backed_up);
    let mut restored = s.annotations(None).unwrap();
    let mut expected = all.clone();
    restored.sort_by(|a, b| a.id.cmp(&b.id));
    expected.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(restored, expected);
    // Views reload their Marks; the Notes Tool and search see the restored Notes.
    assert!(s.annotations_revision() > before);
    assert_eq!(s.notebooks().unwrap().len(), 2);

    // The Safety backup is listed, and Undo restore brings back the state before it.
    let kept = s.kept_backups();
    assert!(
        kept.iter()
            .any(|b| b.kind == BackupKind::Safety && b.path == safety),
        "{kept:?}"
    );
    assert_eq!(s.last_restore().unwrap().safety, safety);
    s.run("data.undo_restore", &Value::Null).unwrap();
    assert_eq!(dump(&s), deleted);
    assert!(s.last_restore().is_none());
    assert!(!s.is_enabled("data.undo_restore"));

    // Restoring the backup again from the kept list works the same way.
    s.run("data.restore", &path(&backup)).unwrap();
    assert_eq!(dump(&s), backed_up);
    drop(s);
    // And it lasts: the next launch reads the restored data.
    let s = f.session();
    assert_eq!(dump(&s), backed_up);
}

#[test]
fn a_restore_reloads_settings_layouts_and_writes_the_workspace_as_it_is() {
    let f = Fixture::new("reload");
    let mut s = f.session();
    s.run("resource.open", &json!({ "id": "kjv" })).unwrap();
    s.run("view.text_larger", &Value::Null).unwrap();
    let larger = s.text.size;
    s.run("layout.save", &json!({ "name": "Study" })).unwrap();
    s.save_last_session().unwrap();
    let backup = f.file("b.obsbackup");
    s.run("data.back_up", &path(&backup)).unwrap();

    s.run("view.text_reset", &Value::Null).unwrap();
    s.run("layout.delete", &json!({ "name": "Study" })).unwrap();
    s.run("resource.open", &json!({ "id": "web" })).unwrap();
    s.save_last_session().unwrap();
    let workspace = s.workspace.snapshot().to_json();
    assert_ne!(s.text.size, larger);

    s.run("data.restore", &path(&backup)).unwrap();
    assert_eq!(s.text.size, larger);
    assert_eq!(s.user.as_ref().unwrap().layouts().unwrap(), ["Study"]);
    // The open Workspace stays, and is what the next save writes, even though the
    // Session had written exactly it before the restore replaced the Last session.
    assert_eq!(s.workspace.snapshot().to_json(), workspace);
    s.save_last_session().unwrap();
    assert_eq!(
        s.user.as_ref().unwrap().last_session().unwrap().as_deref(),
        Some(workspace.as_str())
    );
}

/// A restore leaves every preference as a launch on the Backup would: one the Backup does
/// not have (it was made before the preference was changed) is back at its default, not kept
/// from before the restore.
#[test]
fn a_restore_puts_back_the_default_of_every_preference_the_backup_lacks() {
    let f = Fixture::new("defaults");
    let mut s = f.session();
    let backup = f.file("fresh.obsbackup");
    s.run("data.back_up", &path(&backup)).unwrap();

    let resettable: Vec<&str> = (crate::settings::SETTINGS.iter())
        .filter(|s| s.has_reset())
        .map(|s| s.id)
        .collect();
    for id in &resettable {
        crate::settings_tests::change(&mut s, id);
        assert_eq!(s.setting_is_default(id), Some(false), "{id} did not change");
    }
    s.run(
        "copy.option",
        &json!({ "name": "layout", "value": "verse_per_line" }),
    )
    .unwrap();
    s.run("copy.preset_save", &json!({ "name": "Handout" }))
        .unwrap();
    assert_ne!(s.copy_verses, CopyVerses::default());

    s.run("data.restore", &path(&backup)).unwrap();
    for id in &resettable {
        assert_eq!(s.setting_is_default(id), Some(true), "{id} was kept");
    }
    assert_eq!(s.text.size, obs_layout::TextSettings::default().size);
    assert_eq!(s.fonts, FontChoices::default());
    assert_eq!(s.copy_verses, CopyVerses::default());
    assert_same_preferences(&mut s, &mut f.launched_on(&backup));
}

/// A Backup with View preferences but no font chosen leaves no font chosen, and keeps its own
/// text size.
#[test]
fn a_restore_without_font_choices_leaves_the_bundled_fonts() {
    let f = Fixture::new("fonts");
    let mut s = f.session();
    s.run("view.text_larger", &Value::Null).unwrap();
    let larger = s.text.size;
    let backup = f.file("larger.obsbackup");
    s.run("data.back_up", &path(&backup)).unwrap();

    s.run(
        "view.font",
        &json!({ "slot": "latin", "family": "Charis SIL" }),
    )
    .unwrap();
    s.run("view.text_larger", &Value::Null).unwrap();
    assert!(s.fonts.get(FontSlot::Latin).is_some());

    s.run("data.restore", &path(&backup)).unwrap();
    assert_eq!(s.fonts, FontChoices::default());
    assert_eq!(s.text.size, larger);
    assert_same_preferences(&mut s, &mut f.launched_on(&backup));
}

/// Back up now and Export everything never write over the User database or SQLite's files
/// beside it; it goes on saving and reopens with everything.
#[test]
fn back_up_and_export_refuse_the_user_database_itself() {
    let f = Fixture::new("own-file");
    let mut s = f.session();
    romans(&mut s);
    let before = dump(&s);
    let db = f.file("user.db");
    for dest in [
        db.clone(),
        f.root.join(".").join("user.db"),
        f.root.join("library").join("..").join("user.db"),
        f.file("user.db-wal"),
        f.file("user.db-shm"),
    ] {
        for command in ["data.back_up", "data.export"] {
            let err = s.run(command, &path(&dest)).unwrap_err().to_string();
            assert!(
                err.contains("where your data is kept"),
                "{command} {}: {err}",
                dest.display()
            );
        }
    }
    assert_eq!(dump(&s), before);
    s.run("annotation.note", &json!({ "body": "still saving" }))
        .unwrap();
    let after = dump(&s);
    drop(s);
    let s = f.session();
    assert_eq!(dump(&s), after);
    assert_eq!(s.annotations(None).unwrap().len(), 4);
}

#[test]
fn a_refused_restore_changes_nothing_and_keeps_undo() {
    let f = Fixture::new("refused");
    let mut s = f.session();
    s.run("annotation.note", &json!({ "body": "keep me" }))
        .unwrap();
    let before = dump(&s);
    let junk = f.file("junk.obsbackup");
    std::fs::write(&junk, "not a backup").unwrap();
    assert!(s.restore_preview(&junk).is_err());
    let err = s.run("data.restore", &path(&junk)).unwrap_err();
    assert!(err.to_string().contains("not a backup"), "{err}");
    assert_eq!(dump(&s), before);
    assert!(s.last_restore().is_none());
    assert!(
        !s.kept_backups()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
    // Undo history survived: the Note can still be undone.
    s.run("edit.undo", &Value::Null).unwrap();
    assert!(s.annotations(None).unwrap().is_empty());
}

#[test]
fn export_everything_writes_markdown_notes_and_json_of_every_table() {
    let f = Fixture::new("export");
    let mut s = f.session();
    romans(&mut s);
    s.run("layout.save", &json!({ "name": "Study" })).unwrap();
    s.run("view.text_larger", &Value::Null).unwrap();
    let dest = f.file("data.zip");
    let out = s.run("data.export", &path(&dest)).unwrap();
    assert_eq!(out["annotations"], 3);

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    let mut read = |name: &str| {
        let mut text = String::new();
        zip.by_name(name)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .read_to_string(&mut text)
            .unwrap();
        text
    };
    let romans_md = read("notes/Romans.md");
    assert!(romans_md.starts_with("# Romans"), "{romans_md}");
    assert!(romans_md.contains("while we were yet sinners"));
    assert!(romans_md.contains("Romans 8:28"), "{romans_md}");
    // The default Notebook has its file too, even empty.
    let names: Vec<String> = out["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(names.iter().filter(|n| n.starts_with("notes/")).count(), 2);
    for expected in [
        "manifest.json",
        "README.md",
        "annotations.json",
        "layouts.json",
        "last-session.json",
        "favorites.json",
        "history.json",
        "reading-plans.json",
        "preferred-order.json",
        "settings.json",
    ] {
        assert!(names.iter().any(|n| n == expected), "{expected}: {names:?}");
    }
    let json = |name: &str, read: &mut dyn FnMut(&str) -> String| -> Value {
        serde_json::from_str(&read(name)).unwrap()
    };
    let manifest = json("manifest.json", &mut read);
    assert_eq!(manifest["export_version"], backups::DATA_EXPORT_VERSION);
    assert_eq!(
        manifest["user_database_version"],
        obs_store::USER_SCHEMA_VERSION
    );
    let annotations = json("annotations.json", &mut read);
    assert_eq!(annotations["annotations"].as_array().unwrap().len(), 3);
    assert_eq!(annotations["notebooks"].as_array().unwrap().len(), 2);
    let layouts = json("layouts.json", &mut read);
    assert_eq!(layouts[0]["name"], "Study");
    assert!(layouts[0]["snapshot"]["tabs"].is_array(), "{layouts}");
    let settings = json("settings.json", &mut read);
    assert!(settings["view"].is_object(), "{settings}");
    assert!(s.data_export_file_name().ends_with(".zip"));
    assert!(s.backup_file_name().ends_with(".obsbackup"));
}

#[test]
fn back_up_and_restore_need_the_user_database() {
    let mut s = Session::new(JobMode::Inline);
    for id in [
        "data.back_up",
        "data.restore",
        "data.export",
        "data.undo_restore",
    ] {
        assert!(!s.is_enabled(id), "{id}");
    }
    assert!(s.kept_backups().is_empty());
    // The Tab opens anyway, and says so.
    let tab = s.run("data.backups", &Value::Null).unwrap()["tab"]
        .as_u64()
        .unwrap();
    let t = s.workspace.tab(TabId(tab)).unwrap();
    assert_eq!(t.kind, TabKind::Backups);
    assert_eq!(t.title, BACKUPS_TITLE);
    assert!(
        MENUS
            .iter()
            .find(|m| m.title == "Tools")
            .unwrap()
            .items
            .contains(&MenuItem::Command("data.backups"))
    );
}
