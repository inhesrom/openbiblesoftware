//! The annotation tables of the User database (#61): the default Notebook, row round trips,
//! checked and atomic change lists, and `notes_fts` kept in step with Note bodies.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use obs_model::UnitId;
use obs_scripture::{Book, Passage, Verse};
use obs_store::annotation::{inverse, new_id};
use obs_store::{
    Anchor, Annotation, Change, Content, DEFAULT_NOTEBOOK_NAME, HighlightStyle, Notebook, Row,
    StoreError, TextRange, USER_SCHEMA_VERSION, UserStore,
};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("obs-annot-{name}-{}", std::process::id()));
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
        created_at: 10,
        modified_at: 10,
    }
}

fn range(quote: &str) -> TextRange {
    TextRange {
        resource_id: "kjv".into(),
        unit: UnitId::new("JHN").unwrap(),
        start: 4,
        end: 4 + quote.len() as u32,
        quote: quote.into(),
        prefix: "For ".into(),
        suffix: " so loved".into(),
        content_hash: "sha256:abc".into(),
        native_ref: None,
    }
}

fn jn(v: u16) -> Verse {
    Verse::new(Book::John, 3, v)
}

fn note(notebook: &str, body: &str, anchor: Option<Anchor>) -> Annotation {
    Annotation {
        id: new_id(),
        notebook_id: notebook.into(),
        content: Content::Note { body: body.into() },
        anchor,
        created_at: 20,
        modified_at: 20,
    }
}

fn highlight(notebook: &str, anchor: Anchor) -> Annotation {
    Annotation {
        id: new_id(),
        notebook_id: notebook.into(),
        content: Content::Highlight {
            style: HighlightStyle::Green,
        },
        anchor: Some(anchor),
        created_at: 30,
        modified_at: 30,
    }
}

fn default_of(db: &UserStore) -> Notebook {
    let all = db.notebooks().unwrap();
    let defaults: Vec<_> = all.iter().filter(|n| n.is_default).collect();
    assert_eq!(defaults.len(), 1, "{all:?}");
    defaults[0].clone()
}

#[test]
fn a_default_notebook_always_exists_and_survives_reopening() {
    let dir = Dir::new("default");
    let first = {
        let (db, _) = UserStore::open(&dir.db()).unwrap();
        let d = default_of(&db);
        assert_eq!(d.name, DEFAULT_NOTEBOOK_NAME);
        d
    };
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(default_of(&db), first);
    // It cannot be deleted, nor stop being the default.
    let err = db
        .apply_changes(&[Change::delete(Row::Notebook(first.clone()))])
        .unwrap_err();
    assert!(matches!(err, StoreError::Rejected(_)), "{err}");
    let demoted = Notebook {
        is_default: false,
        ..first.clone()
    };
    assert!(
        db.apply_changes(&[Change::update(
            Row::Notebook(first.clone()),
            Row::Notebook(demoted)
        )])
        .is_err()
    );
    // A second default is refused by the database.
    let second = Notebook {
        is_default: true,
        ..notebook("Another")
    };
    assert!(
        db.apply_changes(&[Change::insert(Row::Notebook(second))])
            .is_err()
    );
    assert_eq!(default_of(&db), first);
}

#[test]
fn the_v4_migration_adds_the_annotation_tables_and_a_default_notebook() {
    let dir = Dir::new("migrate");
    {
        let (v3, _) = UserStore::open_to(&dir.db(), 3).unwrap();
        v3.set_preference("theme", "dark").unwrap();
    }
    let (db, report) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(report.found_version, 3);
    assert!(report.migration_backup.unwrap().is_file());
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    assert_eq!(db.preference("theme").unwrap().as_deref(), Some("dark"));
    default_of(&db);
}

