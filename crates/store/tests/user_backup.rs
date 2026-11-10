//! Backups of the User database (#217): Back up now, the restore preview's contents, restore
//! with its Safety backup, migrations on restore, and the list of kept Backups.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use obs_scripture::{Book, Passage, Verse};
use obs_store::annotation::new_id;
use obs_store::{
    Anchor, Annotation, BackupKind, Change, Content, Notebook, Row, SAFETY_BACKUPS, StoreError,
    UPDATE_BACKUPS, USER_SCHEMA_VERSION, UserStore,
};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("obs-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn db(&self) -> PathBuf {
        self.0.join("user.db")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn notebook(name: &str) -> Notebook {
    Notebook {
        id: new_id(),
        name: name.into(),
        is_default: false,
        show_in_text: true,
        created_at: 10,
        modified_at: 10,
    }
}

fn passage(v: u16) -> Anchor {
    Anchor::Passage {
        passage: Passage::verse(Verse::new(Book::John, 3, v)),
        origin: None,
    }
}

fn note(notebook: &str, body: &str, v: u16) -> Annotation {
    Annotation {
        id: new_id(),
        notebook_id: notebook.into(),
        content: Content::Note { body: body.into() },
        anchor: Some(passage(v)),
        created_at: 20,
        modified_at: 20,
    }
}

fn highlight(notebook: &str, v: u16) -> Annotation {
    Annotation {
        id: new_id(),
        notebook_id: notebook.into(),
        content: Content::Highlight {
            style: "yellow".into(),
        },
        anchor: Some(passage(v)),
        created_at: 30,
        modified_at: 30,
    }
}

/// A database with a "Romans" Notebook holding a Note and a Highlight, a Layout, a Favorite
/// and a preference.
fn filled(db: &UserStore) -> (Notebook, Vec<Annotation>) {
    let nb = notebook("Romans");
    let anns = vec![note(&nb.id, "grace", 16), highlight(&nb.id, 17)];
    let mut changes = vec![Change::insert(Row::Notebook(nb.clone()))];
    changes.extend(
        anns.iter()
            .cloned()
            .map(|a| Change::insert(Row::Annotation(a))),
    );
    db.apply_changes(&changes).unwrap();
    db.save_layout("Study", "{\"v\":1}").unwrap();
    db.add_favorite("", "John 3:16", "{}").unwrap();
    db.set_preference("view", "{\"theme\":\"dark\"}").unwrap();
    (nb, anns)
}

fn delete_notebook(db: &UserStore, nb: &Notebook, anns: &[Annotation]) {
    let mut changes: Vec<Change> = anns
        .iter()
        .cloned()
        .map(|a| Change::delete(Row::Annotation(a)))
        .collect();
    changes.push(Change::delete(Row::Notebook(nb.clone())));
    db.apply_changes(&changes).unwrap();
}

#[test]
fn back_up_now_writes_one_self_contained_file_with_everything() {
    let dir = Dir::new("now");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    filled(&db);
    let dest = dir.0.join("mine.obsbackup");
    db.back_up(&dest).unwrap();
    // No `-wal` file belongs with it, and no partial file is left.
    let names: Vec<String> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("mine"))
        .collect();
    assert_eq!(names, ["mine.obsbackup"]);
    let c = UserStore::backup_contents(&dest).unwrap();
    assert_eq!(c.version, USER_SCHEMA_VERSION);
    assert_eq!(c.notebooks.len(), 2, "{c:?}");
    assert!(c.notebooks.contains(&"Romans".to_string()));
    assert_eq!((c.notes, c.highlights), (1, 1));
    assert_eq!(c.layouts, ["Study"]);
    assert_eq!(c.favorites, 1);
    assert_eq!(c.settings, 1);
    // Backing up again replaces it.
    db.add_favorite("", "John 3:17", "{}").unwrap();
    db.back_up(&dest).unwrap();
    assert_eq!(UserStore::backup_contents(&dest).unwrap().favorites, 2);
}

