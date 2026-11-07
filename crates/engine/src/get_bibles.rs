//! Get Resources (#91, #132): the catalog of Bibles, Lexicons, Commentaries and Dictionaries
//! offered by SWORD repositories. (The module, Tab kind and command ids keep their Get Bibles
//! names: saved Layouts and scripts name them.)
//!
//! A catalog job reads CrossWire's master repository list and each repository's catalog
//! through the app's transport ([`Session::set_fetch`]), keeps the modules SWORD Import knows,
//! and caches them so the Tab opens offline. Every byte comes from the repositories themselves.

use std::collections::BTreeMap;
use std::path::PathBuf;

use obs_model::ResourceType;
use obs_scripture::StrongsLanguage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::library_search::type_plural;
use crate::licence::{LicenceClass, LicenceFilter, licence_class};
use crate::open_catalogue::{OPEN_CATALOGUE, OpenSource};
use crate::{JobEvent, JobEventKind, JobId, Session};

/// The Featured Bibles' data file: module + repository, in display order, and the Featured
/// Hebrew & Greek group.
const FEATURED_BIBLES_TOML: &str = include_str!("../data/featured-bibles.toml");

/// The Featured Lexicons, Commentaries and Dictionaries, one array per type.
const FEATURED_RESOURCES_TOML: &str = include_str!("../data/featured-resources.toml");

/// The Resource types the catalog lists, in the type filter's order.
pub const LISTED_TYPES: [ResourceType; 4] = [
    ResourceType::Bible,
    ResourceType::Lexicon,
    ResourceType::Commentary,
    ResourceType::Dictionary,
];

/// Bumped when the cached catalog's contents change meaning; an older cache is shown but
/// refreshed at once. 2: Lexicons, Commentaries and Dictionaries (#132). 3: Strong's and
/// Morphology tags read from `GlobalOptionFilter` (#133).
pub const CATALOG_VERSION: u32 = 3;

/// Opening Get Bibles refreshes a catalog older than this (6 hours).
pub const REFRESH_AFTER_SECS: u64 = 6 * 60 * 60;

/// The repository preferred when the same module is offered by several.
pub const PREFERRED_REPO: &str = "CrossWire";

/// One Featured row as written in a data file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FeaturedModule {
    pub module: String,
    pub repo: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FeaturedFile {
    bible: Vec<FeaturedModule>,
    hebrew_greek: Vec<FeaturedModule>,
    lexicon: Vec<FeaturedModule>,
    commentary: Vec<FeaturedModule>,
    dictionary: Vec<FeaturedModule>,
}

/// The Featured list of one Resource type, in display order.
pub fn featured(ty: ResourceType) -> Vec<FeaturedModule> {
    let text = if ty == ResourceType::Bible {
        FEATURED_BIBLES_TOML
    } else {
        FEATURED_RESOURCES_TOML
    };
    let Ok(file) = toml::from_str::<FeaturedFile>(text) else {
        return Vec::new();
    };
    match ty {
        ResourceType::Bible => file.bible,
        ResourceType::Lexicon => file.lexicon,
        ResourceType::Commentary => file.commentary,
        ResourceType::Dictionary => file.dictionary,
        ResourceType::CrossReferenceSet | ResourceType::PeopleAndPlaces => Vec::new(),
    }
}

/// The Featured Hebrew & Greek group (#133): original-language Bibles, in display order.
pub fn featured_hebrew_greek() -> Vec<FeaturedModule> {
    toml::from_str::<FeaturedFile>(FEATURED_BIBLES_TOML)
        .map(|f| f.hebrew_greek)
        .unwrap_or_default()
}

/// Every Featured list, Bibles first, then Hebrew & Greek.
pub fn featured_all() -> Vec<FeaturedModule> {
    let mut all = featured(ResourceType::Bible);
    all.extend(featured_hebrew_greek());
    all.extend(LISTED_TYPES.into_iter().skip(1).flat_map(featured));
    all
}

/// What the Get Resources Tab lists: every type, one Resource type, or Hebrew & Greek (the
/// Bibles in Hebrew, Aramaic or Greek, #133). Hebrew & Greek is a language filter on Bibles,
/// not a Resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResourceFilter {
    #[default]
    All,
    Type(ResourceType),
    HebrewAndGreek,
}

impl ResourceFilter {
    /// The filter row, in order.
    pub const CHOICES: [ResourceFilter; 6] = [
        ResourceFilter::Type(ResourceType::Bible),
        ResourceFilter::HebrewAndGreek,
        ResourceFilter::Type(ResourceType::Lexicon),
        ResourceFilter::Type(ResourceType::Commentary),
        ResourceFilter::Type(ResourceType::Dictionary),
        ResourceFilter::All,
    ];

    /// The Resource type listed; `None` for All.
    pub fn resource_type(self) -> Option<ResourceType> {
        match self {
            ResourceFilter::All => None,
            ResourceFilter::Type(t) => Some(t),
            ResourceFilter::HebrewAndGreek => Some(ResourceType::Bible),
        }
    }

    /// Whether a listing passes the filter (type, and language for Hebrew & Greek).
    pub fn accepts(self, listing: &ResourceListing) -> bool {
        match self {
            ResourceFilter::All => true,
            ResourceFilter::Type(t) => listing.kind == t,
            ResourceFilter::HebrewAndGreek => {
                listing.kind == ResourceType::Bible && listing.is_hebrew_or_greek()
            }
        }
    }

    /// The id [`type_filter_from_id`] reads back, for `library.get_resources`; `None` for a
    /// Resource type the online repositories do not offer (Cross-reference sets, People &
    /// places indexes).
    pub fn id(self) -> Option<&'static str> {
        match self {
            ResourceFilter::All => Some("all"),
            ResourceFilter::HebrewAndGreek => Some("hebrew-greek"),
            ResourceFilter::Type(ResourceType::Bible) => Some("bibles"),
            ResourceFilter::Type(ResourceType::Lexicon) => Some("lexicons"),
            ResourceFilter::Type(ResourceType::Commentary) => Some("commentaries"),
            ResourceFilter::Type(ResourceType::Dictionary) => Some("dictionaries"),
            ResourceFilter::Type(
                ResourceType::CrossReferenceSet | ResourceType::PeopleAndPlaces,
            ) => None,
        }
    }

    /// Shown on the filter row and in "All …".
    pub fn label(self) -> &'static str {
        match self {
            ResourceFilter::All => "All",
            ResourceFilter::Type(t) => crate::library_search::type_plural(t),
            ResourceFilter::HebrewAndGreek => HEBREW_AND_GREEK,
        }
    }
}

impl From<Option<ResourceType>> for ResourceFilter {
    fn from(ty: Option<ResourceType>) -> Self {
        ty.map_or(ResourceFilter::All, ResourceFilter::Type)
    }
}

