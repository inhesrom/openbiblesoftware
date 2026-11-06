//! Search index cache files (#58, #16 rules 1–3 and 8): one SQLite FTS5 file per Resource,
//! separate from its `.obr` (ADR 0003) and rebuildable at any time.
//!
//! A cache is keyed by `(resource_id, content hash, index version)`, and the key is the file
//! name ([`IndexKey::file_name`]), so finding a usable cache is a path check and a stale one
//! is simply a file with another name. The key is also stored inside the file and checked on
//! open.
//!
//! Layout:
//!
//! | table | rows |
//! |---|---|
//! | `meta` | `name` → `value`: `resource_id`, `content_hash`, `index_version`, `documents`, `document_kind` |
//! | `units` | `ord`, `id`, `title` of every unit that has a document |
//! | `docs` | one per document: `id` (the verse ordinal for a Bible), `unit` (→ `units.ord`), `start`, `end` (byte range in the unit), `key_scheme`, `key` |
//! | `search` | FTS5 over `text`, `strong`, `lemma`; `detail=full` for phrases |
//!
//! `search` is an external-content table over `docs`: the indexed text lives in the Resource,
//! and `docs` only locates each document there. The column values are never stored (`docs`
//! yields NULL for them): FTS5 reads them only for `snippet()`, `highlight()` and `rebuild`,
//! which the app never uses, because hits are highlighted in Rust on the display text
//! (#16 rule 7). The index itself holds only the tokens and their positions.
//!
//! Builds are deterministic: the same Resource and version give a byte-identical file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use obs_model::{
    Key, KeyEntry, Manifest, Resource, ResourceType, SpanKind, TextUnit, UnitId, tokenize,
};
use obs_scripture::{Passage, Verse};
use obs_search::{
    Compiled, DocumentKind, HitOrder, SearchError, SearchHit, SearchIndex, SearchOptions,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::codec::{key_columns, key_from};
use crate::error::StoreError;

/// `PRAGMA application_id` of every Search index cache: ASCII `OBSI`.
pub const SEARCH_APPLICATION_ID: i32 = 0x4F42_5349;

/// File extension of a Search index cache.
pub const SEARCH_INDEX_EXTENSION: &str = "obsidx";

/// unicode61 over already-normalized words. Marks count as word characters, as in
/// [`obs_model::tokenize`], so both sides split words the same way.
const TOKENIZER: &str = "unicode61 remove_diacritics 0 categories 'L* N* Co M*'";

fn schema() -> String {
    format!(
        "CREATE TABLE meta (name TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
         CREATE TABLE units (ord INTEGER PRIMARY KEY, id TEXT NOT NULL, title TEXT);
         CREATE TABLE docs (
             id INTEGER PRIMARY KEY,
             unit INTEGER NOT NULL,
             start INTEGER NOT NULL,
             end INTEGER NOT NULL,
             key_scheme TEXT,
             key TEXT,
             text TEXT GENERATED ALWAYS AS (NULL) VIRTUAL,
             strong TEXT GENERATED ALWAYS AS (NULL) VIRTUAL,
             lemma TEXT GENERATED ALWAYS AS (NULL) VIRTUAL
         );
         CREATE VIRTUAL TABLE search USING fts5(
             text, strong, lemma,
             content = 'docs', content_rowid = 'id', detail = full,
             tokenize = \"{TOKENIZER}\"
         );"
    )
}

fn sql(e: rusqlite::Error) -> StoreError {
    StoreError::Sqlite(e.to_string())
}

/// What a Search index cache is for: one Resource's content at one index version.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexKey {
    pub resource_id: String,
    /// The manifest's `content_hash` (`sha256:…`).
    pub content_hash: String,
    /// [`obs_search::INDEX_VERSION`] when built.
    pub version: u32,
}

impl IndexKey {
    pub fn new(manifest: &Manifest, version: u32) -> Self {
        Self {
            resource_id: manifest.resource_id.clone(),
            content_hash: manifest.content_hash.clone(),
            version,
        }
    }

