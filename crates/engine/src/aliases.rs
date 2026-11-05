//! People & places ↔ Dictionary headword aliases (#54; decision #20, "derived indexes").
//!
//! The Passage Guide's People & places section links each person or place of a People &
//! places index (TIPNR) to the Dictionary articles about them. Which article that is comes
//! from an [`AliasTable`]: one per (index, Dictionary) pair, mapping entity ids to headwords.
//! It is a **derived cache**, like a Search index: keyed by both Resources' content hashes,
//! [`ALIAS_VERSION`] and the overrides it applied, rebuilt whenever any of them changes,
//! and never user data.
//!
//! Matching, in order: an override from `starter/aliases.toml` (compiled in); then each of
//! the entity's names (its usual name, then its other names) against the Dictionary's
//! headwords, their parts (`Pilate, Pontius` → `Pilate`; `Nebuchadnezzar, Or Nebuchadrezzar`
//! → both) and their aliases (`Abram` → `Abraham`), first as written and then as a plural or
//! singular (`Kenite` → `Kenites`). Headwords compare after [`obs_model::normalize`].
//!
//! Where the caches live: the app's cache folder (`aliases/` beside `search/`), and prebuilt
//! for the Starter library in `aliases/` beside its `.obr` files (`cargo xtask starter`).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use obs_model::{Entity, Key, KeyEntry, Manifest};
use serde::{Deserialize, Serialize};

/// Bump when matching or the file format changes: every cache is then rebuilt.
pub const ALIAS_VERSION: u32 = 1;

/// The committed overrides (`starter/aliases.toml`), applied to every table built.
pub const STARTER_OVERRIDES: &str = include_str!("../../../starter/aliases.toml");

/// Folder of prebuilt caches beside the `.obr` files (and the name of the app's cache folder).
pub const ALIAS_DIR: &str = "aliases";

/// Extension of an alias cache file.
pub const ALIAS_EXTENSION: &str = "aliases.json";

/// One Resource a table was built from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Side {
    pub resource_id: String,
    pub content_hash: String,
}

impl Side {
    fn of(m: &Manifest) -> Self {
        Self {
            resource_id: m.resource_id.clone(),
            content_hash: m.content_hash.clone(),
        }
    }
}

/// Which article of one Dictionary each entity of one People & places index links to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasTable {
    pub version: u32,
    pub index: Side,
    pub dictionary: Side,
    /// Digest of the overrides applied ([`Overrides::digest`]).
    pub overrides: String,
    /// Entity id → the headword of its article, as the Dictionary spells it.
    pub links: BTreeMap<String, String>,
}

impl AliasTable {
    /// The headword of the article `entity` links to.
    pub fn headword(&self, entity: &str) -> Option<&str> {
        self.links.get(entity).map(String::as_str)
    }

    /// Whether the table is current for these Resources and overrides.
    pub fn is_for(&self, index: &Manifest, dictionary: &Manifest, overrides: &Overrides) -> bool {
        self.version == ALIAS_VERSION
            && self.index == Side::of(index)
            && self.dictionary == Side::of(dictionary)
            && self.overrides == overrides.digest
    }

    pub fn file_name(&self) -> String {
        name_for(
            &self.index.resource_id,
            &self.index.content_hash,
            &self.dictionary.resource_id,
            &self.dictionary.content_hash,
        )
    }

    /// Deterministic JSON (sorted keys), so a Starter build is byte-identical.
    pub fn to_json(&self) -> String {
        let mut s = serde_json::to_string_pretty(self).unwrap_or_default();
        s.push('\n');
        s
    }