/// The Hebrew & Greek filter's and Featured group's name.
pub const HEBREW_AND_GREEK: &str = "Hebrew & Greek";

/// A type filter's id in commands: `bibles`, `hebrew-greek`, `lexicons`, `commentaries`,
/// `dictionaries` (singular too, any case); `all` or empty is no filter.
pub fn type_filter_from_id(id: &str) -> Result<ResourceFilter, String> {
    let ty = |t| Ok(ResourceFilter::Type(t));
    match id.trim().to_lowercase().as_str() {
        "" | "all" => Ok(ResourceFilter::All),
        "bible" | "bibles" => ty(ResourceType::Bible),
        "hebrew-greek" | "hebrew & greek" | "originals" => Ok(ResourceFilter::HebrewAndGreek),
        "lexicon" | "lexicons" => ty(ResourceType::Lexicon),
        "commentary" | "commentaries" => ty(ResourceType::Commentary),
        "dictionary" | "dictionaries" => ty(ResourceType::Dictionary),
        other => Err(format!(
            "Unknown type {other:?}: use bibles, hebrew-greek, lexicons, commentaries, \
             dictionaries or all"
        )),
    }
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

fn bible() -> ResourceType {
    ResourceType::Bible
}

/// Where a Get Resources row downloads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ListingSource {
    /// A module of a SWORD repository.
    #[default]
    Sword,
    /// An entry of the Open catalogue (#134), from its publisher's own URL.
    Open,
}

/// One Bible, Lexicon, Commentary or Dictionary a repository offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceListing {
    /// SWORD module name (`NASB1995`).
    pub module: String,
    /// What it imports as. A Dictionary module whose conf has `Feature=GreekDef` or
    /// `HebrewDef` is a Lexicon.
    #[serde(default = "bible")]
    pub kind: ResourceType,
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
    /// A Bible tagged with Strong's numbers (`Feature=StrongsNumbers`, or a
    /// `GlobalOptionFilter` of `OSISStrongs`, `GBFStrongs` or `ThMLStrongs`).
    #[serde(default)]
    pub strongs: bool,
    /// A Bible tagged with Morphology (`GlobalOptionFilter` `OSISMorph`, `GBFMorph` or
    /// `ThMLMorph`; OSIS `morph="robinson:…"` or `oshm:…`).
    #[serde(default)]
    pub morphology: bool,
    /// Why SWORD Import refuses the module today (Commentary numbering, `HREFCom`, …). The
    /// row is shown, but cannot be downloaded.
    #[serde(default)]
    pub refused: Option<String>,
    /// A SWORD module, or an Open catalogue entry (#134; then `module` is the entry's id and
    /// `repo` its publisher).
    #[serde(default)]
    pub source: ListingSource,
    /// An Open catalogue entry's archived licence evidence.
    #[serde(default)]
    pub evidence_url: Option<String>,
    /// An Open catalogue entry's licence class, as the entry declares it beside its evidence.
    /// A SWORD module has none: its class comes from its `DistributionLicense`.
    #[serde(default)]
    pub declared_licence_class: Option<LicenceClass>,
}

impl ResourceListing {
    /// The listing for a module, or `None` when it is not a type the catalog lists
    /// (general books) or is locked.
    fn from_remote(repo: &str, m: &obs_formats::sword_repo::RemoteModule) -> Option<Self> {
        use obs_formats::sword_repo::ModuleKind;
        if m.conf.is_locked() {
            return None;
        }
        let has = |f: &str| m.conf.features().iter().any(|x| x.eq_ignore_ascii_case(f));
        let filters = m.conf.global_option_filters();
        let filter = |names: &[&str]| {
            filters
                .iter()
                .any(|x| names.iter().any(|n| x.trim().eq_ignore_ascii_case(n)))
        };
        let kind = match m.conf.module_kind().ok()? {
            ModuleKind::Bible => ResourceType::Bible,
            ModuleKind::Commentary => ResourceType::Commentary,
            ModuleKind::Dictionary if has("GreekDef") || has("HebrewDef") => ResourceType::Lexicon,
            ModuleKind::Dictionary => ResourceType::Dictionary,
            ModuleKind::GenBook | ModuleKind::Unknown => return None,
        };
        let conf = &m.conf;
        let text = |k: &str| {
            conf.get(k)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        Some(Self {
            module: m.name().to_string(),
            kind,
            repo: repo.to_string(),
            title: text("Description").unwrap_or_else(|| m.name().to_string()),
            abbreviation: text("Abbreviation").unwrap_or_else(|| m.name().to_string()),
            language: text("Lang").unwrap_or_else(|| "und".into()),
            size: text("InstallSize").and_then(|s| s.parse().ok()),
            licence: text("DistributionLicense"),
            version: text("Version"),
            strongs: kind == ResourceType::Bible
                && (has("StrongsNumbers") || filter(&["OSISStrongs", "GBFStrongs", "ThMLStrongs"])),
            morphology: kind == ResourceType::Bible
                && filter(&["OSISMorph", "GBFMorph", "ThMLMorph"]),
            refused: obs_formats::sword_import_type(conf).err(),
            source: ListingSource::Sword,
            evidence_url: None,
            declared_licence_class: None,
        })
    }

    /// What its licence lets the user do (#134): an Open catalogue entry's declared class,
    /// else the class of its `DistributionLicense`. No licence is Unclear.
    pub fn licence_class(&self) -> LicenceClass {
        self.declared_licence_class.unwrap_or_else(|| {
            self.licence
                .as_deref()
                .map_or(LicenceClass::Unclear, licence_class)
        })
    }

    /// From the Open catalogue (#134) rather than a SWORD repository.
    pub fn is_open(&self) -> bool {
        self.source == ListingSource::Open
    }

    /// Which features the Resource feeds, for its row: "Used by Passage Guide › Commentaries".
    pub fn used_by(&self) -> Option<&'static str> {
        match self.kind {
            ResourceType::Bible if self.strongs && self.is_hebrew_or_greek() => {
                Some(if self.morphology {
                    "Used by Word Info, Word Study › Occurrences and Forms, and the Exegetical Guide"
                } else {
                    "Used by Word Info, Word Study › Occurrences, and the Exegetical Guide"
                })
            }
            ResourceType::Bible if self.strongs => {
                Some("Used by Word Study › Translated as and Occurrences")
            }
            ResourceType::Lexicon => Some("Used by Word Study › Lexicon entries and Word Info"),
            ResourceType::Commentary => Some("Used by Passage Guide › Commentaries"),
            ResourceType::Dictionary => Some("Used by Passage Guide › People & places"),
            _ => None,
        }
    }

