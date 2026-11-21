//! The Christian Open Library (ADR 0024, #347): its books downloaded into the Library as Books
//! (ADR 0023), through the user's own access to COL's GitHub repository or a local clone, and
//! kept up to date.
//!
//! This module holds what needs no network: the repository's tree and catalogue as the app
//! reads them, the filters Get Resources offers, each book's Resource id and origin, and the
//! comparison Check for new books makes. Requests go through the app's transport
//! ([`obs_formats::sword_repo::Fetch`]); the engine opens no connection itself.

use std::collections::{BTreeMap, BTreeSet};

use obs_formats::{BookFiles, group_book_files};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The repository COL keeps its books in.
pub const COL_OWNER: &str = "Christian-open-library";
pub const COL_REPO: &str = "books";
/// GitHub's REST API.
pub const GITHUB_API: &str = "https://api.github.com";
/// COL's public catalogue: author, title, year, type, parts and folder of every work, readable
/// without signing in.
pub const PUBLIC_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/Christian-open-library/.github/main/catalog.csv";
/// Where a person asks COL for access.
pub const REQUEST_ACCESS_URL: &str = "https://github.com/Christian-open-library/.github/issues/new?template=request-library-access.yml";
/// COL's website, named in every COL Book's Attribution.
pub const COL_WEBSITE: &str = "https://christianopenlibrary.org";

/// The longest path in the tree the app reads; COL's longest are about 200 bytes.
const MAX_PATH: usize = 400;
/// A file larger than this is never downloaded (the importer refuses it anyway).
pub const MAX_FILE_BYTES: u64 = obs_formats::MAX_BOOK_FILE_BYTES as u64;

// ----------------------------------------------------------------------------------- website

/// The longest search COL's website takes, in characters.
pub const WEBSITE_QUERY_CHARS: usize = 500;

/// COL's website searching for `query` (#343): `https://christianopenlibrary.org/#q=…`, the
/// query's whitespace collapsed and cut to [`WEBSITE_QUERY_CHARS`] at a word, percent-encoded.
/// The search is after the `#`, which the browser keeps to itself: COL's page reads it, and its
/// server never receives it. The app sends nothing. `None` for an empty query.
pub fn website_search_url(query: &str) -> Option<String> {
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut q = String::new();
    for w in words {
        let more = usize::from(!q.is_empty()) + w.chars().count();
        if q.chars().count() + more > WEBSITE_QUERY_CHARS {
            if q.is_empty() {
                q = w.chars().take(WEBSITE_QUERY_CHARS).collect();
            }
            break;
        }
        if !q.is_empty() {
            q.push(' ');
        }
        q.push_str(w);
    }
    if q.is_empty() {
        return None;
    }
    let mut encoded = String::with_capacity(q.len() * 3);
    for b in q.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(b));
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    Some(format!("{COL_WEBSITE}/#q={encoded}"))
}

// ------------------------------------------------------------------------------------- paths

/// Whether a repository path may be read: relative, `/`-separated, no empty, `.` or `..`
/// component, no `.git` folder, no backslash or control character, not too long.
pub fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|c| !c.is_empty() && c != "." && c != ".." && !c.eq_ignore_ascii_case(".git"))
}

/// Whether a repository file is one the app reads: a Markdown book file, or one of the
/// catalogue files.
pub fn wanted_file(path: &str) -> bool {
    safe_path(path)
        && (obs_formats::is_markdown_name(path) || matches!(path, CATALOG_FILE | AUTHORS_FILE))
        && !path.split('/').any(|c| c.starts_with('.'))
}

/// The catalogue of every work, at the repository's root.
pub const CATALOG_FILE: &str = "catalog_full.csv";
/// Authors' dates and traditions, at the repository's root.
pub const AUTHORS_FILE: &str = "authors.csv";

// -------------------------------------------------------------------------------------- tree

/// One file of the repository's tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeFile {
    /// Git's blob SHA (hex).
    pub sha: String,
    pub size: u64,
}

/// The repository's files at one commit: what the app reads of it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColTree {
    /// The commit (or, for a local clone, empty).
    pub commit: String,
    pub files: BTreeMap<String, TreeFile>,
    /// Entries refused (unsafe paths, files too large, links), for the log.
    pub refused: usize,
}

/// Why the tree could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeError {
    /// GitHub cut the listing short (more than 100,000 entries or 7 MB).
    Truncated,
    Invalid(String),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeError::Truncated => f.write_str("the repository's file list is too long to read"),
            TreeError::Invalid(e) => write!(f, "the repository's file list could not be read: {e}"),
        }
    }
}

