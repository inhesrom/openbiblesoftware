//! The `.obr` file: one read-only SQLite database per Resource (ADR 0003).
//!
//! Layout (format version [`FORMAT_VERSION`]):
//!
//! | table | rows |
//! |---|---|
//! | `manifest` | exactly one: every [`Manifest`] field |
//! | `units` | `ord` (document order), `id`, `kind`, `title`, `text` |
//! | `spans` | `unit` (→ `units.ord`), `seq` (canonical order), `start`, `end`, `kind`, `attrs` (JSON) |
//! | `verses` | `ordinal` (document order), `book`, `chapter`, `verse`, `last`, `unit`, `start`, `end` |
//! | `keys` | `scheme`, `key`, `key_norm`, `unit`, `start`, `end`, `alias` |
//! | `unit_alias` | `old` → `new` unit id |
//! | `verse_map` | per Bible Verse (`ordinal` → `verses`): Standard `std_start`, `std_end` (both null for an Unmapped verse) |
//! | `verse_absent` | Absent verses: Standard `verse` ids (`MAT.17.21`) |
//! | `dropped_markup` | `marker`, `count` (for the Import report) |
//! | `xrefs` | Cross-references: Standard `from_start`, `from_end`, `to_start`, `to_end` (verse numbers), `votes` |
//! | `entities` | People & places: `ord`, `id`, `name`, `kind`, `strongs`, `region`, `other_names` (JSON), `relations` (JSON) |
//! | `mentions` | `entity` (→ `entities.ord`), Standard `start`, `end` (verse numbers) |
//!
//! Dataset Verses are stored as verse numbers `BBCCCVVV` (book 1-66, chapter, verse), so a
//! Passage overlap is an integer range query.
//!
//! Files are written once, deterministically (same Resource ⇒ byte-identical file), and opened
//! read-only. `PRAGMA application_id` marks the file type and `PRAGMA user_version` holds the
//! format version, so a file is checked before any table is read.

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use obs_model::{
    BibleVerse, CrossReference, Direction, Entity, EntityKind, EntityRelation, FORMAT_VERSION, Key,
    KeyEntry, KeyScheme, Manifest, Mention, Origin, Resource, ResourceType, Span, TextUnit, UnitId,
    UnitKind,
};
use obs_scripture::{Book, Passage, Standard, Verse, VerseMap, VerseMapEntry};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, params};

use crate::codec::{
    enum_from, enum_name, json, json_from, key_columns, key_from, passage_from_numbers,
    span_columns, span_from, verse_from, verse_number, verse_text,
};
use crate::error::StoreError;
use crate::resource_store::{ResourceStore, Summary};

/// `PRAGMA application_id` of every `.obr` file: ASCII `OBR1`.
pub const APPLICATION_ID: i32 = 0x4F42_5231;