    pub fn from_json(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}

/// The cache file name of a pair: `<index>.<hash16>--<dictionary>.<hash16>.v<N>.aliases.json`.
/// Ids that are not `[a-z0-9-]{1,64}` become `_` and a hash, so a name never leaves its
/// folder.
pub fn file_name(index: &Manifest, dictionary: &Manifest) -> String {
    name_for(
        &index.resource_id,
        &index.content_hash,
        &dictionary.resource_id,
        &dictionary.content_hash,
    )
}

fn name_for(index: &str, index_hash: &str, dictionary: &str, dictionary_hash: &str) -> String {
    format!(
        "{}.{}--{}.{}.v{ALIAS_VERSION}.{ALIAS_EXTENSION}",
        stem(index),
        hash16(index_hash),
        stem(dictionary),
        hash16(dictionary_hash)
    )
}

fn hash16(content_hash: &str) -> String {
    let h: String = content_hash
        .trim_start_matches("sha256:")
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(16)
        .collect();
    if h.is_empty() { "0".into() } else { h }
}

fn stem(resource_id: &str) -> String {
    let plain = (1..=64).contains(&resource_id.len())
        && resource_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if plain {
        resource_id.to_string()
    } else {
        format!("_{:016x}", fnv(resource_id))
    }
}

/// FNV-1a: stable across Rust releases, unlike `DefaultHasher`.
fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// One override: the article `entity` links to in `dictionary` (`None`: no article).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Override {
    pub dictionary: String,
    pub entity: String,
    pub headword: Option<String>,
}

/// Overrides of the name matching, from `starter/aliases.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overrides {
    pub entries: Vec<Override>,
    /// Identifies the overrides in a cache, so changing them rebuilds it.
    pub digest: String,
}

#[derive(Deserialize)]
struct OverridesFile {
    #[serde(default)]
    alias: Vec<OverrideRow>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverrideRow {
    dictionary: String,
    entity: String,
    headword: String,
}

impl Overrides {
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: OverridesFile = toml::from_str(text).map_err(|e| format!("aliases.toml: {e}"))?;
        let entries = file
            .alias
            .into_iter()
            .map(|r| Override {
                dictionary: r.dictionary,
                entity: r.entity,
                headword: Some(r.headword).filter(|h| !h.trim().is_empty()),
            })
            .collect();
        Ok(Self {
            entries,
            digest: format!("{:016x}", fnv(text)),
        })
    }

    /// The committed Starter overrides. They are checked by tests and by the Starter build;
    /// should they ever fail to parse, matching goes on without them.
    pub fn starter() -> Self {
        Self::parse(STARTER_OVERRIDES).unwrap_or_else(|_| Self::none())
    }

    pub fn none() -> Self {
        Self {
            entries: Vec::new(),
            digest: "none".into(),
        }
    }

    fn for_dictionary<'a>(&'a self, dictionary: &'a str) -> impl Iterator<Item = &'a Override> {
        self.entries
            .iter()
            .filter(move |o| o.dictionary == dictionary)
    }
}

/// A byte range inside an entry's unit.
type Span = (u32, u32);

/// How well a lookup form matches a headword (lower is better).
const WHOLE: u8 = 0;
const FIRST_PART: u8 = 1;
const ALIAS: u8 = 1;
const OTHER_PART: u8 = 2;

/// A Dictionary's headwords by every form a name may match.
struct Headwords {
    forms: HashMap<String, (u8, String)>,
    /// Normalized headword → as written, for overrides.
    exact: HashMap<String, String>,
}

impl Headwords {
    fn new(keys: &[KeyEntry]) -> Self {
        // Sorted, so ties go the same way whatever order the keys come in (a Starter build
        // reads them from the Resource, the app from the `.obr`).
        let mut keys = keys.to_vec();
        keys.sort();
        let mut forms: HashMap<String, (u8, String)> = HashMap::new();
        let mut exact = HashMap::new();
        let mut put = |form: &str, rank: u8, headword: &str| {
            let form = obs_model::normalize(form);
            if form.is_empty() {
                return;
            }
            let better = forms.get(&form).is_none_or(|(r, _)| rank < *r);
            if better {
                forms.insert(form, (rank, headword.to_string()));
            }
        };
        // The headword of each article (unit, range), for its aliases.
        let mut articles: HashMap<(&str, Option<Span>), &str> = HashMap::new();
        for k in keys.iter().filter(|k| !k.alias) {
            let Key::Headword(h) = &k.key else { continue };
            articles.entry((k.unit.as_str(), k.range)).or_insert(h);
            exact
                .entry(obs_model::normalize(h))
                .or_insert_with(|| h.clone());
            put(h, WHOLE, h);
            let mut parts = h.split(',').map(str::trim);
            if let Some(first) = parts.next() {
                put(first, FIRST_PART, h);
            }
            for part in parts {
                let part = strip_prefix_word(part, "or");
                let part = strip_prefix_word(part, "the");
                put(part, OTHER_PART, h);
            }
        }
        for k in keys.iter().filter(|k| k.alias) {
            let Key::Headword(alias) = &k.key else {
                continue;
            };
            if let Some(h) = articles.get(&(k.unit.as_str(), k.range)) {
                put(alias, ALIAS, h);
            }
        }
        Self { forms, exact }
    }