impl ColTree {
    /// From GitHub's `git/trees/{sha}?recursive=1` answer. Only [`wanted_file`]s are kept;
    /// links, submodules, unsafe paths and oversized files are refused.
    pub fn from_github(commit: &str, json: &[u8]) -> Result<Self, TreeError> {
        let v: Value =
            serde_json::from_slice(json).map_err(|e| TreeError::Invalid(e.to_string()))?;
        if v.get("truncated").and_then(Value::as_bool) == Some(true) {
            return Err(TreeError::Truncated);
        }
        let entries = v
            .get("tree")
            .and_then(Value::as_array)
            .ok_or_else(|| TreeError::Invalid("no tree".into()))?;
        let mut tree = ColTree {
            commit: commit.to_string(),
            ..Self::default()
        };
        for e in entries {
            let (Some(path), Some(kind)) = (
                e.get("path").and_then(Value::as_str),
                e.get("type").and_then(Value::as_str),
            ) else {
                tree.refused += 1;
                continue;
            };
            if kind != "blob" {
                continue; // folders
            }
            let mode = e.get("mode").and_then(Value::as_str).unwrap_or("");
            let sha = e.get("sha").and_then(Value::as_str).unwrap_or("");
            let size = e.get("size").and_then(Value::as_u64).unwrap_or(u64::MAX);
            let regular = mode == "100644" || mode == "100755";
            if !regular || !is_sha(sha) || size > MAX_FILE_BYTES || !safe_path(path) {
                tree.refused += 1;
                continue;
            }
            if wanted_file(path) {
                tree.files.insert(
                    path.to_string(),
                    TreeFile {
                        sha: sha.to_ascii_lowercase(),
                        size,
                    },
                );
            }
        }
        Ok(tree)
    }

    /// The books in the tree: each main file with its Part files.
    pub fn books(&self) -> Vec<BookFiles> {
        let names: Vec<String> = self
            .files
            .keys()
            .filter(|p| obs_formats::is_markdown_name(p))
            .cloned()
            .collect();
        group_book_files(&names)
    }
}

/// A download's size in words: "64 KB", "4.5 MB", "2.9 GB" (a single sermon is a few KB, the
/// whole library a few GB).
#[allow(clippy::cast_precision_loss)]
pub fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes < 1_000_000 {
        format!("{} KB", bytes.div_ceil(1_000))
    } else if bytes < 1_000_000_000 {
        format!("{:.1} MB", b / 1e6)
    } else {
        format!("{:.1} GB", b / 1e9)
    }
}

/// Whether `s` is a hex SHA-1 (40) or SHA-256 (64) object id.
pub fn is_sha(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Git's blob id of `bytes`: SHA-1 of `blob <len>\0` and the bytes, in hex. A local clone's
/// files are compared with the repository's by it.
pub fn git_blob_sha(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut h = Sha1::new();
    h.update(format!("blob {}\0", bytes.len()).as_bytes());
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

// --------------------------------------------------------------------------------------- csv

/// The rows of a CSV file (RFC 4180: quoted fields may hold commas, quotes doubled, line
/// breaks), each a map from its header's column name. Rows with more fields than the header
/// keep only the named ones. At most `limit` rows.
pub fn csv_rows(text: &str, limit: usize) -> Vec<BTreeMap<String, String>> {
    let mut records = csv_records(text.strip_prefix('\u{feff}').unwrap_or(text), limit + 1);
    if records.is_empty() {
        return Vec::new();
    }
    let header: Vec<String> = records
        .remove(0)
        .into_iter()
        .map(|h| h.trim().to_string())
        .collect();
    records
        .into_iter()
        .map(|r| {
            header
                .iter()
                .cloned()
                .zip(r)
                .filter(|(h, _)| !h.is_empty())
                .collect()
        })
        .collect()
}

fn csv_records(text: &str, limit: usize) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut started = false;
    while let Some(c) = chars.next() {
        if out.len() >= limit {
            break;
        }
        started = true;
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                if !(record.len() == 1 && record[0].is_empty()) {
                    out.push(std::mem::take(&mut record));
                }
                record.clear();
                started = false;
            }
            c => field.push(c),
        }
    }
    if started && out.len() < limit {
        record.push(field);
        if !(record.len() == 1 && record[0].is_empty()) {
            out.push(record);
        }
    }
    out
}