    /// The tag badges on its row: "Strong's" and "Morphology", from the conf.
    pub fn tags(&self) -> Vec<&'static str> {
        let mut tags = Vec::new();
        if self.strongs {
            tags.push(TAG_STRONGS);
        }
        if self.morphology {
            tags.push(TAG_MORPHOLOGY);
        }
        tags
    }

    /// In Hebrew, Aramaic or Greek, as Word Study counts original languages.
    pub fn is_hebrew_or_greek(&self) -> bool {
        [StrongsLanguage::Hebrew, StrongsLanguage::Greek]
            .into_iter()
            .any(|l| crate::key_words::is_original_language(&self.language, l))
    }

    /// Shown as the row's source: "from CrossWire".
    pub fn source_label(&self) -> String {
        format!("from {}", self.repo)
    }

    /// Whether the listing matches a search box entry (title, abbreviation or module).
    pub fn matches(&self, query: &str) -> bool {
        self.matches_lower(&query.trim().to_lowercase())
    }

    /// [`Self::matches`] for an entry already trimmed and in lower case.
    fn matches_lower(&self, q: &str) -> bool {
        q.is_empty()
            || [&self.title, &self.abbreviation, &self.module]
                .iter()
                .any(|f| f.to_lowercase().contains(q))
    }
}

/// The tag badge of a Bible with Strong's numbers.
pub const TAG_STRONGS: &str = "Strong's";
/// The tag badge of a Bible with Morphology.
pub const TAG_MORPHOLOGY: &str = "Morphology";

/// The Bibles, Lexicons, Commentaries and Dictionaries of every repository on the master
/// list, as last fetched.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCatalog {
    /// [`CATALOG_VERSION`] when it was fetched; 0 for a Bibles-only cache from before #132.
    #[serde(default)]
    pub version: u32,
    /// Seconds since the Unix epoch when it was fetched.
    pub fetched_at: u64,
    pub repos: Vec<RepoRef>,
    /// One per module name (case-insensitive), the preferred source kept.
    #[serde(alias = "bibles")]
    pub listings: Vec<ResourceListing>,
    /// Repositories that could not be read this time, and why.
    pub failed: Vec<(String, String)>,
}

impl ResourceCatalog {
    /// Build from per-repository listings, one row per module (see [`dedupe`]).
    pub fn new(fetched_at: u64, repos: Vec<RepoRef>, listings: Vec<ResourceListing>) -> Self {
        Self {
            version: CATALOG_VERSION,
            fetched_at,
            repos,
            listings: dedupe(listings, &featured_all()),
            failed: Vec::new(),
        }
    }

    pub fn repo(&self, name: &str) -> Option<&RepoRef> {
        self.repos.iter().find(|r| r.name == name)
    }

    pub fn get(&self, module: &str) -> Option<&ResourceListing> {
        self.listings
            .iter()
            .find(|b| b.module.eq_ignore_ascii_case(module))
    }

    /// The Featured rows of one type, in data-file order; modules no repository offers (or
    /// offers as another type) are left out.
    pub fn featured(&self, ty: ResourceType) -> Vec<&ResourceListing> {
        featured(ty)
            .iter()
            .filter_map(|f| self.get(&f.module))
            .filter(|b| b.kind == ty)
            .collect()
    }

    /// The Featured Hebrew & Greek rows, in data-file order; modules no repository offers
    /// are left out.
    pub fn featured_hebrew_greek(&self) -> Vec<&ResourceListing> {
        featured_hebrew_greek()
            .iter()
            .filter_map(|f| self.get(&f.module))
            .filter(|b| b.kind == ResourceType::Bible)
            .collect()
    }

    /// The Featured sections a filter shows, titled ("Featured Bibles", "Featured Hebrew &
    /// Greek"), in order. Bibles and All show Hebrew & Greek after the Featured Bibles. With
    /// All, empty sections are left out.
    pub fn featured_sections(
        &self,
        filter: ResourceFilter,
    ) -> Vec<(String, Vec<&ResourceListing>)> {
        let mut out = Vec::new();
        for t in LISTED_TYPES {
            if filter == ResourceFilter::All || filter == ResourceFilter::Type(t) {
                out.push((format!("Featured {}", type_plural(t)), self.featured(t)));
            }
            if t == ResourceType::Bible && filter.resource_type().is_none_or(|ty| ty == t) {
                out.push((
                    format!("Featured {HEBREW_AND_GREEK}"),
                    self.featured_hebrew_greek(),
                ));
            }
        }
        if filter == ResourceFilter::All {
            out.retain(|(_, rows)| !rows.is_empty());
        }
        out
    }