/// #217 acceptance (store side): back up, delete a Notebook, restore: every Annotation is
/// back; the Safety backup taken first undoes the restore.
#[test]
fn restoring_brings_back_a_deleted_notebook_and_the_safety_backup_undoes_it() {
    let dir = Dir::new("restore");
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    let (nb, anns) = filled(&db);
    let before = db.annotation_dump().unwrap();
    let backup = dir.0.join("b.obsbackup");
    db.back_up(&backup).unwrap();

    delete_notebook(&db, &nb, &anns);
    let deleted = db.annotation_dump().unwrap();
    assert_ne!(deleted, before);

    let safety = db.restore(&backup).unwrap();
    assert_eq!(db.annotation_dump().unwrap(), before);
    for a in &anns {
        assert_eq!(db.annotation(&a.id).unwrap().as_ref(), Some(a));
    }
    // Note search works on the restored rows.
    assert_eq!(db.search_notes("\"grace\"").unwrap(), [anns[0].id.clone()]);
    assert!(safety.starts_with(db.backup_dir()), "{}", safety.display());

    // Undo: restore the Safety backup.
    db.restore(&safety).unwrap();
    assert_eq!(db.annotation_dump().unwrap(), deleted);

    // The live database is still the app's WAL database, and reopens as it was left.
    let mode: String = {
        drop(db);
        let (db, _) = UserStore::open(&dir.db()).unwrap();
        assert_eq!(db.annotation_dump().unwrap(), deleted);
        db.preference("view").unwrap().unwrap()
    };
    assert!(mode.contains("dark"));
}

#[test]
fn restoring_an_older_backup_runs_the_migrations() {
    let dir = Dir::new("older");
    let old = dir.0.join("old").join("user.db");
    {
        let (v3, _) = UserStore::open_to(&old, 3).unwrap();
        v3.set_preference("theme", "dark").unwrap();
        v3.save_layout("Old layout", "{}").unwrap();
        v3.back_up(&dir.0.join("v3.obsbackup")).unwrap();
    }
    let c = UserStore::backup_contents(&dir.0.join("v3.obsbackup")).unwrap();
    assert_eq!(c.version, 3);
    assert_eq!(c.layouts, ["Old layout"]);
    // The migrated copy has a default Notebook and the later tables.
    assert_eq!(c.notebooks.len(), 1);

    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    db.restore(&dir.0.join("v3.obsbackup")).unwrap();
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    assert_eq!(db.preference("theme").unwrap().as_deref(), Some("dark"));
    assert_eq!(db.layouts().unwrap(), ["Old layout"]);
    assert_eq!(db.notebooks().unwrap().len(), 1);
    // Later tables work.
    db.add_favorite("", "Gen 1:1", "{}").unwrap();
    // The file itself was not changed.
    let (again, found) = UserStore::open_backup(&dir.0.join("v3.obsbackup")).unwrap();
    assert_eq!(found, 3);
    drop(again);
}