const SCHEMA: &str = "
CREATE TABLE manifest (
    resource_id TEXT NOT NULL,
    title TEXT NOT NULL,
    abbreviation TEXT NOT NULL,
    resource_type TEXT NOT NULL,
    language TEXT NOT NULL,
    direction TEXT NOT NULL,
    key_schemes TEXT NOT NULL,
    edition TEXT,
    revision INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    publisher TEXT,
    source_url TEXT,
    license TEXT NOT NULL,
    attribution TEXT NOT NULL,
    changes TEXT,
    origin TEXT NOT NULL,
    source_format TEXT NOT NULL,
    source_file TEXT,
    import_tool_version TEXT NOT NULL,
    format_version INTEGER NOT NULL
);
CREATE TABLE units (
    ord INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    title TEXT,
    text TEXT NOT NULL
);
CREATE TABLE spans (
    unit INTEGER NOT NULL,
    seq INTEGER NOT NULL,
    start INTEGER NOT NULL,
    end INTEGER NOT NULL,
    kind TEXT NOT NULL,
    attrs TEXT,
    PRIMARY KEY (unit, seq)
) WITHOUT ROWID;
CREATE INDEX spans_range ON spans (unit, start, end);
CREATE TABLE verses (
    ordinal INTEGER PRIMARY KEY,
    book TEXT NOT NULL,
    chapter INTEGER NOT NULL,
    verse INTEGER NOT NULL,
    last INTEGER NOT NULL,
    unit INTEGER NOT NULL,
    start INTEGER NOT NULL,
    end INTEGER NOT NULL
);
CREATE INDEX verses_ref ON verses (book, chapter, verse);
CREATE TABLE keys (
    scheme TEXT NOT NULL,
    key TEXT NOT NULL,
    key_norm TEXT NOT NULL,
    unit INTEGER NOT NULL,
    start INTEGER,
    end INTEGER,
    alias INTEGER NOT NULL
);
CREATE INDEX keys_lookup ON keys (scheme, key_norm);
CREATE TABLE unit_alias (
    old TEXT PRIMARY KEY,
    new TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE verse_map (
    ordinal INTEGER PRIMARY KEY,
    std_start TEXT,
    std_end TEXT
);
CREATE INDEX verse_map_std ON verse_map (std_start);
CREATE TABLE verse_absent (
    verse TEXT PRIMARY KEY
) WITHOUT ROWID;
CREATE TABLE dropped_markup (
    marker TEXT PRIMARY KEY,
    count INTEGER NOT NULL
) WITHOUT ROWID;
CREATE TABLE xrefs (
    from_start INTEGER NOT NULL,
    from_end INTEGER NOT NULL,
    to_start INTEGER NOT NULL,
    to_end INTEGER NOT NULL,
    votes INTEGER NOT NULL
);
CREATE INDEX xrefs_from ON xrefs (from_start);
CREATE TABLE entities (
    ord INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    strongs TEXT,
    region TEXT,
    other_names TEXT NOT NULL,
    relations TEXT NOT NULL
);
CREATE TABLE mentions (
    entity INTEGER NOT NULL,
    start INTEGER NOT NULL,
    end INTEGER NOT NULL
);
CREATE INDEX mentions_start ON mentions (start);
";

fn sql(e: rusqlite::Error) -> StoreError {
    StoreError::Sqlite(e.to_string())
}

fn corrupt(why: impl Into<String>) -> StoreError {
    StoreError::Corrupt(why.into())
}

fn to_u32(v: i64, what: &str) -> Result<u32, StoreError> {
    u32::try_from(v).map_err(|_| corrupt(format!("{what} {v} out of range")))
}

fn to_u16(v: i64, what: &str) -> Result<u16, StoreError> {
    u16::try_from(v).map_err(|_| corrupt(format!("{what} {v} out of range")))
}

fn unit_id(s: String) -> Result<UnitId, StoreError> {
    UnitId::new(s).map_err(|e| corrupt(e.to_string()))
}

/// Write `resource` to `path` as a `.obr`, replacing any file there. The same Resource always
/// produces a byte-identical file.
pub fn write_obr(resource: &Resource, path: &Path) -> Result<(), StoreError> {
    resource.validate().map_err(StoreError::Invalid)?;
    let tmp = temp_path(path);
    if tmp.exists() {
        std::fs::remove_file(&tmp)?;
    }
    let result = write_to(resource, &tmp);
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".partial");
    path.with_file_name(name)
}

