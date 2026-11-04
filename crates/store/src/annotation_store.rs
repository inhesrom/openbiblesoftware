//! Notebooks, Annotations, Anchors and the Note index in the User database (#17, #61).
//!
//! Every edit goes through [`UserStore::apply_changes`]: a list of row [`Change`]s written in
//! one transaction, `notes_fts` included, after checking that each row is still what the
//! Change expects. Undo applies the inverted list the same way.

use std::fmt::Write as _;

use obs_model::UnitId;
use obs_scripture::{Book, Passage, Verse};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, Row as SqlRow, params};

use crate::annotation::{
    Anchor, Annotation, Change, Content, HighlightStyle, Notebook, Row, RowKey, TextRange,
};
use crate::error::StoreError;
use crate::user::{UserStore, sql};

/// Migration 3 → 4.
///
/// - `notebooks`: one row has `is_default = 1` (a partial unique index allows only one).
/// - `annotations`: `kind` is `note` or `highlight`; a Highlight has a `style`, a Note a
///   `body`. Foreign keys are deferred so one transaction can move rows in any order.
/// - `anchors`: at most one per Annotation. Passage anchors keep Standard Verses as canonical
///   book number (Genesis = 1 … Revelation = 66), chapter and verse. A stored Text range
///   always has its quote, context and content hash (ADR 0006), enforced by a CHECK.
/// - `notes_fts`: Note bodies as normalized words (the Search index's form), written in the
///   same transaction as the Note.
pub(crate) const SCHEMA: &str = "
CREATE TABLE notebooks (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    is_default  INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at  INTEGER NOT NULL,
    modified_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX notebooks_one_default ON notebooks (is_default) WHERE is_default = 1;
CREATE TABLE annotations (
    id          TEXT PRIMARY KEY,
    notebook_id TEXT NOT NULL REFERENCES notebooks (id) DEFERRABLE INITIALLY DEFERRED,
    kind        TEXT NOT NULL CHECK (kind IN ('note', 'highlight')),
    style       TEXT,
    body        TEXT NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    modified_at INTEGER NOT NULL,
    CHECK ((kind = 'highlight') = (style IS NOT NULL)),
    CHECK (kind = 'note' OR body = '')
);
CREATE INDEX annotations_notebook ON annotations (notebook_id);
CREATE TABLE anchors (
    annotation_id TEXT PRIMARY KEY
                  REFERENCES annotations (id) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    kind          TEXT NOT NULL CHECK (kind IN ('passage', 'text_range')),
    start_book    INTEGER CHECK (start_book BETWEEN 1 AND 66),
    start_chapter INTEGER,
    start_verse   INTEGER,
    end_book      INTEGER CHECK (end_book BETWEEN 1 AND 66),
    end_chapter   INTEGER,
    end_verse     INTEGER,
    resource_id   TEXT,
    unit_id       TEXT,
    start_byte    INTEGER,
    end_byte      INTEGER,
    quote         TEXT,
    prefix        TEXT,
    suffix        TEXT,
    content_hash  TEXT,
    native_ref    TEXT,
    CHECK ((kind = 'passage') = (start_book IS NOT NULL AND start_chapter IS NOT NULL
           AND start_verse IS NOT NULL AND end_book IS NOT NULL AND end_chapter IS NOT NULL
           AND end_verse IS NOT NULL)),
    CHECK (kind = 'passage' OR resource_id IS NOT NULL),
    CHECK (resource_id IS NULL OR (unit_id IS NOT NULL AND start_byte IS NOT NULL
           AND end_byte >= start_byte AND quote IS NOT NULL AND prefix IS NOT NULL
           AND suffix IS NOT NULL AND content_hash IS NOT NULL))
);
CREATE INDEX anchors_passage ON anchors (start_book, start_chapter, start_verse)
    WHERE kind = 'passage';
CREATE INDEX anchors_resource ON anchors (resource_id) WHERE resource_id IS NOT NULL;
CREATE VIRTUAL TABLE notes_fts USING fts5 (
    body, annotation_id UNINDEXED, tokenize = 'unicode61 remove_diacritics 0'
);
";

/// Name of the default Notebook when it is first made. The user may rename it.
pub const DEFAULT_NOTEBOOK_NAME: &str = "My Notebook";

const NOTEBOOK_COLUMNS: &str = "id, name, is_default, created_at, modified_at";

const ANNOTATION_COLUMNS: &str = "a.id, a.notebook_id, a.kind, a.style, a.body, a.created_at,
     a.modified_at, x.kind, x.start_book, x.start_chapter, x.start_verse, x.end_book,
     x.end_chapter, x.end_verse, x.resource_id, x.unit_id, x.start_byte, x.end_byte, x.quote,
     x.prefix, x.suffix, x.content_hash, x.native_ref";

const ANNOTATION_FROM: &str = "annotations a LEFT JOIN anchors x ON x.annotation_id = a.id";

fn corrupt(what: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(what.into())))
}