#[test]
fn a_newer_or_foreign_file_is_refused_and_changes_nothing() {
    let dir = Dir::new("refuse");
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    filled(&db);
    let before = db.annotation_dump().unwrap();

    let newer = dir.0.join("newer.obsbackup");
    db.back_up(&newer).unwrap();
    assert_eq!(
        set_user_version(&newer, USER_SCHEMA_VERSION + 1),
        USER_SCHEMA_VERSION + 1
    );
    let err = db.restore(&newer).unwrap_err();
    assert!(matches!(err, StoreError::NewerFormat { .. }), "{err}");

    let text = dir.0.join("notes.txt");
    std::fs::write(&text, "not a database").unwrap();
    let err = db.restore(&text).unwrap_err();
    assert!(err.to_string().contains("not a backup"), "{err}");

    let err = db.restore(&dir.0.join("missing.obsbackup")).unwrap_err();
    assert!(err.to_string().contains("not found"), "{err}");

    assert_eq!(db.annotation_dump().unwrap(), before);
    // No Safety backup was taken for a refused restore.
    assert!(
        !db.backups()
            .unwrap()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
}

/// Set a database file's `user_version`, returning it as read back.
fn set_user_version(path: &std::path::Path, v: u32) -> u32 {
    // `user_version` is the big-endian u32 at offset 60 of the file header.
    let mut bytes = std::fs::read(path).unwrap();
    bytes[60..64].copy_from_slice(&v.to_be_bytes());
    std::fs::write(path, &bytes).unwrap();
    u32::from_be_bytes(bytes[60..64].try_into().unwrap())
}

#[test]
fn the_kept_backups_list_launch_safety_and_before_upgrade_copies_newest_first() {
    let dir = Dir::new("list");
    {
        let (v8, _) = UserStore::open_to(&dir.db(), 8).unwrap();
        v8.set_preference("x", "1").unwrap();
    }
    // Opening migrates (a before-upgrade copy) and takes a launch backup.
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    let mine = dir.0.join("mine.obsbackup");
    db.back_up(&mine).unwrap();
    for _ in 0..SAFETY_BACKUPS + 2 {
        db.restore(&mine).unwrap();
    }
    let list = db.backups().unwrap();
    let count = |k: BackupKind| list.iter().filter(|b| b.kind == k).count();
    assert_eq!(count(BackupKind::Launch), 2, "{list:?}");
    assert_eq!(count(BackupKind::Safety), SAFETY_BACKUPS, "{list:?}");
    assert_eq!(count(BackupKind::BeforeUpgrade), 1, "{list:?}");
    assert!(list.windows(2).all(|w| w[0].taken_at >= w[1].taken_at));
    // Every one of them can be read as a Backup.
    for b in &list {
        UserStore::backup_contents(&b.path).unwrap();
    }
    // A Backup the user made elsewhere is not listed.
    assert!(!list.iter().any(|b| b.path == mine));
    assert_eq!(db.local_time(0).unwrap().len(), "1970-01-01 00:00".len());
}

#[test]
fn update_backup_keeps_the_newest_before_update_copies_and_changes_nothing() {
    let dir = Dir::new("update");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    db.set_preference("x", "kept").unwrap();
    let before = db.contents().unwrap();
    let mut taken = Vec::new();
    for _ in 0..UPDATE_BACKUPS + 2 {
        taken.push(db.update_backup().unwrap());
    }
    // The database itself is as it was.
    assert_eq!(db.contents().unwrap(), before);
    assert_eq!(db.preference("x").unwrap().as_deref(), Some("kept"));
    let list = db.backups().unwrap();
    let updates: Vec<_> = list
        .iter()
        .filter(|b| b.kind == BackupKind::BeforeUpdate)
        .collect();
    assert_eq!(updates.len(), UPDATE_BACKUPS, "{list:?}");
    // The newest are kept, and each is a whole Backup of the data.
    assert!(updates.iter().any(|b| b.path == *taken.last().unwrap()));
    assert!(!updates.iter().any(|b| b.path == taken[0]));
    for b in updates {
        assert_eq!(
            UserStore::backup_contents(&b.path).unwrap().settings,
            before.settings
        );
    }
    assert_eq!(BackupKind::BeforeUpdate.label(), "Before an app update");
}

#[test]
fn a_backup_with_another_page_size_is_restored_too() {
    let dir = Dir::new("page-size");
    let other = dir.0.join("other").join("user.db");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    {
        let c = rusqlite::Connection::open(&other).unwrap();
        c.pragma_update(None, "page_size", 8192).unwrap();
        c.execute_batch("VACUUM").unwrap();
    }
    let backup = dir.0.join("other.obsbackup");
    let expected = {
        let (db, _) = UserStore::open(&other).unwrap();
        filled(&db);
        db.back_up(&backup).unwrap();
        db.annotation_dump().unwrap()
    };
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    db.restore(&backup).unwrap();
    assert_eq!(db.annotation_dump().unwrap(), expected);
    drop(db);
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(db.annotation_dump().unwrap(), expected);
    assert_eq!(db.layouts().unwrap(), ["Study"]);
}

/// A restore that fails after rebuilding a Backup of another page size in a scratch file
/// beside the Backups (here: the Safety backup cannot drop the oldest, a folder) leaves no
/// scratch file behind and the data as it was.
#[test]
fn a_failed_restore_leaves_no_scratch_file() {
    let dir = Dir::new("scratch");
    let other = dir.0.join("other").join("user.db");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    {
        let c = rusqlite::Connection::open(&other).unwrap();
        c.pragma_update(None, "page_size", 8192).unwrap();
        c.execute_batch("VACUUM").unwrap();
    }
    let backup = dir.0.join("other.obsbackup");
    {
        let (db, _) = UserStore::open(&other).unwrap();
        filled(&db);
        db.back_up(&backup).unwrap();
    }
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    let before = db.annotation_dump().unwrap();
    // Folders named like the kept Safety backups: removing the oldest fails.
    for n in 1..=SAFETY_BACKUPS {
        std::fs::create_dir_all(db.backup_dir().join(format!("user-safety-{n:06}.db"))).unwrap();
    }
    assert!(db.restore(&backup).is_err());
    assert_eq!(db.annotation_dump().unwrap(), before);
    let scratch: Vec<String> = std::fs::read_dir(db.backup_dir())
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".user-restoring"))
        .collect();
    assert!(scratch.is_empty(), "{scratch:?}");
}

