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
//! | `morph_codes` | `token` → `code`: each Morphology code in the Resource and its token in the `morph` column |
//! | `search` | FTS5 over `text`, `strong`, `lemma`, `morph`; `detail=full` for phrases |
//!
//! `morph` lists every morpheme part of the Tagged words that has a Morphology code, in turn:
//! its Strong's number (then the base form of an extended one), its code's token, then its
//! Lemma's words, once from each Lemma word. A phrase such as `"g26 morph3"` therefore finds
//! one part with both (#145); which tokens a query's Morphology criteria mean is read from
//! `morph_codes` ([`obs_search::IndexQuery::plan`]). In `lemma` and `morph` each part ends at
//! a boundary word no query can contain, so a phrase stays inside one part (#257).
//!
//! Queries one MATCH cannot answer (#180) are planned per index ([`obs_search::Plan`]): a
//! stemmed word becomes the index's own words of that stem, found through an `fts5vocab`
//! table over `search`; `BEFORE/n` keeps the `NEAR` candidates in which the word positions
//! (also read through `fts5vocab`) are in order; `WITHIN n verses` searches each side and
//! keeps the documents with a hit of the other side among their neighbours. Both vocabulary
//! tables are in the connection's `temp` schema, so the cache file stays read-only and its
//! format is unchanged.
//!
//! `search` is an external-content table over `docs`: the indexed text lives in the Resource,
//! and `docs` only locates each document there. The column values are never stored (`docs`
//! yields NULL for them): FTS5 reads them only for `snippet()`, `highlight()` and `rebuild`,
//! which the app never uses, because hits are highlighted in Rust on the display text
//! (#16 rule 7). The index itself holds only the tokens and their positions.
//!
//! Builds are deterministic: the same Resource and version give a byte-identical file.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use obs_model::{
    Key, KeyEntry, Manifest, Resource, ResourceType, SpanKind, TextUnit, UnitId, overlaps_heading,
    searchable_text, tokenize,
};
use obs_scripture::{Passage, Verse};
use obs_search::{
    Compiled, DocumentKind, HitOrder, IndexQuery, IndexVocabulary, MorphCriteria, Pattern, Plan,
    SearchError, SearchHit, SearchIndex, SearchOptions,
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

/// Ends each morpheme part's words in the `lemma` and `morph` fields, so a phrase stays inside
/// one part. A private-use character: [`TOKENIZER`] keeps it as a word (`Co`), and
/// [`obs_model::tokenize`], which makes every word of a query, never does, so no query
/// can ask for it.
const PART_BOUNDARY: &str = "\u{E000}";

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
             lemma TEXT GENERATED ALWAYS AS (NULL) VIRTUAL,
             morph TEXT GENERATED ALWAYS AS (NULL) VIRTUAL
         );
         CREATE TABLE morph_codes (token TEXT PRIMARY KEY, code TEXT NOT NULL) WITHOUT ROWID;
         CREATE VIRTUAL TABLE search USING fts5(
             text, strong, lemma, morph,
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
    morph: String,
}

/// The Morphology codes met while indexing, numbered in the order first met (so a build is
/// deterministic). A code's token is `morph` and its number.
#[derive(Default)]
struct MorphCodes {
    tokens: HashMap<String, String>,
    in_order: Vec<(String, String)>,
}

impl MorphCodes {
    fn token(&mut self, code: &str) -> &str {
        if !self.tokens.contains_key(code) {
            let token = format!("morph{}", self.in_order.len() + 1);
            self.in_order.push((token.clone(), code.to_string()));
            self.tokens.insert(code.to_string(), token);
        }
        self.tokens.get(code).map_or("", String::as_str)
    }
}

/// Normalized words of `text[range]`, skipping `holes` (e.g. headings), appended to `out`
/// with single spaces.
fn push_words(
    text: &str,
    range: std::ops::Range<usize>,
    holes: &[std::ops::Range<usize>],
    out: &mut String,
) {
    for t in tokenize(&searchable_text(text, range, holes)) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&t.text);
    }
}

/// One unit's Tagged words and headings, sorted by start, for slicing by document.
struct UnitMarks<'a> {
    tagged: Vec<(usize, usize, &'a [obs_model::MorphemePart])>,
    headings: Vec<std::ops::Range<usize>>,
}