    /// The best article for any of `names`: the first name that matches wins; within one
    /// name, the closest form, then a plural or singular.
    fn find(&self, names: &[String]) -> Option<&str> {
        for name in names {
            let norm = obs_model::normalize(name);
            if norm.is_empty() {
                continue;
            }
            let mut variants = vec![norm.clone()];
            match norm.strip_suffix('s') {
                Some(singular) if !singular.is_empty() => variants.push(singular.to_string()),
                _ => variants.push(format!("{norm}s")),
            }
            for v in variants {
                if let Some((_, h)) = self.forms.get(&v) {
                    return Some(h);
                }
            }
        }
        None
    }
}

fn strip_prefix_word<'a>(s: &'a str, word: &str) -> &'a str {
    match s.split_once(' ') {
        Some((first, rest)) if first.eq_ignore_ascii_case(word) => rest.trim(),
        _ => s,
    }
}

/// The names an entity may be called, best first: its usual name (TIPNR writes spaces as
/// `_`), then its other names.
pub fn entity_names(e: &Entity) -> Vec<String> {
    let mut names = vec![e.name.replace('_', " ")];
    for n in &e.other_names {
        if !names.contains(n) {
            names.push(n.clone());
        }
    }
    names
}

/// Build the table of one (index, Dictionary) pair. Also returns the overrides for this
/// Dictionary that name an entity or headword these Resources do not have.
pub fn build(
    index: &Manifest,
    entities: &[Entity],
    dictionary: &Manifest,
    headword_keys: &[KeyEntry],
    overrides: &Overrides,
) -> (AliasTable, Vec<String>) {
    let headwords = Headwords::new(headword_keys);
    let mut links = BTreeMap::new();
    for e in entities {
        if let Some(h) = headwords.find(&entity_names(e)) {
            links.insert(e.id.clone(), h.to_string());
        }
    }
    let mut problems = Vec::new();
    for o in overrides.for_dictionary(&dictionary.resource_id) {
        if !entities.iter().any(|e| e.id == o.entity) {
            problems.push(format!(
                "{}: no entity `{}` in {}",
                o.dictionary, o.entity, index.resource_id
            ));
            continue;
        }
        match &o.headword {
            None => {
                links.remove(&o.entity);
            }
            Some(h) => match headwords.exact.get(&obs_model::normalize(h)) {
                Some(written) => {
                    links.insert(o.entity.clone(), written.clone());
                }
                None => problems.push(format!(
                    "{}: no headword `{h}` for `{}`",
                    o.dictionary, o.entity
                )),
            },
        }
    }
    let table = AliasTable {
        version: ALIAS_VERSION,
        index: Side::of(index),
        dictionary: Side::of(dictionary),
        overrides: overrides.digest.clone(),
        links,
    };
    (table, problems)
}

/// The platform cache folder for alias caches: `$OBS_ALIAS_CACHE`, else `aliases` beside the
/// Search index cache folder ([`crate::default_search_cache_dir`]).
pub fn default_alias_cache_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("OBS_ALIAS_CACHE") {
        return Some(PathBuf::from(d));
    }
    Some(crate::default_search_cache_dir()?.with_file_name(ALIAS_DIR))
}