fn write_to(r: &Resource, path: &Path) -> Result<(), StoreError> {
    let mut conn = Connection::open(path).map_err(sql)?;
    conn.execute_batch(&format!(
        "PRAGMA page_size = 4096;
         PRAGMA journal_mode = OFF;
         PRAGMA synchronous = OFF;
         PRAGMA application_id = {APPLICATION_ID};
         PRAGMA user_version = {};",
        r.manifest.format_version
    ))
    .map_err(sql)?;
    conn.execute_batch(SCHEMA).map_err(sql)?;
    let tx = conn.transaction().map_err(sql)?;
    insert_manifest(&tx, &r.manifest)?;
    let mut ords: BTreeMap<&UnitId, i64> = BTreeMap::new();
    {
        let mut unit_stmt = tx
            .prepare("INSERT INTO units (ord, id, kind, title, text) VALUES (?1, ?2, ?3, ?4, ?5)")
            .map_err(sql)?;
        let mut span_stmt = tx
            .prepare(
                "INSERT INTO spans (unit, seq, start, end, kind, attrs) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(sql)?;
        for (ord, unit) in r.units.iter().enumerate() {
            let ord = ord as i64;
            ords.insert(&unit.id, ord);
            unit_stmt
                .execute(params![
                    ord,
                    unit.id.as_str(),
                    enum_name(&unit.kind),
                    unit.title,
                    unit.text
                ])
                .map_err(sql)?;
            for (seq, span) in unit.spans.iter().enumerate() {
                let (kind, attrs) = span_columns(&span.kind);
                span_stmt
                    .execute(params![ord, seq as i64, span.start, span.end, kind, attrs])
                    .map_err(sql)?;
            }
        }
        let ord_of = |id: &UnitId| {
            ords.get(id)
                .copied()
                .ok_or_else(|| StoreError::UnknownUnit(id.to_string()))
        };
        let mut verse_stmt = tx
            .prepare(
                "INSERT INTO verses (ordinal, book, chapter, verse, last, unit, start, end)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .map_err(sql)?;
        for (i, v) in r.verses.iter().enumerate() {
            verse_stmt
                .execute(params![
                    i as i64,
                    v.verse.book.usfm(),
                    v.verse.chapter,
                    v.verse.verse,
                    v.last,
                    ord_of(&v.unit)?,
                    v.start,
                    v.end
                ])
                .map_err(sql)?;
        }
        let mut key_stmt = tx
            .prepare(
                "INSERT INTO keys (scheme, key, key_norm, unit, start, end, alias)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(sql)?;
        for k in &r.keys {
            let (scheme, text, norm) = key_columns(&k.key);
            key_stmt
                .execute(params![
                    scheme,
                    text,
                    norm,
                    ord_of(&k.unit)?,
                    k.range.map(|r| r.0),
                    k.range.map(|r| r.1),
                    k.alias
                ])
                .map_err(sql)?;
        }
        let mut alias_stmt = tx
            .prepare("INSERT INTO unit_alias (old, new) VALUES (?1, ?2)")
            .map_err(sql)?;
        for (old, new) in &r.unit_aliases {
            alias_stmt
                .execute(params![old.as_str(), new.as_str()])
                .map_err(sql)?;
        }
        if let Some(map) = &r.verse_map {
            let mut map_stmt = tx
                .prepare("INSERT INTO verse_map (ordinal, std_start, std_end) VALUES (?1, ?2, ?3)")
                .map_err(sql)?;
            for (i, e) in map.entries().iter().enumerate() {
                map_stmt
                    .execute(params![
                        i as i64,
                        e.standard.map(|p| verse_text(p.start())),
                        e.standard.map(|p| verse_text(p.end()))
                    ])
                    .map_err(sql)?;
            }
            let mut absent_stmt = tx
                .prepare("INSERT INTO verse_absent (verse) VALUES (?1)")
                .map_err(sql)?;
            for v in map.absent() {
                absent_stmt.execute(params![verse_text(*v)]).map_err(sql)?;
            }
        }
        let mut dropped_stmt = tx
            .prepare("INSERT INTO dropped_markup (marker, count) VALUES (?1, ?2)")
            .map_err(sql)?;
        for (marker, count) in &r.dropped_markup {
            let count = i64::try_from(*count).unwrap_or(i64::MAX);
            dropped_stmt.execute(params![marker, count]).map_err(sql)?;
        }
        insert_datasets(&tx, r)?;
    }
    tx.commit().map_err(sql)?;
    conn.execute_batch("VACUUM;").map_err(sql)?;
    conn.close().map_err(|(_, e)| sql(e))?;
    Ok(())
}

fn insert_datasets(tx: &rusqlite::Transaction<'_>, r: &Resource) -> Result<(), StoreError> {
    let mut xref_stmt = tx
        .prepare(
            "INSERT INTO xrefs (from_start, from_end, to_start, to_end, votes)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .map_err(sql)?;
    for x in &r.cross_references {
        xref_stmt
            .execute(params![
                verse_number(x.from.start()),
                verse_number(x.from.end()),
                verse_number(x.to.start()),
                verse_number(x.to.end()),
                x.votes
            ])
            .map_err(sql)?;
    }
    let mut entity_stmt = tx
        .prepare(
            "INSERT INTO entities (ord, id, name, kind, strongs, region, other_names, relations)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .map_err(sql)?;
    let mut ords: BTreeMap<&str, i64> = BTreeMap::new();
    for (ord, e) in r.entities.iter().enumerate() {
        let ord = ord as i64;
        ords.insert(&e.id, ord);
        entity_stmt
            .execute(params![
                ord,
                e.id,
                e.name,
                enum_name(&e.kind),
                e.strongs.map(|s| s.to_string()),
                e.region,
                json(&e.other_names),
                json(&e.relations)
            ])
            .map_err(sql)?;
    }
    let mut mention_stmt = tx
        .prepare("INSERT INTO mentions (entity, start, end) VALUES (?1, ?2, ?3)")
        .map_err(sql)?;
    for m in &r.mentions {
        let ord = ords
            .get(m.entity.as_str())
            .ok_or_else(|| corrupt(format!("mention of unknown entity `{}`", m.entity)))?;
        mention_stmt
            .execute(params![
                ord,
                verse_number(m.passage.start()),
                verse_number(m.passage.end())
            ])
            .map_err(sql)?;
    }
    Ok(())
}

fn insert_manifest(tx: &rusqlite::Transaction<'_>, m: &Manifest) -> Result<(), StoreError> {
    tx.execute(
        "INSERT INTO manifest VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                                      ?15, ?16, ?17, ?18, ?19, ?20)",
        params![
            m.resource_id,
            m.title,
            m.abbreviation,
            enum_name(&m.resource_type),
            m.language,
            enum_name(&m.direction),
            json(&m.key_schemes),
            m.edition,
            m.revision,
            m.content_hash,
            m.publisher,
            m.source_url,
            m.license,
            m.attribution,
            m.changes,
            enum_name(&m.origin),
            m.source_format,
            m.source_file,
            m.import_tool_version,
            m.format_version,
        ],
    )
    .map_err(sql)?;
    Ok(())
}

/// An open `.obr` file.
#[derive(Debug)]
pub struct ObrReader {
    conn: Connection,
    manifest: Manifest,
    /// Longest `xrefs` source Passage, as a verse-number difference (bounds overlap queries).
    xref_span: OnceCell<i64>,
    /// Longest `mentions` Passage, likewise.
    mention_span: OnceCell<i64>,
}

impl ObrReader {
    /// Open read-only, checking the file type and format version before anything else.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if !path.is_file() {
            return Err(StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} does not exist", path.display()),
            )));
        }
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(sql)?;
        let app_id: i32 = conn
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .map_err(|e| StoreError::NotAnObr(e.to_string()))?;
        if app_id != APPLICATION_ID {
            return Err(StoreError::NotAnObr(format!(
                "application id is {app_id:#x}, expected {APPLICATION_ID:#x}"
            )));
        }
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(sql)?;
        let version = to_u32(version, "format version")?;
        if version > FORMAT_VERSION {
            return Err(StoreError::NewerFormat {
                found: version,
                supported: FORMAT_VERSION,
            });
        }
        if version < FORMAT_VERSION {
            return Err(StoreError::OlderFormat {
                found: version,
                supported: FORMAT_VERSION,
            });
        }
        let manifest = read_manifest(&conn)?;
        if manifest.format_version != version {
            return Err(corrupt(format!(
                "manifest says format version {}, file header says {version}",
                manifest.format_version
            )));
        }
        Ok(Self {
            conn,
            manifest,
            xref_span: OnceCell::new(),
            mention_span: OnceCell::new(),
        })
    }

    /// The longest `end - start` in a dataset table; cached, since files never change.
    fn longest_span(&self, cell: &OnceCell<i64>, query: &str) -> Result<i64, StoreError> {
        if let Some(&n) = cell.get() {
            return Ok(n);
        }
        let n: Option<i64> = self.conn.query_row(query, [], |r| r.get(0)).map_err(sql)?;
        Ok(*cell.get_or_init(|| n.unwrap_or(0)))
    }

    fn xref_rows(&self, query: &str, args: &[i64]) -> Result<Vec<CrossReference>, StoreError> {
        let mut stmt = self.conn.prepare_cached(query).map_err(sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .map_err(sql)?;
        let mut out = Vec::new();
        for row in rows {
            let (fs, fe, ts, te, votes) = row.map_err(sql)?;
            out.push(CrossReference {
                from: passage_from_numbers(fs, fe)?,
                to: passage_from_numbers(ts, te)?,
                votes: i32::try_from(votes)
                    .map_err(|_| corrupt(format!("votes {votes} out of range")))?,
            });
        }
        Ok(out)
    }

    fn entity_rows(
        &self,
        filter: &str,
        args: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<Entity>, StoreError> {
        let query = format!(
            "SELECT id, name, kind, strongs, region, other_names, relations FROM entities {filter} ORDER BY ord"
        );
        let mut stmt = self.conn.prepare_cached(&query).map_err(sql)?;
        let rows = stmt
            .query_map(args, |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })
            .map_err(sql)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, kind, strongs, region, other_names, relations) = row.map_err(sql)?;
            out.push(Entity {
                id,
                name,
                kind: enum_from::<EntityKind>("entity kind", &kind)?,
                strongs: strongs
                    .map(|s| {
                        s.parse()
                            .map_err(|_| corrupt(format!("bad Strong's `{s}`")))
                    })
                    .transpose()?,
                region,
                other_names: json_from::<Vec<String>>("entity names", &other_names)?,
                relations: json_from::<Vec<EntityRelation>>("entity relations", &relations)?,
            });
        }
        Ok(out)
    }

    fn mention_rows(&self, filter: &str, args: &[i64]) -> Result<Vec<Mention>, StoreError> {
        let query = format!(
            "SELECT e.id, m.start, m.end, m.entity FROM mentions m LEFT JOIN entities e ON e.ord = m.entity
             {filter} ORDER BY m.entity, m.start, m.end"
        );
        let mut stmt = self.conn.prepare_cached(&query).map_err(sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(args), |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(sql)?;
        let mut out = Vec::new();
        for row in rows {
            let (entity, start, end, ordinal) = row.map_err(sql)?;
            out.push(Mention {
                entity: entity
                    .ok_or_else(|| corrupt(format!("no entity with ordinal {ordinal}")))?,
                passage: passage_from_numbers(start, end)?,
            });
        }
        Ok(out)
    }

    fn unit_ord(&self, id: &UnitId) -> Result<Option<i64>, StoreError> {
        self.conn
            .prepare_cached("SELECT ord FROM units WHERE id = ?1")
            .map_err(sql)?
            .query_row([id.as_str()], |r| r.get(0))
            .optional()
            .map_err(sql)
    }

    fn unit_id_of(&self, ord: i64) -> Result<UnitId, StoreError> {
        let id: String = self
            .conn
            .prepare_cached("SELECT id FROM units WHERE ord = ?1")
            .map_err(sql)?
            .query_row([ord], |r| r.get(0))
            .map_err(|_| corrupt(format!("no unit with ordinal {ord}")))?;
        unit_id(id)
    }

    fn spans_where(&self, ord: i64, filter: &str, args: &[i64]) -> Result<Vec<Span>, StoreError> {
        let query = format!(
            "SELECT start, end, kind, attrs FROM spans WHERE unit = ?1 {filter} ORDER BY seq"
        );
        let mut stmt = self.conn.prepare_cached(&query).map_err(sql)?;
        let mut values: Vec<i64> = vec![ord];
        values.extend_from_slice(args);
        let rows = stmt
            .query_map(rusqlite::params_from_iter(values), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })
            .map_err(sql)?;
        let mut out = Vec::new();
        for row in rows {
            let (start, end, kind, attrs) = row.map_err(sql)?;
            out.push(Span {
                start: to_u32(start, "span start")?,
                end: to_u32(end, "span end")?,
                kind: span_from(&kind, attrs.as_deref())?,
            });
        }
        Ok(out)
    }

    fn verse_row(&self, r: &Row<'_>) -> rusqlite::Result<(String, i64, i64, i64, i64, i64, i64)> {
        Ok((
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            r.get(5)?,
            r.get(6)?,
        ))
    }

    fn to_verse(
        &self,
        (book, chapter, verse, last, unit, start, end): (String, i64, i64, i64, i64, i64, i64),
    ) -> Result<BibleVerse, StoreError> {
        let book = Book::from_usfm(&book).ok_or_else(|| corrupt(format!("bad book `{book}`")))?;
        Ok(BibleVerse {
            verse: Verse::new(book, to_u16(chapter, "chapter")?, to_u16(verse, "verse")?),
            last: to_u16(last, "verse")?,
            unit: self.unit_id_of(unit)?,
            start: to_u32(start, "verse start")?,
            end: to_u32(end, "verse end")?,
        })
    }

    fn key_rows(
        &self,
        query: &str,
        args: &[&dyn rusqlite::ToSql],
    ) -> Result<Vec<KeyEntry>, StoreError> {
        let mut stmt = self.conn.prepare_cached(query).map_err(sql)?;
        let rows = stmt
            .query_map(args, |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, bool>(5)?,
                ))
            })
            .map_err(sql)?;
        let mut out = Vec::new();
        for row in rows {
            let (scheme, key, unit, start, end, alias) = row.map_err(sql)?;
            let range = match (start, end) {
                (Some(s), Some(e)) => Some((to_u32(s, "key start")?, to_u32(e, "key end")?)),
                _ => None,
            };
            out.push(KeyEntry {
                key: key_from(&scheme, &key)?,
                unit: self.unit_id_of(unit)?,
                range,
                alias,
            });
        }
        out.sort();
        Ok(out)
    }
}

