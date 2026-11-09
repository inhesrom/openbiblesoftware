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
        // Text settings are any text: not checked.
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
        "Nothing was changed",
    ] {
        assert!(refused.contains(named), "{named}: {refused}");
    }
    assert!(!refused.contains("log_level"), "{refused}");
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

/// Home and Guide arrangements are refused for each card or section their loaders drop, and
/// read when each is known and held once.
#[test]
fn arrangements_refuse_cards_and_sections_their_loaders_drop() {
    let reads = |key: &str, json: &str| {
        let setting = crate::backup_check::JSON_SETTINGS
            .iter()
            .find(|j| j.names(key))
            .unwrap();
        (setting.reads)(key, json)
    };
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
    ] {
        assert!(reads(key, json), "{key} {json}");
    }
    for (key, json) in [
        ("home", r#"{"cards":[{"id":"nowhere"}]}"#),
        (
            "guide-arrangement/passage_guide",
            r#"{"sections":[{"id":"nowhere"}]}"#,
        ),
        (
            "guide-arrangement/nowhere",
            r#"{"sections":[{"id":"commentaries"}]}"#,
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
        assert!(!reads(key, json), "{key} {json}");
    }
}

/// Each field-by-field setting refuses a field its loader would default or drop, and still
/// reads what the app wrote before (#37's View record, old Selection key ids, no skipped
/// release).
#[test]
fn field_by_field_settings_refuse_unreadable_fields_and_read_older_records() {
    let reads = |key: &str, json: &str| {
        let setting = crate::backup_check::JSON_SETTINGS
            .iter()
            .find(|j| j.names(key))
            .unwrap();
        (setting.reads)(key, json)
    };
    for (key, json) in [
        (
            "view",
            r#"{"text_size":18,"dark_page":true,"verse_numbers":false}"#,
        ),
        (
            "view",
            r#"{"theme":"light","fonts":{"greek":{"family":"Gentium"}},"reading_layout":null}"#,
        ),
        ("view", r#"{"some_later_field":1}"#),
        (
            "toolbar",
            r#"{"position":"left","launchers":[{"id":"tools","shown":false}]}"#,
        ),
        (
            "key_bindings",
            r#"{"selection.highlight_yellow":"Ctrl+1","edit.undo":null}"#,
        ),
        ("updates", r#"{"auto_check":false,"skipped":null}"#),
        ("updates", r#"{"skipped":"0.2.0"}"#),
        ("tour", r#"{"shown":true}"#),
        (
            "custom-theme",
            r##"{"format":"openbible-theme","version":1,"base":"high_contrast_dark","colors":{"ink":"#ffffff"}}"##,
        ),
    ] {
        assert!(reads(key, json), "{key} {json}");
    }
    for (key, json) in [
        ("view", "[]"),
        ("view", r#"{"theme":"purple"}"#),
        ("view", r#"{"red_letter":"yes"}"#),
        ("view", r#"{"verse_numbers":"sideways"}"#),
        ("view", r#"{"fonts":{"latin":{"family":3}}}"#),
        ("view", r#"{"reading_layout":{"flow":"sideways"}}"#),
        ("toolbar", r#"{"position":"middle"}"#),
        (
            "toolbar",
            r#"{"launchers":[{"id":"tools"},{"id":"tools"}]}"#,
        ),
        ("toolbar", r#"{"launchers":[{"id":"tools","shown":"no"}]}"#),
        ("key_bindings", r#"{"annotation.note":42}"#),
        ("key_bindings", r#"{"annotation.note":"Ctrl+Nowhere"}"#),
        ("updates", r#"{"auto_check":"false"}"#),
        ("updates", r#"{"skipped":1}"#),
        ("tour", r#"{"shown":"yes"}"#),
        ("tour", "not JSON"),
        (
            "custom-theme",
            r#"{"format":"x","version":1,"base":"light"}"#,
        ),
        (
            "custom-theme",
            r##"{"format":"openbible-theme","version":1,"base":"light","colors":{"nowhere":"#000000"}}"##,
        ),
        ("view", r#"{"text_size":41}"#),
        ("view", r#"{"line_spacing":0.9}"#),
        ("view", r#"{"line_spacing":2.1}"#),
    ] {
        assert!(!reads(key, json), "{key} {json}");
    }
}

/// #258: a View setting whose Text size or Line spacing the loader would clamp, and a Search
/// history longer than the app keeps (the loader would drop the oldest), are refused by the
/// preview and by restore before any Safety backup; the limits themselves, a Text size below
/// the minimum (reset with a notice, #241) and a pre-#241 default size still read.
#[test]
fn a_backup_with_view_values_or_history_the_loaders_would_change_is_refused() {
    let reads = |key: &str, json: &str| {
        let setting = crate::backup_check::JSON_SETTINGS
            .iter()
            .find(|j| j.names(key))
            .unwrap();
        (setting.reads)(key, json)
    };
    let history = |n: usize| {
        let entries: Vec<Value> = (0..n)
            .map(
                |i| json!({ "search": { "mode": "bible", "query": format!("love {i}") }, "at": i }),
            )
            .collect();
        let json = serde_json::to_string(&entries).unwrap();
        // Each entry is one the loader reads.
        let read: Vec<SearchHistoryEntry> = serde_json::from_str(&json).unwrap();
        assert_eq!(read.len(), n);
        json
    };
    for (key, json) in [
        ("view", r#"{"text_size":40,"line_spacing":2}"#.to_string()),
        ("view", r#"{"text_size":8,"line_spacing":1}"#.to_string()),
        ("view", r#"{"text_size":17}"#.to_string()),
        ("search.history", history(SEARCH_HISTORY_LIMIT)),
    ] {
        assert!(reads(key, &json), "{key} {json}");
    }

    let f = Fixture::new("changed-values");
    let mut s = f.session();
    let good = f.file("good.obsbackup");
    s.run("data.back_up", &path(&good)).unwrap();
    let backup = f.file("damaged.obsbackup");
    let too_long = history(SEARCH_HISTORY_LIMIT + 1);
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
        "the setting `view` (View settings",
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

/// Everything the app itself writes passes the check: a Backup of a Session that has used
/// every setting kept as JSON it can reach here previews and restores.
#[test]
fn a_backup_of_what_the_app_writes_passes_the_restore_check() {
    let f = Fixture::new("app-writes");
    let mut s = f.session();
    for setting in crate::settings::SETTINGS.iter().filter(|s| s.has_reset()) {
        crate::settings_tests::change(&mut s, setting.id);
    }
    let run = |s: &mut Session, id: &str, p: Value| {
        s.run(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    };
    run(
        &mut s,
        "theme.set_color",
        json!({ "role": "lapis", "color": "#FFD000" }),
    );
    run(
        &mut s,
        "keys.set",
        json!({ "command": "view.text_larger", "key": "Ctrl+ArrowUp" }),
    );
    run(
        &mut s,
        "home.hide_card",
        json!({ "card": "verse_of_the_day" }),
    );
    run(
        &mut s,
        "guide.hide_section",
        json!({ "guide": "passage_guide", "section": "commentaries" }),
    );
    run(
        &mut s,
        "copy.option",
        json!({ "name": "layout", "value": "verse_per_line" }),
    );
    run(&mut s, "copy.preset_save", json!({ "name": "Handout" }));
    run(
        &mut s,
        "collection.save",
        json!({ "name": "Bibles", "rules": "type:bible" }),
    );
    run(
        &mut s,
        "resource.tags",
        json!({ "id": "kjv", "tags": "older" }),
    );
    run(
        &mut s,
        "filter.save",
        json!({ "name": "Grace", "query": "grace", "look": "background", "color": "#ffd6aa" }),
    );
    run(&mut s, "help.tour", json!({}));
    run(&mut s, "tour.skip", json!({}));
    run(&mut s, "annotation.note", json!({ "body": "grace" }));
    let out = s
        .run("search.run", &json!({ "text": "grace", "mode": "notes" }))
        .unwrap();
    run(
        &mut s,
        "search.save",
        json!({ "tab": out["tab"], "name": "Grace" }),
    );
    run(&mut s, "command_box.run", json!({ "text": "Matt 5:43" }));
    let bible = s.run("resource.open", &json!({ "id": "kjv" })).unwrap()["tab"].clone();
    run(
        &mut s,
        "tab.go",
        json!({ "tab": bible, "text": "Matt 5:43" }),
    );
    run(&mut s, "favorite.add", json!({ "tab": bible }));
    s.record_history();
    run(&mut s, "layout.save", json!({ "name": "Study" }));

    let backup = f.file("everything.obsbackup");
    s.run("data.back_up", &path(&backup)).unwrap();
    let written: Vec<String> = s
        .user
        .as_ref()
        .unwrap()
        .preferences()
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    let unreached: Vec<&str> = crate::backup_check::JSON_SETTINGS
        .iter()
        .filter(|j| !written.iter().any(|k| j.names(k)))
        .map(|j| j.key)
        .collect();
    // Written only by the Starter library and by an Import.
    assert_eq!(unreached, ["starter-library", "import-report/"]);
    let preview = s.restore_preview(&backup).unwrap();
    assert_eq!(preview.backup.layouts, ["Study"]);
    s.run("data.restore", &path(&backup)).unwrap();
}

/// Every setting the engine names with a constant is either checked as JSON on restore or
/// known to be plain text, so a new JSON setting cannot be missed by the restore check.
#[test]
fn every_setting_the_engine_keeps_is_known_to_the_restore_check() {
    const PLAIN_TEXT: &[&str] = &[
        "highlights.palette",
        "annotations.last_notebook",
        "log_level",
        "page-tone-hint",
        "reading.visual_filters_next_id",
        "search.word_forms",
    ];
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
        let json = crate::backup_check::JSON_SETTINGS
            .iter()
            .any(|j| j.key == key);
        assert!(
            json ^ PLAIN_TEXT.contains(&key.as_str()),
            "`{key}`: add it to JSON_SETTINGS (backup_check.rs) or to PLAIN_TEXT here"
        );
    }
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
