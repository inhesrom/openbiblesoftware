//! Get Resources → Books: the Christian Open Library's books downloaded into the Library and
//! kept up to date (ADR 0024, #347).
//!
//! The user connects with a GitHub token (kept in the OS credential store, [`SecretStore`]) or
//! a local clone. Connecting reads the repository's file list and catalogue on a job; Download
//! hands the chosen works to a book batch ([`crate::books`]) whose [`BookStream`] fetches each
//! book's files by blob SHA, or streams the repository's tarball for many; Check for new books
//! reads the list again and compares blob SHAs with each COL Book's [`ColOrigin`].

#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use obs_formats::BookFile;
use obs_formats::sword_repo::{Fetch, FetchError};
use serde_json::{Value, json};

use crate::books::{BookFailure, BookStream, LoadedBook};
use crate::col::{
    AUTHORS_FILE, CATALOG_FILE, COL_OWNER, COL_REPO, COL_WEBSITE, ColChanges, ColFilter, ColOrigin,
    ColTree, ColWork, GITHUB_API, PUBLIC_CATALOG_URL, PublicWork, TreeError, git_blob_sha,
    public_works, wanted_file,
};
use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params, always};
use crate::jobs::{JobContext, JobEvent, JobEventKind, JobId};
use crate::session::Session;

/// More files than this are downloaded as one tarball rather than file by file (ADR 0024).
pub const TARBALL_FILES: usize = 400;

// ---------------------------------------------------------------------------------- secrets

/// Where the user's GitHub token is kept: the OS credential store in the app (ADR 0024). The
/// engine never writes it anywhere else.
pub trait SecretStore: fmt::Debug + Send + Sync {
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, secret: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
    /// Whether what is set outlives this run of the app (false: kept for this session only).
    fn lasting(&self) -> bool {
        true
    }
}

/// A token kept in memory for this run only: the default, and where no credential store
/// answers.
#[derive(Default)]
pub struct SessionSecret(Mutex<Option<String>>);

impl fmt::Debug for SessionSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SessionSecret(…)")
    }
}

impl SecretStore for SessionSecret {
    fn get(&self) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap_or_else(|p| p.into_inner()).clone())
    }
    fn set(&self, secret: &str) -> Result<(), String> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = Some(secret.to_string());
        Ok(())
    }
    fn delete(&self) -> Result<(), String> {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = None;
        Ok(())
    }
    fn lasting(&self) -> bool {
        false
    }
}

/// A GitHub token. Never printed: its `Debug` shows nothing of it.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(Arc<str>);

impl Token {
    /// A pasted token, trimmed; `None` when empty or holding anything a token never does.
    pub fn new(text: &str) -> Option<Self> {
        let t = text.trim();
        let ok = !t.is_empty()
            && t.len() <= 255
            && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        ok.then(|| Self(Arc::from(t)))
    }

    fn header(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(…)")
    }
}

// ----------------------------------------------------------------------------------- access

/// How the app reads COL's repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColAccess {
    GitHub { token: Token },
    Local { root: PathBuf },
}

impl ColAccess {
    pub fn via(&self) -> &'static str {
        match self {
            ColAccess::GitHub { .. } => "github",
            ColAccess::Local { .. } => "local",
        }
    }
}

/// Why COL could not be read, in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColError {
    /// The account cannot see the repository (404): not invited, or not accepted yet.
    NoAccess,
    /// GitHub refused the token (401).
    BadToken,
    RateLimited,
    Offline(String),
    /// A local clone that is not COL's books (no Markdown, no catalogue).
    NotAClone(String),
    Other(String),
}

impl ColError {
    pub fn message(&self) -> String {
        match self {
            ColError::NoAccess => "This GitHub account cannot see the Christian Open Library's \
                                   books yet. Ask COL for access, accept the invitation GitHub \
                                   emails you, then try again."
                .into(),
            ColError::BadToken => "GitHub did not accept this token. It may have expired or been \
                                   revoked: paste a new one."
                .into(),
            ColError::RateLimited => "GitHub is limiting requests from this account for now. Try \
                                      again later; books already downloaded stay in your Library."
                .into(),
            ColError::Offline(_) => {
                "Could not reach GitHub. Check the connection, then try again.".into()
            }
            ColError::NotAClone(path) => format!(
                "{path} does not look like a copy of the Christian Open Library's books (no \
                 Markdown books found)."
            ),
            ColError::Other(e) => format!("The Christian Open Library could not be read: {e}"),
        }
    }

    fn from_fetch(e: FetchError) -> Self {
        match e {
            FetchError::NotFound => ColError::NoAccess,
            FetchError::Unauthorized => ColError::BadToken,
            FetchError::RateLimited => ColError::RateLimited,
            FetchError::Cancelled => ColError::Other("cancelled".into()),
            FetchError::Network(m) => ColError::Offline(m),
        }
    }
}

impl fmt::Display for ColError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

/// The GitHub REST API at `api` (GitHub's own, or a mock in tests) for one repository.
#[derive(Debug, Clone)]
pub struct GitHubRepo {
    pub api: String,
    pub owner: String,
    pub repo: String,
}

impl Default for GitHubRepo {
    fn default() -> Self {
        Self {
            api: GITHUB_API.into(),
            owner: COL_OWNER.into(),
            repo: COL_REPO.into(),
        }
    }
}

impl GitHubRepo {
    fn url(&self, path: &str) -> String {
        format!(
            "{}/repos/{}/{}{path}",
            self.api.trim_end_matches('/'),
            self.owner,
            self.repo
        )
    }

    fn get(
        &self,
        fetch: &dyn Fetch,
        token: &Token,
        path: &str,
        accept: &str,
        ctx: Option<&JobContext>,
    ) -> Result<Vec<u8>, ColError> {
        let auth = token.header();
        let headers = [
            ("Authorization", auth.as_str()),
            ("Accept", accept),
            ("X-GitHub-Api-Version", "2022-11-28"),
        ];
        fetch
            .get_with(&self.url(path), &headers, &mut |_, _| {
                ctx.is_none_or(|c| !c.is_cancelled())
            })
            .map_err(ColError::from_fetch)
    }