    /// "All": every listing the filter accepts (any listed type for All) matching `query`
    /// and, if given, `language`, by title. The three filters combine.
    pub fn all(
        &self,
        query: &str,
        language: Option<&str>,
        filter: impl Into<ResourceFilter>,
    ) -> Vec<&ResourceListing> {
        let filter = filter.into();
        let mut out: Vec<&ResourceListing> = self
            .listings
            .iter()
            .filter(|b| {
                filter.accepts(b) && b.matches(query) && language.is_none_or(|l| b.language == l)
            })
            .collect();
        out.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then_with(|| a.module.cmp(&b.module))
        });
        out
    }

    /// Language codes present among the listings the filter accepts, most listings first,
    /// for the language filter.
    pub fn languages(&self, filter: impl Into<ResourceFilter>) -> Vec<(String, usize)> {
        let filter = filter.into();
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for b in self.listings.iter().filter(|b| filter.accepts(b)) {
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

/// One row per module name (case-insensitive), whatever its type: a SWORD folder holds one
/// module of a name. The source kept is, in order: the repository a Featured list names, then
/// CrossWire, then the first repository listed.
pub fn dedupe(listings: Vec<ResourceListing>, featured: &[FeaturedModule]) -> Vec<ResourceListing> {
    let rank = |b: &ResourceListing| -> u8 {
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
    let mut kept: Vec<ResourceListing> = Vec::new();
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

/// Read every repository on the master list and keep their Bibles, Lexicons, Commentaries
/// and Dictionaries. Repositories that fail are listed in [`ResourceCatalog::failed`]; only a
/// failure to read the master list (or every repository) fails the whole fetch.
pub fn fetch_catalog(
    installer: &obs_formats::sword_repo::Installer,
    fetch: &dyn obs_formats::sword_repo::Fetch,
    is_cancelled: &dyn Fn() -> bool,
    now: u64,
) -> Result<ResourceCatalog, String> {
    let repos = installer
        .master_repo_list()
        .map_err(|e| format!("Could not read the repository list: {e}"))?;
    let mut catalog = fetch_repos(installer, &repos, is_cancelled, now)?;
    featured_sizes(&mut catalog, fetch, is_cancelled);
    Ok(catalog)
}

/// Featured modules whose conf gives no `InstallSize` (Lockman's do not) get the size of
/// the package zip the download will fetch, asked of the repository without downloading it.
fn featured_sizes(
    catalog: &mut ResourceCatalog,
    fetch: &dyn obs_formats::sword_repo::Fetch,
    is_cancelled: &dyn Fn() -> bool,
) {
    for f in featured_all() {
        if is_cancelled() {
            return;
        }
        let Some(i) = catalog
            .listings
            .iter()
            .position(|b| b.module.eq_ignore_ascii_case(&f.module) && b.size.is_none())
        else {
            continue;
        };
        let module = catalog.listings[i].module.clone();
        let Some(repo) = catalog.repo(&catalog.listings[i].repo) else {
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
        catalog.listings[i].size = size;
    }
}

/// [`fetch_catalog`] for a given set of repositories.
pub fn fetch_repos(
    installer: &obs_formats::sword_repo::Installer,
    repos: &[obs_formats::sword_repo::Repository],
    is_cancelled: &dyn Fn() -> bool,
    now: u64,
) -> Result<ResourceCatalog, String> {
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
                        .modules
                        .iter()
                        .filter_map(|m| ResourceListing::from_remote(&repo.name, m)),
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
    let mut catalog = ResourceCatalog::new(now, refs, listings);
    catalog.failed = failed;
    Ok(catalog)
}

/// Get Resources state on the Session.
#[derive(Debug, Default)]
pub struct GetBibles {
    catalog: Option<ResourceCatalog>,
    /// The Tab's filter. Get Bibles… sets Bibles.
    filter: ResourceFilter,
    /// Why the last refresh failed (offline, repository down), while the cache is shown.
    offline: Option<String>,
    /// A refresh succeeded in this Session. A catalog loaded from the cache at startup was
    /// fetched in an earlier one, so it alone never makes the downloads up to date (#136).
    checked_this_session: bool,
    refreshing: Option<JobId>,
    cache_path: Option<PathBuf>,
    sword_dir: Option<PathBuf>,
    /// The licence filter (#134). It applies to every section and to All.
    licence: LicenceFilter,
    /// The Open catalogue and its rows, when a test sets one; else the compiled-in one.
    open: Option<(Vec<OpenSource>, Vec<ResourceListing>)>,
    /// Counts changes of the catalog and the Open catalogue, for [`Self::all_cache`].
    generation: u64,
    /// The last "All" list ([`Self::all_listings`]): the Tab asks for it every frame, and
    /// filtering and sorting a few thousand rows took most of a frame's budget (#183).
    all_cache: std::sync::Mutex<Option<(AllKey, Vec<AllRow>)>>,
}

/// What an "All" list was made for: the search (trimmed, lower case), language, type and
/// licence filters, and [`GetBibles::generation`].
type AllKey = (String, Option<String>, ResourceFilter, LicenceFilter, u64);

/// A row of the "All" list: an Open catalogue row (true) or a catalog listing, by index.
type AllRow = (bool, usize);

/// The compiled-in Open catalogue's rows.
fn default_open_listings() -> &'static [ResourceListing] {
    static ROWS: std::sync::OnceLock<Vec<ResourceListing>> = std::sync::OnceLock::new();
    ROWS.get_or_init(|| {
        crate::open_catalogue::open_sources()
            .iter()
            .map(OpenSource::listing)
            .collect()
    })
}

impl GetBibles {
    pub fn catalog(&self) -> Option<&ResourceCatalog> {
        self.catalog.as_ref()
    }

    pub fn filter(&self) -> ResourceFilter {
        self.filter
    }

    pub fn set_filter(&mut self, filter: ResourceFilter) {
        self.filter = filter;
    }

    /// The Resource type listed (Bible for Hebrew & Greek); `None` is All.
    pub fn type_filter(&self) -> Option<ResourceType> {
        self.filter.resource_type()
    }

    /// The licence filter (#134).
    pub fn licence_filter(&self) -> LicenceFilter {
        self.licence
    }

    pub fn set_licence_filter(&mut self, licence: LicenceFilter) {
        self.licence = licence;
    }

    /// The Open catalogue's entries (#134).
    pub fn open_sources(&self) -> &[OpenSource] {
        match &self.open {
            Some((sources, _)) => sources,
            None => crate::open_catalogue::open_sources(),
        }
    }

    pub fn open_source(&self, id: &str) -> Option<&OpenSource> {
        self.open_sources().iter().find(|s| s.id == id)
    }

    /// The Open catalogue's rows, in data-file order. They need no fetch: the Open catalogue
    /// lists offline.
    pub fn open_listings(&self) -> &[ResourceListing] {
        match &self.open {
            Some((_, rows)) => rows,
            None => default_open_listings(),
        }
    }

    /// Use another Open catalogue (tests: entries with `file://` URLs).
    pub fn set_open_catalogue(&mut self, sources: Vec<OpenSource>) {
        let rows = sources.iter().map(OpenSource::listing).collect();
        self.open = Some((sources, rows));
        self.generation += 1;
    }

    /// Show a freshly fetched catalog.
    pub(crate) fn set_catalog(&mut self, catalog: ResourceCatalog) {
        self.catalog = Some(catalog);
        self.generation += 1;
    }

    /// The sections above All, titled, in order: the filter's Featured sections (see
    /// [`ResourceCatalog::featured_sections`]), then the Open catalogue's rows the filter
    /// accepts. Rows the licence filter hides are left out. With All, empty Featured sections
    /// are left out; an empty Open catalogue section always is.
    pub fn sections(
        &self,
        filter: ResourceFilter,
        licence: LicenceFilter,
    ) -> Vec<(String, Vec<&ResourceListing>)> {
        let mut out = self
            .catalog
            .as_ref()
            .map(|c| c.featured_sections(filter))
            .unwrap_or_default();
        for (_, rows) in &mut out {
            rows.retain(|b| licence.accepts(b.licence_class()));
        }
        if filter == ResourceFilter::All {
            out.retain(|(_, rows)| !rows.is_empty());
        }
        let open: Vec<&ResourceListing> = self
            .open_listings()
            .iter()
            .filter(|b| filter.accepts(b) && licence.accepts(b.licence_class()))
            .collect();
        if !open.is_empty() {
            out.push((OPEN_CATALOGUE.to_string(), open));
        }
        out
    }

    /// "All": the catalog's listings and the Open catalogue's rows that pass every filter
    /// (type, search, language and licence), by title. Made again only when a filter or the
    /// catalogs changed.
    pub fn all_listings(
        &self,
        query: &str,
        language: Option<&str>,
        filter: ResourceFilter,
        licence: LicenceFilter,
    ) -> Vec<&ResourceListing> {
        let query = query.trim().to_lowercase();
        let key: AllKey = (
            query,
            language.map(str::to_string),
            filter,
            licence,
            self.generation,
        );
        let mut cache = self
            .all_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let rows = self.all_rows(&key.0, language, filter, licence);
            *cache = Some((key, rows));
        }
        let rows = cache.as_ref().map_or(&[][..], |(_, rows)| rows.as_slice());
        rows.iter()
            .filter_map(|&(open, i)| {
                if open {
                    self.open_listings().get(i)
                } else {
                    self.catalog.as_ref().and_then(|c| c.listings.get(i))
                }
            })
            .collect()
    }

    /// [`Self::all_listings`] made afresh, for a lower-case `query`.
    fn all_rows(
        &self,
        query: &str,
        language: Option<&str>,
        filter: ResourceFilter,
        licence: LicenceFilter,
    ) -> Vec<AllRow> {
        let passes = |b: &ResourceListing| {
            filter.accepts(b)
                && b.matches_lower(query)
                && language.is_none_or(|l| b.language == l)
                && licence.accepts(b.licence_class())
        };
        let catalog = self.catalog.iter().flat_map(|c| c.listings.iter());
        let mut rows: Vec<(AllRow, &ResourceListing)> = catalog
            .enumerate()
            .map(|(i, b)| ((false, i), b))
            .chain(
                self.open_listings()
                    .iter()
                    .enumerate()
                    .map(|(i, b)| ((true, i), b)),
            )
            .filter(|(_, b)| passes(b))
            .collect();
        rows.sort_by_cached_key(|(_, b)| (b.title.to_lowercase(), b.module.clone()));
        rows.into_iter().map(|(row, _)| row).collect()
    }

    /// Language codes among the listings the type and licence filters accept, the Open
    /// catalogue's included, most listings first, for the language filter.
    pub fn languages(
        &self,
        filter: ResourceFilter,
        licence: LicenceFilter,
    ) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        let catalog = self.catalog.iter().flat_map(|c| c.listings.iter());
        for b in catalog
            .chain(self.open_listings())
            .filter(|b| filter.accepts(b) && licence.accepts(b.licence_class()))
        {
            *counts.entry(b.language.as_str()).or_default() += 1;
        }
        let mut out: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(l, n)| (l.to_string(), n))
            .collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        out
    }

    /// Set when the last refresh failed; the Tab shows the cached catalog with this message.
    pub fn offline(&self) -> Option<&str> {
        self.offline.as_deref()
    }

    pub fn is_refreshing(&self) -> bool {
        self.refreshing.is_some()
    }

    /// No catalog yet, the last refresh failed, the catalog is older than
    /// [`REFRESH_AFTER_SECS`], or it was cached by an older version ([`CATALOG_VERSION`]).
    pub fn needs_refresh(&self, now: u64) -> bool {
        self.refreshing.is_none()
            && (self.offline.is_some()
                || self.catalog.as_ref().is_none_or(|c| {
                    c.version < CATALOG_VERSION
                        || now.saturating_sub(c.fetched_at) > REFRESH_AFTER_SECS
                }))
    }

    /// Where the catalog is cached. The app sets [`default_catalog_cache`] at startup;
    /// tests use a temp file. Loads the cache if there is one.
    pub fn set_cache_path(&mut self, path: PathBuf) {
        self.cache_path = Some(path);
        self.load_cache();
    }

    /// The app-managed SWORD folder downloads install into. Defaults to
    /// [`default_sword_dir`].
    pub fn set_sword_dir(&mut self, dir: PathBuf) {
        self.sword_dir = Some(dir);
    }

    pub fn sword_dir(&self) -> Option<PathBuf> {
        self.sword_dir.clone().or_else(default_sword_dir)
    }

    fn load_cache(&mut self) {
        let Some(path) = &self.cache_path else {
            return;
        };
        if let Ok(text) = std::fs::read_to_string(path)
            && let Ok(catalog) = serde_json::from_str::<ResourceCatalog>(&text)
        {
            self.set_catalog(catalog);
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

/// The app-managed SWORD folder: `$OBS_SWORD_DIR`, else `sword` in the app's data folder.
pub fn default_sword_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OBS_SWORD_DIR") {
        return Some(PathBuf::from(p));
    }
    Some(crate::library::data_dir()?.join("sword"))
}

/// Delete a downloaded module's SWORD copy (data folder and conf) from `sword_dir`.
pub(crate) fn remove_sword_copy(sword_dir: &std::path::Path, module: &str) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        obs_formats::sword_repo::uninstall(sword_dir, module).map_err(|e| e.to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (sword_dir, module);
        Err("Downloads require desktop".into())
    }
}

/// What a listing's row offers: Download, progress, Open, or Retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListingStatus {
    Available,
    /// Downloading or importing; `phase` as on the Import report.
    InProgress {
        resource_id: String,
        phase: crate::ImportPhase,
        done: u64,
        total: u64,
    },
    Installed {
        resource_id: String,
    },
    /// In the Library, and its repository offers a newer `Version` of the module (#136).
    UpdateAvailable {
        resource_id: String,
        /// The version imported (empty when its conf had none).
        installed: String,
        /// The version the repository offers.
        available: String,
    },
    Failed {
        resource_id: String,
        error: String,
    },
}