    /// `<stem>.<first 16 hex digits of the content hash>.v<version>.obsidx`, where `<stem>`
    /// is the resource id if it is a plain id ([`file_stem`]). The name never leaves the
    /// directory it is joined to, and its four dot-separated parts identify exactly one
    /// Resource's caches.
    pub fn file_name(&self) -> String {
        let hash: String = self
            .content_hash
            .trim_start_matches("sha256:")
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(16)
            .collect();
        let hash = if hash.is_empty() { "0".into() } else { hash };
        format!(
            "{}.{hash}.v{}.{SEARCH_INDEX_EXTENSION}",
            file_stem(&self.resource_id),
            self.version
        )
    }
}

/// The file-name stem of `resource_id`'s caches: the id itself if it is `[a-z0-9-]{1,64}`
/// (every id the importers make), else `_` and a hash of it, so an unchecked id from a
/// dropped `.obr` (`a.b`, `../x`, `C:\x`) can neither escape the cache folder nor share
/// another Resource's names. `_` is never in a plain id, so the two kinds cannot collide.
fn file_stem(resource_id: &str) -> String {
    let plain = (1..=64).contains(&resource_id.len())
        && resource_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if plain {
        return resource_id.to_string();
    }
    // FNV-1a: stable across Rust releases, unlike `DefaultHasher`.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in resource_id.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("_{h:016x}")
}

/// Whether `name` is exactly a Search index cache file name of `resource_id`:
/// `<stem>.<hash>.v<version>.obsidx`.
fn is_cache_of(name: &str, resource_id: &str) -> bool {
    let parts: Vec<&str> = name.split('.').collect();
    let [stem, hash, version, ext] = parts.as_slice() else {
        return false;
    };
    *stem == file_stem(resource_id)
        && (1..=16).contains(&hash.len())
        && hash.bytes().all(|b| b.is_ascii_alphanumeric())
        && version
            .strip_prefix('v')
            .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        && *ext == SEARCH_INDEX_EXTENSION
}

/// `resource_id`'s cache files in `dir` (any content hash or version), sorted by name.
pub fn search_index_files(dir: &Path, resource_id: &str) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .filter(|e| is_cache_of(&e.file_name().to_string_lossy(), resource_id))
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    out.sort();
    out
}

/// Delete `resource_id`'s cache files in `dir`, except `keep`. Best effort: a file that
/// cannot be deleted now (open elsewhere) is left for next time. Returns the files deleted.
pub fn remove_search_indexes(dir: &Path, resource_id: &str, keep: Option<&str>) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return removed;
    };
    for entry in rd.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_cache_of(&name, resource_id)
            && keep != Some(name.as_str())
            && std::fs::remove_file(entry.path()).is_ok()
        {
            removed.push(entry.path());
        }
    }
    removed
}

// ---------------------------------------------------------------------------- documents

/// One search document before it is written.
struct Doc {
    id: i64,
    unit: i64,
    start: u32,
    end: u32,
    key: Option<Key>,
    text: String,
    strong: String,
    lemma: String,
}

/// Normalized words of `text[range]`, skipping `holes` (sorted, e.g. headings), appended to
/// `out` with single spaces.
fn push_words(
    text: &str,
    range: std::ops::Range<usize>,
    holes: &[(usize, usize)],
    out: &mut String,
) {
    let mut push = |a: usize, b: usize| {
        if a < b
            && let Some(piece) = text.get(a..b)
        {
            for t in tokenize(piece) {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&t.text);
            }
        }
    };
    let mut at = range.start;
    let first = holes.partition_point(|&(_, e)| e <= range.start);
    for &(s, e) in &holes[first..] {
        if s >= range.end {
            break;
        }
        push(at, s.min(range.end));
        at = at.max(e);
    }
    push(at, range.end);
}

/// One unit's Tagged words and headings, sorted by start, for slicing by document.
struct UnitMarks<'a> {
    tagged: Vec<(usize, usize, &'a [obs_model::MorphemePart])>,
    headings: Vec<(usize, usize)>,
}

