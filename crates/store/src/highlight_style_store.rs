//! Highlight styles and Highlight palettes in the User database (#181, ADR 0012).
//!
//! They are rows of the annotation tables: every edit is a [`Change`](crate::Change) applied
//! by [`UserStore::apply_changes`](crate::UserStore::apply_changes) with the Annotations it
//! touches, so deleting a style and moving its Highlights is one undo step.

use rusqlite::{Connection, OptionalExtension, Row as SqlRow, params};

use crate::error::StoreError;
use crate::highlight_style::{HighlightLook, HighlightPalette, HighlightStyle, StyleAppearance};
use crate::user::{UserStore, sql};

/// Migration 9 → 10 (#181).
///
/// - `highlight_palettes`: named, ordered groups of styles.
/// - `highlight_styles`: a colour (`#rrggbb`) with a look, an optional name and an optional
///   adjusted colour per Page tone (NULL: derived when drawn). Each is in one palette.
/// - The Standard palette and the five v0.1 styles with their ids, so every Highlight made
///   before keeps its style ([`crate::built_in_styles`]; a test checks the two agree).
///
/// `annotations.style` stays free text: the store checks a written Highlight's style exists
/// and refuses to delete a style a Highlight uses.
pub(crate) const SCHEMA_10: &str = "
CREATE TABLE highlight_palettes (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    position    INTEGER NOT NULL,
    created_at  INTEGER NOT NULL,
    modified_at INTEGER NOT NULL
);
CREATE TABLE highlight_styles (
    id          TEXT PRIMARY KEY,
    palette_id  TEXT NOT NULL
                REFERENCES highlight_palettes (id) DEFERRABLE INITIALLY DEFERRED,
    position    INTEGER NOT NULL,
    name        TEXT NOT NULL DEFAULT '',
    look        TEXT NOT NULL CHECK (look IN ('background', 'underline', 'box', 'text_color')),
    color       TEXT NOT NULL CHECK (length(color) = 7),
    paper       TEXT CHECK (paper IS NULL OR length(paper) = 7),
    sepia       TEXT CHECK (sepia IS NULL OR length(sepia) = 7),
    dark        TEXT CHECK (dark IS NULL OR length(dark) = 7),
    created_at  INTEGER NOT NULL,
    modified_at INTEGER NOT NULL
);
CREATE INDEX highlight_styles_palette ON highlight_styles (palette_id, position);
CREATE INDEX annotations_style ON annotations (style) WHERE style IS NOT NULL;
INSERT INTO highlight_palettes (id, name, position, created_at, modified_at)
    VALUES ('standard', 'Standard', 0, 0, 0);
INSERT INTO highlight_styles (id, palette_id, position, name, look, color, paper, sepia, dark,
                              created_at, modified_at) VALUES
    ('yellow', 'standard', 0, 'Yellow', 'background', '#ffec8c', NULL, '#f8e080', '#463c10',
     0, 0),
    ('green', 'standard', 1, 'Green', 'background', '#c8eebe', NULL, '#cce2b0', '#1a3824',
     0, 0),
    ('blue', 'standard', 2, 'Blue', 'background', '#c4defa', NULL, '#c4d6e8', '#182e4c',
     0, 0),
    ('pink', 'standard', 3, 'Pink', 'background', '#facee0', NULL, '#f0c8d0', '#482032',
     0, 0),
    ('red_underline', 'standard', 4, 'Red underline', 'underline', '#c82828', NULL, '#b82820',
     '#f07878', 0, 0);
";

const STYLE_COLUMNS: &str = "id, palette_id, position, name, look, color, paper, sepia, dark,
     created_at, modified_at";

const PALETTE_COLUMNS: &str = "id, name, position, created_at, modified_at";

fn corrupt(what: String) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(what)))
}

fn rgb(r: &SqlRow<'_>, at: usize) -> rusqlite::Result<Option<crate::Rgb>> {
    r.get::<_, Option<String>>(at)?
        .map(|s| s.parse().map_err(corrupt))
        .transpose()
}

fn style_from(r: &SqlRow<'_>) -> rusqlite::Result<HighlightStyle> {
    let look: String = r.get(4)?;
    Ok(HighlightStyle {
        id: r.get(0)?,
        palette_id: r.get(1)?,
        position: r.get(2)?,
        name: r.get(3)?,
        appearance: StyleAppearance {
            look: HighlightLook::from_id(&look)
                .ok_or_else(|| corrupt(format!("Highlight look `{look}`")))?,
            color: rgb(r, 5)?.ok_or_else(|| corrupt("a style without a colour".into()))?,
            paper: rgb(r, 6)?,
            sepia: rgb(r, 7)?,
            dark: rgb(r, 8)?,
        },
        created_at: r.get(9)?,
        modified_at: r.get(10)?,
    })
}

fn palette_from(r: &SqlRow<'_>) -> rusqlite::Result<HighlightPalette> {
    Ok(HighlightPalette {
        id: r.get(0)?,
        name: r.get(1)?,
        position: r.get(2)?,
        created_at: r.get(3)?,
        modified_at: r.get(4)?,
    })
}