/// A downloaded Resource whose repository offers a newer version of its module (#136).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceUpdate {
    pub resource_id: String,
    pub module: String,
    pub repo: String,
    /// The version imported (empty when its conf had none).
    pub installed: String,
    pub available: String,
}

/// How far Get Resources could check the downloaded Resources for updates (#136). Only
/// [`UpdateCheck::Checked`] with nothing left [`UpdatesSummary::unchecked`] lets the Tab say
/// they are up to date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateCheck {
    /// No catalog yet: nothing was checked.
    NotChecked,
    /// The catalog is being fetched now.
    Checking,
    /// The last fetch failed (offline, repositories down); any update shown is from the
    /// saved catalog.
    Offline(String),
    /// Only the catalog saved by an earlier run, fetched at this time, is known: nothing
    /// was fetched in this Session, so any update shown is from then and no claim is made.
    Saved { at: u64 },
    /// Checked against the catalog fetched at this time (seconds since the Unix epoch).
    Checked { at: u64 },
}

/// The updates Get Resources offers, and how far that is known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatesSummary {
    pub check: UpdateCheck,
    pub updates: Vec<ResourceUpdate>,
    /// Downloaded Resources that could not be checked: their repository did not answer or
    /// no longer lists the module, or the version they were imported at is not known.
    pub unchecked: Vec<String>,
    /// Downloaded Resources being updated now: their revision in the Library is still the
    /// old one, so they are neither offered an update nor up to date until it is in.
    pub updating: Vec<String>,
    /// Downloaded Resources in the Library.
    pub downloaded: usize,
}