impl<'a> UnitMarks<'a> {
    fn of(unit: &'a TextUnit) -> Self {
        let mut tagged = Vec::new();
        let mut headings = Vec::new();
        for s in &unit.spans {
            match &s.kind {
                SpanKind::TaggedWord { parts } => {
                    tagged.push((s.start as usize, s.end as usize, parts.as_slice()));
                }
                SpanKind::Heading { .. } if s.start < s.end => {
                    headings.push((s.start as usize, s.end as usize));
                }
                _ => {}
            }
        }
        tagged.sort_by_key(|t| (t.0, t.1));
        headings.sort_unstable();
        Self { tagged, headings }
    }

    /// The `strong` and `lemma` fields of the Tagged words that start inside `range`.
    fn fields(&self, range: std::ops::Range<usize>) -> (String, String) {
        let mut strong = String::new();
        let mut lemma = String::new();
        let first = self.tagged.partition_point(|t| t.0 < range.start);
        for (s, _, parts) in &self.tagged[first..] {
            if *s >= range.end {
                break;
            }
            for p in *parts {
                let n = p.strongs;
                for form in std::iter::once(n).chain(n.is_extended().then(|| n.base())) {
                    if !strong.is_empty() {
                        strong.push(' ');
                    }
                    strong.push_str(&form.to_string().to_lowercase());
                }
                if let Some(l) = &p.lemma {
                    push_words(l, 0..l.len(), &[], &mut lemma);
                }
            }
        }
        (strong, lemma)
    }
}

/// The Key a paragraph at `start` carries: the unit's non-alias Key whose range holds it,
/// else the unit's whole-unit Key, else its first Key.
fn key_at(keys: &[&KeyEntry], start: u32) -> Option<Key> {
    let ranged = keys
        .iter()
        .filter(|k| {
            k.range
                .is_some_and(|(a, b)| a <= start && start < b.max(a + 1))
        })
        .max_by_key(|k| k.range.map(|(a, _)| a));
    ranged
        .or_else(|| keys.iter().find(|k| k.range.is_none()))
        .or_else(|| keys.first())
        .map(|k| k.key.clone())
}

/// Paragraph ranges of a unit: its non-empty `para` spans (outermost first, nested ones
/// skipped), plus every stretch outside them that holds a word (headings, unparagraphed
/// entries).
fn paragraphs(unit: &TextUnit) -> Vec<(usize, usize)> {
    let mut paras: Vec<(usize, usize)> = unit
        .spans
        .iter()
        .filter(|s| matches!(s.kind, SpanKind::Para { .. }) && s.start < s.end)
        .map(|s| (s.start as usize, s.end as usize))
        .collect();
    paras.sort_by_key(|&(s, e)| (s, std::cmp::Reverse(e)));
    let mut out: Vec<(usize, usize)> = Vec::new();
    // A stretch outside the paragraphs, trimmed of white space, if it holds a word.
    let gap = |a: usize, b: usize, out: &mut Vec<(usize, usize)>| {
        let Some(t) = unit.text.get(a..b) else {
            return;
        };
        if t.chars().any(char::is_alphanumeric) {
            let start = a + (t.len() - t.trim_start().len());
            let end = a + t.trim_end().len();
            out.push((start, end));
        }
    };
    let mut at = 0;
    for (s, e) in paras {
        if s < at {
            continue; // nested inside (or overlapping) the previous paragraph
        }
        gap(at, s, &mut out);
        out.push((s, e));
        at = e;
    }
    gap(at, unit.text.len(), &mut out);
    out
}