fn notebook_from(r: &SqlRow<'_>) -> rusqlite::Result<Notebook> {
    Ok(Notebook {
        id: r.get(0)?,
        name: r.get(1)?,
        is_default: r.get(2)?,
        created_at: r.get(3)?,
        modified_at: r.get(4)?,
    })
}

fn verse_from(r: &SqlRow<'_>, at: usize) -> rusqlite::Result<Verse> {
    let book: i64 = r.get(at)?;
    let book = usize::try_from(book - 1)
        .ok()
        .and_then(Book::from_index)
        .ok_or_else(|| corrupt(format!("book number {book}")))?;
    Ok(Verse::new(book, r.get(at + 1)?, r.get(at + 2)?))
}

fn text_range_from(r: &SqlRow<'_>) -> rusqlite::Result<Option<TextRange>> {
    let Some(resource_id) = r.get::<_, Option<String>>(14)? else {
        return Ok(None);
    };
    let unit: String = r.get(15)?;
    Ok(Some(TextRange {
        resource_id,
        unit: UnitId::new(unit).map_err(|e| corrupt(e.to_string()))?,
        start: r.get(16)?,
        end: r.get(17)?,
        quote: r.get(18)?,
        prefix: r.get(19)?,
        suffix: r.get(20)?,
        content_hash: r.get(21)?,
        native_ref: r.get(22)?,
    }))
}

fn annotation_from(r: &SqlRow<'_>) -> rusqlite::Result<Annotation> {
    let kind: String = r.get(2)?;
    let content = match kind.as_str() {
        "note" => Content::Note { body: r.get(4)? },
        _ => {
            let style: String = r.get(3)?;
            Content::Highlight {
                style: HighlightStyle::from_id(&style)
                    .ok_or_else(|| corrupt(format!("Highlight style `{style}`")))?,
            }
        }
    };
    let anchor = match r.get::<_, Option<String>>(7)?.as_deref() {
        None => None,
        Some("passage") => {
            let (start, end) = (verse_from(r, 8)?, verse_from(r, 11)?);
            Some(Anchor::Passage {
                passage: Passage::new(start, end).map_err(|e| corrupt(e.to_string()))?,
                origin: text_range_from(r)?,
            })
        }
        Some(_) => Some(Anchor::TextRange {
            range: text_range_from(r)?.ok_or_else(|| corrupt("Text-range anchor without range"))?,
        }),
    };
    Ok(Annotation {
        id: r.get(0)?,
        notebook_id: r.get(1)?,
        content,
        anchor,
        created_at: r.get(5)?,
        modified_at: r.get(6)?,
    })
}

fn book_number(v: Verse) -> i64 {
    i64::try_from(v.book.index()).unwrap_or(0) + 1
}

