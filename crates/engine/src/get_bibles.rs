//! Get Bibles (#91): the catalog of Bibles offered by SWORD repositories.
//!
//! A catalog job reads CrossWire's master repository list and each repository's catalog
//! through the app's transport ([`Session::set_fetch`]), keeps the Bibles, and caches them so
//! the Tab opens offline. Every byte comes from the repositories themselves.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::{JobEvent, JobEventKind, JobId, Session};

/// The Featured section's data file: module + repository, in display order.
const FEATURED_TOML: &str = include_str!("../data/featured-bibles.toml");

/// The repository preferred when the same module is offered by several.
pub const PREFERRED_REPO: &str = "CrossWire";

/// One Featured row as written in the data file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FeaturedBible {
    pub module: String,
    pub repo: String,
}

#[derive(Debug, Deserialize)]
struct FeaturedFile {
    bible: Vec<FeaturedBible>,
}

/// The Featured list, in display order.
pub fn featured_bibles() -> Vec<FeaturedBible> {
    parse_featured(FEATURED_TOML).unwrap_or_default()
}

fn parse_featured(text: &str) -> Result<Vec<FeaturedBible>, String> {
    toml::from_str::<FeaturedFile>(text)
        .map(|f| f.bible)
        .map_err(|e| e.to_string())
}

/// How to reach one repository again (for downloads), from its catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRef {
    pub name: String,
    pub uid: Option<String>,
    /// Base URLs, tried in order.
    pub urls: Vec<String>,
}

impl RepoRef {
    fn from_repository(r: &obs_formats::sword_repo::Repository) -> Self {
        Self {
            name: r.name.clone(),
            uid: r.uid.clone(),
            urls: r.sources.iter().map(|s| s.base_url()).collect(),
        }
    }

    /// The repository, for the SWORD installer.
    pub fn repository(&self) -> obs_formats::sword_repo::Repository {
        obs_formats::sword_repo::Repository {
            name: self.name.clone(),
            uid: self.uid.clone(),
            sources: self
                .urls
                .iter()
                .filter_map(|u| obs_formats::sword_repo::Source::from_url(u))
                .collect(),
        }
    }
}

/// One Bible a repository offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BibleListing {
    /// SWORD module name (`NASB1995`).
    pub module: String,
    /// Repository caption (`Lockman Foundation`).
    pub repo: String,
    /// `Description` from the conf.
    pub title: String,
    /// `Abbreviation` from the conf, else the module name.
    pub abbreviation: String,
    /// `Lang` from the conf (`en`, `grc`, …); `und` when missing.
    pub language: String,
    /// `InstallSize` in bytes, when the conf gives it.
    pub size: Option<u64>,
    /// `DistributionLicense` from the conf, as published.
    pub licence: Option<String>,
    pub version: Option<String>,
}

impl BibleListing {
    fn from_remote(repo: &str, m: &obs_formats::sword_repo::RemoteModule) -> Self {
        let conf = &m.conf;
        let text = |k: &str| {
            conf.get(k)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        Self {
            module: m.name().to_string(),
            repo: repo.to_string(),
            title: text("Description").unwrap_or_else(|| m.name().to_string()),
            abbreviation: text("Abbreviation").unwrap_or_else(|| m.name().to_string()),
            language: text("Lang").unwrap_or_else(|| "und".into()),
            size: text("InstallSize").and_then(|s| s.parse().ok()),
            licence: text("DistributionLicense"),
            version: text("Version"),
        }
    }

    /// Shown as the row's source: "from CrossWire".
    pub fn source_label(&self) -> String {
        format!("from {}", self.repo)
    }

    /// Whether the listing matches a search box entry (title, abbreviation or module).
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty()
            || [&self.title, &self.abbreviation, &self.module]
                .iter()
                .any(|f| f.to_lowercase().contains(&q))
    }
}