/// Every document of `r`, in id order, passed to `emit` with the unit's position in
/// `r.units`. `emit` returns false to stop (cancelled).
fn documents(
    r: &Resource,
    mut emit: impl FnMut(Doc, usize) -> Result<bool, StoreError>,
) -> Result<(DocumentKind, bool), StoreError> {
    let unit_index: HashMap<&UnitId, usize> = r
        .units
        .iter()
        .enumerate()
        .map(|(i, u)| (&u.id, i))
        .collect();
    if r.manifest.resource_type == ResourceType::Bible {
        let mut marks: HashMap<usize, UnitMarks<'_>> = HashMap::new();
        for (ordinal, v) in r.verses.iter().enumerate() {
            let Some(&ui) = unit_index.get(&v.unit) else {
                return Err(StoreError::UnknownUnit(v.unit.to_string()));
            };
            let unit = &r.units[ui];
            let m = marks.entry(ui).or_insert_with(|| UnitMarks::of(unit));
            let range = v.range();
            let mut text = String::new();
            push_words(&unit.text, range.clone(), &m.headings, &mut text);
            let (strong, lemma) = m.fields(range);
            let last = Verse::new(v.verse.book, v.verse.chapter, v.last.max(v.verse.verse));
            let doc = Doc {
                id: i64::try_from(ordinal).unwrap_or(i64::MAX),
                unit: ui as i64,
                start: v.start,
                end: v.end,
                key: Passage::new(v.verse, last).ok().map(Key::Passage),
                text,
                strong,
                lemma,
            };
            if !emit(doc, ui)? {
                return Ok((DocumentKind::Verse, false));
            }
        }
        return Ok((DocumentKind::Verse, true));
    }
    let mut keys: HashMap<&UnitId, Vec<&KeyEntry>> = HashMap::new();
    for k in r.keys.iter().filter(|k| !k.alias) {
        keys.entry(&k.unit).or_default().push(k);
    }
    let mut id = 0i64;
    for (ui, unit) in r.units.iter().enumerate() {
        let marks = UnitMarks::of(unit);
        let unit_keys = keys.get(&unit.id).map_or(&[][..], Vec::as_slice);
        for (s, e) in paragraphs(unit) {
            let mut text = String::new();
            push_words(&unit.text, s..e, &[], &mut text);
            let (strong, lemma) = marks.fields(s..e);
            if text.is_empty() && strong.is_empty() && lemma.is_empty() {
                continue;
            }
            let start = u32::try_from(s).unwrap_or(u32::MAX);
            let doc = Doc {
                id,
                unit: ui as i64,
                start,
                end: u32::try_from(e).unwrap_or(u32::MAX),
                key: key_at(unit_keys, start),
                text,
                strong,
                lemma,
            };
            id += 1;
            if !emit(doc, ui)? {
                return Ok((DocumentKind::Paragraph, false));
            }
        }
    }
    Ok((DocumentKind::Paragraph, true))
}

/// The documents a Resource will have (for progress totals).
fn document_estimate(r: &Resource) -> u64 {
    if r.manifest.resource_type == ResourceType::Bible {
        r.verses.len() as u64
    } else {
        r.units.len() as u64
    }
}

// -------------------------------------------------------------------------------- build

