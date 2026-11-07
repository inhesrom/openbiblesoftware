//! Highlight styles and palettes in the User database (#181, ADR 0012): the migration seeds
//! the five v0.1 styles under their ids, style rows go through checked change lists like
//! Annotations, and a style a Highlight uses can never be deleted from under it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;

use obs_scripture::{Book, Passage, Verse};
use obs_store::annotation::{inverse, new_id};
use obs_store::{
    Anchor, Annotation, Change, Content, HighlightLook, HighlightPalette, HighlightStyle, Rgb, Row,
    STANDARD_PALETTE_ID, StoreError, StyleAppearance, USER_SCHEMA_VERSION, UserStore,
    built_in_styles, standard_palette,
};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("obs-styles-{name}-{}", std::process::id()));
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

fn highlight(db: &UserStore, style: &str) -> Annotation {
    let nb = db.notebooks().unwrap().remove(0);
    Annotation {
        id: new_id(),
        notebook_id: nb.id,
        content: Content::Highlight {
            style: style.into(),
        },
        anchor: Some(Anchor::Passage {
            passage: Passage::verse(Verse::new(Book::Romans, 8, 28)),
            origin: None,
        }),
        created_at: 30,
        modified_at: 30,
    }
}

fn promises() -> HighlightStyle {
    HighlightStyle {
        id: new_id(),
        palette_id: STANDARD_PALETTE_ID.into(),
        position: 5,
        name: "Promises".into(),
        appearance: StyleAppearance::new(HighlightLook::Box, Rgb::new(0xc9, 0xa2, 0x27)),
        created_at: 40,
        modified_at: 40,
    }
}

#[test]
fn a_new_database_has_the_standard_palette_with_the_five_built_in_styles() {
    let dir = Dir::new("new");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(db.highlight_palettes().unwrap(), [standard_palette()]);
    assert_eq!(db.highlight_styles().unwrap(), built_in_styles());
}

/// Migration 9 → 10 keeps every Highlight made before, each still on its style.
#[test]
fn the_v10_migration_seeds_the_built_ins_under_their_v0_1_ids() {
    let dir = Dir::new("migrate10");
    let kept: Vec<Annotation> = {
        let (v9, _) = UserStore::open_to(&dir.db(), 9).unwrap();
        let nb = v9.notebooks().unwrap().remove(0).id;
        drop(v9);
        let c = rusqlite::Connection::open(dir.db()).unwrap();
        ["yellow", "green", "blue", "pink", "red_underline"]
            .iter()
            .map(|style| {
                let id = new_id();
                c.execute(
                    "INSERT INTO annotations (id, notebook_id, kind, style, body, created_at,
                         modified_at) VALUES (?1, ?2, 'highlight', ?3, '', 30, 30)",
                    rusqlite::params![id, nb, style],
                )
                .unwrap();
                c.execute(
                    "INSERT INTO anchors (annotation_id, kind, start_book, start_chapter,
                         start_verse, end_book, end_chapter, end_verse)
                     VALUES (?1, 'passage', 45, 8, 28, 45, 8, 28)",
                    [&id],
                )
                .unwrap();
                Annotation {
                    id,
                    notebook_id: nb.clone(),
                    content: Content::Highlight {
                        style: (*style).into(),
                    },
                    anchor: Some(Anchor::Passage {
                        passage: Passage::verse(Verse::new(Book::Romans, 8, 28)),
                        origin: None,
                    }),
                    created_at: 30,
                    modified_at: 30,
                }
            })
            .collect()
    };
    let (db, report) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(report.found_version, 9);
    assert_eq!(db.schema_version().unwrap(), USER_SCHEMA_VERSION);
    assert_eq!(db.highlight_styles().unwrap(), built_in_styles());
    for a in &kept {
        assert_eq!(db.annotation(&a.id).unwrap().as_ref(), Some(a));
    }
    let uses = db.highlight_style_uses().unwrap();
    assert_eq!(uses.len(), 5, "{uses:?}");
    assert!(uses.iter().all(|(_, n)| *n == 1), "{uses:?}");
}

#[test]
fn style_and_palette_rows_round_trip_and_survive_reopening() {
    let dir = Dir::new("round-trip");
    let style = HighlightStyle {
        appearance: StyleAppearance {
            dark: Some(Rgb::new(0xe8, 0xc4, 0x50)),
            ..promises().appearance
        },
        ..promises()
    };
    let themes = HighlightPalette {
        id: new_id(),
        name: "Themes".into(),
        position: 1,
        created_at: 50,
        modified_at: 50,
    };
    let in_themes = HighlightStyle {
        id: new_id(),
        palette_id: themes.id.clone(),
        position: 0,
        name: String::new(),
        appearance: StyleAppearance::new(HighlightLook::TextColor, Rgb::new(30, 90, 160)),
        ..promises()
    };
    {
        let (db, _) = UserStore::open(&dir.db()).unwrap();
        db.apply_changes(&[
            Change::insert(Row::HighlightStyle(style.clone())),
            Change::insert(Row::HighlightStyle(in_themes.clone())),
            Change::insert(Row::HighlightPalette(themes.clone())),
        ])
        .unwrap();
    }
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    assert_eq!(
        db.highlight_palettes().unwrap(),
        [standard_palette(), themes]
    );
    let mut want = built_in_styles();
    want.push(style);
    want.push(in_themes);
    assert_eq!(db.highlight_styles().unwrap(), want);
}

#[test]
fn a_highlight_needs_an_existing_style() {
    let dir = Dir::new("unknown");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let err = db
        .apply_changes(&[Change::insert(Row::Annotation(highlight(&db, "nope")))])
        .unwrap_err();
    assert!(matches!(err, StoreError::Rejected(_)), "{err}");
    assert!(db.annotations(None).unwrap().is_empty());
}

/// Deleting a used style alone is refused; moving its Highlights first, in the same list,
/// deletes it, and the inverted list brings back the style and the Highlights on it.
#[test]
fn a_used_style_is_deleted_only_with_its_highlights_moved_and_undo_restores_both() {
    let dir = Dir::new("delete");
    let (db, _) = UserStore::open(&dir.db()).unwrap();
    let style = promises();
    db.apply_changes(&[Change::insert(Row::HighlightStyle(style.clone()))])
        .unwrap();
    let h = highlight(&db, &style.id);
    db.apply_changes(&[Change::insert(Row::Annotation(h.clone()))])
        .unwrap();
    let before = db.annotation_dump().unwrap();

    let err = db
        .apply_changes(&[Change::delete(Row::HighlightStyle(style.clone()))])
        .unwrap_err();
    assert!(matches!(err, StoreError::Rejected(_)), "{err}");
    assert!(err.to_string().contains("1 Highlight"), "{err}");
    assert_eq!(db.annotation_dump().unwrap(), before, "nothing was written");

    let moved = Annotation {
        content: Content::Highlight {
            style: "green".into(),
        },
        ..h.clone()
    };
    let changes = [
        Change::update(Row::Annotation(h.clone()), Row::Annotation(moved.clone())),
        Change::delete(Row::HighlightStyle(style.clone())),
    ];
    db.apply_changes(&changes).unwrap();
    assert_eq!(db.annotation(&h.id).unwrap(), Some(moved));
    assert!(!db.highlight_styles().unwrap().contains(&style));

    db.apply_changes(&inverse(&changes)).unwrap();
    assert_eq!(db.annotation_dump().unwrap(), before);
    assert_eq!(db.annotation(&h.id).unwrap(), Some(h));
    assert!(db.highlight_styles().unwrap().contains(&style));
}