/// The Bibles of every repository on the master list, as last fetched.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BibleCatalog {
    /// Seconds since the Unix epoch when it was fetched.
    pub fetched_at: u64,
    pub repos: Vec<RepoRef>,
    /// One per module name (case-insensitive), the preferred source kept.
    pub bibles: Vec<BibleListing>,
    /// Repositories that could not be read this time, and why.
    pub failed: Vec<(String, String)>,
}

impl BibleCatalog {
    /// Build from per-repository listings, one row per module (see [`dedupe`]).
    pub fn new(fetched_at: u64, repos: Vec<RepoRef>, listings: Vec<BibleListing>) -> Self {
        Self {
            fetched_at,
            repos,
            bibles: dedupe(listings, &featured_bibles()),
            failed: Vec::new(),
        }
    }

    pub fn repo(&self, name: &str) -> Option<&RepoRef> {
        self.repos.iter().find(|r| r.name == name)
    }

    pub fn get(&self, module: &str) -> Option<&BibleListing> {
        self.bibles
            .iter()
            .find(|b| b.module.eq_ignore_ascii_case(module))
    }

    /// The Featured rows, in data-file order; modules no repository offers are left out.
    pub fn featured(&self) -> Vec<&BibleListing> {
        featured_bibles()
            .iter()
            .filter_map(|f| self.get(&f.module))
            .collect()
    }

    /// "All Bibles": every listing matching `query` and, if given, `language`, by title.
    pub fn all(&self, query: &str, language: Option<&str>) -> Vec<&BibleListing> {
        let mut out: Vec<&BibleListing> = self
            .bibles
            .iter()
            .filter(|b| b.matches(query) && language.is_none_or(|l| b.language == l))
            .collect();
        out.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then_with(|| a.module.cmp(&b.module))
        });
        out
    }

    /// Language codes present, most Bibles first, for the language filter.
    pub fn languages(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for b in &self.bibles {
            *counts.entry(b.language.as_str()).or_default() += 1;
        }
        let mut out: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(l, n)| (l.to_string(), n))
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }
}

/// One row per module name (case-insensitive). The source kept is, in order: the
/// repository the Featured list names, then CrossWire, then the first repository listed.
pub fn dedupe(listings: Vec<BibleListing>, featured: &[FeaturedBible]) -> Vec<BibleListing> {
    let rank = |b: &BibleListing| -> u8 {
        let featured_here = featured.iter().any(|f| {
            f.module.eq_ignore_ascii_case(&b.module) && f.repo.eq_ignore_ascii_case(&b.repo)
        });
        if featured_here {
            0
        } else if b.repo.eq_ignore_ascii_case(PREFERRED_REPO) {
            1
        } else {
            2
        }
    };
    let mut kept: Vec<BibleListing> = Vec::new();
    let mut at: BTreeMap<String, usize> = BTreeMap::new();
    for b in listings {
        let key = b.module.to_lowercase();
        match at.get(&key) {
            Some(&i) => {
                if rank(&b) < rank(&kept[i]) {
                    kept[i] = b;
                }
            }
            None => {
                at.insert(key, kept.len());
                kept.push(b);
            }
        }
    }
    kept
}

/// Read every repository on the master list and keep their Bibles. Repositories that fail
/// are listed in [`BibleCatalog::failed`]; only a failure to read the master list (or every
/// repository) fails the whole fetch.
pub fn fetch_catalog(
    installer: &obs_formats::sword_repo::Installer,
    fetch: &dyn obs_formats::sword_repo::Fetch,
    is_cancelled: &dyn Fn() -> bool,
    now: u64,
) -> Result<BibleCatalog, String> {
    let repos = installer
        .master_repo_list()
        .map_err(|e| format!("Could not read the repository list: {e}"))?;
    let mut catalog = fetch_repos(installer, &repos, is_cancelled, now)?;
    featured_sizes(&mut catalog, fetch, is_cancelled);
    Ok(catalog)
}