/// Build `resource`'s Search index cache in `dir` at `version`, replacing any older cache of
/// the same Resource there. `progress(done, total)` is called as documents are indexed;
/// returning false cancels the build ([`StoreError::Cancelled`]) and leaves nothing behind.
/// Returns the cache file.
pub fn build_search_index(
    resource: &Resource,
    version: u32,
    dir: &Path,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<PathBuf, StoreError> {
    let path = write_search_index(resource, version, dir, progress)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    remove_search_indexes(dir, &resource.manifest.resource_id, name.as_deref());
    Ok(path)
}

/// [`build_search_index`] that keeps the Resource's older caches in `dir`: a Resource
/// revision being imported is not in the library yet, and the revision before it searches
/// through its own cache until the new one is (#136). The caller removes them after.
pub fn write_search_index(
    resource: &Resource,
    version: u32,
    dir: &Path,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<PathBuf, StoreError> {
    let key = IndexKey::new(&resource.manifest, version);
    std::fs::create_dir_all(dir)?;
    let name = key.file_name();
    let path = dir.join(&name);
    // Unique per build, so two builds of the same key (two app instances, or an Import and
    // a background rebuild) never share, or delete, each other's temp file.
    static BUILDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = BUILDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = dir.join(format!("{name}.{}-{n}.partial", std::process::id()));
    if tmp.exists() {
        std::fs::remove_file(&tmp)?;
    }
    if let Err(e) = write_index(resource, &key, &tmp, progress) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        // The same key is already there (built by another instance, and maybe open): the
        // build is deterministic, so keep that one.
        let _ = std::fs::remove_file(&tmp);
        if !path.is_file() {
            return Err(e.into());
        }
    }
    Ok(path)
}

fn write_index(
    r: &Resource,
    key: &IndexKey,
    path: &Path,
    progress: &mut dyn FnMut(u64, u64) -> bool,
) -> Result<(), StoreError> {
    let mut conn = Connection::open(path).map_err(sql)?;
    conn.execute_batch(&format!(
        "PRAGMA page_size = 4096;
         PRAGMA journal_mode = OFF;
         PRAGMA synchronous = OFF;
         PRAGMA application_id = {SEARCH_APPLICATION_ID};
         PRAGMA user_version = {};",
        key.version
    ))
    .map_err(sql)?;
    conn.execute_batch(&schema()).map_err(sql)?;
    let tx = conn.transaction().map_err(sql)?;
    let total = document_estimate(r);
    if !progress(0, total) {
        return Err(StoreError::Cancelled);
    }
    let (kind, count) = {
        let mut doc_stmt = tx
            .prepare(
                "INSERT INTO docs (id, unit, start, end, key_scheme, key)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(sql)?;
        let mut fts_stmt = tx
            .prepare("INSERT INTO search (rowid, text, strong, lemma) VALUES (?1, ?2, ?3, ?4)")
            .map_err(sql)?;
        let mut unit_stmt = tx
            .prepare("INSERT OR IGNORE INTO units (ord, id, title) VALUES (?1, ?2, ?3)")
            .map_err(sql)?;
        let mut last_unit: Option<usize> = None;
        let mut reported = 0u64;
        let mut count = 0u64;
        let mut units_done = 0u64;
        let is_bible = r.manifest.resource_type == ResourceType::Bible;
        let (kind, finished) = documents(r, |doc, ui| {
            if last_unit != Some(ui) {
                if !is_bible {
                    units_done += 1;
                }
                let unit = &r.units[ui];
                unit_stmt
                    .execute(params![ui as i64, unit.id.as_str(), unit.title])
                    .map_err(sql)?;
                last_unit = Some(ui);
            }
            let (scheme, text) = match &doc.key {
                Some(k) => {
                    let (s, t, _) = key_columns(k);
                    (Some(s), Some(t))
                }
                None => (None, None),
            };
            doc_stmt
                .execute(params![doc.id, doc.unit, doc.start, doc.end, scheme, text])
                .map_err(sql)?;
            fts_stmt
                .execute(params![doc.id, doc.text, doc.strong, doc.lemma])
                .map_err(sql)?;
            count += 1;
            let done = if is_bible { count } else { units_done };
            if done >= reported + 256 {
                reported = done;
                if !progress(done, total) {
                    return Ok(false);
                }
            }
            Ok(true)
        })?;
        if !finished {
            return Err(StoreError::Cancelled);
        }
        (kind, count)
    };
    for (name, value) in [
        ("resource_id", key.resource_id.clone()),
        ("content_hash", key.content_hash.clone()),
        ("index_version", key.version.to_string()),
        ("documents", count.to_string()),
        ("document_kind", kind.name().to_string()),
    ] {
        tx.execute(
            "INSERT INTO meta (name, value) VALUES (?1, ?2)",
            params![name, value],
        )
        .map_err(sql)?;
    }
    // One b-tree segment: fastest to query, and the same bytes on every build.
    tx.execute("INSERT INTO search (search) VALUES ('optimize')", [])
        .map_err(sql)?;
    tx.commit().map_err(sql)?;
    if !progress(total, total) {
        return Err(StoreError::Cancelled);
    }
    conn.execute_batch("VACUUM;").map_err(sql)?;
    conn.close().map_err(|(_, e)| sql(e))?;
    Ok(())
}

// --------------------------------------------------------------------------------- read

/// An open Search index cache.
#[derive(Debug)]
pub struct SqliteSearchIndex {
    conn: Connection,
    key: IndexKey,
    kind: DocumentKind,
    documents: u64,
}

impl SqliteSearchIndex {
    /// Open a cache read-only. With `expected`, refuse one built for another Resource,
    /// content or version.
    pub fn open(path: &Path, expected: Option<&IndexKey>) -> Result<Self, StoreError> {
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
            .map_err(sql)?;
        if app_id != SEARCH_APPLICATION_ID {
            return Err(StoreError::Corrupt(format!(
                "{} is not a Search index (application id {app_id:#x})",
                path.display()
            )));
        }
        let meta = |name: &str| -> Result<String, StoreError> {
            conn.query_row("SELECT value FROM meta WHERE name = ?1", [name], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .map_err(sql)?
            .ok_or_else(|| StoreError::Corrupt(format!("Search index has no `{name}`")))
        };
        let number = |name: &str| -> Result<u64, StoreError> {
            meta(name)?
                .parse()
                .map_err(|_| StoreError::Corrupt(format!("Search index `{name}` is not a number")))
        };
        let key = IndexKey {
            resource_id: meta("resource_id")?,
            content_hash: meta("content_hash")?,
            version: u32::try_from(number("index_version")?)
                .map_err(|_| StoreError::Corrupt("index version out of range".into()))?,
        };
        if let Some(want) = expected
            && *want != key
        {
            return Err(StoreError::Corrupt(format!(
                "Search index is for {} {} v{}, expected {} {} v{}",
                key.resource_id,
                key.content_hash,
                key.version,
                want.resource_id,
                want.content_hash,
                want.version
            )));
        }
        let kind = DocumentKind::from_name(&meta("document_kind")?)
            .ok_or_else(|| StoreError::Corrupt("unknown document kind".into()))?;
        let documents = number("documents")?;
        Ok(Self {
            conn,
            key,
            kind,
            documents,
        })
    }

    pub fn key(&self) -> &IndexKey {
        &self.key
    }
}

fn search_error(e: rusqlite::Error) -> SearchError {
    SearchError(e.to_string())
}

impl SearchIndex for SqliteSearchIndex {
    fn resource_id(&self) -> &str {
        &self.key.resource_id
    }

    fn document_kind(&self) -> DocumentKind {
        self.kind
    }

    fn document_count(&self) -> u64 {
        self.documents
    }

    fn search(
        &self,
        query: &Compiled,
        options: &SearchOptions,
    ) -> Result<Vec<SearchHit>, SearchError> {
        let Compiled::Match(expr) = query else {
            return Ok(Vec::new());
        };
        let relevance = options.order == HitOrder::Relevance;
        let mut q =
            String::from("SELECT search.rowid, u.id, u.title, d.start, d.end, d.key_scheme, d.key");
        q.push_str(if relevance { ", search.rank" } else { ", 0.0" });
        q.push_str(
            " FROM search JOIN docs d ON d.id = search.rowid JOIN units u ON u.ord = d.unit
              WHERE search MATCH ?1",
        );
        if !options.documents.is_empty() {
            let ranges: Vec<String> = options
                .documents
                .iter()
                .map(|r| format!("search.rowid BETWEEN {} AND {}", r.start(), r.end()))
                .collect();
            q.push_str(&format!(" AND ({})", ranges.join(" OR ")));
        }
        q.push_str(if relevance {
            " ORDER BY search.rank"
        } else {
            " ORDER BY search.rowid"
        });
        if let Some(limit) = options.limit {
            q.push_str(&format!(" LIMIT {limit}"));
        }
        let mut stmt = self.conn.prepare_cached(&q).map_err(search_error)?;
        let rows = stmt
            .query_map([expr], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, Option<String>>(6)?,
                    r.get::<_, f64>(7)?,
                ))
            })
            .map_err(search_error)?;
        let mut out = Vec::new();
        for row in rows {
            let (document, unit, title, start, end, scheme, key, score) =
                row.map_err(search_error)?;
            let unit = UnitId::new(unit).map_err(|e| SearchError(e.to_string()))?;
            let key = match (scheme, key) {
                (Some(s), Some(k)) => {
                    Some(key_from(&s, &k).map_err(|e| SearchError(e.to_string()))?)
                }
                _ => None,
            };
            let offset = |v: i64| u32::try_from(v).map_err(|_| SearchError("bad offset".into()));
            out.push(SearchHit {
                document,
                unit,
                range: offset(start)?..offset(end)?,
                key,
                title,
                score,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
#[path = "search_index_tests.rs"]
mod tests;