pub(crate) fn read_style(c: &Connection, id: &str) -> Result<Option<HighlightStyle>, StoreError> {
    c.prepare_cached(&format!(
        "SELECT {STYLE_COLUMNS} FROM highlight_styles WHERE id = ?1"
    ))
    .and_then(|mut s| s.query_row([id], style_from).optional())
    .map_err(sql)
}

pub(crate) fn read_palette(
    c: &Connection,
    id: &str,
) -> Result<Option<HighlightPalette>, StoreError> {
    c.prepare_cached(&format!(
        "SELECT {PALETTE_COLUMNS} FROM highlight_palettes WHERE id = ?1"
    ))
    .and_then(|mut s| s.query_row([id], palette_from).optional())
    .map_err(sql)
}

/// How many Highlights use style `id`.
fn uses(c: &Connection, id: &str) -> Result<u64, StoreError> {
    c.query_row(
        "SELECT COUNT(*) FROM annotations WHERE style = ?1",
        [id],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| u64::try_from(n).unwrap_or(0))
    .map_err(sql)
}

/// Whether style `id` exists (a written Highlight must have one). A database opened at a
/// schema before 10 (migration tests write through it) has no styles to check against.
pub(crate) fn style_exists(c: &Connection, id: &str) -> Result<bool, StoreError> {
    let has_table: bool = c
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'highlight_styles')",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if !has_table {
        return Ok(true);
    }
    c.query_row(
        "SELECT EXISTS (SELECT 1 FROM highlight_styles WHERE id = ?1)",
        [id],
        |r| r.get(0),
    )
    .map_err(sql)
}

/// Write style `id` as `s`, or delete it (refused while a Highlight uses it: Highlights are
/// moved to another style first, in the same Change list).
pub(crate) fn write_style(
    c: &Connection,
    id: &str,
    s: Option<&HighlightStyle>,
) -> Result<(), StoreError> {
    let Some(s) = s else {
        let n = uses(c, id)?;
        if n > 0 {
            return Err(StoreError::Rejected(format!(
                "Highlight style {id} is used by {n} Highlight(s): move them to another style \
                 first"
            )));
        }
        return c
            .execute("DELETE FROM highlight_styles WHERE id = ?1", [id])
            .map(drop)
            .map_err(sql);
    };
    let a = &s.appearance;
    let hex = |c: Option<crate::Rgb>| c.map(|c| c.to_string());
    c.execute(
        "INSERT INTO highlight_styles (id, palette_id, position, name, look, color, paper,
             sepia, dark, created_at, modified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT (id) DO UPDATE SET palette_id = excluded.palette_id,
             position = excluded.position, name = excluded.name, look = excluded.look,
             color = excluded.color, paper = excluded.paper, sepia = excluded.sepia,
             dark = excluded.dark, created_at = excluded.created_at,
             modified_at = excluded.modified_at",
        params![
            s.id,
            s.palette_id,
            s.position,
            s.name,
            a.look.id(),
            a.color.to_string(),
            hex(a.paper),
            hex(a.sepia),
            hex(a.dark),
            s.created_at,
            s.modified_at
        ],
    )
    .map(drop)
    .map_err(sql)
}

/// Write palette `id` as `p`, or delete it (its styles are deleted by Changes of their own).
pub(crate) fn write_palette(
    c: &Connection,
    id: &str,
    p: Option<&HighlightPalette>,
) -> Result<(), StoreError> {
    match p {
        None => c
            .execute("DELETE FROM highlight_palettes WHERE id = ?1", [id])
            .map(drop),
        Some(p) => c
            .execute(
                "INSERT INTO highlight_palettes (id, name, position, created_at, modified_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (id) DO UPDATE SET name = excluded.name,
                     position = excluded.position, created_at = excluded.created_at,
                     modified_at = excluded.modified_at",
                params![p.id, p.name, p.position, p.created_at, p.modified_at],
            )
            .map(drop),
    }
    .map_err(sql)
}

impl UserStore {
    /// Every Highlight palette, in order.
    pub fn highlight_palettes(&self) -> Result<Vec<HighlightPalette>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!(
                "SELECT {PALETTE_COLUMNS} FROM highlight_palettes ORDER BY position, id"
            ))
            .map_err(sql)?;
        let rows = stmt.query_map([], palette_from).map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    /// Every Highlight style, by palette order and then in order within its palette.
    pub fn highlight_styles(&self) -> Result<Vec<HighlightStyle>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT s.id, s.palette_id, s.position, s.name, s.look, s.color, s.paper,
                     s.sepia, s.dark, s.created_at, s.modified_at
                 FROM highlight_styles s JOIN highlight_palettes p ON p.id = s.palette_id
                 ORDER BY p.position, p.id, s.position, s.id",
            )
            .map_err(sql)?;
        let rows = stmt.query_map([], style_from).map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    /// How many Highlights use each style (styles no Highlight uses are left out).
    pub fn highlight_style_uses(&self) -> Result<Vec<(String, u64)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT style, COUNT(*) FROM annotations WHERE style IS NOT NULL
                 GROUP BY style",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0)))
            })
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }
}