/// Note body as `notes_fts` holds it: normalized words, as in the Search index.
fn index_form(body: &str) -> String {
    obs_model::tokenize(body)
        .into_iter()
        .map(|t| t.text)
        .collect::<Vec<_>>()
        .join(" ")
}

fn read_notebook(c: &Connection, id: &str) -> Result<Option<Notebook>, StoreError> {
    c.prepare_cached(&format!(
        "SELECT {NOTEBOOK_COLUMNS} FROM notebooks WHERE id = ?1"
    ))
    .and_then(|mut s| s.query_row([id], notebook_from).optional())
    .map_err(sql)
}

fn read_annotation(c: &Connection, id: &str) -> Result<Option<Annotation>, StoreError> {
    c.prepare_cached(&format!(
        "SELECT {ANNOTATION_COLUMNS} FROM {ANNOTATION_FROM} WHERE a.id = ?1"
    ))
    .and_then(|mut s| s.query_row([id], annotation_from).optional())
    .map_err(sql)
}

fn read_row(c: &Connection, key: &RowKey) -> Result<Option<Row>, StoreError> {
    Ok(match key {
        RowKey::Notebook(id) => read_notebook(c, id)?.map(Row::Notebook),
        RowKey::Annotation(id) => read_annotation(c, id)?.map(Row::Annotation),
    })
}

fn write_notebook(c: &Connection, id: &str, n: Option<&Notebook>) -> Result<(), StoreError> {
    match n {
        None => c
            .execute("DELETE FROM notebooks WHERE id = ?1", [id])
            .map(drop),
        Some(n) => c
            .execute(
                "INSERT INTO notebooks (id, name, is_default, created_at, modified_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (id) DO UPDATE SET name = excluded.name,
                     is_default = excluded.is_default, created_at = excluded.created_at,
                     modified_at = excluded.modified_at",
                params![n.id, n.name, n.is_default, n.created_at, n.modified_at],
            )
            .map(drop),
    }
    .map_err(sql)
}

fn write_anchor(c: &Connection, id: &str, anchor: Option<&Anchor>) -> Result<(), StoreError> {
    c.execute("DELETE FROM anchors WHERE annotation_id = ?1", [id])
        .map_err(sql)?;
    let Some(anchor) = anchor else {
        return Ok(());
    };
    let (kind, passage, range) = match anchor {
        Anchor::Passage { passage, origin } => ("passage", Some(*passage), origin.as_ref()),
        Anchor::TextRange { range } => ("text_range", None, Some(range)),
    };
    let s = passage.map(|p| p.start());
    let e = passage.map(|p| p.end());
    c.execute(
        "INSERT INTO anchors (annotation_id, kind, start_book, start_chapter, start_verse,
             end_book, end_chapter, end_verse, resource_id, unit_id, start_byte, end_byte,
             quote, prefix, suffix, content_hash, native_ref)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            id,
            kind,
            s.map(book_number),
            s.map(|v| v.chapter),
            s.map(|v| v.verse),
            e.map(book_number),
            e.map(|v| v.chapter),
            e.map(|v| v.verse),
            range.map(|r| &r.resource_id),
            range.map(|r| r.unit.as_str()),
            range.map(|r| r.start),
            range.map(|r| r.end),
            range.map(|r| &r.quote),
            range.map(|r| &r.prefix),
            range.map(|r| &r.suffix),
            range.map(|r| &r.content_hash),
            range.and_then(|r| r.native_ref.as_ref()),
        ],
    )
    .map(drop)
    .map_err(sql)
}