    /// The default branch's latest commit.
    fn head(&self, fetch: &dyn Fetch, token: &Token) -> Result<String, ColError> {
        let repo = self.get(fetch, token, "", "application/vnd.github+json", None)?;
        let repo: Value =
            serde_json::from_slice(&repo).map_err(|e| ColError::Other(e.to_string()))?;
        let branch = repo
            .get("default_branch")
            .and_then(Value::as_str)
            .filter(|b| crate::col::safe_path(b))
            .ok_or_else(|| ColError::Other("no default branch".into()))?;
        let sha = self.get(
            fetch,
            token,
            &format!("/commits/{branch}"),
            "application/vnd.github.sha",
            None,
        )?;
        let sha = String::from_utf8_lossy(&sha).trim().to_string();
        if !crate::col::is_sha(&sha) {
            return Err(ColError::Other("no commit".into()));
        }
        Ok(sha)
    }

    fn tree(&self, fetch: &dyn Fetch, token: &Token, commit: &str) -> Result<ColTree, ColError> {
        let json = self.get(
            fetch,
            token,
            &format!("/git/trees/{commit}?recursive=1"),
            "application/vnd.github+json",
            None,
        )?;
        ColTree::from_github(commit, &json).map_err(|e| match e {
            TreeError::Truncated | TreeError::Invalid(_) => ColError::Other(e.to_string()),
        })
    }

    /// A blob's bytes, checked against its SHA.
    fn blob(
        &self,
        fetch: &dyn Fetch,
        token: &Token,
        sha: &str,
        ctx: Option<&JobContext>,
    ) -> Result<Vec<u8>, ColError> {
        let bytes = self.get(
            fetch,
            token,
            &format!("/git/blobs/{sha}"),
            "application/vnd.github.raw+json",
            ctx,
        )?;
        if git_blob_sha(&bytes) != sha {
            return Err(ColError::Other("a file did not match its checksum".into()));
        }
        Ok(bytes)
    }
}

// -------------------------------------------------------------------------------- catalogue

/// What connecting read: the repository's files and its works.
#[derive(Debug, Clone, Default)]
pub struct ColCatalog {
    pub via: String,
    pub tree: ColTree,
    pub works: Vec<ColWork>,
}

/// Read the repository: its file list, then its catalogue and authors files.
fn load_github(
    fetch: &dyn Fetch,
    repo: &GitHubRepo,
    token: &Token,
    ctx: &JobContext,
) -> Result<ColCatalog, ColError> {
    ctx.phase("Connect");
    let commit = repo.head(fetch, token)?;
    let tree = repo.tree(fetch, token, &commit)?;
    let text = |name: &str| -> Result<Option<String>, ColError> {
        match tree.files.get(name) {
            Some(f) => Ok(Some(
                String::from_utf8_lossy(&repo.blob(fetch, token, &f.sha, Some(ctx))?).into_owned(),
            )),
            None => Ok(None),
        }
    };
    let catalog = text(CATALOG_FILE)?;
    let authors = text(AUTHORS_FILE)?;
    let works = crate::col::works(&tree, catalog.as_deref(), authors.as_deref());
    Ok(ColCatalog {
        via: "github".into(),
        tree,
        works,
    })
}

/// Read a local clone: every wanted file under it, hashed as git does.
#[cfg(not(target_arch = "wasm32"))]
fn load_local(root: &Path, ctx: &JobContext) -> Result<ColCatalog, ColError> {
    ctx.phase("Connect");
    let names = crate::books::markdown_files(root)
        .map_err(|e| ColError::Other(format!("{}: {e}", root.display())))?;
    let mut tree = ColTree::default();
    let total = names.len() as u64;
    for (i, name) in names
        .iter()
        .chain([CATALOG_FILE.to_string(), AUTHORS_FILE.to_string()].iter())
        .enumerate()
    {
        if ctx.is_cancelled() {
            return Err(ColError::Other("cancelled".into()));
        }
        if !wanted_file(name) {
            continue;
        }
        let Ok(bytes) = std::fs::read(root.join(name)) else {
            continue;
        };
        if bytes.len() as u64 > crate::col::MAX_FILE_BYTES {
            tree.refused += 1;
            continue;
        }
        tree.files.insert(
            name.clone(),
            crate::col::TreeFile {
                sha: git_blob_sha(&bytes),
                size: bytes.len() as u64,
            },
        );
        ctx.progress(i as u64 + 1, total);
    }
    if !tree.files.keys().any(|p| obs_formats::is_markdown_name(p)) {
        return Err(ColError::NotAClone(root.display().to_string()));
    }
    let read = |name: &str| std::fs::read_to_string(root.join(name)).ok();
    let works = crate::col::works(
        &tree,
        read(CATALOG_FILE).as_deref(),
        read(AUTHORS_FILE).as_deref(),
    );
    Ok(ColCatalog {
        via: "local".into(),
        tree,
        works,
    })
}

// ---------------------------------------------------------------------------------- streams

/// The attribution a COL Book's About view adds to its own (ADR 0024).
fn col_note(path: &str) -> String {
    format!("From the Christian Open Library ({COL_WEBSITE}), its copy {path}.")
}

fn text_of(name: &str, bytes: Vec<u8>) -> Result<BookFile, String> {
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{name}: not UTF-8 text (a Markdown book must be UTF-8 text)"))?;
    Ok(BookFile {
        name: name.to_string(),
        text,
    })
}

/// Books fetched file by file, by blob SHA, from GitHub.
pub(crate) struct BlobStream {
    pub fetch: Arc<dyn Fetch>,
    pub repo: GitHubRepo,
    pub token: Token,
    pub commit: String,
    pub works: VecDeque<(ColWork, BTreeMap<String, String>)>,
    pub total: usize,
}