/// The Session's alias tables: in memory, in the cache folder, prebuilt beside the `.obr`
/// files, or built on demand. Shared by the People & places section's jobs.
#[derive(Debug)]
pub struct AliasCache {
    dir: Mutex<Option<PathBuf>>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    tables: Mutex<BTreeMap<String, Arc<AliasTable>>>,
    overrides: Overrides,
}

impl Default for AliasCache {
    fn default() -> Self {
        Self::new(Overrides::starter())
    }
}

impl AliasCache {
    pub fn new(overrides: Overrides) -> Self {
        Self {
            dir: Mutex::new(None),
            tables: Mutex::new(BTreeMap::new()),
            overrides,
        }
    }

    /// Where built tables are written (without one they are kept in memory only).
    pub fn set_dir(&self, dir: PathBuf) {
        *self.dir.lock().unwrap_or_else(|p| p.into_inner()) = Some(dir);
    }

    pub fn dir(&self) -> Option<PathBuf> {
        self.dir.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    pub fn overrides(&self) -> &Overrides {
        &self.overrides
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn remembered(&self, name: &str) -> Option<Arc<AliasTable>> {
        self.tables
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(name)
            .cloned()
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn remember(&self, table: Arc<AliasTable>) {
        let mut tables = self.tables.lock().unwrap_or_else(|p| p.into_inner());
        // An older table of the same pair (either side changed) is stale.
        tables.retain(|_, t| {
            !(t.index.resource_id == table.index.resource_id
                && t.dictionary.resource_id == table.dictionary.resource_id)
        });
        tables.insert(table.file_name(), table);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl AliasCache {
    /// The table for one (People & places index, Dictionary) pair. Looks in memory, then the
    /// cache folder, then `aliases/` beside either `.obr`; builds (and saves) it when none is
    /// current. A cache-folder file that is not current is deleted.
    pub fn table(
        &self,
        index: &crate::LibraryEntry,
        dictionary: &crate::LibraryEntry,
    ) -> Result<Arc<AliasTable>, String> {
        let (im, dm) = (&index.manifest, &dictionary.manifest);
        let name = file_name(im, dm);
        if let Some(t) = self
            .remembered(&name)
            .filter(|t| t.is_for(im, dm, &self.overrides))
        {
            return Ok(t);
        }
        let dir = self.dir();
        let mut folders: Vec<(PathBuf, bool)> = Vec::new();
        if let Some(d) = &dir {
            folders.push((d.clone(), true));
        }
        for obr in [&index.path, &dictionary.path] {
            if let Some(p) = obr.parent() {
                let beside = p.join(ALIAS_DIR);
                if !folders.iter().any(|(f, _)| *f == beside) {
                    folders.push((beside, false));
                }
            }
        }
        for (folder, ours) in &folders {
            let path = folder.join(&name);
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            match AliasTable::from_json(&text).filter(|t| t.is_for(im, dm, &self.overrides)) {
                Some(t) => {
                    let t = Arc::new(t);
                    self.remember(Arc::clone(&t));
                    return Ok(t);
                }
                None if *ours => {
                    let _ = std::fs::remove_file(&path);
                }
                None => {}
            }
        }
        let t = Arc::new(self.build_table(index, dictionary)?);
        if let Some(d) = &dir {
            write_table(d, &t).map_err(|e| format!("alias cache {}: {e}", d.display()))?;
        }
        self.remember(Arc::clone(&t));
        Ok(t)
    }

    fn build_table(
        &self,
        index: &crate::LibraryEntry,
        dictionary: &crate::LibraryEntry,
    ) -> Result<AliasTable, String> {
        use obs_store::ResourceStore;
        let read = |e: &crate::LibraryEntry| {
            obs_store::ObrReader::open(&e.path)
                .map_err(|err| format!("{}: {err}", e.manifest.abbreviation))
        };
        let entities = read(index)?.entities().map_err(|e| e.to_string())?;
        let keys = read(dictionary)?
            .keys(obs_model::KeyScheme::Headword)
            .map_err(|e| e.to_string())?;
        let (table, _) = build(
            &index.manifest,
            &entities,
            &dictionary.manifest,
            &keys,
            &self.overrides,
        );
        Ok(table)
    }

    /// Make every pair of the library current (run at library load, on the job queue), and
    /// delete cache files in the cache folder that no pair uses any more. Returns how many
    /// pairs there are.
    pub fn refresh(&self, library: &crate::Library) -> Result<usize, String> {
        use obs_model::ResourceType;
        let mut names = Vec::new();
        let mut errors = Vec::new();
        for index in library.of_type(ResourceType::PeopleAndPlaces) {
            for dictionary in library.of_type(ResourceType::Dictionary) {
                names.push(file_name(&index.manifest, &dictionary.manifest));
                if let Err(e) = self.table(index, dictionary) {
                    errors.push(e);
                }
            }
        }
        if let Some(dir) = self.dir()
            && let Ok(list) = std::fs::read_dir(&dir)
        {
            for entry in list.flatten() {
                let file = entry.file_name().to_string_lossy().into_owned();
                if file.ends_with(ALIAS_EXTENSION) && !names.contains(&file) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        if errors.is_empty() {
            Ok(names.len())
        } else {
            Err(errors.join("; "))
        }
    }
}

/// Write a table into `dir` under its file name (through a temp file, so a reader never
/// sees half a file). Returns its path.
#[cfg(not(target_arch = "wasm32"))]
pub fn write_table(dir: &std::path::Path, table: &AliasTable) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(table.file_name());
    let temp = dir.join(format!(
        "{}.{}.partial",
        table.file_name(),
        std::process::id()
    ));
    std::fs::write(&temp, table.to_json())?;
    std::fs::rename(&temp, &path)?;
    Ok(path)
}

#[cfg(target_arch = "wasm32")]
impl AliasCache {
    pub fn table(
        &self,
        _: &crate::LibraryEntry,
        _: &crate::LibraryEntry,
    ) -> Result<Arc<AliasTable>, String> {
        Err("Opening Resources is not supported here".into())
    }

    pub fn refresh(&self, _: &crate::Library) -> Result<usize, String> {
        Err("Opening Resources is not supported here".into())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use obs_model::{EntityKind, ResourceType, UnitId};

    fn entity(id: &str, others: &[&str]) -> Entity {
        Entity {
            id: id.into(),
            name: id.split('@').next().unwrap().into(),
            kind: EntityKind::Male,
            strongs: None,
            other_names: others.iter().map(|s| s.to_string()).collect(),
            region: None,
            relations: Vec::new(),
        }
    }

    fn key(h: &str, unit: &str, alias: bool) -> KeyEntry {
        KeyEntry {
            key: Key::Headword(h.into()),
            unit: UnitId::new(unit).unwrap(),
            range: None,
            alias,
        }
    }

    fn manifests() -> (Manifest, Manifest) {
        let mut i = Manifest::new(
            "tipnr",
            "TIPNR",
            "TIPNR",
            ResourceType::PeopleAndPlaces,
            "en",
        );
        i.content_hash = "sha256:aaaa".into();
        let mut d = Manifest::new("easton", "Easton", "Easton", ResourceType::Dictionary, "en");
        d.content_hash = "sha256:bbbb".into();
        (i, d)
    }

    #[test]
    fn names_match_headwords_their_parts_aliases_and_plurals() {
        let (i, d) = manifests();
        let keys = [
            key("Abraham", "u1", false),
            key("Abram", "u1", true),
            key("Abram", "u2", false),
            key("Pilate, Pontius", "u3", false),
            key("Nebuchadnezzar, Or Nebuchadrezzar", "u4", false),
            key("Kenites", "u5", false),
            key("Amorite, The Amorites", "u6", false),
            key("Mary Magdalene", "u7", false),
        ];
        let entities = [
            entity("Abraham@Gen.11.26-1Pe", &["Abram"]),
            entity("Pilate@Mat.27.2-1Ti", &["Pontius"]),
            entity("Nebuchadrezzar@Jer.21.2-", &[]),
            entity("Kenite@Gen.15.19-", &[]),
            entity("Amorites@Gen.10.16-", &[]),
            entity("Mary_Magdalene@Mat.27.56-Jhn", &[]),
            entity("Nobody@Gen.1.1", &["Anyone"]),
        ];
        let (t, problems) = build(&i, &entities, &d, &keys, &Overrides::none());
        assert!(problems.is_empty());
        let got: Vec<(&str, &str)> = t
            .links
            .iter()
            .map(|(e, h)| (e.split('@').next().unwrap(), h.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("Abraham", "Abraham"),
                ("Amorites", "Amorite, The Amorites"),
                ("Kenite", "Kenites"),
                ("Mary_Magdalene", "Mary Magdalene"),
                ("Nebuchadrezzar", "Nebuchadnezzar, Or Nebuchadrezzar"),
                ("Pilate", "Pilate, Pontius"),
            ]
        );
        // An alias leads to its article's headword, not to a separate short entry.
        let only_abram = [entity("Abram@Gen.11.26", &[])];
        let keys = [key("Abraham", "u1", false), key("Abram", "u1", true)];
        let (t, _) = build(&i, &only_abram, &d, &keys, &Overrides::none());
        assert_eq!(t.headword("Abram@Gen.11.26"), Some("Abraham"));
    }

    #[test]
    fn overrides_pin_or_remove_an_article_and_report_what_they_cannot_find() {
        let (i, d) = manifests();
        let keys = [
            key("Herod the Great", "u1", false),
            key("Mary", "u2", false),
        ];
        let entities = [
            entity("Herod@Mat.2.1-Act", &[]),
            entity("Mary@Mat.1.16-Act", &[]),
        ];
        let overrides = Overrides::parse(
            r#"
[[alias]]
dictionary = "easton"
entity = "Herod@Mat.2.1-Act"
headword = "herod the great"

[[alias]]
dictionary = "easton"
entity = "Mary@Mat.1.16-Act"
headword = ""

[[alias]]
dictionary = "smith"
entity = "Mary@Mat.1.16-Act"
headword = "Elsewhere"

[[alias]]
dictionary = "easton"
entity = "Gone@Gen.1.1"
headword = "Mary"

[[alias]]
dictionary = "easton"
entity = "Herod@Mat.2.1-Act"
headword = "No Such Article"
"#,
        )
        .unwrap();
        let (t, problems) = build(&i, &entities, &d, &keys, &overrides);
        // The bad entry after the good one leaves the good one in place.
        assert_eq!(t.headword("Herod@Mat.2.1-Act"), Some("Herod the Great"));
        assert_eq!(t.headword("Mary@Mat.1.16-Act"), None, "removed by \"\"");
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems[0].contains("Gone@Gen.1.1"));
        assert!(problems[1].contains("No Such Article"));
        assert_eq!(t.overrides, overrides.digest);
        assert!(Overrides::parse("[[alias]]\nentity = 1").is_err());
    }

    #[test]
    fn the_committed_starter_overrides_parse() {
        let o = Overrides::parse(STARTER_OVERRIDES).unwrap();
        assert!(!o.entries.is_empty());
        assert_eq!(Overrides::starter(), o);
        assert!(o.entries.iter().all(|e| e.entity.contains('@')));
    }

    #[test]
    fn file_names_change_with_either_content_hash_and_stay_in_their_folder() {
        let (i, mut d) = manifests();
        let a = file_name(&i, &d);
        assert_eq!(a, "tipnr.aaaa--easton.bbbb.v1.aliases.json");
        d.content_hash = "sha256:cccc".into();
        assert_ne!(file_name(&i, &d), a);
        let mut odd = i.clone();
        odd.resource_id = "../x".into();
        let n = file_name(&odd, &d);
        assert!(!n.contains('/') && n.starts_with('_'), "{n}");
        let (t, _) = build(&i, &[], &d, &[], &Overrides::none());
        assert_eq!(AliasTable::from_json(&t.to_json()), Some(t.clone()));
        assert!(t.is_for(&i, &d, &Overrides::none()));
        assert!(!t.is_for(&i, &d, &Overrides::starter()));
    }
}