fn write_annotation(
    c: &Connection,
    id: &str,
    before: Option<&Annotation>,
    after: Option<&Annotation>,
) -> Result<(), StoreError> {
    let Some(a) = after else {
        for stmt in [
            "DELETE FROM notes_fts WHERE annotation_id = ?1",
            "DELETE FROM anchors WHERE annotation_id = ?1",
            "DELETE FROM annotations WHERE id = ?1",
        ] {
            c.execute(stmt, [id]).map_err(sql)?;
        }
        return Ok(());
    };
    if let Some(why) = a.problem() {
        return Err(StoreError::Rejected(why.to_string()));
    }
    let (kind, style, body) = match &a.content {
        Content::Note { body } => ("note", None, body.as_str()),
        Content::Highlight { style } => ("highlight", Some(style.id()), ""),
    };
    c.execute(
        "INSERT INTO annotations (id, notebook_id, kind, style, body, created_at, modified_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (id) DO UPDATE SET notebook_id = excluded.notebook_id,
             kind = excluded.kind, style = excluded.style, body = excluded.body,
             created_at = excluded.created_at, modified_at = excluded.modified_at",
        params![
            a.id,
            a.notebook_id,
            kind,
            style,
            body,
            a.created_at,
            a.modified_at
        ],
    )
    .map_err(sql)?;
    if before.map(|b| &b.anchor) != Some(&a.anchor) {
        write_anchor(c, id, a.anchor.as_ref())?;
    }
    // Rewrite the Note's index entry only when its text changed: finding the old entry
    // scans the index's (small) content table.
    fn indexed(x: &Annotation) -> Option<&str> {
        x.is_note().then(|| x.body())
    }
    if before.and_then(indexed) != indexed(a) {
        if before.and_then(indexed).is_some() {
            c.execute("DELETE FROM notes_fts WHERE annotation_id = ?1", [id])
                .map_err(sql)?;
        }
        if a.is_note() {
            c.execute(
                "INSERT INTO notes_fts (body, annotation_id) VALUES (?1, ?2)",
                params![index_form(a.body()), id],
            )
            .map_err(sql)?;
        }
    }
    Ok(())
}

impl UserStore {
    /// Make the default Notebook if there is none.
    pub(crate) fn ensure_default_notebook(&self) -> Result<(), StoreError> {
        let has: bool = self
            .conn
            .query_row(
                "SELECT EXISTS (SELECT 1 FROM notebooks WHERE is_default = 1)",
                [],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if has {
            return Ok(());
        }
        let now = crate::annotation::now_ms();
        write_notebook(
            &self.conn,
            "",
            Some(&Notebook {
                id: crate::annotation::new_id_at(now),
                name: DEFAULT_NOTEBOOK_NAME.to_string(),
                is_default: true,
                created_at: now,
                modified_at: now,
            }),
        )
    }

    /// Every Notebook: the default first, then by name.
    pub fn notebooks(&self) -> Result<Vec<Notebook>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!(
                "SELECT {NOTEBOOK_COLUMNS} FROM notebooks
                 ORDER BY is_default DESC, name COLLATE NOCASE, id"
            ))
            .map_err(sql)?;
        let rows = stmt.query_map([], notebook_from).map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    pub fn notebook(&self, id: &str) -> Result<Option<Notebook>, StoreError> {
        read_notebook(&self.conn, id)
    }