/// Featured Bibles whose conf gives no `InstallSize` (Lockman's do not) get the size of
/// the package zip the download will fetch, asked of the repository without downloading it.
fn featured_sizes(
    catalog: &mut BibleCatalog,
    fetch: &dyn obs_formats::sword_repo::Fetch,
    is_cancelled: &dyn Fn() -> bool,
) {
    for f in featured_bibles() {
        if is_cancelled() {
            return;
        }
        let Some(i) = catalog
            .bibles
            .iter()
            .position(|b| b.module.eq_ignore_ascii_case(&f.module) && b.size.is_none())
        else {
            continue;
        };
        let module = catalog.bibles[i].module.clone();
        let Some(repo) = catalog.repo(&catalog.bibles[i].repo) else {
            continue;
        };
        let mut names = vec![module.clone()];
        if module.to_lowercase() != module {
            names.push(module.to_lowercase());
        }
        let size = repo.urls.iter().find_map(|base| {
            names.iter().find_map(|n| {
                fetch
                    .size(&format!("{base}packages/{n}.zip"))
                    .ok()
                    .flatten()
            })
        });
        catalog.bibles[i].size = size;
    }
}

/// [`fetch_catalog`] for a given set of repositories.
pub fn fetch_repos(
    installer: &obs_formats::sword_repo::Installer,
    repos: &[obs_formats::sword_repo::Repository],
    is_cancelled: &dyn Fn() -> bool,
    now: u64,
) -> Result<BibleCatalog, String> {
    let mut listings = Vec::new();
    let mut refs = Vec::new();
    let mut failed = Vec::new();
    for repo in repos {
        if is_cancelled() {
            return Err("Cancelled".into());
        }
        match installer.catalog(repo) {
            Ok(catalog) => {
                refs.push(RepoRef::from_repository(repo));
                listings.extend(
                    catalog
                        .of_kind(obs_formats::sword_repo::ModuleKind::Bible)
                        .filter(|m| !m.conf.is_locked())
                        .map(|m| BibleListing::from_remote(&repo.name, m)),
                );
            }
            Err(e) if obs_formats::sword_repo::is_cancelled(&e) => {
                return Err("Cancelled".into());
            }
            Err(e) => failed.push((repo.name.clone(), e.to_string())),
        }
    }
    if refs.is_empty() && !repos.is_empty() {
        let why = failed
            .first()
            .map(|(r, e)| format!("{r}: {e}"))
            .unwrap_or_default();
        return Err(format!("Could not reach any SWORD repository ({why})"));
    }
    let mut catalog = BibleCatalog::new(now, refs, listings);
    catalog.failed = failed;
    Ok(catalog)
}

/// Get Bibles state on the Session.
#[derive(Debug, Default)]
pub struct GetBibles {
    catalog: Option<BibleCatalog>,
    /// Why the last refresh failed (offline, repository down), while the cache is shown.
    offline: Option<String>,
    refreshing: Option<JobId>,
    cache_path: Option<PathBuf>,
}

impl GetBibles {
    pub fn catalog(&self) -> Option<&BibleCatalog> {
        self.catalog.as_ref()
    }

    /// Set when the last refresh failed; the Tab shows the cached catalog with this message.
    pub fn offline(&self) -> Option<&str> {
        self.offline.as_deref()
    }

    pub fn is_refreshing(&self) -> bool {
        self.refreshing.is_some()
    }

    /// Where the catalog is cached. The app sets [`default_catalog_cache`] at startup;
    /// tests use a temp file. Loads the cache if there is one.
    pub fn set_cache_path(&mut self, path: PathBuf) {
        self.cache_path = Some(path);
        self.load_cache();
    }

    fn load_cache(&mut self) {
        let Some(path) = &self.cache_path else {
            return;
        };
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(catalog) = serde_json::from_str::<BibleCatalog>(&text)
        {
            self.catalog = Some(catalog);
        }
    }