impl BookStream for BlobStream {
    fn total(&self) -> Option<usize> {
        Some(self.total)
    }

    fn next_book(
        &mut self,
        ctx: &JobContext,
    ) -> Result<Option<Result<LoadedBook, BookFailure>>, String> {
        let Some((work, blobs)) = self.works.pop_front() else {
            return Ok(None);
        };
        let mut files = Vec::new();
        for path in work.files() {
            let Some(sha) = blobs.get(path) else {
                return Ok(Some(Err(BookFailure {
                    name: work.path.clone(),
                    error: format!("{path} is no longer in the Christian Open Library"),
                })));
            };
            match self
                .repo
                .blob(self.fetch.as_ref(), &self.token, sha, Some(ctx))
            {
                Ok(bytes) => match text_of(path, bytes) {
                    Ok(f) => files.push(f),
                    Err(error) => {
                        return Ok(Some(Err(BookFailure {
                            name: work.path.clone(),
                            error,
                        })));
                    }
                },
                // Gone since the list was read: this book only.
                Err(ColError::NoAccess) => {
                    return Ok(Some(Err(BookFailure {
                        name: work.path.clone(),
                        error: format!("{path} is no longer in the Christian Open Library"),
                    })));
                }
                Err(ColError::Other(e)) if e.contains("checksum") => {
                    return Ok(Some(Err(BookFailure {
                        name: work.path.clone(),
                        error: format!("{path} did not match its checksum"),
                    })));
                }
                // Stops the batch: the token, the connection or the rate limit.
                Err(e) => return Err(e.message()),
            }
        }
        Ok(Some(Ok(loaded(
            &work,
            files,
            blobs,
            "github",
            &self.commit,
        ))))
    }
}

/// A COL work's files read, as the batch imports it.
fn loaded(
    work: &ColWork,
    mut files: Vec<BookFile>,
    blobs: BTreeMap<String, String>,
    via: &str,
    commit: &str,
) -> LoadedBook {
    let main = work
        .main
        .as_ref()
        .and_then(|m| files.iter().position(|f| &f.name == m))
        .map(|i| files.remove(i));
    LoadedBook {
        name: work.path.clone(),
        main,
        parts: files,
        resource_id: Some(work.resource_id()),
        note: Some(col_note(&work.path)),
        col: Some(ColOrigin {
            via: via.into(),
            path: work.path.clone(),
            commit: commit.into(),
            blobs,
        }),
    }
}

/// Books read from a local clone.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct LocalStream {
    pub root: PathBuf,
    pub works: VecDeque<ColWork>,
    pub total: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl BookStream for LocalStream {
    fn total(&self) -> Option<usize> {
        Some(self.total)
    }

    fn next_book(
        &mut self,
        _: &JobContext,
    ) -> Result<Option<Result<LoadedBook, BookFailure>>, String> {
        let Some(work) = self.works.pop_front() else {
            return Ok(None);
        };
        let mut files = Vec::new();
        let mut blobs = BTreeMap::new();
        for path in work.files() {
            let read = std::fs::read(self.root.join(path)).map_err(|e| e.to_string());
            match read.and_then(|bytes| {
                blobs.insert(path.clone(), git_blob_sha(&bytes));
                text_of(path, bytes)
            }) {
                Ok(f) => files.push(f),
                Err(error) => {
                    return Ok(Some(Err(BookFailure {
                        name: work.path.clone(),
                        error,
                    })));
                }
            }
        }
        Ok(Some(Ok(loaded(&work, files, blobs, "local", ""))))
    }
}

/// Books read from the repository's tarball as it streams in (ADR 0024): a reader thread
/// unpacks it entry by entry and hands each book over as soon as all its files have passed,
/// through a short channel, so the archive is never held or unpacked whole.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct TarStream {
    rx: std::sync::mpsc::Receiver<Result<Result<LoadedBook, BookFailure>, String>>,
    total: usize,
}

#[cfg(not(target_arch = "wasm32"))]
impl TarStream {
    /// Start reading the tarball of `commit`; `works` are the works to take from it (every
    /// other entry is skipped), with the blob SHA the tree gave each file.
    pub fn start(
        fetch: Arc<dyn Fetch>,
        repo: GitHubRepo,
        token: Token,
        commit: String,
        works: Vec<(ColWork, BTreeMap<String, String>)>,
    ) -> Self {
        let total = works.len();
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        let spawned = std::thread::Builder::new()
            .name("obs-col-tarball".into())
            .spawn(move || {
                let result = read_tarball(fetch.as_ref(), &repo, &token, &commit, works, &tx);
                if let Err(e) = result {
                    let _ = tx.send(Err(e));
                }
            });
        if let Err(e) = spawned {
            let (tx, rx2) = std::sync::mpsc::sync_channel(1);
            let _ = tx.send(Err(format!("could not start the download: {e}")));
            return Self { rx: rx2, total };
        }
        Self { rx, total }
    }
}

#[cfg(not(target_arch = "wasm32"))]
type BookSender = std::sync::mpsc::SyncSender<Result<Result<LoadedBook, BookFailure>, String>>;