#[test]
fn notebooks_and_annotations_round_trip_with_both_anchor_kinds() {
    let dir = Dir::new("roundtrip");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let nb = notebook("Sermon prep");
    let passage = Passage::new(jn(16), jn(17)).unwrap();
    let a = highlight(
        &nb.id,
        Anchor::Passage {
            passage,
            origin: Some(range("God")),
        },
    );
    let b = note(
        &nb.id,
        "Love *is* the point.",
        Some(Anchor::Passage {
            passage: Passage::new(
                Verse::new(Book::Malachi, 4, 6),
                Verse::new(Book::Matthew, 1, 1),
            )
            .unwrap(),
            origin: None,
        }),
    );
    let c = note(
        &default_of(&db).id,
        "Greek addition",
        Some(Anchor::TextRange {
            range: TextRange {
                native_ref: Some("Esth.10.4".into()),
                unit: UnitId::new("EST").unwrap(),
                ..range("And Mardocheus said")
            },
        }),
    );
    let d = note(&nb.id, "", None);
    let mut changes = vec![Change::insert(Row::Notebook(nb.clone()))];
    for x in [&a, &b, &c, &d] {
        changes.push(Change::insert(Row::Annotation(x.clone())));
    }
    db.apply_changes(&changes).unwrap();
    for x in [&a, &b, &c, &d] {
        assert_eq!(db.annotation(&x.id).unwrap().as_ref(), Some(x));
    }
    assert_eq!(db.notebook(&nb.id).unwrap(), Some(nb.clone()));
    assert_eq!(db.annotations(Some(&nb.id)).unwrap().len(), 3);
    assert_eq!(db.annotations(None).unwrap().len(), 4);
    assert_eq!(db.annotations_referring("kjv").unwrap(), 2);
    let counts = db.notebook_counts().unwrap();
    assert!(counts.contains(&(nb.id.clone(), 3)), "{counts:?}");
    // A Highlight without an Anchor breaks the model and is refused.
    let mut bare = highlight(&nb.id, Anchor::TextRange { range: range("x") });
    bare.anchor = None;
    assert!(matches!(
        db.apply_changes(&[Change::insert(Row::Annotation(bare))]),
        Err(StoreError::Rejected(_))
    ));
}

#[test]
fn a_stale_or_broken_change_list_writes_nothing() {
    let dir = Dir::new("atomic");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let nb = notebook("A");
    let n = note(&nb.id, "first", None);
    db.apply_changes(&[
        Change::insert(Row::Notebook(nb.clone())),
        Change::insert(Row::Annotation(n.clone())),
    ])
    .unwrap();
    let before = db.annotation_dump().unwrap();
    let edited = Annotation {
        content: Content::Note {
            body: "second".into(),
        },
        ..n.clone()
    };
    // The first change is fine, the second expects a row that is not there.
    let stale = vec![
        Change::update(Row::Annotation(n.clone()), Row::Annotation(edited.clone())),
        Change::delete(Row::Notebook(notebook("ghost"))),
    ];
    assert!(matches!(
        db.apply_changes(&stale),
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(db.annotation_dump().unwrap(), before);
    // Deleting a Notebook that still holds Annotations breaks a foreign key.
    assert!(
        db.apply_changes(&[Change::delete(Row::Notebook(nb.clone()))])
            .is_err()
    );
    assert_eq!(db.annotation_dump().unwrap(), before);
    // Applying a list and then its inverse restores the same database.
    let edit = vec![
        Change::update(Row::Annotation(n.clone()), Row::Annotation(edited)),
        Change::insert(Row::Notebook(notebook("B"))),
    ];
    db.apply_changes(&edit).unwrap();
    assert_ne!(db.annotation_dump().unwrap(), before);
    db.apply_changes(&inverse(&edit)).unwrap();
    assert_eq!(db.annotation_dump().unwrap(), before);
}

#[test]
fn notes_fts_matches_body_edits_immediately() {
    let dir = Dir::new("fts");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let nb = default_of(&db).id;
    let n = note(&nb, "Amazing Grâce, how sweet", None);
    let h = highlight(
        &nb,
        Anchor::TextRange {
            range: range("God"),
        },
    );
    db.apply_changes(&[
        Change::insert(Row::Annotation(n.clone())),
        Change::insert(Row::Annotation(h)),
    ])
    .unwrap();
    // Matching is on normalized words: case and diacritics fold.
    assert_eq!(
        db.search_notes("\"grace\"").unwrap(),
        std::slice::from_ref(&n.id)
    );
    assert!(db.search_notes("\"god\"").unwrap().is_empty());
    let edited = Annotation {
        content: Content::Note {
            body: "Mercy endures".into(),
        },
        modified_at: 21,
        ..n.clone()
    };
    db.apply_changes(&[Change::update(
        Row::Annotation(n.clone()),
        Row::Annotation(edited.clone()),
    )])
    .unwrap();
    assert!(db.search_notes("\"grace\"").unwrap().is_empty());
    assert_eq!(
        db.search_notes("\"mercy\"").unwrap(),
        std::slice::from_ref(&n.id)
    );
    // Deleting the Note removes its entry.
    db.apply_changes(&[Change::delete(Row::Annotation(edited))])
        .unwrap();
    assert!(db.search_notes("\"mercy\"").unwrap().is_empty());
    db.annotation_dump().unwrap();
}
