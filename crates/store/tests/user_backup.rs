//! Backups of the User database (#217): Back up now, the restore preview's contents, restore
//! with its Safety backup, migrations on restore, and the list of kept Backups.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use obs_scripture::{Book, Passage, Verse};
use obs_store::annotation::new_id;
use obs_store::{
    Anchor, Annotation, BackupKind, Change, Content, HighlightStyle, Notebook, Row, SAFETY_BACKUPS,
    StoreError, USER_SCHEMA_VERSION, UserStore,
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
            style: HighlightStyle::Yellow,
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