    fn save_cache(&self) -> Result<(), String> {
        let (Some(path), Some(catalog)) = (&self.cache_path, &self.catalog) else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string(catalog).map_err(|e| e.to_string())?;
        // Write then rename, so a crash never leaves a half-written cache.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}

/// The catalog cache: `$OBS_SWORD_CATALOG`, else `sword-catalog.json` in the app's data folder.
pub fn default_catalog_cache() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OBS_SWORD_CATALOG") {
        return Some(PathBuf::from(p));
    }
    Some(crate::library::data_dir()?.join("sword-catalog.json"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Session {
    /// Start a catalog refresh unless one is running. Returns the job.
    pub fn refresh_bible_catalog(&mut self) -> JobId {
        if let Some(job) = self.get_bibles.refreshing {
            return job;
        }
        let fetch = self.fetch();
        let job = self.jobs().submit_fn("Get Bibles catalog", move |ctx| {
            let installer = crate::sword_installer(fetch.clone(), ctx);
            let cancelled = || ctx.is_cancelled();
            fetch_catalog(&installer, fetch.as_ref(), &cancelled, now_secs())
        });
        self.get_bibles.refreshing = Some(job);
        job
    }

    /// Apply Get Bibles job events; return the rest.
    pub(crate) fn poll_get_bibles(&mut self, events: Vec<JobEvent>) -> Vec<JobEvent> {
        let mut other = Vec::new();
        for event in events {
            if Some(event.id) != self.get_bibles.refreshing {
                other.push(event);
                continue;
            }
            match event.kind {
                JobEventKind::Finished(out) => {
                    self.get_bibles.refreshing = None;
                    if let Ok(catalog) = out.downcast::<BibleCatalog>() {
                        self.get_bibles.catalog = Some(*catalog);
                        self.get_bibles.offline = None;
                        if let Err(e) = self.get_bibles.save_cache() {
                            self.workspace.push_effect(crate::Effect::Message(format!(
                                "Could not cache the Bible catalog: {e}"
                            )));
                        }
                    }
                }
                JobEventKind::Failed(e) => {
                    self.get_bibles.refreshing = None;
                    self.get_bibles.offline = Some(e);
                }
                JobEventKind::Cancelled => {
                    self.get_bibles.refreshing = None;
                }
                _ => {}
            }
        }
        other
    }
}

const QUERY: ParamSpec = ParamSpec {
    name: "query",
    kind: ParamKind::String,
    required: false,
    help: "Filter by title, abbreviation or module name",
};
const LANGUAGE: ParamSpec = ParamSpec {
    name: "language",
    kind: ParamKind::String,
    required: false,
    help: "Only Bibles in this language (BCP 47 code)",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "getbibles.refresh",
        label: "Refresh Bible catalog",
        shortcut: None,
        params: &[],
        enabled: crate::command::always,
        run: |s, _| {
            let job = s.refresh_bible_catalog();
            Ok(json!({"job": job.0}))
        },
    },
    CommandSpec {
        id: "getbibles.list",
        label: "List available Bibles",
        shortcut: None,
        params: &[QUERY, LANGUAGE],
        enabled: crate::command::always,
        run: list,
    },
];

fn list(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let Some(catalog) = s.get_bibles.catalog() else {
        return Ok(json!({"featured": [], "all": [], "offline": s.get_bibles.offline()}));
    };
    let row = |b: &BibleListing| {
        json!({"module": b.module, "repo": b.repo, "title": b.title,
               "abbreviation": b.abbreviation, "language": b.language,
               "size": b.size, "licence": b.licence})
    };
    let query = p.str("query").unwrap_or_default();
    let language = p.str("language").filter(|l| !l.is_empty());
    Ok(json!({
        "fetched_at": catalog.fetched_at,
        "offline": s.get_bibles.offline(),
        "featured": catalog.featured().into_iter().map(row).collect::<Vec<_>>(),
        "all": catalog.all(query, language).into_iter().map(row).collect::<Vec<_>>(),
    }))
}