/// A catalogue value as one bounded line of text.
fn clean(value: &str) -> String {
    let s: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(2_000)
        .collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ------------------------------------------------------------------------------- catalogue

/// The kind of a work, as COL's catalogue names it.
pub const WORK_TYPES: [&str; 6] = [
    "book",
    "commentary",
    "confession",
    "creed",
    "catechism",
    "hymnal",
];

/// One work COL offers: a book of the tree, with what the catalogue says of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColWork {
    /// Its main file's path (or its first Part's, when it has no main file).
    pub path: String,
    pub main: Option<String>,
    pub parts: Vec<String>,
    pub author: String,
    pub title: String,
    pub year: Option<u16>,
    /// `book`, `commentary`, `confession`, `creed`, `catechism`, `hymnal`, or as COL writes it.
    pub kind: String,
    /// The collection it came from: the repository's top folder (`EEBO-TCP`, …).
    pub collection: String,
    /// The author's tradition, from `authors.csv` (Puritan, Reformed, …).
    pub tradition: String,
    pub license: String,
    pub source: String,
    /// Bytes of all its files.
    pub size: u64,
}

impl ColWork {
    /// Every file of the work, the main file first.
    pub fn files(&self) -> impl Iterator<Item = &String> {
        self.main.iter().chain(self.parts.iter())
    }

    /// Its Resource id (ADR 0024): `col-`, the slug of its file name, and 6 hex digits of the
    /// sha256 of its path stem, at most 64 characters. The same on every computer.
    pub fn resource_id(&self) -> String {
        col_resource_id(&self.path)
    }
}

/// The Resource id of the COL book whose main (or first Part) file is `path` (ADR 0024).
pub fn col_resource_id(path: &str) -> String {
    use sha2::{Digest, Sha256};
    let (folder, file) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = obs_formats::book_stem(file);
    let digest = Sha256::digest(format!("{folder}/{stem}").as_bytes());
    let hash: String = digest.iter().take(3).map(|b| format!("{b:02x}")).collect();
    let slug = obs_formats::slugify(stem);
    let slug: String = slug.chars().take(64 - "col--".len() - hash.len()).collect();
    format!("col-{}-{hash}", slug.trim_end_matches('-'))
}

/// A year (100 to 2100) in a catalogue value.
fn year(value: &str) -> Option<u16> {
    crate::cited_in::year_of(Some(value))
}

/// The works of a tree, described by its catalogue (`catalog_full.csv`) and authors
/// (`authors.csv`) where they name them; a book the catalogue does not list is still offered,
/// titled by its file name. In path order.
pub fn works(tree: &ColTree, catalog: Option<&str>, authors: Option<&str>) -> Vec<ColWork> {
    let rows: BTreeMap<String, BTreeMap<String, String>> = catalog
        .map(|c| csv_rows(c, 200_000))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| Some((r.get("File")?.trim().to_string(), r)))
        .collect();
    let traditions: BTreeMap<String, String> = authors
        .map(|a| csv_rows(a, 100_000))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| {
            let name = clean(r.get("Author")?);
            let tradition = clean(r.get("Tradition")?);
            (!name.is_empty() && !tradition.is_empty()).then_some((name, tradition))
        })
        .collect();
    let mut out = Vec::new();
    for book in tree.books() {
        let Some(path) = book.files().next().cloned() else {
            continue;
        };
        let row = book.main.as_ref().and_then(|m| rows.get(m));
        let get = |k: &str| {
            row.and_then(|r| r.get(k))
                .map(|v| clean(v))
                .unwrap_or_default()
        };
        let file = path.rsplit('/').next().unwrap_or(&path);
        let title = Some(get("Title"))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| obs_formats::book_stem(file).to_string());
        let author = get("Author");
        let size = book
            .files()
            .filter_map(|f| tree.files.get(f))
            .map(|f| f.size)
            .sum();
        let kind = Some(get("Type").to_lowercase())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "book".into());
        out.push(ColWork {
            collection: path.split('/').next().unwrap_or("").to_string(),
            tradition: traditions.get(&author).cloned().unwrap_or_default(),
            year: year(&get("Year")),
            license: get("License"),
            source: get("Source"),
            main: book.main.clone(),
            parts: book.parts.clone(),
            path,
            author,
            title,
            kind,
            size,
        });
    }
    out
}

/// A row of COL's public catalogue (`catalog.csv`), readable before connecting: what is there,
/// without its file paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicWork {
    pub author: String,
    pub title: String,
    pub year: Option<u16>,
    pub kind: String,
    pub folder: String,
}