/// The tarball reader: entries in path order (git's tree order is byte order of the full
/// paths), so a book's Part files (`Title — Part N.md`, `Title, Part N.md`) all come before
/// its main file (`Title.md`), and a book is complete once an entry after its main file's
/// path has passed.
#[cfg(not(target_arch = "wasm32"))]
fn read_tarball(
    fetch: &dyn Fetch,
    repo: &GitHubRepo,
    token: &Token,
    commit: &str,
    works: Vec<(ColWork, BTreeMap<String, String>)>,
    tx: &BookSender,
) -> Result<(), String> {
    use std::io::Read;
    // Each wanted file → its work's index.
    let mut of_file: HashMap<String, usize> = HashMap::new();
    for (i, (work, _)) in works.iter().enumerate() {
        for f in work.files() {
            of_file.insert(f.clone(), i);
        }
    }
    // Each work's files read so far; the works still waiting, by the path their main file
    // has (or would have).
    let mut got: Vec<Vec<BookFile>> = vec![Vec::new(); works.len()];
    let mut failed: Vec<Option<String>> = vec![None; works.len()];
    let mut done: Vec<bool> = vec![false; works.len()];
    let mut waiting: std::collections::BTreeMap<String, usize> = works
        .iter()
        .enumerate()
        .map(|(i, (w, _))| (main_path(w), i))
        .collect();
    let send = |i: usize,
                got: &mut Vec<Vec<BookFile>>,
                failed: &mut Vec<Option<String>>,
                done: &mut Vec<bool>|
     -> bool {
        if done[i] {
            return true;
        }
        done[i] = true;
        let (work, blobs) = &works[i];
        let book = match failed[i].take() {
            Some(error) => Err(BookFailure {
                name: work.path.clone(),
                error,
            }),
            None if got[i].len() < work.files().count() => Err(BookFailure {
                name: work.path.clone(),
                error: "some of its files were not in the download".into(),
            }),
            None => Ok(loaded(
                work,
                std::mem::take(&mut got[i]),
                blobs.clone(),
                "github",
                commit,
            )),
        };
        tx.send(Ok(book)).is_ok()
    };
    let auth = token.header();
    let headers = [
        ("Authorization", auth.as_str()),
        ("Accept", "application/vnd.github+json"),
        ("X-GitHub-Api-Version", "2022-11-28"),
    ];
    let body = fetch
        .open(&repo.url(&format!("/tarball/{commit}")), &headers)
        .map_err(|e| ColError::from_fetch(e).message())?;
    let gz = flate2::read::GzDecoder::new(body);
    let mut archive = tar::Archive::new(gz);
    let entries = archive
        .entries()
        .map_err(|e| format!("the download: {e}"))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("the download: {e}"))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let raw = entry
            .path()
            .map_err(|e| format!("the download: {e}"))?
            .to_string_lossy()
            .into_owned();
        // `owner-repo-sha/` comes before every path.
        let Some((_, path)) = raw.split_once('/') else {
            continue;
        };
        // Every waiting book whose main file's path is before this one is complete.
        let complete: Vec<String> = waiting
            .range(..path.to_string())
            .map(|(k, _)| k.clone())
            .collect();
        for k in complete {
            if let Some(i) = waiting.remove(&k)
                && !send(i, &mut got, &mut failed, &mut done)
            {
                return Ok(()); // the batch stopped
            }
        }
        let Some(&i) = of_file.get(path) else {
            continue;
        };
        if entry.size() > crate::col::MAX_FILE_BYTES {
            failed[i] = Some(format!("{path} is too large to import"));
            continue;
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("the download: {e}"))?;
        if works[i]
            .1
            .get(path)
            .is_some_and(|sha| *sha != git_blob_sha(&bytes))
        {
            failed[i] = Some(format!("{path} did not match its checksum"));
            continue;
        }
        match text_of(path, bytes) {
            Ok(f) => got[i].push(f),
            Err(e) => failed[i] = Some(e),
        }
    }
    for i in std::mem::take(&mut waiting).into_values() {
        if !send(i, &mut got, &mut failed, &mut done) {
            return Ok(());
        }
    }
    Ok(())
}

/// The path a work's main file has, or would have when it has only Parts.
fn main_path(work: &ColWork) -> String {
    work.main.clone().unwrap_or_else(|| {
        let (folder, file) = work.path.rsplit_once('/').unwrap_or(("", &work.path));
        let stem = obs_formats::book_stem(file);
        if folder.is_empty() {
            format!("{stem}.md")
        } else {
            format!("{folder}/{stem}.md")
        }
    })
}

#[cfg(not(target_arch = "wasm32"))]
impl BookStream for TarStream {
    fn total(&self) -> Option<usize> {
        Some(self.total)
    }

    fn next_book(
        &mut self,
        ctx: &JobContext,
    ) -> Result<Option<Result<LoadedBook, BookFailure>>, String> {
        loop {
            if ctx.is_cancelled() {
                return Err("Import cancelled".into());
            }
            match self.rx.recv_timeout(std::time::Duration::from_millis(200)) {
                Ok(Ok(book)) => return Ok(Some(book)),
                Ok(Err(e)) => return Err(e),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(None),
            }
        }
    }
}

// ------------------------------------------------------------------------------------ state

/// Get Resources → Books' state.
#[derive(Debug)]
pub struct ColState {
    pub(crate) access: Option<ColAccess>,
    secrets: Arc<dyn SecretStore>,
    pub(crate) repo: GitHubRepo,
    /// The connect or refresh job running.
    loading: Option<JobId>,
    /// The public catalogue job running.
    public_job: Option<JobId>,
    catalog: Option<Arc<ColCatalog>>,
    public: Option<Arc<Vec<PublicWork>>>,
    error: Option<ColError>,
    pub filter: ColFilter,
    /// Works chosen for Download, by path.
    pub selected: BTreeSet<String>,
    changes: Option<ColChanges>,
    /// The book batch downloading or updating, by its report id.
    batch: Option<String>,
    /// Tried the credential store for a saved token this run.
    restored: bool,
    /// Settings → Search → Search Christian Open Library (#343), once read.
    search_link: Option<bool>,
}

impl Default for ColState {
    fn default() -> Self {
        Self {
            access: None,
            secrets: Arc::new(SessionSecret::default()),
            repo: GitHubRepo::default(),
            loading: None,
            public_job: None,
            catalog: None,
            public: None,
            error: None,
            filter: ColFilter::default(),
            selected: BTreeSet::new(),
            changes: None,
            batch: None,
            restored: false,
            search_link: None,
        }
    }
}