impl UpdatesSummary {
    /// Whether the Tab may say every downloaded Resource is up to date: checked online just
    /// now, every one of them, and none has an update.
    pub fn up_to_date(&self) -> bool {
        matches!(self.check, UpdateCheck::Checked { .. })
            && self.downloaded > 0
            && self.updates.is_empty()
            && self.unchecked.is_empty()
            && self.updating.is_empty()
    }
}

/// `available` is newer than `installed` (SWORD version order; empty counts as 0).
fn is_newer(available: Option<&str>, installed: &str) -> bool {
    obs_formats::sword_repo::compare_versions(available.unwrap_or("0"), installed)
        == std::cmp::Ordering::Greater
}

/// Whether Import record `r` is a download of `listing`.
fn imported_from(r: &crate::ImportRecord, listing: &ResourceListing) -> bool {
    match listing.source {
        ListingSource::Sword => r.origin.as_ref().is_some_and(|o| {
            o.module.eq_ignore_ascii_case(&listing.module) && o.repo == listing.repo
        }),
        ListingSource::Open => r
            .open_origin
            .as_ref()
            .is_some_and(|o| o.id == listing.module),
    }
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Session {
    /// Where `listing` stands: in the Library, downloading, failed, or available.
    pub fn listing_status(&self, listing: &ResourceListing) -> ListingStatus {
        let mut best = ListingStatus::Available;
        for r in self.imports.records() {
            if !imported_from(r, listing) {
                continue;
            }
            let origin = r.origin.as_ref();
            let id = r.resource_id.clone();
            let status = if self.imports.job_of(&id).is_some() {
                ListingStatus::InProgress {
                    resource_id: id,
                    phase: r.phase.clone(),
                    done: r.done,
                    total: r.total,
                }
            } else if r.phase == crate::ImportPhase::Complete && self.library.get(&id).is_some() {
                match origin.and_then(|o| o.version.as_deref()) {
                    Some(installed) if is_newer(listing.version.as_deref(), installed) => {
                        ListingStatus::UpdateAvailable {
                            resource_id: id,
                            installed: installed.to_string(),
                            available: listing.version.clone().unwrap_or_default(),
                        }
                    }
                    _ => ListingStatus::Installed { resource_id: id },
                }
            } else if r.phase == crate::ImportPhase::Failed {
                ListingStatus::Failed {
                    resource_id: id,
                    error: r.error.clone().unwrap_or_default(),
                }
            } else {
                continue;
            };
            let rank = |s: &ListingStatus| match s {
                ListingStatus::Installed { .. } | ListingStatus::UpdateAvailable { .. } => 3,
                ListingStatus::InProgress { .. } => 2,
                ListingStatus::Failed { .. } => 1,
                ListingStatus::Available => 0,
            };
            if rank(&status) > rank(&best) {
                best = status;
            }
        }
        best
    }

    /// Download `module` from its repository and import it, with its live Import report.
    /// A failed earlier attempt is replaced (Retry). Returns the Resource id, report Tab and job.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn download_resource(&mut self, module: &str) -> Result<(String, u64, JobId), String> {
        let catalog = self
            .get_bibles
            .catalog()
            .ok_or("The Resource catalog has not been fetched yet")?;
        let listing = catalog
            .get(module)
            .cloned()
            .ok_or_else(|| format!("No repository offers {module}"))?;
        let repo = catalog
            .repo(&listing.repo)
            .cloned()
            .ok_or_else(|| format!("Repository {} is not in the catalog", listing.repo))?;
        if let Some(why) = &listing.refused {
            return Err(format!("{} cannot be imported yet: {why}", listing.title));
        }
        match self.listing_status(&listing) {
            ListingStatus::Installed { .. } | ListingStatus::UpdateAvailable { .. } => {
                return Err(format!("{} is already in your Library", listing.title));
            }
            ListingStatus::InProgress { .. } => {
                return Err(format!("{} is already downloading", listing.title));
            }
            ListingStatus::Failed { .. } | ListingStatus::Available => {}
        }
        self.drop_failed_downloads(&listing);
        let sword_dir = self
            .get_bibles
            .sword_dir()
            .ok_or("No folder for downloaded SWORD modules")?;
        self.start_import(crate::ImportSource::Repository {
            repo,
            module: listing.module,
            sword_dir,
        })
    }

    /// Retry: drop earlier failed downloads of `listing` and their report Tabs.
    #[cfg(not(target_arch = "wasm32"))]
    fn drop_failed_downloads(&mut self, listing: &ResourceListing) {
        let failed: Vec<String> = self
            .imports
            .records()
            .filter(|r| r.phase == crate::ImportPhase::Failed && imported_from(r, listing))
            .map(|r| r.resource_id.clone())
            .collect();
        for id in failed {
            self.imports.reports.remove(&id);
            let tabs: Vec<crate::TabId> = self
                .workspace
                .tabs()
                .filter(|t| t.kind == crate::TabKind::ImportReport(id.clone()))
                .map(|t| t.id)
                .collect();
            for tab in tabs {
                self.workspace.close(tab);
            }
        }
    }

    /// Download Open catalogue entry `id` from its publisher, check it, and import it, with
    /// its live Import report (#134). A failed earlier attempt is replaced (Retry). Returns the
    /// Resource id, report Tab and job.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn download_open(&mut self, id: &str) -> Result<(String, u64, JobId), String> {
        let source = self
            .get_bibles
            .open_source(id)
            .cloned()
            .ok_or_else(|| format!("The Open catalogue has no entry {id:?}"))?;
        let listing = source.listing();
        match self.listing_status(&listing) {
            ListingStatus::Installed { .. } | ListingStatus::UpdateAvailable { .. } => {
                return Err(format!("{} is already in your Library", listing.title));
            }
            ListingStatus::InProgress { .. } => {
                return Err(format!("{} is already downloading", listing.title));
            }
            ListingStatus::Failed { .. } | ListingStatus::Available => {}
        }
        self.drop_failed_downloads(&listing);
        self.start_import(crate::ImportSource::Open(Box::new(source)))
    }

    /// The downloaded Resources in the Library: (Resource id, origin of the revision in the
    /// Library). A Resource being updated counts with the revision it is replacing.
    fn downloaded(&self) -> Vec<(String, crate::import::RepoOrigin)> {
        self.imports
            .records()
            .map(|r| self.imports.replaced(&r.resource_id).unwrap_or(r))
            .filter(|r| {
                r.phase == crate::ImportPhase::Complete
                    && self.library.get(&r.resource_id).is_some()
            })
            .filter_map(|r| Some((r.resource_id.clone(), r.origin.clone()?)))
            .collect()
    }

    /// The SWORD copy Resource `resource_id` was imported from, when it was downloaded and
    /// the copy is still in the SWORD folder: (SWORD folder, module). `None` when another
    /// Resource in the Library was downloaded from the same module.
    pub fn sword_copy_of(&self, resource_id: &str) -> Option<(PathBuf, String)> {
        let origin = self.imports.report(resource_id)?.origin.clone()?;
        let shared = self
            .downloaded()
            .iter()
            .any(|(id, o)| id != resource_id && o.module.eq_ignore_ascii_case(&origin.module));
        if shared {
            return None;
        }
        let dir = self.get_bibles.sword_dir()?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let modules = obs_formats::SwordSource::open(&dir).ok()?.modules();
            let m = modules
                .into_iter()
                .find(|m| m.name.eq_ignore_ascii_case(&origin.module))?;
            Some((dir, m.name))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = dir;
            None
        }
    }

    /// Every downloaded Resource whose repository offers a newer version, and how far the
    /// check could go (#136). Checked against the catalog as last fetched; nothing is fetched
    /// here. Starter Resources are not downloaded, so they are never listed.
    pub fn resource_updates(&self) -> UpdatesSummary {
        let gb = &self.get_bibles;
        let check = match (gb.catalog(), gb.offline()) {
            _ if gb.is_refreshing() => UpdateCheck::Checking,
            (_, Some(why)) => UpdateCheck::Offline(why.to_string()),
            (None, None) => UpdateCheck::NotChecked,
            (Some(c), None) if !gb.checked_this_session => UpdateCheck::Saved { at: c.fetched_at },
            (Some(c), None) => UpdateCheck::Checked { at: c.fetched_at },
        };
        let downloaded = self.downloaded();
        let mut updates = Vec::new();
        let mut unchecked = Vec::new();
        let mut updating = Vec::new();
        for (id, origin) in &downloaded {
            if self.imports.is_update(id) {
                updating.push(id.clone());
                continue;
            }
            let listing = gb.catalog().and_then(|c| {
                let l = c.get(&origin.module)?;
                let repo_failed = c.failed.iter().any(|(r, _)| *r == origin.repo);
                (l.repo == origin.repo && !repo_failed).then_some(l)
            });
            match (listing, origin.version.as_deref()) {
                (Some(l), Some(installed)) => {
                    if is_newer(l.version.as_deref(), installed) {
                        updates.push(ResourceUpdate {
                            resource_id: id.clone(),
                            module: l.module.clone(),
                            repo: l.repo.clone(),
                            installed: installed.to_string(),
                            available: l.version.clone().unwrap_or_default(),
                        });
                    }
                }
                _ => unchecked.push(id.clone()),
            }
        }
        UpdatesSummary {
            check,
            updates,
            unchecked,
            updating,
            downloaded: downloaded.len(),
        }
    }

    /// Download the newer version of `module` and import it as the next Resource revision
    /// of the Resource it was downloaded as (#136). Only when the user asks: nothing is ever
    /// updated in the background. Returns the Resource id and the job.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn update_resource(&mut self, module: &str) -> Result<(String, JobId), String> {
        let catalog = self
            .get_bibles
            .catalog()
            .ok_or("The Resource catalog has not been fetched yet")?;
        let listing = catalog
            .get(module)
            .cloned()
            .ok_or_else(|| format!("No repository offers {module}"))?;
        let repo = catalog
            .repo(&listing.repo)
            .cloned()
            .ok_or_else(|| format!("Repository {} is not in the catalog", listing.repo))?;
        let ListingStatus::UpdateAvailable { resource_id, .. } = self.listing_status(&listing)
        else {
            return Err(format!("{} has no update", listing.title));
        };
        let sword_dir = self
            .get_bibles
            .sword_dir()
            .ok_or("No folder for downloaded SWORD modules")?;
        let job = self.start_update(&resource_id, repo, sword_dir)?;
        Ok((resource_id, job))
    }

    /// Fill in the imported version of Resources downloaded before #136 from their SWORD
    /// copies, so they can be checked for updates. A Resource whose SWORD copy is gone stays
    /// unknown (never "up to date").
    pub fn fill_installed_versions(&mut self) {
        let missing = self.imports.records().any(|r| {
            r.phase == crate::ImportPhase::Complete
                && r.origin.as_ref().is_some_and(|o| o.version.is_none())
        });
        if !missing {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(modules) = self
                .get_bibles
                .sword_dir()
                .and_then(|d| obs_formats::SwordSource::open(&d).ok())
                .map(|s| s.modules())
            else {
                return;
            };
            let mut filled = Vec::new();
            for r in self.imports.reports.values_mut() {
                if r.phase != crate::ImportPhase::Complete {
                    continue;
                }
                let Some(origin) = r.origin.as_mut().filter(|o| o.version.is_none()) else {
                    continue;
                };
                if let Some(m) = modules
                    .iter()
                    .find(|m| m.name.eq_ignore_ascii_case(&origin.module))
                {
                    origin.version = Some(m.version.clone().unwrap_or_default());
                    filled.push(r.resource_id.clone());
                }
            }
            for id in filled {
                self.save_import_record(&id);
            }
        }
    }

    /// Start a catalog refresh unless one is running. Returns the job.
    pub fn refresh_resource_catalog(&mut self) -> JobId {
        if let Some(job) = self.get_bibles.refreshing {
            return job;
        }
        let fetch = self.fetch();
        let job = self.jobs().submit_fn("Get Resources catalog", move |ctx| {
            let installer = crate::sword_installer(fetch.clone(), ctx);
            let cancelled = || ctx.is_cancelled();
            fetch_catalog(&installer, fetch.as_ref(), &cancelled, now_secs())
        });
        self.get_bibles.refreshing = Some(job);
        job
    }

    /// Apply Get Resources catalog job events; return the rest.
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
                    if let Ok(catalog) = out.downcast::<ResourceCatalog>() {
                        self.get_bibles.set_catalog(*catalog);
                        self.get_bibles.offline = None;
                        self.get_bibles.checked_this_session = true;
                        self.fill_installed_versions();
                        if let Err(e) = self.get_bibles.save_cache() {
                            self.workspace.push_effect(crate::Effect::Message(format!(
                                "Could not cache the Resource catalog: {e}"
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

const MODULE: ParamSpec = ParamSpec {
    name: "module",
    kind: ParamKind::String,
    required: true,
    help: "SWORD module name from the catalog",
};
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
    help: "Only Resources in this language (BCP 47 code)",
};
/// The licence filter, for `getbibles.list` and `library.get_resources` (#134).
pub(crate) const LICENCE: ParamSpec = ParamSpec {
    name: "licence",
    kind: ParamKind::String,
    required: false,
    help: "free-to-share, personal-use or all",
};
const OPEN_ID: ParamSpec = ParamSpec {
    name: "id",
    kind: ParamKind::String,
    required: true,
    help: "Open catalogue entry id",
};
/// The type filter, for `getbibles.list` and `library.get_resources`.
pub(crate) const TYPE: ParamSpec = ParamSpec {
    name: "type",
    kind: ParamKind::String,
    required: false,
    help: "bibles, hebrew-greek, lexicons, commentaries, dictionaries or all",
};

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "getbibles.refresh",
        label: "Refresh Resource catalog",
        shortcut: None,
        params: &[],
        enabled: crate::command::always,
        run: |s, _| {
            let job = s.refresh_resource_catalog();
            Ok(json!({"job": job.0}))
        },
    },
    CommandSpec {
        id: "getbibles.download",
        label: "Download Resource",
        shortcut: None,
        params: &[MODULE],
        enabled: desktop,
        run: download,
    },
    CommandSpec {
        id: "getbibles.download_open",
        label: "Download from the Open catalogue",
        shortcut: None,
        params: &[OPEN_ID],
        enabled: desktop,
        run: download_open,
    },
    CommandSpec {
        id: "getbibles.update",
        label: "Update Resource",
        shortcut: None,
        params: &[MODULE],
        enabled: desktop,
        run: update,
    },
    CommandSpec {
        id: "getbibles.update_all",
        label: "Update all Resources",
        shortcut: None,
        params: &[],
        enabled: desktop,
        run: update_all,
    },
    CommandSpec {
        id: "getbibles.list",
        label: "List available Resources",
        shortcut: None,
        params: &[QUERY, LANGUAGE, TYPE, LICENCE],
        enabled: crate::command::always,
        run: list,
    },
];

fn desktop(_: &Session) -> bool {
    cfg!(not(target_arch = "wasm32"))
}

#[cfg(target_arch = "wasm32")]
fn download(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(CommandError::Failed {
        command: "getbibles.download",
        message: "Downloads require desktop".into(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn download(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let module = p.str("module").unwrap_or_default();
    let (id, tab, job) = s
        .download_resource(module)
        .map_err(|message| CommandError::Failed {
            command: "getbibles.download",
            message,
        })?;
    Ok(json!({"id": id, "tab": tab, "job": job.0}))
}

#[cfg(target_arch = "wasm32")]
fn download_open(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(CommandError::Failed {
        command: "getbibles.download_open",
        message: "Downloads require desktop".into(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn download_open(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let id = p.str("id").unwrap_or_default();
    let (rid, tab, job) = s
        .download_open(id)
        .map_err(|message| CommandError::Failed {
            command: "getbibles.download_open",
            message,
        })?;
    Ok(json!({"id": rid, "tab": tab, "job": job.0}))
}

#[cfg(target_arch = "wasm32")]
fn update(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(CommandError::Failed {
        command: "getbibles.update",
        message: "Downloads require desktop".into(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn update(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let module = p.str("module").unwrap_or_default();
    let (id, job) = s
        .update_resource(module)
        .map_err(|message| CommandError::Failed {
            command: "getbibles.update",
            message,
        })?;
    Ok(json!({"id": id, "job": job.0}))
}

#[cfg(target_arch = "wasm32")]
fn update_all(_: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    Err(CommandError::Failed {
        command: "getbibles.update_all",
        message: "Downloads require desktop".into(),
    })
}

/// Update every downloaded Resource with an update, one job each. Returns the Resources
/// started; one that cannot start is reported and does not stop the others.
#[cfg(not(target_arch = "wasm32"))]
fn update_all(s: &mut Session, _: Params<'_>) -> Result<Value, CommandError> {
    let mut started = Vec::new();
    let mut failed = Vec::new();
    for u in s.resource_updates().updates {
        if s.imports.job_of(&u.resource_id).is_some() {
            continue;
        }
        match s.update_resource(&u.module) {
            Ok((id, _)) => started.push(id),
            Err(e) => failed.push(json!({"id": u.resource_id, "error": e})),
        }
    }
    Ok(json!({"started": started, "failed": failed}))
}

/// The listings, filtered by `query`, `language`, `type` (all types when not given) and
/// `licence` (all licences when not given). `featured` holds the rows of the filter's Featured
/// sections, in order (Bibles, then Hebrew & Greek, then the other types); `open` the Open
/// catalogue's rows (#134); `sections` names them all. Every row gives its `licence_class`.
fn list(s: &mut Session, p: Params<'_>) -> Result<Value, CommandError> {
    let failed = |message| CommandError::Failed {
        command: "getbibles.list",
        message,
    };
    let filter = type_filter_from_id(p.str("type").unwrap_or_default()).map_err(failed)?;
    let licence = LicenceFilter::from_id(p.str("licence").unwrap_or_default()).map_err(failed)?;
    let summary = s.resource_updates();
    let updates = json!({
        "available": summary.updates.iter().map(|u| json!({
            "id": u.resource_id, "module": u.module, "installed": u.installed,
            "available": u.available})).collect::<Vec<_>>(),
        "unchecked": summary.unchecked,
        "updating": summary.updating,
        "checked_at": match summary.check {
            UpdateCheck::Checked { at } => Some(at),
            _ => None,
        },
        "up_to_date": summary.up_to_date(),
    });
    let row = |b: &ResourceListing| {
        let update = match s.listing_status(b) {
            ListingStatus::UpdateAvailable {
                installed,
                available,
                ..
            } => Some(json!({"installed": installed, "available": available})),
            _ => None,
        };
        let source = match b.source {
            ListingSource::Sword => "sword",
            ListingSource::Open => "open",
        };
        json!({"module": b.module, "type": crate::library_search::type_id(b.kind),
               "repo": b.repo, "title": b.title, "abbreviation": b.abbreviation,
               "language": b.language, "size": b.size, "licence": b.licence,
               "licence_class": b.licence_class(), "source": source,
               "evidence_url": b.evidence_url,
               "version": b.version, "tags": b.tags(), "used_by": b.used_by(),
               "refused": b.refused, "update": update})
    };
    let gb = &s.get_bibles;
    let query = p.str("query").unwrap_or_default();
    let language = p.str("language").filter(|l| !l.is_empty());
    let sections = gb.sections(filter, licence);
    let rows_of = |open: bool| -> Vec<Value> {
        sections
            .iter()
            .filter(|(title, _)| (title == OPEN_CATALOGUE) == open)
            .flat_map(|(_, rows)| rows.iter().map(|b| row(b)))
            .collect()
    };
    let section_rows: Vec<Value> = sections
        .iter()
        .map(|(title, rows)| {
            json!({"title": title, "modules": rows.iter().map(|b| &b.module).collect::<Vec<_>>()})
        })
        .collect();
    Ok(json!({
        "fetched_at": gb.catalog().map(|c| c.fetched_at),
        "offline": gb.offline(),
        "updates": updates,
        "featured": rows_of(false),
        "open": rows_of(true),
        "sections": section_rows,
        "all": gb.all_listings(query, language, filter, licence)
            .into_iter()
            .map(row)
            .collect::<Vec<_>>(),
    }))
}