impl<'a> UnitMarks<'a> {
    fn of(unit: &'a TextUnit) -> Self {
        let mut tagged = Vec::new();
        for s in &unit.spans {
            if let SpanKind::TaggedWord { parts } = &s.kind {
                tagged.push((s.start as usize, s.end as usize, parts.as_slice()));
            }
        }
        tagged.sort_by_key(|t| (t.0, t.1));
        Self {
            tagged,
            headings: unit.heading_ranges(),
        }
    }

    /// The `strong`, `lemma` and `morph` fields of the Tagged words that start inside
    /// `range`, less those in `headings` (a Bible Verse's, left out as its text leaves them:
    /// [`obs_model::overlaps_heading`]).
    ///
    /// A `lemma:` term means words of one morpheme part's Lemma, anywhere in it, as the
    /// highlighter reads it (#257). So in `lemma` each part's Lemma ends at a
    /// [`PART_BOUNDARY`], and a phrase never runs on into the next part's. In `morph` each
    /// part with a code has a run (Strong's numbers, code token, Lemma) for each word its
    /// Lemma may be searched from (`g697 morph7 αρειοσ παγοσ`, then `g697 morph7 παγοσ`), so
    /// `token word` is a phrase whichever Lemma word is asked for; each run ends at a
    /// boundary too.
    fn fields(
        &self,
        range: std::ops::Range<usize>,
        headings: &[std::ops::Range<usize>],
        codes: &mut MorphCodes,
    ) -> (String, String, String) {
        let mut strong = String::new();
        let mut lemma = String::new();
        let mut morph = String::new();
        let push = |out: &mut String, word: &str| {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(word);
        };
        let first = self.tagged.partition_point(|t| t.0 < range.start);
        for (s, e, parts) in &self.tagged[first..] {
            if *s >= range.end {
                break;
            }
            if overlaps_heading(headings, &(*s..*e)) {
                continue;
            }
            for p in *parts {
                let n = p.strongs;
                let forms: Vec<String> = std::iter::once(n)
                    .chain(n.is_extended().then(|| n.base()))
                    .map(|f| f.to_string().to_lowercase())
                    .collect();
                for form in &forms {
                    push(&mut strong, form);
                }
                let words: Vec<String> = p
                    .lemma
                    .as_deref()
                    .map(|l| tokenize(l).into_iter().map(|t| t.text).collect())
                    .unwrap_or_default();
                if !words.is_empty() {
                    if !lemma.is_empty() {
                        push(&mut lemma, PART_BOUNDARY);
                    }
                    for w in &words {
                        push(&mut lemma, w);
                    }
                }
                // Every `morph` phrase has a code's token, so a part without a code has no
                // run.
                let Some(code) = p.morph.as_deref().map(str::trim).filter(|c| !c.is_empty()) else {
                    continue;
                };
                let token = codes.token(code).to_string();
                for from in 0..words.len().max(1) {
                    if !morph.is_empty() {
                        push(&mut morph, PART_BOUNDARY);
                    }
                    for form in &forms {
                        push(&mut morph, form);
                    }
                    push(&mut morph, &token);
                    for w in words.get(from..).unwrap_or_default() {
                        push(&mut morph, w);
                    }
                }
            }
        }
        (strong, lemma, morph)
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

/// Every document of `r`, in id order, passed to `emit` with the unit's position in
/// `r.units`. `emit` returns false to stop (cancelled).
fn documents(
    r: &Resource,
    codes: &mut MorphCodes,
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
            let (strong, lemma, morph) = m.fields(range, &m.headings, codes);
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
                morph,
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
        for std::ops::Range { start: s, end: e } in unit.paragraphs() {
            let mut text = String::new();
            push_words(&unit.text, s..e, &[], &mut text);
            let (strong, lemma, morph) = marks.fields(s..e, &[], codes);
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
                morph,
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

/// Copy the Search index cache at `src` into `dir` as the cache of another Resource with the
/// same content (`manifest`, as [`crate::copy_obr_as`] makes it): the copy is named and keyed
/// for `manifest`'s resource id. Returns the new cache file. For synthetic libraries (the
/// scale bench, #227).
pub fn copy_search_index_as(
    src: &Path,
    dir: &Path,
    manifest: &Manifest,
) -> Result<PathBuf, StoreError> {
    let from = SqliteSearchIndex::open(src, None)?.key;
    if from.content_hash != manifest.content_hash {
        return Err(StoreError::Rejected(format!(
            "{} indexes content {}, not {}",
            src.display(),
            from.content_hash,
            manifest.content_hash
        )));
    }
    let key = IndexKey::new(manifest, from.version);
    std::fs::create_dir_all(dir)?;
    let path = dir.join(key.file_name());
    let tmp = dir.join(format!("{}.copy.partial", key.file_name()));
    std::fs::copy(src, &tmp)?;
    let relabel = || -> Result<(), StoreError> {
        let conn = Connection::open(&tmp).map_err(sql)?;
        conn.execute_batch("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;")
            .map_err(sql)?;
        conn.execute(
            "UPDATE meta SET value = ?1 WHERE name = 'resource_id'",
            [&key.resource_id],
        )
        .map_err(sql)?;
        Ok(())
    };
    if let Err(e) = relabel() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &path)?;
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
    let mut codes = MorphCodes::default();
    let (kind, count) = {
        let mut doc_stmt = tx
            .prepare(
                "INSERT INTO docs (id, unit, start, end, key_scheme, key)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(sql)?;
        let mut fts_stmt = tx
            .prepare(
                "INSERT INTO search (rowid, text, strong, lemma, morph)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(sql)?;
        let mut unit_stmt = tx
            .prepare("INSERT OR IGNORE INTO units (ord, id, title) VALUES (?1, ?2, ?3)")
            .map_err(sql)?;
        let mut last_unit: Option<usize> = None;
        let mut reported = 0u64;
        let mut count = 0u64;
        let mut units_done = 0u64;
        let is_bible = r.manifest.resource_type == ResourceType::Bible;
        let (kind, finished) = documents(r, &mut codes, |doc, ui| {
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
                .execute(params![doc.id, doc.text, doc.strong, doc.lemma, doc.morph])
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
    for (token, code) in &codes.in_order {
        tx.execute(
            "INSERT INTO morph_codes (token, code) VALUES (?1, ?2)",
            params![token, code],
        )
        .map_err(sql)?;
    }
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
    codes: IndexCodes,
}

/// An open index's Morphology codes, decoded once, with their tokens.
#[derive(Debug, Default)]
struct IndexCodes(Vec<(String, obs_model::Morph)>);

impl IndexCodes {
    fn tokens(&self, criteria: &MorphCriteria) -> Vec<String> {
        self.0
            .iter()
            .filter(|(_, m)| criteria.matches_morph(m))
            .map(|(t, _)| t.clone())
            .collect()
    }
}

/// An open index's codes and words, for [`obs_search::IndexQuery::plan`], and the first
/// error reading its words: the search reports it rather than finding nothing.
struct Vocabulary<'a> {
    index: &'a SqliteSearchIndex,
    error: RefCell<Option<StoreError>>,
}

impl<'a> Vocabulary<'a> {
    fn new(index: &'a SqliteSearchIndex) -> Self {
        Self {
            index,
            error: RefCell::new(None),
        }
    }

    /// How `query` runs on the index, or why its words could not be read.
    fn plan(self, query: &IndexQuery) -> Result<Plan, SearchError> {
        let plan = query.plan(&self);
        match self.error.into_inner() {
            Some(e) => Err(SearchError(e.to_string())),
            None => Ok(plan),
        }
    }
}

impl IndexVocabulary for Vocabulary<'_> {
    fn morph_tokens(&self, criteria: &MorphCriteria) -> Vec<String> {
        self.index.codes.tokens(criteria)
    }

    fn words_with_stem(&self, stem: &str) -> Vec<String> {
        self.index.words_with_stem(stem).unwrap_or_else(|e| {
            self.error.borrow_mut().get_or_insert(e);
            Vec::new()
        })
    }
}

/// The `fts5vocab` tables of an open index, in its connection's `temp` schema: `search_terms`
/// (each term of each column) and `search_positions` (each occurrence of a term).
const VOCABULARY_TABLES: &str =
    "CREATE VIRTUAL TABLE IF NOT EXISTS temp.search_terms USING fts5vocab(main, search, col);
     CREATE VIRTUAL TABLE IF NOT EXISTS temp.search_positions USING fts5vocab(main, search, instance);";

/// The first string after every string that begins with `prefix`, if there is one.
fn prefix_end(prefix: &str) -> Option<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    while let Some(last) = chars.pop() {
        let next = (u32::from(last) + 1..=u32::from(char::MAX)).find_map(char::from_u32);
        if let Some(next) = next {
            chars.push(next);
            return Some(chars.into_iter().collect());
        }
    }
    None
}

/// Documents found, by id.
type Found = BTreeMap<i64, SearchHit>;

/// Start and end word positions of a pattern's occurrences in each document.
type Occurrences = HashMap<i64, Vec<(u32, u32)>>;

/// The codes of an index; none in a cache built before #145 (it has no `morph` column).
fn read_codes(conn: &Connection) -> Result<IndexCodes, StoreError> {
    let has_table: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE name = 'morph_codes')",
            [],
            |r| r.get(0),
        )
        .map_err(sql)?;
    if !has_table {
        return Ok(IndexCodes::default());
    }
    let mut stmt = conn
        .prepare("SELECT token, code FROM morph_codes")
        .map_err(sql)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(sql)?;
    let mut out = Vec::new();
    for row in rows {
        let (token, code) = row.map_err(sql)?;
        // A code the decoder cannot read meets no criteria.
        if let Ok(m) = obs_model::morph::parse(&code) {
            out.push((token, m));
        }
    }
    Ok(IndexCodes(out))
}

/// Run a search that finds nothing through every table a search reads, so a cache whose
/// tables are gone or unreadable fails to open (and is rebuilt) instead of failing every
/// search (#257). The term is a private-use character no document holds, so FTS5 only
/// seeks it.
fn probe(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT search.rowid, u.id, d.start FROM search
         JOIN docs d ON d.id = search.rowid JOIN units u ON u.ord = d.unit
         WHERE search MATCH ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query(["\"\u{E001}\""])?;
    while rows.next()?.is_some() {}
    Ok(())
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
        let codes = read_codes(&conn)?;
        probe(&conn).map_err(|e| StoreError::Corrupt(format!("Search index is damaged: {e}")))?;
        Ok(Self {
            conn,
            key,
            kind,
            documents,
            codes,
        })
    }

    pub fn key(&self) -> &IndexKey {
        &self.key
    }

    fn vocabulary_tables(&self) -> Result<(), StoreError> {
        self.conn.execute_batch(VOCABULARY_TABLES).map_err(sql)
    }

    /// The words of the `text` field whose stem is `stem`.
    fn words_with_stem(&self, stem: &str) -> Result<Vec<String>, StoreError> {
        if !stem.bytes().all(|b| b.is_ascii_lowercase()) {
            return Ok(vec![stem.to_string()]);
        }
        self.vocabulary_tables()?;
        let prefix = obs_search::stem_prefix(stem);
        let end = prefix_end(prefix).unwrap_or_default();
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT term FROM temp.search_terms
                 WHERE term >= ?1 AND term < ?2 AND col = 'text' ORDER BY term",
            )
            .map_err(sql)?;
        let rows = stmt
            .query_map(params![prefix, end], |r| r.get::<_, String>(0))
            .map_err(sql)?;
        let mut out = Vec::new();
        for term in rows {
            let term = term.map_err(sql)?;
            if obs_search::stem(&term) == stem {
                out.push(term);
            }
        }
        Ok(out)
    }

    /// Word positions in the `text` field of each term `word` (a prefix when `prefix`) in
    /// `documents`.
    fn word_positions(
        &self,
        word: &str,
        prefix: bool,
        documents: &BTreeSet<i64>,
    ) -> Result<HashMap<i64, Vec<u32>>, StoreError> {
        let end = if prefix { prefix_end(word) } else { None };
        let mut q = String::from("SELECT doc, offset FROM temp.search_positions WHERE ");
        q.push_str(match (prefix, &end) {
            (false, _) => "term = ?1 AND ?2 IS NULL",
            (true, Some(_)) => "term >= ?1 AND term < ?2",
            (true, None) => "term >= ?1 AND ?2 IS NULL",
        });
        q.push_str(" AND col = 'text'");
        let mut stmt = self.conn.prepare_cached(&q).map_err(sql)?;
        let rows = stmt
            .query_map(params![word, end], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(sql)?;
        let mut out: HashMap<i64, Vec<u32>> = HashMap::new();
        for row in rows {
            let (doc, offset) = row.map_err(sql)?;
            if documents.contains(&doc) {
                out.entry(doc)
                    .or_default()
                    .push(u32::try_from(offset).unwrap_or(u32::MAX));
            }
        }
        Ok(out)
    }

    /// Where `pattern` occurs in each of `documents`: its words at consecutive positions.
    fn occurrences(
        &self,
        pattern: &Pattern,
        documents: &BTreeSet<i64>,
    ) -> Result<Occurrences, StoreError> {
        self.vocabulary_tables()?;
        let n = pattern.words.len();
        let mut words = Vec::with_capacity(n);
        for (i, w) in pattern.words.iter().enumerate() {
            let prefix = pattern.prefix && i + 1 == n;
            words.push(self.word_positions(w, prefix, documents)?);
        }
        let mut out = Occurrences::new();
        let Some((first, rest)) = words.split_first() else {
            return Ok(out);
        };
        let last = u32::try_from(n.saturating_sub(1)).unwrap_or(u32::MAX);
        for (doc, starts) in first {
            for &p in starts {
                let in_row = rest.iter().enumerate().all(|(k, positions)| {
                    let at = p.saturating_add(u32::try_from(k + 1).unwrap_or(u32::MAX));
                    positions.get(doc).is_some_and(|ps| ps.contains(&at))
                });
                if in_row {
                    out.entry(*doc)
                        .or_default()
                        .push((p, p.saturating_add(last)));
                }
            }
        }
        Ok(out)
    }

    /// The documents of `plan` (in `options.documents`), every hit, scored when ranked.
    fn evaluate(&self, plan: &Plan, options: &SearchOptions) -> Result<Found, SearchError> {
        let found =
            |hits: Vec<SearchHit>| -> Found { hits.into_iter().map(|h| (h.document, h)).collect() };
        match plan {
            Plan::Match(expr) => Ok(found(self.run_match(expr, options)?)),
            Plan::Nothing => Ok(Found::new()),
            Plan::All { all, not } => {
                let mut out: Option<Found> = None;
                for p in all {
                    let next = self.evaluate(p, options)?;
                    out = Some(match out {
                        None => next,
                        Some(mut acc) => {
                            acc.retain(|id, _| next.contains_key(id));
                            for (id, hit) in &mut acc {
                                hit.score += next.get(id).map_or(0.0, |h| h.score);
                            }
                            acc
                        }
                    });
                    if out.as_ref().is_some_and(BTreeMap::is_empty) {
                        break;
                    }
                }
                let mut out = out.unwrap_or_default();
                for p in not {
                    if out.is_empty() {
                        break;
                    }
                    let excluded = self.evaluate(p, options)?;
                    out.retain(|id, _| !excluded.contains_key(id));
                }
                Ok(out)
            }
            Plan::Any(any) => {
                let mut out = Found::new();
                for p in any {
                    for (id, hit) in self.evaluate(p, options)? {
                        match out.get_mut(&id) {
                            Some(h) => h.score = h.score.min(hit.score),
                            None => {
                                out.insert(id, hit);
                            }
                        }
                    }
                }
                Ok(out)
            }
            Plan::Before {
                candidates,
                first,
                second,
                words,
            } => {
                let mut out = found(self.run_match(candidates, options)?);
                if out.is_empty() {
                    return Ok(out);
                }
                let documents: BTreeSet<i64> = out.keys().copied().collect();
                let occurrences = |patterns: &[Pattern]| -> Result<Occurrences, SearchError> {
                    let mut all = Occurrences::new();
                    for p in patterns {
                        let found = self
                            .occurrences(p, &documents)
                            .map_err(|e| SearchError(e.to_string()))?;
                        for (doc, spans) in found {
                            all.entry(doc).or_default().extend(spans);
                        }
                    }
                    Ok(all)
                };
                let firsts = occurrences(first)?;
                let seconds = occurrences(second)?;
                out.retain(|doc, _| {
                    let (Some(a), Some(b)) = (firsts.get(doc), seconds.get(doc)) else {
                        return false;
                    };
                    a.iter().any(|&(_, end)| {
                        b.iter()
                            .any(|&(start, _)| start > end && start - end - 1 <= *words)
                    })
                });
                Ok(out)
            }
            Plan::Within {
                left,
                right,
                documents,
            } => {
                let left = self.evaluate(left, options)?;
                if left.is_empty() {
                    return Ok(left);
                }
                let right = self.evaluate(right, options)?;
                let reach = i64::from(documents.saturating_sub(1));
                let mut near_left = BTreeSet::new();
                let mut near_right = BTreeSet::new();
                for (id, hit) in &left {
                    for (other, h) in
                        right.range(id.saturating_sub(reach)..=id.saturating_add(reach))
                    {
                        if self.neighbours(hit, h) {
                            near_left.insert(*id);
                            near_right.insert(*other);
                        }
                    }
                }
                // A document found by both sides scores for both, as with `AND`, so the
                // two sides can be swapped (#257).
                let mut out = Found::new();
                for (side, ids) in [(&left, near_left), (&right, near_right)] {
                    for id in ids {
                        let Some(hit) = side.get(&id) else { continue };
                        match out.get_mut(&id) {
                            Some(h) => h.score += hit.score,
                            None => {
                                out.insert(id, hit.clone());
                            }
                        }
                    }
                }
                Ok(out)
            }
        }
    }

    /// Whether two documents can be neighbours for `WITHIN n verses`: Verses of one Bible
    /// book, or paragraphs of one text unit.
    fn neighbours(&self, a: &SearchHit, b: &SearchHit) -> bool {
        let book = |h: &SearchHit| match &h.key {
            Some(Key::Passage(p)) => Some(p.start().book),
            _ => None,
        };
        match self.kind {
            DocumentKind::Verse => book(a).is_some() && book(a) == book(b),
            DocumentKind::Paragraph => a.unit == b.unit,
        }
    }

    /// The hits of one FTS5 MATCH expression.
    fn run_match(
        &self,
        expr: &str,
        options: &SearchOptions,
    ) -> Result<Vec<SearchHit>, SearchError> {
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

/// Damage a cache file as a disk fault or another program might, by running `sql` on it
/// (`DROP TABLE search`, `UPDATE docs SET start = -1`): for tests of what the app does
/// with a damaged cache.
#[cfg(feature = "test-support")]
pub fn damage_search_index(path: &Path, sql_text: &str) -> Result<(), StoreError> {
    let conn = Connection::open(path).map_err(sql)?;
    conn.execute_batch(sql_text).map_err(sql)
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
        let plan = match query {
            Compiled::Match(expr) => return self.run_match(expr, options),
            Compiled::PerIndex(q) => Vocabulary::new(self).plan(q)?,
            Compiled::Nothing => return Ok(Vec::new()),
        };
        match plan {
            Plan::Match(expr) => self.run_match(&expr, options),
            Plan::Nothing => Ok(Vec::new()),
            plan => {
                // Every hit of each part, then the order and limit asked for.
                let parts = SearchOptions {
                    documents: options.documents.clone(),
                    order: options.order,
                    limit: None,
                };
                let mut hits: Vec<SearchHit> =
                    self.evaluate(&plan, &parts)?.into_values().collect();
                if options.order == HitOrder::Relevance {
                    hits.sort_by(|a, b| {
                        a.score
                            .total_cmp(&b.score)
                            .then(a.document.cmp(&b.document))
                    });
                }
                if let Some(limit) = options.limit {
                    hits.truncate(limit);
                }
                Ok(hits)
            }
        }
    }
}

#[cfg(test)]
#[path = "search_index_tests.rs"]
mod tests;