    /// How many Annotations each Notebook holds (Notebooks with none are left out).
    pub fn notebook_counts(&self) -> Result<Vec<(String, u64)>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT notebook_id, COUNT(*) FROM annotations GROUP BY notebook_id")
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get(0)?, u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0)))
            })
            .map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    pub fn annotation(&self, id: &str) -> Result<Option<Annotation>, StoreError> {
        read_annotation(&self.conn, id)
    }

    /// Annotations in one Notebook (or all), oldest first.
    pub fn annotations(&self, notebook: Option<&str>) -> Result<Vec<Annotation>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(&format!(
                "SELECT {ANNOTATION_COLUMNS} FROM {ANNOTATION_FROM}
                 WHERE ?1 IS NULL OR a.notebook_id = ?1 ORDER BY a.created_at, a.id"
            ))
            .map_err(sql)?;
        let rows = stmt.query_map([notebook], annotation_from).map_err(sql)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(sql)
    }

    /// How many Annotations have a Text range in Resource `resource_id`.
    pub fn annotations_referring(&self, resource_id: &str) -> Result<u64, StoreError> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM anchors WHERE resource_id = ?1",
                [resource_id],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| u64::try_from(n).unwrap_or(0))
            .map_err(sql)
    }

    /// Ids of Notes whose body matches an FTS5 `MATCH` expression over normalized words,
    /// most recently modified first.
    pub fn search_notes(&self, fts_query: &str) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT a.id FROM notes_fts f JOIN annotations a ON a.id = f.annotation_id
                 WHERE notes_fts MATCH ?1 ORDER BY a.modified_at DESC, a.id DESC",
            )
            .map_err(sql)?;
        let rows = stmt.query_map([fts_query], |r| r.get(0)).map_err(sql)?;
        rows.collect::<Result<Vec<String>, _>>().map_err(sql)
    }

    /// Apply `changes` in one transaction: each row must still be its `before` (else
    /// [`StoreError::Conflict`] and nothing is written), then becomes its `after`. The default
    /// Notebook cannot be deleted or stop being the default.
    pub fn apply_changes(&self, changes: &[Change]) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        for change in changes {
            let key = change
                .key()
                .ok_or_else(|| StoreError::Rejected("a change must be about one row".into()))?;
            let current = read_row(&tx, &key)?;
            if current != change.before {
                let what = match &key {
                    RowKey::Notebook(id) => format!("Notebook {id}"),
                    RowKey::Annotation(id) => format!("Annotation {id}"),
                };
                return Err(StoreError::Conflict(what));
            }
            match (&key, &change.before, &change.after) {
                (RowKey::Notebook(id), before, after) => {
                    let was_default = matches!(before, Some(Row::Notebook(n)) if n.is_default);
                    let is_default = matches!(after, Some(Row::Notebook(n)) if n.is_default);
                    if was_default && !is_default {
                        return Err(StoreError::Rejected(
                            "the default Notebook cannot be deleted".into(),
                        ));
                    }
                    let after = match after {
                        Some(Row::Notebook(n)) => Some(n),
                        _ => None,
                    };
                    write_notebook(&tx, id, after)?;
                }
                (RowKey::Annotation(id), before, after) => {
                    let pick = |r: &Option<Row>| match r {
                        Some(Row::Annotation(a)) => Some(a.clone()),
                        _ => None,
                    };
                    write_annotation(&tx, id, pick(before).as_ref(), pick(after).as_ref())?;
                }
            }
        }
        tx.commit().map_err(sql)
    }

    /// Every row of `notebooks`, `annotations`, `anchors` and `notes_fts` as text, after
    /// checking the FTS index against its rows; tests compare these to prove undo and redo
    /// restore the same database.
    #[doc(hidden)]
    pub fn annotation_dump(&self) -> Result<String, StoreError> {
        self.conn
            .execute(
                "INSERT INTO notes_fts (notes_fts, rank) VALUES ('integrity-check', 1)",
                [],
            )
            .map_err(sql)?;
        let mut out = String::new();
        for (table, query) in [
            ("notebooks", "SELECT * FROM notebooks ORDER BY id"),
            ("annotations", "SELECT * FROM annotations ORDER BY id"),
            ("anchors", "SELECT * FROM anchors ORDER BY annotation_id"),
            (
                "notes_fts",
                "SELECT annotation_id, body FROM notes_fts ORDER BY annotation_id, body",
            ),
        ] {
            let _ = writeln!(out, "[{table}]");
            let mut stmt = self.conn.prepare(query).map_err(sql)?;
            let n = stmt.column_count();
            let mut rows = stmt.query([]).map_err(sql)?;
            while let Some(r) = rows.next().map_err(sql)? {
                let values = (0..n)
                    .map(|i| r.get::<_, SqlValue>(i).map(|v| format!("{v:?}")))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(sql)?;
                let _ = writeln!(out, "{}", values.join(" | "));
            }
        }
        Ok(out)
    }
}