/// A Backup this build cannot use (a table or index missing, a row of the wrong type) is
/// refused by the preview and by restore, before anything is touched: the live data, its
/// undo (the Safety backups) and the next launch are as they were.
#[test]
fn a_backup_missing_part_of_the_format_or_with_unreadable_rows_is_refused() {
    let dir = Dir::new("damaged");
    let (mut db, _) = UserStore::open(&dir.db()).unwrap();
    let (nb, _) = filled(&db);
    let before = db.annotation_dump().unwrap();
    for (name, damage, why) in [
        ("no-anchors", "DROP TABLE anchors", "no table `anchors`"),
        (
            "no-index",
            "DROP INDEX anchors_passage",
            "no index `anchors_passage`",
        ),
        (
            "no-column",
            "ALTER TABLE favorite DROP COLUMN label",
            "no column `label`",
        ),
        (
            "bad-row",
            &*format!(
                "UPDATE notebooks SET created_at = 'yesterday' WHERE id = '{}'",
                nb.id
            ),
            "it is damaged (database error: Invalid column type Text",
        ),
        (
            // Seven characters pass the table's own check; only reading the colour fails.
            "bad-style-colour",
            "UPDATE highlight_styles SET color = '#zzzzzz' WHERE id = 'yellow'",
            "it is damaged",
        ),
        (
            "bad-palette-row",
            "UPDATE highlight_palettes SET position = 'first' WHERE id = 'standard'",
            "it is damaged",
        ),
        (
            // SQLite's integrity check does not look at foreign keys (#258): both Annotations
            // then belong to no Notebook.
            "no-notebook",
            "PRAGMA foreign_keys = OFF;
             UPDATE annotations SET notebook_id = 'nonexistent'",
            "2 rows of `annotations` point at a missing row of `notebooks`",
        ),
    ] {
        let backup = dir.0.join(format!("{name}.obsbackup"));
        db.back_up(&backup).unwrap();
        {
            let c = rusqlite::Connection::open(&backup).unwrap();
            c.execute_batch(damage).unwrap();
        }
        let err = UserStore::backup_contents(&backup).unwrap_err().to_string();
        assert!(
            err.contains("not a backup") && err.contains(why),
            "{name}: {err}"
        );
        let err = db.restore(&backup).unwrap_err().to_string();
        assert!(err.contains(why), "{name}: {err}");
        assert_eq!(db.annotation_dump().unwrap(), before, "{name}");
    }
    assert!(
        !db.backups()
            .unwrap()
            .iter()
            .any(|b| b.kind == BackupKind::Safety)
    );
    drop(db);
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(db.annotation_dump().unwrap(), before);
}

/// Back up now never writes over the User database or SQLite's files beside it, whatever path
/// names them; the database goes on working.
#[test]
fn back_up_refuses_the_user_database_and_its_sidecar_files() {
    let dir = Dir::new("own-file");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    filled(&db);
    let before = db.annotation_dump().unwrap();
    std::fs::create_dir_all(dir.0.join("sub")).unwrap();
    let mut refused = vec![
        dir.db(),
        dir.0.join(".").join("user.db"),
        dir.0.join("sub").join("..").join("user.db"),
        dir.0.join("user.db-wal"),
        dir.0.join("user.db-shm"),
        dir.0.join("user.db-journal"),
    ];
    let hard_link = dir.0.join("linked.db");
    std::fs::hard_link(dir.db(), &hard_link).unwrap();
    refused.push(hard_link);
    #[cfg(unix)]
    {
        let symlink = dir.0.join("sub").join("alias.db");
        std::os::unix::fs::symlink(dir.db(), &symlink).unwrap();
        refused.push(symlink);
    }
    for dest in &refused {
        let err = (db.back_up(dest).err())
            .unwrap_or_else(|| panic!("{} was written over", dest.display()));
        assert!(
            matches!(&err, StoreError::Rejected(m) if m.contains("where your data is kept")),
            "{}: {err}",
            dest.display()
        );
        assert!(db.refuse_own_file(dest).is_err(), "{}", dest.display());
    }
    // Another file beside it is fine.
    db.refuse_own_file(&dir.0.join("user.db.obsbackup"))
        .unwrap();
    // The database still reads and writes, and reopens with everything.
    assert_eq!(db.annotation_dump().unwrap(), before);
    db.save_layout("After", "{}").unwrap();
    drop(db);
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(db.annotation_dump().unwrap(), before);
    assert_eq!(db.layouts().unwrap(), ["After", "Study"]);
}