/// What the Books list shows about a work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkStatus {
    NotInLibrary,
    InLibrary,
    /// In the Library, and COL's copy changed since.
    Changed,
}

impl ColState {
    /// Where the token is kept (the app installs the OS credential store).
    pub fn set_secrets(&mut self, secrets: Arc<dyn SecretStore>) {
        self.secrets = secrets;
        self.restored = false;
    }

    /// The API to read (tests name a mock).
    pub fn set_api(&mut self, api: impl Into<String>) {
        self.repo.api = api.into();
    }

    /// Read the preferences again when next asked (a restore replaced the User database). A
    /// token stays in the keychain; a local copy is named by the database, so it is forgotten.
    pub(crate) fn forget_preferences(&mut self) {
        self.search_link = None;
        self.restored = false;
        if matches!(self.access, Some(ColAccess::Local { .. })) {
            self.access = None;
            self.catalog = None;
            self.changes = None;
            self.selected.clear();
        }
    }

    pub fn access(&self) -> Option<&ColAccess> {
        self.access.as_ref()
    }

    /// Whether the token is kept only for this run (no credential store answered).
    pub fn token_for_session_only(&self) -> bool {
        matches!(self.access, Some(ColAccess::GitHub { .. })) && !self.secrets.lasting()
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    pub fn is_loading_public(&self) -> bool {
        self.public_job.is_some()
    }

    pub fn catalog(&self) -> Option<&Arc<ColCatalog>> {
        self.catalog.as_ref()
    }

    pub fn public(&self) -> Option<&Arc<Vec<PublicWork>>> {
        self.public.as_ref()
    }

    pub fn error(&self) -> Option<&ColError> {
        self.error.as_ref()
    }

    pub fn changes(&self) -> Option<&ColChanges> {
        self.changes.as_ref()
    }

    /// The batch downloading now (its Import report id).
    pub fn downloading(&self) -> Option<&str> {
        self.batch.as_deref()
    }

    /// The works the filter shows, in path order.
    pub fn shown(&self) -> Vec<&ColWork> {
        self.catalog
            .as_ref()
            .map(|c| c.works.iter().filter(|w| self.filter.accepts(w)).collect())
            .unwrap_or_default()
    }

    /// The works chosen, in path order.
    pub fn chosen(&self) -> Vec<&ColWork> {
        self.catalog
            .as_ref()
            .map(|c| {
                c.works
                    .iter()
                    .filter(|w| self.selected.contains(&w.path))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// How many works and how many bytes.
pub fn size_of(works: &[&ColWork]) -> (usize, u64) {
    (works.len(), works.iter().map(|w| w.size).sum())
}

impl Session {
    /// The COL Books in the Library, with where each came from.
    pub fn col_books(&self) -> BTreeMap<String, ColOrigin> {
        self.imports
            .records()
            .filter(|r| self.library.get(&r.resource_id).is_some())
            .filter_map(|r| Some((r.resource_id.clone(), r.col.clone()?)))
            .collect()
    }

    /// Whether a work is in the Library, and changed in COL since.
    pub fn col_status(&self, work: &ColWork) -> WorkStatus {
        let id = work.resource_id();
        if self.library.get(&id).is_none() {
            return WorkStatus::NotInLibrary;
        }
        let changed = self
            .col
            .changes
            .as_ref()
            .is_some_and(|c| c.changed.iter().any(|(r, _)| *r == id));
        if changed {
            WorkStatus::Changed
        } else {
            WorkStatus::InLibrary
        }
    }

    /// Reconnect with the token the credential store kept, once per run, when the Books list
    /// is first shown. Reads nothing from the network until then (ADR 0024).
    pub fn col_restore(&mut self) {
        if self.col.restored || self.col.access.is_some() {
            return;
        }
        self.col.restored = true;
        let local = self
            .user
            .as_ref()
            .and_then(|u| u.preference(LOCAL_PREF).ok().flatten())
            .filter(|p| !p.is_empty());
        if let Some(path) = local {
            self.col.access = Some(ColAccess::Local {
                root: PathBuf::from(path),
            });
            return;
        }
        if let Ok(Some(token)) = self.col.secrets.get()
            && let Some(token) = Token::new(&token)
        {
            self.col.access = Some(ColAccess::GitHub { token });
        }
    }

    /// Read the repository again (Connect, Check for new books).
    #[cfg(not(target_arch = "wasm32"))]
    fn col_load(&mut self) -> Result<(), String> {
        let access = self.col.access.clone().ok_or("Not connected")?;
        if let Some(job) = self.col.loading.take() {
            self.jobs().cancel(job);
        }
        self.col.error = None;
        let fetch = self.fetch();
        let repo = self.col.repo.clone();
        let job = self.jobs().submit_fn("Christian Open Library", move |ctx| {
            match &access {
                ColAccess::GitHub { token } => load_github(fetch.as_ref(), &repo, token, ctx),
                ColAccess::Local { root } => load_local(root, ctx),
            }
            .map_err(|e| serde_json::to_string(&ErrorCode::of(&e)).unwrap_or_default())
        });
        self.col.loading = Some(job);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn col_load_public(&mut self) {
        if self.col.public_job.is_some() {
            return;
        }
        let fetch = self.fetch();
        let job = self
            .jobs()
            .submit_fn("Christian Open Library catalogue", move |ctx| {
                let bytes = fetch
                    .get(PUBLIC_CATALOG_URL, &mut |_, _| !ctx.is_cancelled())
                    .map_err(|e| ColError::from_fetch(e).message())?;
                Ok(public_works(&String::from_utf8_lossy(&bytes)))
            });
        self.col.public_job = Some(job);
    }

    /// Apply COL job events.
    pub(crate) fn poll_col(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let mut other = Vec::new();
        for event in events {
            if Some(event.id) == self.col.public_job {
                match event.kind {
                    JobEventKind::Finished(out) => {
                        self.col.public_job = None;
                        if let Ok(works) = out.downcast::<Vec<PublicWork>>() {
                            self.col.public = Some(Arc::new(*works));
                        }
                    }
                    JobEventKind::Failed(e) => {
                        self.col.public_job = None;
                        self.col.error = Some(ColError::Other(e));
                    }
                    JobEventKind::Cancelled => self.col.public_job = None,
                    _ => {}
                }
                continue;
            }
            if Some(event.id) != self.col.loading {
                other.push(event);
                continue;
            }
            match event.kind {
                JobEventKind::Finished(out) => {
                    self.col.loading = None;
                    if let Ok(catalog) = out.downcast::<ColCatalog>() {
                        let catalog = Arc::new(*catalog);
                        let installed = self.col_books();
                        self.col.changes = Some(crate::col::compare(
                            &installed,
                            &catalog.tree,
                            &catalog.works,
                        ));
                        let paths: BTreeSet<&String> =
                            catalog.works.iter().map(|w| &w.path).collect();
                        self.col.selected.retain(|p| paths.contains(p));
                        self.col.catalog = Some(catalog);
                    }
                }
                JobEventKind::Failed(e) => {
                    self.col.loading = None;
                    let error = ErrorCode::read(&e);
                    // A refused token is no use kept.
                    if error == ColError::BadToken {
                        let _ = self.col.secrets.delete();
                        self.col.access = None;
                    }
                    self.col.error = Some(error);
                }
                JobEventKind::Cancelled => self.col.loading = None,
                _ => {}
            }
        }
        // A batch that ended: the next list shows its books in the Library.
        if let Some(batch) = &self.col.batch
            && self.imports.job_of(batch).is_none()
        {
            self.col.batch = None;
            if let Some(c) = &self.col.catalog {
                let installed = self.col_books();
                self.col.changes = Some(crate::col::compare(&installed, &c.tree, &c.works));
            }
        }
        other
    }

    /// Download (or update) `works` as Books, in one book batch.
    #[cfg(not(target_arch = "wasm32"))]
    fn col_download(&mut self, works: Vec<ColWork>, everything: bool) -> Result<Value, String> {
        if works.is_empty() {
            return Err("Choose at least one book".into());
        }
        if let Some(batch) = &self.col.batch
            && self.imports.job_of(batch).is_some()
        {
            return Err("A download is already running".into());
        }
        let catalog = self.col.catalog.clone().ok_or("Connect first")?;
        let access = self.col.access.clone().ok_or("Connect first")?;
        let files: usize = works.iter().map(|w| w.files().count()).sum();
        let n = works.len();
        let with_blobs = |w: ColWork| {
            let blobs: BTreeMap<String, String> = w
                .files()
                .filter_map(|f| Some((f.clone(), catalog.tree.files.get(f)?.sha.clone())))
                .collect();
            (w, blobs)
        };
        let stream: Box<dyn BookStream> = match access {
            ColAccess::Local { root } => Box::new(LocalStream {
                root,
                total: n,
                works: works.into(),
            }),
            ColAccess::GitHub { token } if everything || files > TARBALL_FILES => {
                Box::new(TarStream::start(
                    self.fetch(),
                    self.col.repo.clone(),
                    token,
                    catalog.tree.commit.clone(),
                    works.into_iter().map(with_blobs).collect(),
                ))
            }
            ColAccess::GitHub { token } => Box::new(BlobStream {
                fetch: self.fetch(),
                repo: self.col.repo.clone(),
                token,
                commit: catalog.tree.commit.clone(),
                total: n,
                works: works.into_iter().map(with_blobs).collect(),
            }),
        };
        let (id, tab, job) = self.start_book_batch(
            stream,
            "the Christian Open Library".into(),
            "Christian Open Library",
        )?;
        self.col.batch = Some(id.clone());
        Ok(json!({ "id": id, "tab": tab, "job": job.0, "books": n }))
    }
}

/// A [`ColError`] carried through a job's error string.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "code", content = "detail")]
enum ErrorCode {
    NoAccess,
    BadToken,
    RateLimited,
    Offline(String),
    NotAClone(String),
    Other(String),
}

impl ErrorCode {
    fn of(e: &ColError) -> Self {
        match e.clone() {
            ColError::NoAccess => Self::NoAccess,
            ColError::BadToken => Self::BadToken,
            ColError::RateLimited => Self::RateLimited,
            ColError::Offline(m) => Self::Offline(m),
            ColError::NotAClone(p) => Self::NotAClone(p),
            ColError::Other(m) => Self::Other(m),
        }
    }

    fn read(s: &str) -> ColError {
        match serde_json::from_str::<Self>(s) {
            Ok(Self::NoAccess) => ColError::NoAccess,
            Ok(Self::BadToken) => ColError::BadToken,
            Ok(Self::RateLimited) => ColError::RateLimited,
            Ok(Self::Offline(m)) => ColError::Offline(m),
            Ok(Self::NotAClone(p)) => ColError::NotAClone(p),
            Ok(Self::Other(m)) => ColError::Other(m),
            Err(_) => ColError::Other(s.to_string()),
        }
    }
}

/// The User database preference naming a local clone (the path only; never a token).
const LOCAL_PREF: &str = "col/local-clone";
/// Settings → Search → Search Christian Open Library (#343): `off` hides it.
pub(crate) const SEARCH_LINK_PREF: &str = "col/search-link";

impl Session {
    /// Whether menus and the Passage Guide offer Search Christian Open Library (#343); on
    /// unless the user turned it off.
    pub fn col_search_link(&mut self) -> bool {
        if let Some(on) = self.col.search_link {
            return on;
        }
        let on = self
            .user
            .as_ref()
            .and_then(|u| u.preference(SEARCH_LINK_PREF).ok().flatten())
            .is_none_or(|v| v != "off");
        self.col.search_link = Some(on);
        on
    }
}

/// `col.search_website {text}`: open COL's website searching for `text` (#343).
fn search_website(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let url = crate::col::website_search_url(p.str("text").unwrap_or_default())
        .ok_or_else(|| failed("col.search_website", "Nothing to search for"))?;
    s.workspace.push_effect(crate::Effect::OpenUrl(url.clone()));
    Ok(json!({ "url": url }))
}

/// `col.search_link {on}`: show or hide Search Christian Open Library (#343).
fn search_link(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let on = p.bool("on").unwrap_or(!s.col_search_link());
    s.col.search_link = Some(on);
    if let Some(user) = &s.user {
        user.set_preference(SEARCH_LINK_PREF, if on { "on" } else { "off" })
            .map_err(|e| failed("col.search_link", format!("saving: {e}")))?;
    }
    Ok(json!({ "on": on }))
}

const TEXT: ParamSpec = ParamSpec {
    name: "text",
    kind: ParamKind::String,
    required: true,
    help: "A Bible reference or words to search COL's website for",
};

// --------------------------------------------------------------------------------- commands

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn desktop(_: &Session) -> bool {
    cfg!(not(target_arch = "wasm32"))
}

const TOKEN: ParamSpec = ParamSpec {
    name: "token",
    kind: ParamKind::String,
    required: true,
    help: "A GitHub token that can read the Christian Open Library's books",
};
const PATH: ParamSpec = ParamSpec {
    name: "path",
    kind: ParamKind::String,
    required: true,
    help: "A folder holding a copy (git clone) of the Christian Open Library's books",
};
const SCOPE: ParamSpec = ParamSpec {
    name: "scope",
    kind: ParamKind::String,
    required: false,
    help: "selected (default), shown (every work the filter shows), everything, or paths",
};
const PATHS: ParamSpec = ParamSpec {
    name: "paths",
    kind: ParamKind::String,
    required: false,
    help: "Works to download, by path, separated by new lines",
};
const fn filter_param(name: &'static str, kind: ParamKind, help: &'static str) -> ParamSpec {
    ParamSpec {
        name,
        kind,
        required: false,
        help,
    }
}
const FILTER: &[ParamSpec] = &[
    filter_param("text", ParamKind::String, "Words of the title or author"),
    filter_param(
        "kind",
        ParamKind::String,
        "book, commentary, confession, creed, catechism or hymnal",
    ),
    filter_param(
        "author",
        ParamKind::String,
        "The author, as the catalogue names them",
    ),
    filter_param(
        "tradition",
        ParamKind::String,
        "The author's tradition (Puritan, Reformed, …)",
    ),
    filter_param(
        "collection",
        ParamKind::String,
        "The collection (EEBO-TCP, …)",
    ),
    filter_param("year_from", ParamKind::Integer, "Works from this year"),
    filter_param("year_to", ParamKind::Integer, "Works up to this year"),
];
const ON: ParamSpec = ParamSpec {
    name: "on",
    kind: ParamKind::Boolean,
    required: false,
    help: "Choose (true, the default) or leave out (false)",
};
const IDS: ParamSpec = ParamSpec {
    name: "ids",
    kind: ParamKind::String,
    required: false,
    help: "COL Books to update, comma-separated; every changed one when omitted",
};

fn str_param(p: &Params<'_>, name: &str) -> Option<String> {
    p.str(name)
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
}

#[cfg(not(target_arch = "wasm32"))]
fn connect_token(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "col.connect_token";
    let token = p
        .str("token")
        .and_then(Token::new)
        .ok_or_else(|| failed(ID, "That is not a GitHub token"))?;
    // Kept in the credential store; where none answers, for this session only.
    let raw = token.0.to_string();
    if s.col.secrets.set(&raw).is_err() {
        let session = Arc::new(SessionSecret::default());
        let _ = session.set(&raw);
        s.col.secrets = session;
    }
    if let Some(user) = &s.user {
        let _ = user.set_preference(LOCAL_PREF, "");
    }
    s.col.access = Some(ColAccess::GitHub { token });
    s.col.restored = true;
    s.col.catalog = None;
    s.col_load().map_err(|e| failed(ID, e))?;
    Ok(json!({ "lasting": s.col.secrets.lasting() }))
}

#[cfg(not(target_arch = "wasm32"))]
fn connect_local(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "col.connect_local";
    let path = str_param(&p, "path").ok_or_else(|| failed(ID, "Choose a folder"))?;
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(failed(ID, format!("{path} is not a folder")));
    }
    let _ = s.col.secrets.delete();
    if let Some(user) = &s.user {
        let _ = user.set_preference(LOCAL_PREF, &path);
    }
    s.col.access = Some(ColAccess::Local { root });
    s.col.restored = true;
    s.col.catalog = None;
    s.col_load().map_err(|e| failed(ID, e))?;
    Ok(Value::Null)
}

fn disconnect(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    let deleted = s.col.secrets.delete();
    if let Some(user) = &s.user {
        let _ = user.set_preference(LOCAL_PREF, "");
    }
    if let Some(job) = s.col.loading.take() {
        s.jobs().cancel(job);
    }
    s.col.access = None;
    s.col.catalog = None;
    s.col.changes = None;
    s.col.selected.clear();
    s.col.error = None;
    deleted.map_err(|e| {
        failed(
            "col.disconnect",
            format!("The token could not be removed: {e}"),
        )
    })?;
    Ok(Value::Null)
}

#[cfg(not(target_arch = "wasm32"))]
fn refresh(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    s.col_restore();
    s.col_load().map_err(|e| failed("col.refresh", e))?;
    Ok(Value::Null)
}

#[cfg(not(target_arch = "wasm32"))]
fn browse_public(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    s.col_load_public();
    Ok(Value::Null)
}

fn set_filter(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let year = |name: &str| p.i64(name).and_then(|y| u16::try_from(y).ok());
    s.col.filter = ColFilter {
        text: p.str("text").unwrap_or_default().to_string(),
        kind: str_param(&p, "kind"),
        author: str_param(&p, "author"),
        tradition: str_param(&p, "tradition"),
        collection: str_param(&p, "collection"),
        year_from: year("year_from"),
        year_to: year("year_to"),
    };
    Ok(json!({ "shown": s.col.shown().len() }))
}

fn select(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let on = p.bool("on").unwrap_or(true);
    let paths: Vec<String> = match str_param(&p, "paths") {
        Some(list) => list.lines().map(str::to_string).collect(),
        // Every work the filter shows.
        None => s.col.shown().iter().map(|w| w.path.clone()).collect(),
    };
    for path in paths {
        if on {
            s.col.selected.insert(path);
        } else {
            s.col.selected.remove(&path);
        }
    }
    Ok(json!({ "selected": s.col.selected.len() }))
}

#[cfg(not(target_arch = "wasm32"))]
fn download(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "col.download";
    let catalog = s
        .col
        .catalog
        .clone()
        .ok_or_else(|| failed(ID, "Connect first"))?;
    let scope = str_param(&p, "scope").unwrap_or_else(|| "selected".into());
    let paths: Option<BTreeSet<String>> =
        str_param(&p, "paths").map(|l| l.lines().map(str::to_string).collect());
    let works: Vec<ColWork> = match (&paths, scope.as_str()) {
        (Some(paths), _) => catalog
            .works
            .iter()
            .filter(|w| paths.contains(&w.path))
            .cloned()
            .collect(),
        (None, "everything") => catalog.works.clone(),
        (None, "shown") => s.col.shown().into_iter().cloned().collect(),
        (None, "selected") => s.col.chosen().into_iter().cloned().collect(),
        (None, other) => return Err(failed(ID, format!("no scope {other:?}"))),
    };
    let everything = paths.is_none() && scope == "everything";
    let out = s
        .col_download(works, everything)
        .map_err(|e| failed(ID, e))?;
    s.col.selected.clear();
    Ok(out)
}

#[cfg(not(target_arch = "wasm32"))]
fn update(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    const ID: &str = "col.update";
    let catalog = s
        .col
        .catalog
        .clone()
        .ok_or_else(|| failed(ID, "Check for new books first"))?;
    let changes = s.col.changes.clone().unwrap_or_default();
    let ids: Option<BTreeSet<String>> =
        str_param(&p, "ids").map(|l| l.split(',').map(|i| i.trim().to_string()).collect());
    let paths: BTreeSet<&String> = changes
        .changed
        .iter()
        .filter(|(id, _)| ids.as_ref().is_none_or(|ids| ids.contains(id)))
        .map(|(_, path)| path)
        .collect();
    let works: Vec<ColWork> = catalog
        .works
        .iter()
        .filter(|w| paths.contains(&w.path))
        .cloned()
        .collect();
    if works.is_empty() {
        return Err(failed(ID, "Every COL Book is up to date"));
    }
    s.col_download(works, false).map_err(|e| failed(ID, e))
}

#[cfg(target_arch = "wasm32")]
fn connect_token(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(failed(
        "col.connect_token",
        "Downloads need the desktop app",
    ))
}
#[cfg(target_arch = "wasm32")]
fn connect_local(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(failed(
        "col.connect_local",
        "Downloads need the desktop app",
    ))
}
#[cfg(target_arch = "wasm32")]
fn refresh(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(failed("col.refresh", "Downloads need the desktop app"))
}
#[cfg(target_arch = "wasm32")]
fn browse_public(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(failed(
        "col.browse_public",
        "Downloads need the desktop app",
    ))
}
#[cfg(target_arch = "wasm32")]
fn download(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(failed("col.download", "Downloads need the desktop app"))
}
#[cfg(target_arch = "wasm32")]
fn update(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(failed("col.update", "Downloads need the desktop app"))
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "col.search_website",
        label: "Search Christian Open Library",
        shortcut: None,
        params: &[TEXT],
        enabled: always,
        run: search_website,
    },
    CommandSpec {
        id: "col.search_link",
        label: "Offer Search Christian Open Library",
        shortcut: None,
        params: &[ON],
        enabled: always,
        run: search_link,
    },
    CommandSpec {
        id: "col.connect_token",
        label: "Connect to the Christian Open Library with a token",
        shortcut: None,
        params: &[TOKEN],
        enabled: desktop,
        run: connect_token,
    },
    CommandSpec {
        id: "col.connect_local",
        label: "Use a local copy of the Christian Open Library",
        shortcut: None,
        params: &[PATH],
        enabled: desktop,
        run: connect_local,
    },
    CommandSpec {
        id: "col.disconnect",
        label: "Disconnect from the Christian Open Library",
        shortcut: None,
        params: &[],
        enabled: always,
        run: disconnect,
    },
    CommandSpec {
        id: "col.refresh",
        label: "Check for new books",
        shortcut: None,
        params: &[],
        enabled: desktop,
        run: refresh,
    },
    CommandSpec {
        id: "col.browse_public",
        label: "Browse the Christian Open Library's catalogue",
        shortcut: None,
        params: &[],
        enabled: desktop,
        run: browse_public,
    },
    CommandSpec {
        id: "col.filter",
        label: "Filter the Christian Open Library's books",
        shortcut: None,
        params: FILTER,
        enabled: always,
        run: set_filter,
    },
    CommandSpec {
        id: "col.select",
        label: "Choose books to download",
        shortcut: None,
        params: &[PATHS, ON],
        enabled: always,
        run: select,
    },
    CommandSpec {
        id: "col.download",
        label: "Download books",
        shortcut: None,
        params: &[SCOPE, PATHS],
        enabled: desktop,
        run: download,
    },
    CommandSpec {
        id: "col.update",
        label: "Update changed books",
        shortcut: None,
        params: &[IDS],
        enabled: desktop,
        run: update,
    },
];