/// The rows of COL's public catalogue.
pub fn public_works(csv: &str) -> Vec<PublicWork> {
    csv_rows(csv, 200_000)
        .into_iter()
        .map(|r| {
            let get = |k: &str| r.get(k).map(|v| clean(v)).unwrap_or_default();
            PublicWork {
                author: get("Author"),
                title: get("Title"),
                year: year(&get("Year")),
                kind: Some(get("Type").to_lowercase())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| "book".into()),
                folder: get("Folder"),
            }
        })
        .filter(|w| !w.title.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------------- filters

/// What the Books list shows: every field empty or `None` shows every work.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColFilter {
    /// Words that must each start a word of the title or the author (old spellings aside:
    /// case, accents, ſ/s, u/v and i/j are ignored).
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub tradition: Option<String>,
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    pub year_from: Option<u16>,
    #[serde(default)]
    pub year_to: Option<u16>,
}

/// Text folded for matching old spellings: lowercase, no accents, ſ → s, v → u, j → i.
pub fn fold(text: &str) -> String {
    obs_model::normalize(text)
        .chars()
        .map(|c| match c {
            'ſ' => 's',
            'v' => 'u',
            'j' => 'i',
            c => c,
        })
        .collect()
}

impl ColFilter {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether `work` passes. Works without a year pass a year bound only when none is set.
    pub fn accepts(&self, work: &ColWork) -> bool {
        let eq = |want: &Option<String>, have: &str| {
            want.as_ref()
                .is_none_or(|w| w.trim().is_empty() || w.eq_ignore_ascii_case(have))
        };
        if !eq(&self.kind, &work.kind)
            || !eq(&self.author, &work.author)
            || !eq(&self.tradition, &work.tradition)
            || !eq(&self.collection, &work.collection)
        {
            return false;
        }
        if self.year_from.is_some() || self.year_to.is_some() {
            let Some(y) = work.year else {
                return false;
            };
            if self.year_from.is_some_and(|f| y < f) || self.year_to.is_some_and(|t| y > t) {
                return false;
            }
        }
        let words: Vec<String> = fold(&self.text)
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect();
        if words.is_empty() {
            return true;
        }
        let hay = fold(&format!("{} {}", work.title, work.author));
        let hay: Vec<&str> = hay
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        words
            .iter()
            .all(|w| hay.iter().any(|h| h.starts_with(w.as_str())))
    }
}

/// The distinct values of a field across `works`, sorted, with how many works have each.
pub fn facet(works: &[ColWork], field: impl Fn(&ColWork) -> &str) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for w in works {
        let v = field(w);
        if !v.is_empty() {
            *counts.entry(v.to_string()).or_default() += 1;
        }
    }
    counts.into_iter().collect()
}

// ---------------------------------------------------------------------------------- origin

/// Where a COL Book came from (ADR 0024), kept on its Import record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColOrigin {
    /// `github` or `local`.
    pub via: String,
    /// The book's main (or first Part) file's path in the repository.
    pub path: String,
    /// The commit it was downloaded at (empty for a local clone).
    #[serde(default)]
    pub commit: String,
    /// Each of its files' git blob SHA.
    pub blobs: BTreeMap<String, String>,
}

/// What Check for new books found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColChanges {
    /// Works not in the Library.
    pub new: Vec<String>,
    /// Library Resources whose work's files changed: (resource id, work path).
    pub changed: Vec<(String, String)>,
    /// Library Resources whose work is no longer in COL.
    pub removed: Vec<String>,
    /// Library Resources up to date.
    pub current: usize,
}

/// Compare the Library's COL Books (resource id → origin) with the works of a tree.
pub fn compare(
    installed: &BTreeMap<String, ColOrigin>,
    tree: &ColTree,
    works: &[ColWork],
) -> ColChanges {
    let mut changes = ColChanges::default();
    let by_path: BTreeMap<&str, &ColWork> = works.iter().map(|w| (w.path.as_str(), w)).collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (id, origin) in installed {
        let Some(work) = by_path.get(origin.path.as_str()) else {
            changes.removed.push(id.clone());
            continue;
        };
        seen.insert(work.path.as_str());
        let now: BTreeMap<String, String> = work
            .files()
            .filter_map(|f| Some((f.clone(), tree.files.get(f)?.sha.clone())))
            .collect();
        if now == origin.blobs {
            changes.current += 1;
        } else {
            changes.changed.push((id.clone(), work.path.clone()));
        }
    }
    changes.new = works
        .iter()
        .filter(|w| !seen.contains(w.path.as_str()))
        .map(|w| w.path.clone())
        .collect();
    changes
}

#[cfg(test)]
#[path = "col_tests.rs"]
mod tests;