/// Opened through a symbolic link, SQLite keeps its `-wal`, `-shm` and `-journal` files beside
/// the link's target: Back up now refuses those too, and the database goes on working.
#[cfg(unix)]
#[test]
fn back_up_refuses_the_sidecar_files_of_a_database_opened_through_a_link() {
    let dir = Dir::new("own-file-link");
    std::fs::create_dir_all(dir.0.join("real")).unwrap();
    std::fs::create_dir_all(dir.0.join("alias")).unwrap();
    let real = dir.0.join("real").join("real.db");
    let alias = dir.0.join("alias").join("alias.db");
    UserStore::open(&real).unwrap();
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let (db, _) = UserStore::open(&alias).unwrap();
    filled(&db);
    let before = db.annotation_dump().unwrap();
    assert!(dir.0.join("real").join("real.db-wal").exists());
    for dest in [
        real.clone(),
        alias.clone(),
        dir.0.join("real").join("real.db-wal"),
        dir.0.join("real").join("real.db-shm"),
        dir.0.join("real").join("real.db-journal"),
        dir.0.join("alias").join("alias.db-wal"),
    ] {
        let err = (db.back_up(&dest).err())
            .unwrap_or_else(|| panic!("{} was written over", dest.display()));
        assert!(
            matches!(&err, StoreError::Rejected(m) if m.contains("where your data is kept")),
            "{}: {err}",
            dest.display()
        );
    }
    assert_eq!(db.annotation_dump().unwrap(), before);
    db.save_layout("After", "{}").unwrap();
    drop(db);
    let (db, _) = UserStore::open(&alias).unwrap();
    assert_eq!(db.annotation_dump().unwrap(), before);
    assert_eq!(db.layouts().unwrap(), ["After", "Study"]);
}

/// Back up now writes beside its destination under a name of its own: a file already there
/// (here, the User database itself, named as a scratch file might be) is never removed or
/// written over.
#[test]
fn back_up_never_removes_a_file_named_like_its_scratch_file() {
    let dir = Dir::new("scratch-name");
    let names = [
        ".mine.obsbackup.partial".to_string(),
        format!(".mine.obsbackup.{}-0.partial", std::process::id()),
    ];
    for name in names {
        let live = dir.0.join(&name);
        let (db, _) = UserStore::open(&live).unwrap();
        filled(&db);
        let before = db.annotation_dump().unwrap();
        let dest = dir.0.join("mine.obsbackup");
        db.back_up(&dest).unwrap();
        assert_eq!(UserStore::backup_contents(&dest).unwrap().favorites, 1);
        assert!(live.is_file(), "{name} was removed");
        assert_eq!(db.annotation_dump().unwrap(), before, "{name}");
        db.save_layout("After", "{}").unwrap();
        drop(db);
        let (db, _) = UserStore::open(&live).unwrap();
        assert_eq!(db.annotation_dump().unwrap(), before, "{name}");
        assert_eq!(db.layouts().unwrap(), ["After", "Study"], "{name}");
        drop(db);
        std::fs::remove_file(&dest).unwrap();
    }
    // No scratch file is left behind.
    let left: Vec<String> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("mine") && n.ends_with(".partial"))
        .filter(|n| !n.ends_with("-0.partial") && n != ".mine.obsbackup.partial")
        .collect();
    assert!(left.is_empty(), "{left:?}");
}