fn read_manifest(conn: &Connection) -> Result<Manifest, StoreError> {
    let mut stmt = conn
        .prepare("SELECT * FROM manifest")
        .map_err(|e| StoreError::NotAnObr(e.to_string()))?;
    let mut rows = stmt.query([]).map_err(sql)?;
    let row = rows
        .next()
        .map_err(sql)?
        .ok_or_else(|| corrupt("manifest table is empty"))?;
    let get_s = |i: usize| row.get::<_, String>(i).map_err(sql);
    let get_o = |i: usize| row.get::<_, Option<String>>(i).map_err(sql);
    let manifest = Manifest {
        resource_id: get_s(0)?,
        title: get_s(1)?,
        abbreviation: get_s(2)?,
        resource_type: enum_from::<ResourceType>("resource type", &get_s(3)?)?,
        language: get_s(4)?,
        direction: enum_from::<Direction>("direction", &get_s(5)?)?,
        key_schemes: json_from::<Vec<KeyScheme>>("key schemes", &get_s(6)?)?,
        edition: get_o(7)?,
        revision: to_u32(row.get(8).map_err(sql)?, "revision")?,
        content_hash: get_s(9)?,
        publisher: get_o(10)?,
        source_url: get_o(11)?,
        license: get_s(12)?,
        attribution: get_s(13)?,
        changes: get_o(14)?,
        origin: enum_from::<Origin>("origin", &get_s(15)?)?,
        source_format: get_s(16)?,
        source_file: get_o(17)?,
        import_tool_version: get_s(18)?,
        format_version: to_u32(row.get(19).map_err(sql)?, "format version")?,
    };
    if rows.next().map_err(sql)?.is_some() {
        return Err(corrupt("manifest table has more than one row"));
    }
    Ok(manifest)
}

