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
    assert_eq!(restored.custom_theme, launched.custom_theme);
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

/// A User database opened through a symbolic link keeps SQLite's files beside the link's
/// target: Back up now and Export everything refuse those as well.
#[cfg(unix)]
#[test]
fn back_up_and_export_refuse_the_real_files_of_a_linked_user_database() {
    let f = Fixture::new("own-file-link");
    std::fs::create_dir_all(f.file("real")).unwrap();
    let real = f.file("real").join("real.db");
    obs_store::UserStore::open(&real).unwrap();
    std::os::unix::fs::symlink(&real, f.file("user.db")).unwrap();
    let mut s = f.session();
    romans(&mut s);
    let before = dump(&s);
    for dest in [
        real.clone(),
        f.file("real").join("real.db-wal"),
        f.file("real").join("real.db-shm"),
        f.file("real").join("real.db-journal"),
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
    // A Resource removed since: only the User database remembers its name.
    s.user
        .as_ref()
        .unwrap()
        .save_resource_names(&[("gone".into(), "GONE".into(), "A Removed Bible".into())])
        .unwrap();
    assert!(s.library.get("gone").is_none());
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
    assert!(
        !annotations["highlight_styles"]
            .as_array()
            .unwrap()
            .is_empty(),
        "{annotations}"
    );
    assert!(
        !annotations["highlight_palettes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let names = annotations["resource_names"].as_array().unwrap();
    assert!(
        names.contains(&json!({
            "resource_id": "gone",
            "abbreviation": "GONE",
            "title": "A Removed Bible",
        })),
        "{names:?}"
    );
    assert!(
        names.iter().any(|n| n["resource_id"] == "kjv"),
        "the Resources in the library are remembered too: {names:?}"
    );
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
    assert_eq!(crate::menu_of("data.backups"), Some("File"));
}

/// A copy of the Backup `from` written to `to`, with `damage` done to it first through the
/// store (which keeps whatever text it is given).
fn damaged_copy(
    f: &Fixture,
    from: &std::path::Path,
    to: &std::path::Path,
    damage: impl FnOnce(&obs_store::UserStore),
) {
    let db = f.root.join("damaging").join("user.db");
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    std::fs::copy(from, &db).unwrap();
    let (store, _) = obs_store::UserStore::open(&db).unwrap();
    damage(&store);
    store.back_up(to).unwrap();
}

/// #258: a Backup holding a Layout, a Favorite or a setting kept as JSON that the app could
/// not read is refused by the preview and by restore, naming each, before anything is
/// touched; a restore would otherwise drop or default them silently.
#[test]
fn a_backup_with_an_unreadable_layout_setting_or_favorite_is_refused_naming_each() {
    let f = Fixture::new("unreadable");
    let mut s = f.session();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    let backup = f.file("damaged.obsbackup");
    damaged_copy(&f, &good, &backup, |db| {
        db.set_preference("search.saved", "not JSON").unwrap();
        db.set_preference("library.collections", "{\"oops\":1}")
            .unwrap();
        // A Log level the app does not know would be back at its default.
        db.set_preference("log_level", "not JSON").unwrap();
        db.save_layout("Broken", "not JSON").unwrap();
        db.add_favorite("", "John 3:16", "nowhere").unwrap();
        db.record_visit("John 3:17", "nowhere").unwrap();
    });
    s.run(
        "annotation.note",
        &json!({ "body": "made after the backup" }),
    )
    .unwrap();
    let before = dump(&s);

    let refused = s.restore_preview(&backup).unwrap_err();
    for named in [
        "the Layout “Broken”",
        "the Favorite “John 3:16”",
        "1 History visit",
        "the setting `search.saved` (Saved searches)",
        "the setting `library.collections` (Collections)",
        "the setting `log_level` (Log level)",
        "Nothing was changed",
    ] {
        assert!(refused.contains(named), "{named}: {refused}");
    }
    let err = s.run("data.restore", &path(&backup)).unwrap_err();
    assert!(err.to_string().contains("the Layout “Broken”"), "{err}");
    assert_eq!(dump(&s), before);
    assert!(s.last_restore().is_none());
    assert!(
        !s.kept_backups()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
    // The undamaged Backup still restores.
    s.restore_preview(&good).unwrap();
    s.run("data.restore", &path(&good)).unwrap();
}

/// #258: restore puts back the copy of the Backup it checked. A file replaced between the
/// checks and the restore (here by a damaged Backup the checks would refuse) is not read
/// again, so nothing unchecked is restored and then defaulted silently.
#[test]
fn restore_puts_back_the_backup_it_checked_even_if_the_file_changes_meanwhile() {
    let f = Fixture::new("swapped");
    let good = f.file("good.obsbackup");
    {
        let mut s = f.session();
        s.run(
            "annotation.note",
            &json!({ "body": "kept in the good Backup" }),
        )
        .unwrap();
        s.run("data.back_up", &path(&good)).unwrap();
    }
    let damaged = f.file("damaged.obsbackup");
    damaged_copy(&f, &good, &damaged, |db| {
        db.set_preference("search.saved", "not JSON").unwrap();
        db.save_layout("Broken", "not JSON").unwrap();
    });
    assert!(f.session().restore_preview(&damaged).is_err());

    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&f.root.join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&f.root.join("user.db")).unwrap();
    s.attach_user(Box::new(crate::testing::WritesFail {
        swap_backup: Some(damaged),
        ..crate::testing::WritesFail::visits(store, 0)
    }));
    s.run(
        "annotation.note",
        &json!({ "body": "made after the backup" }),
    )
    .unwrap();

    s.restore_backup(&good).unwrap();
    // The file now is the damaged Backup; what was restored is the good one.
    let (on_disk, _) = obs_store::UserStore::open_backup(&good).unwrap();
    assert_eq!(
        on_disk.preference("search.saved").unwrap().as_deref(),
        Some("not JSON")
    );
    let user = s.user.as_ref().unwrap();
    assert_ne!(
        user.preference("search.saved").unwrap().as_deref(),
        Some("not JSON")
    );
    assert!(user.layout("Broken").unwrap().is_none());
    let notes = dump(&s);
    assert!(notes.contains("kept in the good Backup"), "{notes}");
    assert!(!notes.contains("made after the backup"), "{notes}");
}

/// #258: a setting read field by field (View, Toolbar, Keyboard shortcuts, Update check)
/// that is valid JSON but holds a field the app could not read is refused too; the loader
/// would leave that field at its default or drop the key.
#[test]
fn a_backup_with_an_unreadable_field_of_a_setting_is_refused_naming_it() {
    let f = Fixture::new("unreadable-field");
    let mut s = f.session();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    let backup = f.file("damaged.obsbackup");
    damaged_copy(&f, &good, &backup, |db| {
        db.set_preference("view", r#"{"theme":"dark","text_size":"large"}"#)
            .unwrap();
        db.set_preference("toolbar", r#"{"launchers":[{"id":"nowhere"}]}"#)
            .unwrap();
        db.set_preference("key_bindings", r#"{"annotation.note":42}"#)
            .unwrap();
        db.set_preference("updates", r#"{"auto_check":"false"}"#)
            .unwrap();
    });
    s.run(
        "annotation.note",
        &json!({ "body": "made after the backup" }),
    )
    .unwrap();
    let before = dump(&s);

    let refused = s.restore_preview(&backup).unwrap_err();
    for named in [
        "the setting `view` (View settings",
        "the setting `toolbar` (Toolbar)",
        "the setting `key_bindings` (Keyboard shortcuts)",
        "the setting `updates` (Update check settings)",
        "Nothing was changed",
    ] {
        assert!(refused.contains(named), "{named}: {refused}");
    }
    let err = s.run("data.restore", &path(&backup)).unwrap_err();
    assert!(err.to_string().contains("Keyboard shortcuts"), "{err}");
    assert_eq!(dump(&s), before);
    assert!(s.last_restore().is_none());
    assert!(
        !s.kept_backups()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
}

/// #258: Keyboard shortcuts holding a Selection key under its old id and its new one would
/// lose one binding on restore (the loader applies the old id to the new one); a Backup with
/// them is refused by the preview and by restore before any Safety backup, naming the setting.
#[test]
fn a_backup_with_a_shortcut_under_its_old_and_new_id_is_refused() {
    let f = Fixture::new("shortcut-aliases");
    let mut s = f.session();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    let backup = f.file("damaged.obsbackup");
    damaged_copy(&f, &good, &backup, |db| {
        db.set_preference(
            "key_bindings",
            r#"{"selection.highlight_1":"Alt+2","selection.highlight_yellow":"Alt+3"}"#,
        )
        .unwrap();
    });
    s.run(
        "annotation.note",
        &json!({ "body": "made after the backup" }),
    )
    .unwrap();
    let before = dump(&s);

    let refused = s.restore_preview(&backup).unwrap_err();
    for named in [
        "the setting `key_bindings` (Keyboard shortcuts)",
        "Nothing was changed",
    ] {
        assert!(refused.contains(named), "{named}: {refused}");
    }
    let err = s.run("data.restore", &path(&backup)).unwrap_err();
    assert!(err.to_string().contains("Keyboard shortcuts"), "{err}");
    assert_eq!(dump(&s), before);
    assert!(s.last_restore().is_none());
    assert!(
        !s.kept_backups()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
}

/// #258: a Home or Guide arrangement holding a card or section its loader drops (repeated,
/// unknown, not that Guide's) and an Import report naming another Resource than its key are
/// refused before any Safety backup, naming them; the loaders would silently drop them.
#[test]
fn a_backup_with_records_the_loaders_drop_is_refused_naming_them() {
    let f = Fixture::new("dropped-records");
    let mut s = f.session();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    let backup = f.file("damaged.obsbackup");
    damaged_copy(&f, &good, &backup, |db| {
        db.set_preference(
            "home",
            r#"{"cards":[{"id":"verse_of_the_day","hidden":false},{"id":"verse_of_the_day","hidden":true}]}"#,
        )
        .unwrap();
        db.set_preference(
            "guide-arrangement/passage_guide",
            r#"{"sections":[{"id":"commentaries"},{"id":"commentaries","hidden":true}]}"#,
        )
        .unwrap();
        // A report of `web` under the key of `kjv`, and one under its own key.
        let report = json!({
            "resource_id": "web", "source": "web.zip", "phase": "Complete", "done": 1,
            "total": 1, "report": null, "index": "Deferred", "error": null,
        })
        .to_string();
        db.set_preference("import-report/kjv", &report).unwrap();
        db.set_preference("import-report/web", &report).unwrap();
    });
    let before = dump(&s);

    let refused = s.restore_preview(&backup).unwrap_err();
    for named in [
        "the setting `home` (Home)",
        "the setting `guide-arrangement/passage_guide` (Guide arrangement)",
        "the setting `import-report/kjv` (Import report)",
        "Nothing was changed",
    ] {
        assert!(refused.contains(named), "{named}: {refused}");
    }
    assert!(!refused.contains("import-report/web"), "{refused}");
    let err = s.run("data.restore", &path(&backup)).unwrap_err();
    assert!(err.to_string().contains("(Home)"), "{err}");
    assert_eq!(dump(&s), before);
    assert!(s.last_restore().is_none());
    assert!(
        !s.kept_backups()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
}

/// #258: a Reading plan whose start day cannot be read is not loaded, and a Reading whose
/// Passages or day cannot be read is left out of its plan; a Backup holding either is refused
/// before any Safety backup, naming the plan or the Reading.
#[test]
fn a_backup_with_a_reading_plan_or_reading_the_loader_drops_is_refused_naming_it() {
    let f = Fixture::new("unreadable-plan");
    let mut s = f.session();
    s.run(
        "reading_plan.create",
        &json!({ "template": "custom", "books": "Jude", "days": 3, "name": "Jude" }),
    )
    .unwrap();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    let backup = f.file("damaged.obsbackup");
    damaged_copy(&f, &good, &backup, |db| {
        let plan = |name: &str, start: &str| obs_store::ReadingPlanRow {
            id: 0,
            name: name.to_string(),
            template: "custom".to_string(),
            bible_id: "kjv".to_string(),
            start_day: start.to_string(),
            weekdays: 0b111_1111,
            created_at: 0,
        };
        let reading = |passages: &str, day: &str| obs_store::PlanReadingRow {
            position: 0,
            passages: passages.to_string(),
            day: day.to_string(),
            read_at: None,
        };
        db.add_reading_plan(
            &plan("Broken start", "not-a-date"),
            &[reading("Gen 1", "2024-01-01")],
        )
        .unwrap();
        db.add_reading_plan(
            &plan("Gospel", "2024-01-01"),
            &[
                reading("Matt 1", "2024-01-01"),
                reading("Nowhere 9", "2024-01-02"),
                reading("Matt 3", "someday"),
            ],
        )
        .unwrap();
    });
    s.run(
        "annotation.note",
        &json!({ "body": "made after the backup" }),
    )
    .unwrap();
    let before = dump(&s);
    let plans_before = s.reading_plans().to_vec();

    let refused = s.restore_preview(&backup).unwrap_err();
    for named in [
        "the Reading plan “Broken start”",
        "Reading 2 of the Reading plan “Gospel”",
        "Reading 3 of the Reading plan “Gospel”",
        "Nothing was changed",
    ] {
        assert!(refused.contains(named), "{named}: {refused}");
    }
    for kept in ["Reading 1 of", "“Jude”"] {
        assert!(!refused.contains(kept), "{kept}: {refused}");
    }
    let err = s.run("data.restore", &path(&backup)).unwrap_err();
    assert!(
        err.to_string().contains("the Reading plan “Broken start”"),
        "{err}"
    );
    assert_eq!(dump(&s), before);
    assert_eq!(s.reading_plans(), plans_before);
    assert!(s.last_restore().is_none());
    assert!(
        !s.kept_backups()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
    // The plan as the app wrote it still restores, every Reading kept.
    s.restore_preview(&good).unwrap();
    s.run("data.restore", &path(&good)).unwrap();
    assert_eq!(s.reading_plans(), plans_before);
}

/// #258: a Layout holding two Tabs with one id reads, but restoring it keeps one Tab per id
/// and drops the other; a Backup with one is refused before any Safety backup, naming it.
#[test]
fn a_backup_with_a_layout_of_two_tabs_with_one_id_is_refused_naming_it() {
    let f = Fixture::new("repeated-tab");
    let mut s = f.session();
    for id in ["kjv", "web"] {
        s.run("resource.open", &json!({ "id": id })).unwrap();
    }
    s.run("layout.save", &json!({ "name": "Study" })).unwrap();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    let backup = f.file("damaged.obsbackup");
    damaged_copy(&f, &good, &backup, |db| {
        let json = db.layout("Study").unwrap().unwrap();
        let mut snap = WorkspaceSnapshot::from_json(&json).unwrap();
        let kjv = snap
            .tabs
            .iter()
            .position(|t| t.kind.resource_id() == Some("kjv"))
            .unwrap();
        let web = snap
            .tabs
            .iter()
            .position(|t| t.kind.resource_id() == Some("web"))
            .unwrap();
        // The WEB Tab takes the KJV Tab's id, which its Panel holds once.
        snap.tabs[web].id = snap.tabs[kjv].id;
        let keep: std::collections::BTreeSet<u64> = snap.tabs.iter().map(|t| t.id).collect();
        snap.layout = snap.layout.and_then(|l| l.retain_tabs(&keep));
        assert!(WorkspaceSnapshot::from_json(&snap.to_json()).is_some());
        db.save_layout("Study", &snap.to_json()).unwrap();
    });
    s.run(
        "annotation.note",
        &json!({ "body": "made after the backup" }),
    )
    .unwrap();
    let before = dump(&s);

    let refused = s.restore_preview(&backup).unwrap_err();
    for named in ["the Layout “Study”", "Nothing was changed"] {
        assert!(refused.contains(named), "{named}: {refused}");
    }
    let err = s.run("data.restore", &path(&backup)).unwrap_err();
    assert!(err.to_string().contains("the Layout “Study”"), "{err}");
    assert_eq!(dump(&s), before);
    assert!(s.last_restore().is_none());
    assert!(
        !s.kept_backups()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
    // The Layout as the app saved it still restores.
    s.restore_preview(&good).unwrap();
    s.run("data.restore", &path(&good)).unwrap();
}

/// #258: a Layout is compared whole, its stored JSON first: a field this version does not
/// know inside a Tab or on the Layout itself (which reading would drop) and a remembered
/// Panel under a Tab kind it does not know (which restoring would drop) are refused, naming
/// the Layout. What a launch leaves out is not: a Tab of a Resource not in the library, and a
/// focused Tab or remembered or last Panel the Layout does not have, which fall back.
#[test]
fn a_backup_with_a_layout_the_app_would_not_keep_whole_is_refused_naming_it() {
    let f = Fixture::new("layout-whole");
    let mut s = f.session();
    for id in ["kjv", "web"] {
        s.run("resource.open", &json!({ "id": id })).unwrap();
    }
    s.run("layout.save", &json!({ "name": "Study" })).unwrap();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    // The Layout as the app arranges it: both Tabs in Panel 5, remembered for Resources.
    let mut arranged =
        WorkspaceSnapshot::from_json(&s.user.as_ref().unwrap().layout("Study").unwrap().unwrap())
            .unwrap();
    let tabs: Vec<u64> = arranged.tabs.iter().map(|t| t.id).collect();
    arranged.layout = Some(LayoutNode::Panel {
        panel: 5,
        tabs: tabs.clone(),
        active: Some(tabs[0]),
    });
    arranged.remembered = vec![("\"Resource\"".into(), 5)];
    arranged.last_reading_panel = Some(5);
    arranged.focused = Some(tabs[0]);

    let changed_layout = |name: &str, layout: String| {
        let backup = f.file(&format!("{name}.obsbackup"));
        damaged_copy(&f, &good, &backup, |db| {
            db.save_layout("Study", &layout).unwrap();
        });
        backup
    };
    let raw = serde_json::to_value(&arranged).unwrap();
    let mut refused_cases: Vec<(&str, Value)> = Vec::new();
    let mut v = raw.clone();
    v["tabs"][0]["reading_layout"]["zz_later_field"] = json!(7);
    refused_cases.push(("a later field of a Tab's Reading layout", v));
    let mut v = raw.clone();
    v["zz_later_field"] = json!(7);
    refused_cases.push(("a later field of the Layout", v));
    let mut v = raw.clone();
    v["remembered"]
        .as_array_mut()
        .unwrap()
        .push(json!(["\"ZzLaterKind\"", 5]));
    refused_cases.push(("a Panel remembered for a later Tab kind", v));
    for (what, layout) in refused_cases {
        assert!(
            WorkspaceSnapshot::from_json(&layout.to_string()).is_some(),
            "{what}: reads"
        );
        let backup = changed_layout("refused", layout.to_string());
        let before = dump(&s);
        let refused = s.restore_preview(&backup).unwrap_err();
        for named in ["the Layout “Study”", "Nothing was changed"] {
            assert!(refused.contains(named), "{what}: {named}: {refused}");
        }
        let err = s.run("data.restore", &path(&backup)).unwrap_err();
        assert!(
            err.to_string().contains("the Layout “Study”"),
            "{what}: {err}"
        );
        assert_eq!(dump(&s), before, "{what}");
        assert!(s.last_restore().is_none(), "{what}");
    }

    // As arranged, and with what a launch leaves out, it restores, the Layout as stored.
    let mut fallback = arranged.clone();
    let mut gone = fallback.tabs[0].clone();
    gone.id = 900;
    gone.kind = TabKind::Resource("gone".into());
    fallback.tabs.push(gone);
    if let Some(LayoutNode::Panel { tabs, .. }) = fallback.layout.as_mut() {
        tabs.push(900);
    }
    fallback.focused = Some(901);
    fallback.remembered.push(("\"Library\"".into(), 902));
    fallback.last_other_panel = Some(902);
    for layout in [arranged.to_json(), fallback.to_json()] {
        let backup = changed_layout("restores", layout.clone());
        s.restore_preview(&backup).unwrap();
        s.run("data.restore", &path(&backup)).unwrap();
        let user = s.user.as_ref().unwrap();
        assert_eq!(user.layout("Study").unwrap().unwrap(), layout);
        s.run("layout.restore", &json!({ "name": "Study" }))
            .unwrap();
        assert_eq!(s.workspace.snapshot().layout, arranged.layout, "{layout}");
    }
}

/// What the trial restore would not keep of the Backup `good` with the setting `key` set to
/// `value` (written to a copy beside it): the lines a refusal names.
fn unkept_setting(
    f: &Fixture,
    s: &Session,
    good: &std::path::Path,
    key: &str,
    value: &str,
) -> Vec<String> {
    let backup = f.file("setting.obsbackup");
    damaged_copy(f, good, &backup, |db| {
        db.set_preference(key, value).unwrap()
    });
    let (opened, _) = s.user.as_ref().unwrap().open_backup(&backup).unwrap();
    s.trial_restore(&*opened).unwrap()
}

/// Home and Guide arrangements are refused for each card or section their loaders drop, and
/// kept when each is known and held once. A key no loader reads (a Guide this version does
/// not have) is restored as it is.
#[test]
fn arrangements_refuse_cards_and_sections_their_loaders_drop() {
    let f = Fixture::new("arrangements");
    let mut s = f.session();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    for (key, json) in [
        (
            "home",
            r#"{"cards":[{"id":"verse_of_the_day","hidden":true}]}"#,
        ),
        ("home", r#"{"open_at_launch":true}"#),
        (
            "guide-arrangement/passage_guide",
            r#"{"sections":[{"id":"cross_references","collapsed":true},{"id":"commentaries"}]}"#,
        ),
        ("import-report/kjv", "null"),
        (
            "guide-arrangement/nowhere",
            r#"{"sections":[{"id":"commentaries"}]}"#,
        ),
    ] {
        let unkept = unkept_setting(&f, &s, &good, key, json);
        assert!(unkept.is_empty(), "{key} {json}: {unkept:?}");
    }
    for (key, json) in [
        ("home", r#"{"cards":[{"id":"nowhere"}]}"#),
        (
            "guide-arrangement/passage_guide",
            r#"{"sections":[{"id":"nowhere"}]}"#,
        ),
        (
            "guide-arrangement/word_study",
            r#"{"sections":[{"id":"commentaries"}]}"#,
        ),
        (
            "guide-arrangement/passage_guide",
            r#"{"sections":[{"id":"cross_references","collection":"Bibles"}]}"#,
        ),
    ] {
        let unkept = unkept_setting(&f, &s, &good, key, json);
        assert_eq!(unkept.len(), 1, "{key} {json}: {unkept:?}");
        assert!(unkept[0].contains(&format!("`{key}`")), "{unkept:?}");
    }
}

/// Each setting refuses a field its loader would default, drop or clamp, and keeps what the
/// app wrote before (#37's View record, a Text size saved while it was the default, old
/// Selection key ids, no skipped release).
#[test]
fn settings_refuse_fields_their_loaders_would_change_and_keep_older_records() {
    let f = Fixture::new("fields");
    let mut s = f.session();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    let history = |n: usize| {
        let entries: Vec<Value> = (0..n)
            .map(
                |i| json!({ "search": { "mode": "bible", "query": format!("love {i}") }, "at": i }),
            )
            .collect();
        serde_json::to_string(&entries).unwrap()
    };
    for (key, json) in [
        (
            "view",
            r#"{"text_size":18,"dark_page":true,"verse_numbers":false}"#.to_string(),
        ),
        ("view", r#"{"verse_numbers":true}"#.to_string()),
        (
            "view",
            r#"{"theme":"light","fonts":{"greek":{"family":"Gentium"}}}"#.to_string(),
        ),
        ("view", r#"{"text_size":40,"line_spacing":2}"#.to_string()),
        // Saved while 17 was the default (before #241): the default now.
        ("view", r#"{"text_size":17}"#.to_string()),
        ("view", r#"{"text_size":17,"text_size_default":17}"#.to_string()),
        // Fields at their defaults say nothing to lose.
        ("view", r#"{"some_later_field":false}"#.to_string()),
        (
            "toolbar",
            r#"{"position":"left","launchers":[{"id":"tools","shown":false}]}"#.to_string(),
        ),
        (
            "key_bindings",
            r#"{"selection.highlight_yellow":"Ctrl+1","edit.undo":null}"#.to_string(),
        ),
        ("updates", r#"{"auto_check":false,"skipped":null}"#.to_string()),
        ("updates", r#"{"skipped":"0.2.0"}"#.to_string()),
        ("tour", r#"{"shown":true}"#.to_string()),
        (
            "custom-theme",
            r##"{"format":"openbible-theme","version":1,"base":"high_contrast_dark","colors":{"ink":"#ffffff"}}"##.to_string(),
        ),
        ("search.history", history(SEARCH_HISTORY_LIMIT)),
        ("log_level", "debug".to_string()),
        ("search.word_forms", "true".to_string()),
    ] {
        let unkept = unkept_setting(&f, &s, &good, key, &json);
        assert!(unkept.is_empty(), "{key} {json}: {unkept:?}");
    }
    for (key, json) in [
        ("view", "[]".to_string()),
        ("view", r#"{"theme":"purple"}"#.to_string()),
        ("view", r#"{"red_letter":"yes"}"#.to_string()),
        ("view", r#"{"verse_numbers":"sideways"}"#.to_string()),
        ("view", r#"{"fonts":{"latin":{"family":3}}}"#.to_string()),
        ("view", r#"{"reading_layout":{"flow":"sideways"}}"#.to_string()),
        // Read as no Reading layout, so the default.
        ("view", r#"{"reading_layout":null}"#.to_string()),
        // A field this version does not know would be gone at the next save.
        ("view", r#"{"some_later_field":1}"#.to_string()),
        ("view", r#"{"text_size":41}"#.to_string()),
        ("view", r#"{"line_spacing":0.9}"#.to_string()),
        ("view", r#"{"line_spacing":2.1}"#.to_string()),
        // Too small to read: reset to the default, with a notice, but reset all the same.
        ("view", r#"{"text_size":8}"#.to_string()),
        ("toolbar", r#"{"position":"middle"}"#.to_string()),
        (
            "toolbar",
            r#"{"launchers":[{"id":"tools"},{"id":"tools"}]}"#.to_string(),
        ),
        (
            "toolbar",
            r#"{"launchers":[{"id":"tools","shown":"no"}]}"#.to_string(),
        ),
        ("toolbar", r#"{"launchers":[{"id":"nowhere"}]}"#.to_string()),
        ("key_bindings", r#"{"annotation.note":42}"#.to_string()),
        (
            "key_bindings",
            r#"{"annotation.note":"Ctrl+Nowhere"}"#.to_string(),
        ),
        // An old Selection key id beside its new one: the loader keeps one of the two.
        (
            "key_bindings",
            r#"{"selection.highlight_1":"Alt+2","selection.highlight_yellow":"Alt+3"}"#
                .to_string(),
        ),
        ("updates", r#"{"auto_check":"false"}"#.to_string()),
        ("updates", r#"{"skipped":1}"#.to_string()),
        ("tour", r#"{"shown":"yes"}"#.to_string()),
        ("tour", "not JSON".to_string()),
        (
            "custom-theme",
            r#"{"format":"x","version":1,"base":"light"}"#.to_string(),
        ),
        (
            "custom-theme",
            r##"{"format":"openbible-theme","version":1,"base":"light","colors":{"nowhere":"#000000"}}"##.to_string(),
        ),
        ("search.history", history(SEARCH_HISTORY_LIMIT + 1)),
        ("search.saved", "not JSON".to_string()),
        ("search.recent.notes", r#"["grace", 3]"#.to_string()),
        ("command-box.recent", "{}".to_string()),
        ("log_level", "loud".to_string()),
        ("search.word_forms", "yes".to_string()),
        ("reading.visual_filters_next_id", "many".to_string()),
        ("starter-library", r#"{"oops":1}"#.to_string()),
        ("resource-attribution/kjv/abc", r#"{"credit":3}"#.to_string()),
    ] {
        let unkept = unkept_setting(&f, &s, &good, key, &json);
        assert_eq!(unkept.len(), 1, "{key} {json}: {unkept:?}");
        assert!(unkept[0].contains(&format!("`{key}`")), "{unkept:?}");
    }
}

/// #258: a View setting whose Text size or Line spacing the loader would clamp, and a Search
/// history longer than the app keeps (the loader would drop the oldest), are refused by the
/// preview and by restore before any Safety backup.
#[test]
fn a_backup_with_view_values_or_history_the_loaders_would_change_is_refused() {
    let f = Fixture::new("changed-values");
    let mut s = f.session();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    let backup = f.file("damaged.obsbackup");
    let entries: Vec<Value> = (0..=SEARCH_HISTORY_LIMIT)
        .map(|i| json!({ "search": { "mode": "bible", "query": format!("love {i}") }, "at": i }))
        .collect();
    let too_long = serde_json::to_string(&entries).unwrap();
    damaged_copy(&f, &good, &backup, |db| {
        db.set_preference("view", r#"{"text_size":999,"line_spacing":9}"#)
            .unwrap();
        db.set_preference("search.history", &too_long).unwrap();
    });
    s.run(
        "annotation.note",
        &json!({ "body": "made after the backup" }),
    )
    .unwrap();
    let before = dump(&s);

    let refused = s.restore_preview(&backup).unwrap_err();
    for named in [
        "the setting `view` (View settings)",
        "the setting `search.history` (Search history)",
        "Nothing was changed",
    ] {
        assert!(refused.contains(named), "{named}: {refused}");
    }
    let err = s.run("data.restore", &path(&backup)).unwrap_err();
    assert!(err.to_string().contains("(Search history)"), "{err}");
    assert_eq!(dump(&s), before);
    assert!(s.last_restore().is_none());
    assert!(
        !s.kept_backups()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
}

/// The Session after using every setting, record and list the app keeps in the User
/// database that can be reached here.
fn write_everything(s: &mut Session) {
    for setting in crate::settings::SETTINGS.iter().filter(|s| s.has_reset()) {
        crate::settings_tests::change(s, setting.id);
    }
    let run = |s: &mut Session, id: &str, p: Value| {
        s.run(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    };
    run(
        s,
        "theme.set_color",
        json!({ "role": "lapis", "color": "#FFD000" }),
    );
    run(
        s,
        "keys.set",
        json!({ "command": "view.text_larger", "key": "Ctrl+ArrowUp" }),
    );
    run(s, "home.hide_card", json!({ "card": "verse_of_the_day" }));
    run(
        s,
        "guide.hide_section",
        json!({ "guide": "passage_guide", "section": "commentaries" }),
    );
    run(
        s,
        "copy.option",
        json!({ "name": "layout", "value": "verse_per_line" }),
    );
    run(s, "copy.preset_save", json!({ "name": "Handout" }));
    run(
        s,
        "collection.save",
        json!({ "name": "Bibles", "rules": "type:bible" }),
    );
    run(s, "resource.tags", json!({ "id": "kjv", "tags": "older" }));
    run(
        s,
        "filter.save",
        json!({ "name": "Grace", "query": "grace", "look": "background", "color": "#ffd6aa" }),
    );
    run(s, "help.tour", json!({}));
    run(s, "tour.skip", json!({}));
    run(s, "annotation.note", json!({ "body": "grace" }));
    let out = s
        .run("search.run", &json!({ "text": "grace", "mode": "notes" }))
        .unwrap();
    run(
        s,
        "search.save",
        json!({ "tab": out["tab"], "name": "Grace" }),
    );
    s.set_search_word_forms(true);
    run(s, "command_box.run", json!({ "text": "Matt 5:43" }));
    let bible = s.run("resource.open", &json!({ "id": "kjv" })).unwrap()["tab"].clone();
    run(s, "tab.go", json!({ "tab": bible, "text": "Matt 5:43" }));
    run(s, "favorite.add", json!({ "tab": bible }));
    s.record_history();
    run(s, "layout.save", json!({ "name": "Study" }));
    run(
        s,
        "reading_plan.create",
        json!({ "template": "custom", "books": "Jude", "days": 3 }),
    );
}

/// Settings the trial restore does not compare: each names another record and falls back to
/// the default when that record is gone, as at launch (see `backup_check.rs`).
const NOT_COMPARED: &[&str] = &["highlights.palette", "annotations.last_notebook"];

/// Everything the app itself writes is kept by the trial restore: a Backup of a Session that
/// has used every setting it can reach here previews and restores, and every setting it wrote
/// was compared.
#[test]
fn a_backup_of_what_the_app_writes_passes_the_restore_check() {
    let f = Fixture::new("app-writes");
    let mut s = f.session();
    write_everything(&mut s);
    let backup = f.file("everything.obsbackup");
    s.run("data.back_up", &path(&backup)).unwrap();

    let (opened, _) = s.user.as_ref().unwrap().open_backup(&backup).unwrap();
    assert_eq!(s.trial_restore(&*opened).unwrap(), Vec::<String>::new());
    let stored: std::collections::BTreeMap<String, String> = s
        .user
        .as_ref()
        .unwrap()
        .preferences()
        .unwrap()
        .into_iter()
        .collect();
    let compared: Vec<String> = s
        .held_settings(&stored)
        .into_iter()
        .map(|h| h.key)
        .collect();
    for key in stored.keys() {
        assert!(
            compared.contains(key) || NOT_COMPARED.contains(&key.as_str()),
            "the trial restore does not compare `{key}`"
        );
    }
    let preview = s.restore_preview(&backup).unwrap();
    assert_eq!(preview.backup.layouts, ["Study"]);
    s.run("data.restore", &path(&backup)).unwrap();
    let stored = crate::backup_check::StoredRecords::read(&*opened).unwrap();
    assert_eq!(s.unkept_records(&stored), Vec::<String>::new());
}

/// Every setting the engine names with a constant is compared by the trial restore or is a
/// pointer it leaves alone ([`NOT_COMPARED`]), so a new setting cannot be missed.
#[test]
fn every_setting_the_engine_keeps_is_compared_by_the_trial_restore() {
    let f = Fixture::new("every-setting");
    let mut s = f.session();
    let attribution = "resource-attribution/kjv/abc";
    let stored: std::collections::BTreeMap<String, String> = [(
        attribution.to_string(),
        r#"{"credit":"","confirmed_at":1}"#.to_string(),
    )]
    .into();
    let compared: Vec<String> = s
        .held_settings(&stored)
        .into_iter()
        .map(|h| h.key)
        .collect();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut keys = Vec::new();
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        for line in std::fs::read_to_string(&path).unwrap().lines() {
            let Some((name, rest)) = line
                .trim_start()
                .trim_start_matches("pub ")
                .trim_start_matches("pub(crate) ")
                .strip_prefix("const ")
                .and_then(|l| l.split_once(": &str = \""))
            else {
                continue;
            };
            if name.ends_with("_PREF") || name.ends_with("_PREF_PREFIX") || name.ends_with("_KEY") {
                keys.push(rest.trim_end_matches("\";").to_string());
            }
        }
    }
    assert!(keys.len() > 20, "{keys:?}");
    for key in keys {
        let prefix = key.ends_with(['/', '.']);
        let is_compared = compared
            .iter()
            .any(|c| *c == key || (prefix && c.starts_with(&key)));
        assert!(
            is_compared ^ NOT_COMPARED.contains(&key.as_str()),
            "`{key}`: compare it in `Session::held_settings` (backup_check.rs) or add it to \
             NOT_COMPARED"
        );
    }
}

/// #258 (review): a Reading plan or Reading whose date is far outside the calendar, and a
/// Reading at the last position a database can hold, are refused by the preview and by
/// restore, naming each, with nothing changed and no Safety backup; reading them panics
/// nowhere.
#[test]
fn a_backup_with_dates_or_positions_at_their_limits_is_refused_without_a_panic() {
    let f = Fixture::new("limits");
    let mut s = f.session();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    let backup = f.file("damaged.obsbackup");
    damaged_copy(&f, &good, &backup, |db| {
        let plan = |name: &str, start: &str| obs_store::ReadingPlanRow {
            id: 0,
            name: name.to_string(),
            template: "custom".to_string(),
            bible_id: "kjv".to_string(),
            start_day: start.to_string(),
            weekdays: 0b111_1111,
            created_at: 0,
        };
        let reading = |passages: &str, day: &str| obs_store::PlanReadingRow {
            position: 0,
            passages: passages.to_string(),
            day: day.to_string(),
            read_at: None,
        };
        db.add_reading_plan(
            &plan("Far future", "9223372036854775807-01-01"),
            &[reading("Jude 1", "2024-01-01")],
        )
        .unwrap();
        db.add_reading_plan(
            &plan("Far day", "2024-01-01"),
            &[reading("Jude 1", "-9223372036854775808-12-31")],
        )
        .unwrap();
        db.add_reading_plan(
            &plan("Last reading", "2024-01-01"),
            &[
                reading("Jude 1", "2024-01-01"),
                reading("Nowhere 9", "2024-01-02"),
            ],
        )
        .unwrap();
        db.execute_unchecked(
            "UPDATE plan_reading SET position = 4294967295 WHERE passages = 'Nowhere 9'",
        )
        .unwrap();
    });
    s.run(
        "annotation.note",
        &json!({ "body": "made after the backup" }),
    )
    .unwrap();
    let before = dump(&s);
    let plans_before = s.reading_plans().to_vec();

    let refused = s.restore_preview(&backup).unwrap_err();
    for named in [
        "the Reading plan “Far future”",
        "Reading 1 of the Reading plan “Far day”",
        "Reading 4294967296 of the Reading plan “Last reading”",
        "Nothing was changed",
    ] {
        assert!(refused.contains(named), "{named}: {refused}");
    }
    let err = s.run("data.restore", &path(&backup)).unwrap_err();
    assert!(err.to_string().contains("Reading 4294967296"), "{err}");
    assert_eq!(dump(&s), before);
    assert_eq!(s.reading_plans(), plans_before);
    assert!(s.last_restore().is_none());
    assert!(
        !s.kept_backups()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
}

/// #258: a loader panicking on a record it never expected refuses the Backup as damaged, in
/// the preview and on restore, with nothing changed and no Safety backup; the app goes on.
#[test]
fn a_panic_while_loading_a_backup_refuses_it_as_damaged() {
    let f = Fixture::new("panics");
    let good = f.file("good.obsbackup");
    f.session().run("data.back_up", &path(&good)).unwrap();
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&f.root.join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&f.root.join("user.db")).unwrap();
    s.attach_user(Box::new(crate::testing::WritesFail {
        copies_panic: true,
        ..crate::testing::WritesFail::visits(store, 0)
    }));
    s.run(
        "annotation.note",
        &json!({ "body": "made after the backup" }),
    )
    .unwrap();
    let before = dump(&s);

    let refused = s.restore_preview(&good).unwrap_err();
    assert!(
        refused
            .contains("it is damaged (the app stopped reading it: a Favorite no loader expected)"),
        "{refused}"
    );
    assert!(refused.contains("Nothing was changed"), "{refused}");
    let err = s.run("data.restore", &path(&good)).unwrap_err();
    assert!(err.to_string().contains("it is damaged"), "{err}");
    assert_eq!(dump(&s), before);
    assert!(s.last_restore().is_none());
    assert!(
        !s.kept_backups()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
    s.run("annotation.note", &json!({ "body": "still working" }))
        .unwrap();
}

/// A step into a JSON value: a field of an object or an item of a list.
#[derive(Debug, Clone)]
enum Step {
    Field(String),
    Item(usize),
}

/// Every value inside `v` (and `v` itself), by its path.
fn paths(v: &Value, at: &mut Vec<Step>, out: &mut Vec<Vec<Step>>) {
    out.push(at.clone());
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                at.push(Step::Field(k.clone()));
                paths(x, at, out);
                at.pop();
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                at.push(Step::Item(i));
                paths(x, at, out);
                at.pop();
            }
        }
        _ => {}
    }
}

fn at_mut<'a>(v: &'a mut Value, path: &[Step]) -> &'a mut Value {
    path.iter().fold(v, |v, step| match step {
        Step::Field(k) => &mut v[k.as_str()],
        Step::Item(i) => &mut v[*i],
    })
}

/// Values no loader expects in place of another.
fn hostile_values() -> Vec<Value> {
    vec![
        Value::Null,
        json!(1e300),
        json!(-1),
        json!(u64::MAX),
        json!(i64::MIN),
        json!("zz-unknown"),
        json!(true),
        json!([]),
        json!({}),
        json!([1, 2]),
    ]
}

/// The mutations of a JSON value `v`, each with what it changed: each value inside it
/// replaced by a hostile one (in turn, from `next`), each list given its first item again,
/// each object a field this version does not know.
fn mutations_of(v: &Value, next: &mut usize) -> Vec<(String, Value)> {
    let hostile = hostile_values();
    let mut all = Vec::new();
    paths(v, &mut Vec::new(), &mut all);
    let mut out = Vec::new();
    for path in all {
        let mut m = v.clone();
        let to = hostile[*next % hostile.len()].clone();
        *next += 1;
        *at_mut(&mut m, &path) = to.clone();
        out.push((format!("{path:?} = {to}"), m));
        let mut m = v.clone();
        match at_mut(&mut m, &path) {
            Value::Array(a) if !a.is_empty() => {
                let first = a[0].clone();
                a.push(first);
                out.push((format!("{path:?} repeats its first item"), m));
            }
            Value::Object(o) => {
                o.insert("zz_later_field".into(), json!(7));
                out.push((format!("{path:?} gains a field"), m));
            }
            _ => {}
        }
    }
    out
}

/// SQL text: `s` quoted.
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Every Panel id in a Layout's JSON `layout` tree.
fn panel_ids(layout: &Value, out: &mut Vec<Value>) {
    if layout["kind"] == "panel" {
        out.push(layout["panel"].clone());
    }
    for child in layout["children"].as_array().into_iter().flatten() {
        panel_ids(&child[0], out);
    }
}

/// The oracle of `every_change_to_a_backup_is_refused_cleanly_or_restored_whole` for the
/// Layout “Study” of a restored Backup `written`, apart from the trial restore's own check:
/// its stored JSON itself, but what names a Tab or Panel it does not have, is held by the
/// Workspace `layout.restore` gives. A Layout with a Tab of a Resource not in the library
/// (which a launch leaves out) is left to the trial's check.
fn assert_layout_restored_whole(s: &mut Session, written: &obs_store::UserStore, what: &str) {
    let stored = written.layout("Study").unwrap().unwrap();
    let read = WorkspaceSnapshot::from_json(&stored).unwrap();
    if read.tabs.iter().any(|t| {
        t.kind
            .resource_id()
            .is_some_and(|id| s.library.get(id).is_none())
    }) {
        return;
    }
    let mut expected: Value = serde_json::from_str(&stored).unwrap();
    let mut panels = Vec::new();
    panel_ids(&expected["layout"], &mut panels);
    for w in expected["windows"].as_array().into_iter().flatten() {
        panel_ids(&w["layout"], &mut panels);
    }
    let tabs: Vec<Value> = expected["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].clone())
        .collect();
    // What names no Tab or Panel of the Layout falls back; the restore may name one it made.
    let fields = expected.as_object_mut().unwrap();
    if !tabs.contains(&fields["focused"]) {
        fields.remove("focused");
    }
    for last in ["last_reading_panel", "last_other_panel"] {
        if !panels.contains(&fields[last]) {
            fields.remove(last);
        }
    }
    expected["remembered"]
        .as_array_mut()
        .unwrap()
        .retain(|r| panels.contains(&r[1]));
    s.run("layout.restore", &json!({ "name": "Study" }))
        .unwrap_or_else(|e| panic!("{what}: {e}"));
    let mut now = serde_json::to_value(s.workspace.snapshot()).unwrap();
    // Tabs the stored arrangement does not place get a Panel of their own on the right.
    let stored_arrangement = expected.as_object_mut().unwrap().remove("layout").unwrap();
    let arrangement = now.as_object_mut().unwrap().remove("layout").unwrap();
    let holds = crate::backup_check::holds;
    let beside =
        arrangement["kind"] == "row" && holds(&stored_arrangement, &arrangement["children"][0][0]);
    assert!(
        holds(&expected, &now)
            && (stored_arrangement.is_null() || holds(&stored_arrangement, &arrangement) || beside),
        "the Layout was not restored whole: {what}\nstored {stored}\nnow {now} {arrangement}"
    );
}

/// #258 proof: every value of a Backup the app wrote, changed to one no loader expects (huge
/// and negative numbers, wrong types, unknown ids, impossible dates, repeated items and ids,
/// old and new ids of one shortcut, text that is not JSON), is either refused, by the preview
/// and by restore alike, with the live data and the kept Backups unchanged, or restored with
/// nothing changed by the restore and every loader keeping all of it. Nothing panics.
#[test]
fn every_change_to_a_backup_is_refused_cleanly_or_restored_whole() {
    let f = Fixture::new("mutations");
    let mut s = f.session();
    write_everything(&mut s);
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    let user = s.user.as_ref().unwrap();

    type Damage = Box<dyn Fn(&obs_store::UserStore)>;
    let mut cases: Vec<(String, Damage)> = Vec::new();
    let mut next = 0;
    for (key, value) in user.preferences().unwrap() {
        let k = key.clone();
        cases.push((
            format!("{key} = not JSON"),
            Box::new(move |db| db.set_preference(&k, "not JSON{").unwrap()),
        ));
        let Ok(v) = serde_json::from_str::<Value>(&value) else {
            continue;
        };
        for (change, m) in mutations_of(&v, &mut next) {
            let (k, text) = (key.clone(), m.to_string());
            cases.push((
                format!("{key}: {change}"),
                Box::new(move |db| db.set_preference(&k, &text).unwrap()),
            ));
        }
    }
    let row_json = |table: &str, column: &str, json: &str, next: &mut usize| {
        let v: Value = serde_json::from_str(json).unwrap();
        let mut out: Vec<(String, Damage)> = Vec::new();
        for (change, m) in mutations_of(&v, next) {
            let sql = format!("UPDATE {table} SET {column} = {}", quoted(&m.to_string()));
            out.push((
                format!("{table}.{column}: {change}"),
                Box::new(move |db| {
                    db.execute_unchecked(&sql).unwrap();
                }),
            ));
        }
        out
    };
    let favorite = &user.favorites().unwrap()[0].location;
    cases.extend(row_json("favorite", "location", favorite, &mut next));
    let visit = &user.history_as_of(0).unwrap()[0].location;
    cases.extend(row_json("history", "location", visit, &mut next));
    let layout = user.layout("Study").unwrap().unwrap();
    cases.extend(row_json("layout", "snapshot", &layout, &mut next));
    for sql in [
        "UPDATE reading_plan SET start_day = '9223372036854775807-01-01'",
        "UPDATE reading_plan SET start_day = '-9223372036854775808-01-01'",
        "UPDATE reading_plan SET start_day = '2024-02-30'",
        "UPDATE reading_plan SET start_day = '0-01-01'",
        "UPDATE reading_plan SET start_day = '10000-01-01'",
        "UPDATE reading_plan SET start_day = ''",
        "UPDATE reading_plan SET weekdays = 255",
        "UPDATE reading_plan SET weekdays = 0",
        "UPDATE plan_reading SET day = '9223372036854775807-12-31' WHERE position = 0",
        "UPDATE plan_reading SET day = '2024-1-1' WHERE position = 0",
        "UPDATE plan_reading SET passages = 'Nowhere 9' WHERE position = 0",
        "UPDATE plan_reading SET position = 4294967295 WHERE position = 0",
        "UPDATE plan_reading SET position = 4294967295, passages = '' WHERE position = 0",
        "UPDATE favorite SET location = 'not JSON'",
        "UPDATE history SET location = 'not JSON'",
        "UPDATE layout SET snapshot = 'not JSON'",
        "UPDATE preference SET value = '{\"selection.highlight_1\":\"Alt+2\",\"selection.highlight_yellow\":\"Alt+3\"}' WHERE key = 'key_bindings'",
    ] {
        cases.push((
            sql.to_string(),
            Box::new(move |db| {
                db.execute_unchecked(sql).unwrap();
            }),
        ));
    }
    assert!(cases.len() > 150, "{}", cases.len());

    let kept_backups =
        |s: &Session| -> Vec<PathBuf> { s.kept_backups().into_iter().map(|b| b.path).collect() };
    let preferences = |s: &Session| s.user.as_ref().unwrap().preferences().unwrap();
    let (mut refused, mut restored) = (0, 0);
    let backup = f.file("changed.obsbackup");
    for (what, damage) in &cases {
        damaged_copy(&f, &good, &backup, |db| damage(db));
        let before = (dump(&s), preferences(&s), kept_backups(&s));
        let preview =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.restore_preview(&backup)))
                .unwrap_or_else(|_| panic!("the preview panicked: {what}"));
        let restore =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.restore_backup(&backup)))
                .unwrap_or_else(|_| panic!("restore panicked: {what}"));
        assert_eq!(preview.is_ok(), restore.is_ok(), "{what}: {preview:?}");
        if restore.is_err() {
            refused += 1;
            assert_eq!(
                (dump(&s), preferences(&s), kept_backups(&s)),
                before,
                "a refused restore changed something: {what}"
            );
            continue;
        }
        restored += 1;
        let (written, _) = obs_store::UserStore::open_backup(&backup).unwrap();
        // Loading the library confirms its Resources' Attributions from their manifests, as
        // every launch does (#154); nothing else may change what was restored.
        let restored_settings = |all: Vec<(String, String)>| -> Vec<(String, String)> {
            all.into_iter()
                .filter(|(k, _)| !k.starts_with("resource-attribution/"))
                .collect()
        };
        assert_eq!(
            restored_settings(preferences(&s)),
            restored_settings(written.preferences().unwrap()),
            "the restore changed the restored settings: {what}"
        );
        let stored = crate::backup_check::StoredRecords::read(&written).unwrap();
        assert_eq!(
            s.unkept_records(&stored),
            Vec::<String>::new(),
            "a loader did not keep all of it: {what}"
        );
        assert_layout_restored_whole(&mut s, &written, what);
        s.undo_restore().unwrap();
    }
    assert!(
        refused > 50 && restored > 20,
        "{refused} refused, {restored} restored"
    );
}

/// #258: a restore replaces the Notes a Notes search found; open Search Tabs run their
/// searches again over the restored data, keeping their query.
#[test]
fn a_restore_runs_open_notes_searches_again_over_the_restored_data() {
    let f = Fixture::new("rerun");
    let mut s = f.session();
    let empty = f.file("empty.obsbackup");
    s.run("data.back_up", &path(&empty)).unwrap();
    s.run("annotation.note", &json!({ "body": "grace after backup" }))
        .unwrap();
    let out = s
        .run("search.run", &json!({ "text": "grace", "mode": "notes" }))
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    let hits = |s: &Session| {
        s.search_tab(tab)
            .unwrap()
            .notes
            .as_ref()
            .unwrap()
            .hits
            .len()
    };
    assert_eq!(hits(&s), 1);

    s.run("data.restore", &path(&empty)).unwrap();
    assert!(s.annotations(None).unwrap().is_empty());
    assert_eq!(hits(&s), 0);
    assert_eq!(s.search_tab(tab).unwrap().query, "grace");
    assert_eq!(s.search_tab(tab).unwrap().selected, None);

    s.run("data.undo_restore", &Value::Null).unwrap();
    assert_eq!(hits(&s), 1);
}

/// #258: the searches a restore runs again are not new searches: a Backup's own Recent
/// searches come back as they were (none cut to make room for the open Tab's query), and
/// Undo restore brings back the ones from before.
#[test]
fn a_restore_running_searches_again_keeps_the_backups_recent_searches() {
    let f = Fixture::new("rerun-recent");
    let mut s = f.session();
    for i in 0..crate::bible_search::RECENT_LIMIT {
        s.run(
            "search.run",
            &json!({ "text": format!("grace {i}"), "mode": "notes" }),
        )
        .unwrap();
    }
    let backed_up = s.recent_searches(SearchMode::Notes);
    assert_eq!(backed_up.len(), crate::bible_search::RECENT_LIMIT);
    let backup = f.file("recent.obsbackup");
    s.run("data.back_up", &path(&backup)).unwrap();
    s.run("search.run", &json!({ "text": "mercy", "mode": "notes" }))
        .unwrap();
    let before = s.recent_searches(SearchMode::Notes);
    assert_eq!(before[0], "mercy");

    s.run("data.restore", &path(&backup)).unwrap();
    assert_eq!(s.recent_searches(SearchMode::Notes), backed_up);
    let mut launched = f.launched_on(&backup);
    assert_eq!(launched.recent_searches(SearchMode::Notes), backed_up);

    s.run("data.undo_restore", &Value::Null).unwrap();
    assert_eq!(s.recent_searches(SearchMode::Notes), before);
}
