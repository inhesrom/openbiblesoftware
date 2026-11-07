//! The annotation tables of the User database (#61): the default Notebook, row round trips,
//! checked and atomic change lists, and `notes_fts` kept in step with Note bodies.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use obs_model::UnitId;
use obs_scripture::{Book, Passage, Verse};
use obs_store::annotation::{inverse, new_id};
use obs_store::{
    Anchor, Annotation, Change, Content, DEFAULT_NOTEBOOK_NAME, Notebook, Row, StoreError,
    TextRange, USER_SCHEMA_VERSION, UserStore,
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
        show_in_text: true,
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
        whole_verses: false,
        orphaned: false,
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
            style: "green".into(),
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

/// Migration 4 → 5 (#63): every existing Notebook shows in text, and Show in text and
/// whole-Verse coverage round-trip.
#[test]
fn the_v5_migration_adds_show_in_text_and_whole_verse_coverage() {
    let dir = Dir::new("migrate5");
    let before = {
        let (v4, _) = UserStore::open_to(&dir.db(), 4).unwrap();
        assert_eq!(v4.schema_version().unwrap(), 4);
        v4.annotation_dump().unwrap()
    };
    let (db, report) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(report.found_version, 4);
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    let default = default_of(&db);
    assert!(
        before.contains(&default.id),
        "the v4 default Notebook is kept"
    );
    assert!(default.show_in_text, "existing Notebooks show in text");

    // Show in text is a Notebook column; whole_verses a Text-range column.
    let hidden = Notebook {
        show_in_text: false,
        ..notebook("Hidden")
    };
    let whole = highlight(
        &hidden.id,
        Anchor::Passage {
            passage: Passage::verse(jn(16)),
            origin: Some(TextRange {
                whole_verses: true,
                ..range("God")
            }),
        },
    );
    db.apply_changes(&[
        Change::insert(Row::Notebook(hidden.clone())),
        Change::insert(Row::Annotation(whole.clone())),
    ])
    .unwrap();
    assert_eq!(db.notebook(&hidden.id).unwrap(), Some(hidden.clone()));
    assert_eq!(db.annotation(&whole.id).unwrap(), Some(whole));
    let shown = Notebook {
        show_in_text: true,
        ..hidden.clone()
    };
    db.apply_changes(&[Change::update(
        Row::Notebook(hidden),
        Row::Notebook(shown.clone()),
    )])
    .unwrap();
    assert_eq!(db.notebook(&shown.id).unwrap(), Some(shown));
}

/// Migration 5 → 6 (#65): existing anchors read as not orphaned; an Orphaned annotation's
/// Text-range anchor round-trips with its quote, as does a Passage anchor's origin that was
/// not found; Resource names are remembered.
#[test]
fn the_v6_migration_adds_the_orphaned_status_and_resource_names() {
    let dir = Dir::new("migrate6");
    let kept = {
        let (v5, _) = UserStore::open_to(&dir.db(), 5).unwrap();
        let nb = default_of(&v5);
        drop(v5);
        // A Text-range anchor as schema 5 wrote it (no status column yet).
        let a = highlight(
            &nb.id,
            Anchor::TextRange {
                range: range("God"),
            },
        );
        let r = range("God");
        let c = rusqlite::Connection::open(dir.db()).unwrap();
        c.execute(
            "INSERT INTO annotations (id, notebook_id, kind, style, body, created_at, modified_at)
             VALUES (?1, ?2, 'highlight', 'green', '', ?3, ?4)",
            rusqlite::params![a.id, nb.id, a.created_at, a.modified_at],
        )
        .unwrap();
        c.execute(
            "INSERT INTO anchors (annotation_id, kind, resource_id, unit_id, start_byte,
                 end_byte, quote, prefix, suffix, content_hash)
             VALUES (?1, 'text_range', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                a.id,
                r.resource_id,
                r.unit.as_str(),
                r.start,
                r.end,
                r.quote,
                r.prefix,
                r.suffix,
                r.content_hash
            ],
        )
        .unwrap();
        a
    };
    let (db, report) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(report.found_version, 5);
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    let read = db.annotation(&kept.id).unwrap().unwrap();
    assert_eq!(read, kept);
    assert!(!read.is_orphaned());

    let orphaned = Annotation {
        anchor: Some(Anchor::TextRange {
            range: TextRange {
                orphaned: true,
                ..range("God")
            },
        }),
        ..read.clone()
    };
    db.apply_changes(&[Change::update(
        Row::Annotation(read),
        Row::Annotation(orphaned.clone()),
    )])
    .unwrap();
    let back = db.annotation(&kept.id).unwrap().unwrap();
    assert!(back.is_orphaned());
    assert_eq!(back, orphaned);
    assert!(db.annotation_dump().unwrap().contains("orphaned"));

    let nb = default_of(&db);
    let kept_origin = highlight(
        &nb.id,
        Anchor::Passage {
            passage: Passage::verse(jn(16)),
            origin: Some(TextRange {
                orphaned: true,
                ..range("God")
            }),
        },
    );
    // A Passage anchor's origin that was not found is kept, marked, and round-trips; the
    // Annotation itself is not orphaned (it shows as whole Verses).
    db.apply_changes(&[Change::insert(Row::Annotation(kept_origin.clone()))])
        .unwrap();
    let back = db.annotation(&kept_origin.id).unwrap().unwrap();
    assert_eq!(back, kept_origin);
    assert!(!back.is_orphaned());

    // The last known Resource names: none after the migration; saved ones are replaced
    // by id and others kept.
    assert!(db.resource_names().unwrap().is_empty());
    let name =
        |id: &str, abbr: &str, title: &str| (id.to_string(), abbr.to_string(), title.to_string());
    db.save_resource_names(&[
        name("web", "WEB", "World English Bible"),
        name("tcm", "TCM", "Old"),
    ])
    .unwrap();
    db.save_resource_names(&[name("tcm", "TCM2", "Test Commentary")])
        .unwrap();
    assert_eq!(
        db.resource_names().unwrap(),
        vec![
            name("tcm", "TCM2", "Test Commentary"),
            name("web", "WEB", "World English Bible")
        ]
    );
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

/// Deleting a Highlight never touches `notes_fts` (#115): finding an entry there scans the
/// whole index, so deleting a Notebook of N Highlights must not cost N scans. Proved by
/// dropping the index: Highlights still delete, a Note does not.
#[test]
fn deleting_highlights_skips_the_note_index() {
    let dir = Dir::new("fts-delete");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let nb = notebook("Highlights only");
    let marks: Vec<Annotation> = ["God", "loved", "world"]
        .into_iter()
        .map(|q| highlight(&nb.id, Anchor::TextRange { range: range(q) }))
        .collect();
    let n = note(&default_of(&db).id, "kept", None);
    let mut made = vec![
        Change::insert(Row::Notebook(nb.clone())),
        Change::insert(Row::Annotation(n.clone())),
    ];
    made.extend(
        marks
            .iter()
            .map(|a| Change::insert(Row::Annotation(a.clone()))),
    );
    db.apply_changes(&made).unwrap();

    let raw = rusqlite::Connection::open(dir.db()).unwrap();
    raw.execute_batch("DROP TABLE notes_fts").unwrap();
    // Delete the Notebook with its Highlights, as `notebook.delete` with `delete` does.
    let mut gone: Vec<Change> = marks
        .iter()
        .map(|a| Change::delete(Row::Annotation(a.clone())))
        .collect();
    gone.push(Change::delete(Row::Notebook(nb.clone())));
    db.apply_changes(&gone).unwrap();
    assert_eq!(db.annotations(None).unwrap(), vec![n.clone()]);
    assert_eq!(db.notebook(&nb.id).unwrap(), None);
    // A Note does have an index entry to delete.
    let err = db
        .apply_changes(&[Change::delete(Row::Annotation(n.clone()))])
        .unwrap_err();
    assert!(err.to_string().contains("notes_fts"), "{err}");
    assert_eq!(db.annotation(&n.id).unwrap(), Some(n));
}

/// A stored Text range covers some text (#115): the model refuses an empty one before SQL,
/// and the CHECK refuses one written past it (equal offsets, an empty quote, a missing end).
#[test]
fn a_text_range_that_covers_no_text_is_refused() {
    let dir = Dir::new("empty-range");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let nb = default_of(&db).id;
    let empty_quote = TextRange {
        quote: String::new(),
        ..range("God")
    };
    let no_bytes = TextRange {
        end: 4,
        ..range("God")
    };
    for bad in [&empty_quote, &no_bytes] {
        for a in [
            highlight(&nb, Anchor::TextRange { range: bad.clone() }),
            note(
                &nb,
                "x",
                Some(Anchor::Passage {
                    passage: Passage::verse(jn(16)),
                    origin: Some(bad.clone()),
                }),
            ),
        ] {
            let err = db
                .apply_changes(&[Change::insert(Row::Annotation(a))])
                .unwrap_err();
            assert!(
                matches!(&err, StoreError::Rejected(why) if why.contains("cover some text")),
                "{err}"
            );
        }
    }
    assert!(db.annotations(None).unwrap().is_empty());

    let ok = highlight(
        &nb,
        Anchor::TextRange {
            range: range("God"),
        },
    );
    db.apply_changes(&[Change::insert(Row::Annotation(ok.clone()))])
        .unwrap();
    let raw = rusqlite::Connection::open(dir.db()).unwrap();
    for (what, set) in [
        ("equal offsets", "end_byte = start_byte"),
        ("an empty quote", "quote = ''"),
        ("no end", "end_byte = NULL"),
    ] {
        let err = raw
            .execute(
                &format!("UPDATE anchors SET {set} WHERE annotation_id = ?1"),
                [&ok.id],
            )
            .unwrap_err();
        assert!(err.to_string().contains("CHECK"), "{what}: {err}");
    }
    assert_eq!(db.annotation(&ok.id).unwrap(), Some(ok));
}

/// Migration 8 → 9 (#115) rebuilds `anchors` with the stricter CHECK. Rows that cover no
/// text, which the app never wrote, are neither deleted nor repaired (ADR 0006: nothing is
/// deleted automatically): the migration refuses, names their Annotations and leaves the
/// database at schema 8 as it was. Once they are fixed it runs and keeps every row.
#[test]
fn the_v9_migration_refuses_text_ranges_that_cover_no_text_and_keeps_every_row() {
    let dir = Dir::new("migrate9");
    let (nb, good, bad) = {
        let (v8, _) = UserStore::open_to(&dir.db(), 8).unwrap();
        assert_eq!(v8.schema_version().unwrap(), 8);
        let nb = default_of(&v8).id;
        let good = highlight(
            &nb,
            Anchor::Passage {
                passage: Passage::verse(jn(16)),
                origin: Some(TextRange {
                    whole_verses: true,
                    orphaned: true,
                    ..range("God")
                }),
            },
        );
        let good_note = note(
            &nb,
            "Grace upon grace",
            Some(Anchor::TextRange {
                range: TextRange {
                    native_ref: Some("Esth.10.4".into()),
                    ..range("Mardocheus")
                },
            }),
        );
        v8.apply_changes(&[
            Change::insert(Row::Annotation(good.clone())),
            Change::insert(Row::Annotation(good_note.clone())),
        ])
        .unwrap();
        drop(v8);
        // Rows schema 8 accepted although they cover no text, written past the model.
        // As read back once fixed by hand: each covers "x".
        let bad = vec![
            highlight(
                &nb,
                Anchor::Passage {
                    passage: Passage::verse(jn(17)),
                    origin: Some(range("x")),
                },
            ),
            note(&nb, "", Some(Anchor::TextRange { range: range("x") })),
            highlight(&nb, Anchor::TextRange { range: range("x") }),
        ];
        let c = rusqlite::Connection::open(dir.db()).unwrap();
        for (a, kind) in bad.iter().zip(["highlight", "note", "highlight"]) {
            c.execute(
                "INSERT INTO annotations (id, notebook_id, kind, style, body, created_at,
                     modified_at)
                 VALUES (?1, ?2, ?3, ?4, '', ?5, ?5)",
                rusqlite::params![
                    a.id,
                    nb,
                    kind,
                    (kind == "highlight").then_some("green"),
                    a.created_at
                ],
            )
            .unwrap();
        }
        let r = range("x");
        for (id, kind, book, end, quote) in [
            (&bad[0].id, "passage", Some(43), Some(r.end), ""),
            (&bad[1].id, "text_range", None, Some(r.start), "x"),
            (&bad[2].id, "text_range", None, None, "x"),
        ] {
            c.execute(
                "INSERT INTO anchors (annotation_id, kind, start_book, start_chapter,
                     start_verse, end_book, end_chapter, end_verse, resource_id, unit_id,
                     start_byte, end_byte, quote, prefix, suffix, content_hash)
                 VALUES (?1, ?2, ?3, 3, 17, ?3, 3, 17, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    id,
                    kind,
                    book,
                    r.resource_id,
                    r.unit.as_str(),
                    r.start,
                    end,
                    quote,
                    r.prefix,
                    r.suffix,
                    r.content_hash
                ],
            )
            .unwrap();
        }
        (nb, vec![good, good_note], bad)
    };
    let dump = |c: &rusqlite::Connection| -> Vec<String> {
        c.prepare("SELECT annotation_id || ':' || coalesce(end_byte, 'NULL') || ':' || quote FROM anchors ORDER BY 1")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    let raw = rusqlite::Connection::open(dir.db()).unwrap();
    let before = dump(&raw);
    assert_eq!(before.len(), 5);
    let Err(StoreError::Rejected(why)) = UserStore::open(&dir.db()) else {
        panic!("the migration ran over Text ranges that cover no text");
    };
    assert!(
        why.contains("3 stored Text range(s) cover no text"),
        "{why}"
    );
    assert!(why.contains("Nothing was changed"), "{why}");
    for a in &bad {
        assert!(why.contains(&a.id), "{why}");
    }
    for a in &good {
        assert!(!why.contains(&a.id), "{why}");
    }
    let version: u32 = raw
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 8);
    assert_eq!(dump(&raw), before);
    let rows: i64 = raw
        .query_row("SELECT count(*) FROM annotations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 5);
    // Fixed as the message says, the migration runs and keeps every row.
    for a in &bad {
        raw.execute(
            "UPDATE anchors SET end_byte = start_byte + 1, quote = 'x' WHERE annotation_id = ?1",
            [&a.id],
        )
        .unwrap();
    }
    let (db, report) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(report.found_version, 8);
    assert!(report.migration_backup.unwrap().is_file());
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    for a in good.iter().chain(&bad) {
        assert_eq!(db.annotation(&a.id).unwrap().as_ref(), Some(a));
    }
    assert_eq!(db.annotations(Some(&nb)).unwrap().len(), 5);
    drop(raw);
    // The rebuilt table keeps its indexes and enforces the new CHECK.
    let raw = rusqlite::Connection::open(dir.db()).unwrap();
    let indexes: Vec<String> = raw
        .prepare(
            "SELECT name FROM sqlite_master WHERE tbl_name = 'anchors' AND type = 'index'
                  AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(indexes, ["anchors_passage", "anchors_resource"]);
    let err = raw
        .execute(
            "UPDATE anchors SET quote = '' WHERE annotation_id = ?1",
            [&good[0].id],
        )
        .unwrap_err();
    assert!(err.to_string().contains("CHECK"), "{err}");
    // Undo still lands: a migrated row deletes and comes back unchanged.
    let back = good[1].clone();
    db.apply_changes(&[Change::delete(Row::Annotation(back.clone()))])
        .unwrap();
    db.apply_changes(&[Change::insert(Row::Annotation(back.clone()))])
        .unwrap();
    assert_eq!(db.annotation(&back.id).unwrap(), Some(back));
}