impl ResourceStore for ObrReader {
    fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    fn summary(&self) -> Result<Summary, StoreError> {
        let count = |table: &str| -> Result<u64, StoreError> {
            let n: i64 = self
                .conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .map_err(sql)?;
            Ok(n.max(0) as u64)
        };
        Ok(Summary {
            units: count("units")?,
            spans: count("spans")?,
            verses: count("verses")?,
            keys: count("keys")?,
            unit_aliases: count("unit_alias")?,
            cross_references: count("xrefs")?,
            entities: count("entities")?,
            mentions: count("mentions")?,
        })
    }

    fn unit_ids(&self) -> Result<Vec<UnitId>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT id FROM units ORDER BY ord")
            .map_err(sql)?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        ids.into_iter().map(unit_id).collect()
    }

    fn unit(&self, id: &UnitId) -> Result<Option<TextUnit>, StoreError> {
        let row = self
            .conn
            .prepare_cached("SELECT ord, kind, title, text FROM units WHERE id = ?1")
            .map_err(sql)?
            .query_row([id.as_str()], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .optional()
            .map_err(sql)?;
        let Some((ord, kind, title, text)) = row else {
            return Ok(None);
        };
        Ok(Some(TextUnit {
            id: id.clone(),
            kind: enum_from::<UnitKind>("unit kind", &kind)?,
            title,
            text,
            spans: self.spans_where(ord, "", &[])?,
        }))
    }

    fn spans_in(&self, unit: &UnitId, range: Range<usize>) -> Result<Vec<Span>, StoreError> {
        let ord = self
            .unit_ord(unit)?
            .ok_or_else(|| StoreError::UnknownUnit(unit.to_string()))?;
        let s = i64::try_from(range.start).unwrap_or(i64::MAX);
        let e = i64::try_from(range.end).unwrap_or(i64::MAX);
        if s >= e {
            // A point: spans that contain it or sit exactly on it.
            return self.spans_where(ord, "AND start <= ?2 AND end >= ?2", &[s]);
        }
        self.spans_where(
            ord,
            "AND ((start < ?3 AND end > ?2) OR (start = end AND start BETWEEN ?2 AND ?3))",
            &[s, e],
        )
    }

    fn verse(&self, verse: Verse) -> Result<Option<BibleVerse>, StoreError> {
        let row = self
            .conn
            .prepare_cached(
                "SELECT book, chapter, verse, last, unit, start, end FROM verses
                 WHERE book = ?1 AND chapter = ?2 AND verse <= ?3 AND last >= ?3
                 ORDER BY verse DESC LIMIT 1",
            )
            .map_err(sql)?
            .query_row(
                params![verse.book.usfm(), verse.chapter, verse.verse],
                |r| self.verse_row(r),
            )
            .optional()
            .map_err(sql)?;
        row.map(|r| self.to_verse(r)).transpose()
    }

    fn verses(&self) -> Result<Vec<BibleVerse>, StoreError> {
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT book, chapter, verse, last, unit, start, end FROM verses ORDER BY ordinal",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map([], |r| self.verse_row(r))
            .map_err(sql)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(sql)?;
        rows.into_iter().map(|r| self.to_verse(r)).collect()
    }

    fn lookup_key(&self, key: &Key) -> Result<Vec<KeyEntry>, StoreError> {
        let (scheme, _, norm) = key_columns(key);
        self.key_rows(
            "SELECT scheme, key, unit, start, end, alias FROM keys
             WHERE scheme = ?1 AND key_norm = ?2",
            &[&scheme, &norm],
        )
    }

    fn keys(&self, scheme: KeyScheme) -> Result<Vec<KeyEntry>, StoreError> {
        self.key_rows(
            "SELECT scheme, key, unit, start, end, alias FROM keys WHERE scheme = ?1",
            &[&enum_name(&scheme)],
        )
    }

    fn unit_alias(&self, old: &UnitId) -> Result<Option<UnitId>, StoreError> {
        let new: Option<String> = self
            .conn
            .prepare_cached("SELECT new FROM unit_alias WHERE old = ?1")
            .map_err(sql)?
            .query_row([old.as_str()], |r| r.get(0))
            .optional()
            .map_err(sql)?;
        new.map(unit_id).transpose()
    }

    fn verse_map(&self) -> Result<Option<VerseMap>, StoreError> {
        let rows: Vec<(i64, Option<String>, Option<String>)> = {
            let mut stmt = self
                .conn
                .prepare_cached(
                    "SELECT ordinal, std_start, std_end FROM verse_map ORDER BY ordinal",
                )
                .map_err(sql)?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(sql)?
                .collect::<Result<_, _>>()
                .map_err(sql)?
        };
        if rows.is_empty() {
            return Ok(None);
        }
        let verses = self.verses()?;
        if rows.len() != verses.len() {
            return Err(corrupt("verse map and verse list differ in length"));
        }
        let mut entries = Vec::with_capacity(rows.len());
        for ((ordinal, start, end), v) in rows.into_iter().zip(&verses) {
            let standard = match (start, end) {
                (Some(a), Some(b)) => Some(
                    Passage::new(verse_from(&a)?, verse_from(&b)?)
                        .map_err(|e| corrupt(format!("verse map row {ordinal}: {e}")))?,
                ),
                (None, None) => None,
                _ => return Err(corrupt(format!("verse map row {ordinal} is half empty"))),
            };
            entries.push(VerseMapEntry {
                verse: v.verse,
                last: v.last,
                standard,
            });
        }
        let absent: Vec<Verse> = {
            let mut stmt = self
                .conn
                .prepare_cached("SELECT verse FROM verse_absent")
                .map_err(sql)?;
            let texts: Vec<String> = stmt
                .query_map([], |r| r.get(0))
                .map_err(sql)?
                .collect::<Result<_, _>>()
                .map_err(sql)?;
            texts
                .iter()
                .map(|t| verse_from(t))
                .collect::<Result<_, _>>()?
        };
        Ok(Some(VerseMap::new(entries, absent, &Standard)))
    }

    fn cross_references_from(&self, passage: Passage) -> Result<Vec<CrossReference>, StoreError> {
        let longest = self.longest_span(
            &self.xref_span,
            "SELECT max(from_end - from_start) FROM xrefs",
        )?;
        let (start, end) = (verse_number(passage.start()), verse_number(passage.end()));
        self.xref_rows(
            "SELECT from_start, from_end, to_start, to_end, votes FROM xrefs
             WHERE from_start BETWEEN ?1 AND ?2 AND from_end >= ?3
             ORDER BY from_start, from_end, votes DESC, to_start, to_end",
            &[start - longest, end, start],
        )
    }

    fn entity(&self, id: &str) -> Result<Option<Entity>, StoreError> {
        Ok(self
            .entity_rows("WHERE id = ?1", &[&id])?
            .into_iter()
            .next())
    }

    fn mentions_in(&self, passage: Passage) -> Result<Vec<Mention>, StoreError> {
        let longest =
            self.longest_span(&self.mention_span, "SELECT max(end - start) FROM mentions")?;
        let (start, end) = (verse_number(passage.start()), verse_number(passage.end()));
        self.mention_rows(
            "WHERE m.start BETWEEN ?1 AND ?2 AND m.end >= ?3",
            &[start - longest, end, start],
        )
    }

    fn load(&self) -> Result<Resource, StoreError> {
        let mut units = Vec::new();
        for id in self.unit_ids()? {
            units.push(
                self.unit(&id)?
                    .ok_or_else(|| StoreError::UnknownUnit(id.to_string()))?,
            );
        }
        let mut keys = Vec::new();
        for scheme in [
            KeyScheme::Passage,
            KeyScheme::StrongsNumber,
            KeyScheme::Lemma,
            KeyScheme::Headword,
        ] {
            keys.extend(self.keys(scheme)?);
        }
        keys.sort();
        let mut unit_aliases = BTreeMap::new();
        {
            let mut stmt = self
                .conn
                .prepare_cached("SELECT old, new FROM unit_alias ORDER BY old")
                .map_err(sql)?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .map_err(sql)?;
            for row in rows {
                let (old, new) = row.map_err(sql)?;
                unit_aliases.insert(unit_id(old)?, unit_id(new)?);
            }
        }
        let mut dropped_markup = BTreeMap::new();
        {
            let mut stmt = self
                .conn
                .prepare_cached("SELECT marker, count FROM dropped_markup ORDER BY marker")
                .map_err(sql)?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
                .map_err(sql)?;
            for row in rows {
                let (marker, count) = row.map_err(sql)?;
                dropped_markup.insert(marker, count.max(0) as u64);
            }
        }
        let resource = Resource {
            manifest: self.manifest.clone(),
            units,
            verses: self.verses()?,
            keys,
            dropped_markup,
            unit_aliases,
            verse_map: self.verse_map()?,
            cross_references: self.xref_rows(
                "SELECT from_start, from_end, to_start, to_end, votes FROM xrefs
                 ORDER BY from_start, from_end, to_start, to_end, votes",
                &[],
            )?,
            entities: self.entity_rows("", &[])?,
            mentions: self.mention_rows("", &[])?,
        };
        resource.validate().map_err(StoreError::Invalid)?;
        let hash = resource.content_hash();
        if hash != self.manifest.content_hash {
            return Err(corrupt(format!(
                "content hash is {hash}, manifest says {}",
                self.manifest.content_hash
            )));
        }
        Ok(resource)
    }
}

#[cfg(test)]
#[path = "obr_tests.rs"]
mod tests;
