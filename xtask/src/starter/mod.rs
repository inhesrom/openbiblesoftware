//! `cargo xtask starter`: build the Starter library from `starter/sources.toml` (#38, #20).
//!
//! 1. Load the lock and `starter/id-changes.toml` and run the provenance gate.
//! 2. Fetch every source into the download cache and verify its sha256.
//! 3. Run the same Import parsers and `ResourceBuilder` as Import (USFM for Bibles, the
//!    lexicon converters for Lexicons), then fill in the manifest from the lock (licence,
//!    Attribution, changes, `origin = starter`).
//! 4. Diff unit ids against the previous release manifest; a removed id fails the build unless
//!    `id-changes.toml` maps it (shipped as `unit_alias`).
//! 5. Run the structural checks over the whole library (`checks`): every failure names its
//!    item, and nothing is written until they pass.
//! 6. Write `dist/starter/<id>.obr`, the prebuilt Search index caches in
//!    `dist/starter/search/` (#58; the app uses a cache found in `search/` beside a Resource's
//!    `.obr` as long as its key matches), the People & places ↔ Dictionary alias caches in
//!    `dist/starter/aliases/` (#54, with `starter/aliases.toml` applied; an override naming an
//!    entity or headword the Resources lack fails the build), `CHANGES-<id>.md` for CC BY sources,
//!    `starter-manifest.json` and `starter-build-report.md`, and check the size budget
//!    (Resources plus caches).
//! 7. Compare the golden-passage snapshots in `starter/goldens/` (`goldens`); a difference
//!    fails the build with a unified diff, in the report and on stderr.
//!
//! Builds are deterministic: the same lock and code give byte-identical outputs.
//! `--check-determinism` builds a second time and compares every file.

mod checks;
mod fetch;
mod goldens;
mod ids;
mod lock;
mod study;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use obs_formats::{
    DatasetOptions, ImportReport, LexiconOptions, StepbibleImport, StepbibleOptions, StepbibleText,
    UsfmImport, UsfmOptions, import_abbott_smith, import_bdb, import_openbible_cross_references,
    import_strongs_greek, import_strongs_hebrew, import_tbes, import_tipnr,
    import_usfm_parallel_passages, usfm_book_code, usfm_zip_books,
};
use obs_model::{Origin, Resource, UnitId};
use obs_store::write_obr;

pub use fetch::fetch_file;
use fetch::{fetch, sha256_hex};
use goldens::{GoldenList, Status};
use ids::{IdDiff, MANIFEST_FORMAT, ReleaseManifest, ReleaseResource};
pub use lock::{Lock, PinnedFile};
use lock::{IdChanges, Source};

pub const MANIFEST_FILE: &str = "starter-manifest.json";
pub const REPORT_FILE: &str = "starter-build-report.md";
/// Where the prebuilt Search index caches go, inside the output directory.
pub const SEARCH_DIR: &str = "search";
/// Where the prebuilt People & places ↔ Dictionary alias caches go (#54).
pub const ALIAS_DIR: &str = obs_engine::aliases::ALIAS_DIR;

const USAGE: &str = "usage: cargo xtask starter [--out DIR] [--cache DIR] [--previous MANIFEST] [--check-determinism] [--update-goldens]

  --out DIR            where the outputs go (default dist/starter)
  --cache DIR          download cache (default target/starter-cache)
  --previous MANIFEST  the previous release's starter-manifest.json, for the stable-id diff
  --check-determinism  build twice and fail unless every output is byte-identical
  --update-goldens     rewrite the golden-passage snapshots in starter/goldens/ from this build
                       instead of failing on a difference (commit the result for review)";

/// Inputs and outputs of one build.
#[derive(Debug, Clone)]
pub struct Config {
    pub lock: PathBuf,
    pub id_changes: PathBuf,
    pub previous: Option<PathBuf>,
    pub out: PathBuf,
    pub cache: PathBuf,
    /// Golden-passage snapshots: `passages.toml` and one `<resource_id>/<key>.json` each.
    pub goldens: PathBuf,
    /// Rewrite differing or missing snapshots instead of failing.
    pub update_goldens: bool,
}

impl Config {
    pub fn defaults(root: &Path) -> Self {
        Self {
            lock: root.join("starter/sources.toml"),
            id_changes: root.join("starter/id-changes.toml"),
            previous: None,
            out: root.join("dist/starter"),
            cache: root.join("target/starter-cache"),
            goldens: root.join("starter/goldens"),
            update_goldens: false,
        }
    }
}

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    let mut config = Config::defaults(root);
    let mut check_determinism = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || {
            it.next()
                .map(PathBuf::from)
                .ok_or_else(|| anyhow!("`{a}` needs a value\n\n{USAGE}"))
        };
        match a.as_str() {
            "--out" => config.out = value()?,
            "--cache" => config.cache = value()?,
            "--previous" => config.previous = Some(value()?),
            "--check-determinism" => check_determinism = true,
            "--update-goldens" => config.update_goldens = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => bail!("unknown option `{other}`\n\n{USAGE}"),
        }
    }
    let summary = build(&config)?;
    eprint!("{summary}");
    if check_determinism {
        let again = Config {
            out: root.join("target/starter-recheck"),
            update_goldens: false,
            ..config.clone()
        };
        eprintln!("\nstarter: second build into {} …", again.out.display());
        build(&again)?;
        compare_dirs(&config.out, &again.out)?;
        eprintln!("starter: second build is byte-identical");
    }
    Ok(())
}

/// One Resource, built and checked, before it is written.
struct Built {
    resource: Resource,
    report: ImportReport,
    diff: IdDiff,
    /// id-changes entries skipped because their old id is live again.
    shadowed_aliases: Vec<String>,
}

/// Build the whole Starter library described by `config`. Returns a short summary.
pub fn build(config: &Config) -> Result<String> {
    let lock_file = Lock::load(&config.lock)?;
    let lock_dir = config.lock.parent().unwrap_or(Path::new("."));
    let changes = IdChanges::load(&config.id_changes)?;
    lock::check(&lock_file, lock_dir, &changes)?;
    let golden_list = GoldenList::load(&config.goldens)?;
    let previous = match &config.previous {
        Some(p) => Some(ReleaseManifest::load(p)?),
        None => None,
    };
    if let Some(prev) = &previous {
        let ids: BTreeSet<&str> = lock_file
            .sources
            .iter()
            .map(|s| s.resource_id.as_str())
            .collect();
        ids::check_dropped_resources(prev, &ids, &changes)?;
    }

    // Fetch first (sequentially, so downloads are polite and errors come in lock order).
    let mut inputs = Vec::with_capacity(lock_file.sources.len());
    for source in &lock_file.sources {
        inputs.push((source, fetch(source, &config.cache)?));
    }

    // Import every source in parallel; each build is independent and deterministic.
    let results: Vec<Result<Built>> = std::thread::scope(|scope| {
        let handles: Vec<_> = inputs
            .iter()
            .map(|(source, files)| {
                let previous = previous.as_ref();
                let changes = &changes;
                scope.spawn(move || build_one(source, files, previous, changes))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(anyhow!("a build thread panicked")))
            })
            .collect()
    });
    let mut built = Vec::with_capacity(results.len());
    let mut errors = Vec::new();
    for r in results {
        match r {
            Ok(b) => built.push(b),
            Err(e) => errors.push(format!("{e:#}")),
        }
    }
    if !errors.is_empty() {
        bail!("the Starter build failed:\n{}", errors.join("\n"));
    }

    let resources: Vec<&Resource> = built.iter().map(|b| &b.resource).collect();
    let structural = checks::check(&resources);
    if !structural.failures.is_empty() {
        bail!(
            "the Starter library failed {} structural check(s):\n{}",
            structural.failures.len(),
            list_failures(&structural.failures)
        );
    }

    clean_outputs(&config.out)?;
    let mut released = Vec::with_capacity(built.len());
    for b in &built {
        let id = &b.resource.manifest.resource_id;
        let path = config.out.join(format!("{id}.obr"));
        write_obr(&b.resource, &path)
            .with_context(|| format!("cannot write {}", path.display()))?;
        let bytes = std::fs::read(&path)?;
        let m = &b.resource.manifest;
        released.push(ReleaseResource {
            resource_id: id.clone(),
            revision: m.revision,
            content_hash: m.content_hash.clone(),
            file_sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
            title: m.title.clone(),
            abbreviation: m.abbreviation.clone(),
            resource_type: Some(m.resource_type),
            language: m.language.clone(),
            license: m.license.clone(),
            units: b
                .resource
                .units
                .iter()
                .map(|u| u.id.as_str().to_string())
                .collect(),
            unit_aliases: b
                .resource
                .unit_aliases
                .iter()
                .map(|(o, n)| (o.as_str().to_string(), n.as_str().to_string()))
                .collect(),
        });
    }
    // Prebuilt Search index caches, one per Resource, built in parallel (each is
    // deterministic on its own).
    let search_dir = config.out.join(SEARCH_DIR);
    let caches: Vec<Result<u64>> = std::thread::scope(|scope| {
        let handles: Vec<_> = built
            .iter()
            .map(|b| {
                let dir = &search_dir;
                scope.spawn(move || -> Result<u64> {
                    let id = &b.resource.manifest.resource_id;
                    let path = obs_store::build_search_index(
                        &b.resource,
                        obs_search::INDEX_VERSION,
                        dir,
                        &mut |_, _| true,
                    )
                    .map_err(|e| anyhow!("{id}: Search index: {e}"))?;
                    Ok(std::fs::metadata(&path)?.len())
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| Err(anyhow!("a Search index thread panicked")))
            })
            .collect()
    });
    let search_bytes: Vec<u64> = caches.into_iter().collect::<Result<_>>()?;
    let overrides = obs_engine::aliases::Overrides::parse(obs_engine::aliases::STARTER_OVERRIDES)
        .map_err(|e| anyhow!("starter/aliases.toml: {e}"))?;
    let alias_bytes = write_alias_caches(&resources, &config.out.join(ALIAS_DIR), &overrides)?
        .iter()
        .map(|p| std::fs::metadata(p).map(|m| m.len()))
        .sum::<std::io::Result<u64>>()?;
    for source in lock_file
        .sources
        .iter()
        .filter(|s| obs_model::is_cc_by(&s.spdx))
    {
        let path = config
            .out
            .join(obs_model::changes_record_file(&source.resource_id));
        std::fs::write(&path, changes_record(source))
            .with_context(|| format!("cannot write {}", path.display()))?;
    }
    let manifest = ReleaseManifest {
        format: MANIFEST_FORMAT,
        resources: released,
    };
    let mut json = serde_json::to_string_pretty(&manifest)?;
    json.push('\n');
    std::fs::write(config.out.join(MANIFEST_FILE), json)?;

    let snapshots = goldens::check(
        &golden_list,
        &resources,
        &config.goldens,
        config.update_goldens,
    )?;
    let budget = lock_file.size_budget_mb.saturating_mul(1_000_000);
    let report = build_report(
        &built,
        &manifest,
        previous.as_ref(),
        CacheBytes {
            search: &search_bytes,
            aliases: alias_bytes,
        },
        budget,
        &structural.coverage,
        &snapshots,
    );
    std::fs::write(config.out.join(REPORT_FILE), &report)?;
    let failed: Vec<&goldens::Checked> = snapshots.iter().filter(|s| !s.passed()).collect();
    if !failed.is_empty() {
        let mut message = format!(
            "{} golden snapshot(s) differ from the committed files in {} (review the diff, then \
             run `cargo xtask starter --update-goldens` and commit the result):",
            failed.len(),
            config.goldens.display()
        );
        for s in failed {
            let _ = write!(message, "\n- {}: {}", s.name, describe(&s.status, &s.file));
            if let Status::Changed(diff) = &s.status {
                let _ = write!(message, "\n{diff}");
            }
        }
        bail!(message);
    }
    let total = dir_bytes(&config.out)?;
    if total > budget {
        bail!(
            "the Starter library is {} MB, over the {} MB size budget (see {})",
            mb(total),
            lock_file.size_budget_mb,
            config.out.join(REPORT_FILE).display()
        );
    }
    let mut summary = String::new();
    for r in &manifest.resources {
        let _ = writeln!(
            summary,
            "starter: {:<14} r{:<3} {:>8} MB  {}",
            r.resource_id,
            r.revision,
            mb(r.bytes),
            r.content_hash
        );
    }
    let _ = writeln!(
        summary,
        "starter: {} Resource(s), {} MB of {} MB budget, written to {}",
        manifest.resources.len(),
        mb(total),
        lock_file.size_budget_mb,
        config.out.display()
    );
    Ok(summary)
}

/// Write the alias cache of every (People & places index, Dictionary) pair among `resources`
/// into `dir` (the derived index of #20 that matches TIPNR names to Dictionary headwords).
/// Fails, naming each, if an override for one of these Dictionaries names an entity or a
/// headword the pair does not have. Returns the files written.
pub fn write_alias_caches(
    resources: &[&Resource],
    dir: &Path,
    overrides: &obs_engine::aliases::Overrides,
) -> Result<Vec<PathBuf>> {
    use obs_model::{KeyScheme, ResourceType};
    let of = |ty: ResourceType| {
        resources
            .iter()
            .filter(move |r| r.manifest.resource_type == ty)
    };
    let mut written = Vec::new();
    // An override names a TIPNR entity; another index (the geocoding, #150) need not have it,
    // so only an entity no index has is a problem.
    let entities: BTreeSet<&str> = of(ResourceType::PeopleAndPlaces)
        .flat_map(|r| r.entities.iter().map(|e| e.id.as_str()))
        .collect();
    let dictionaries: BTreeSet<&str> = of(ResourceType::Dictionary)
        .map(|r| r.manifest.resource_id.as_str())
        .collect();
    let mut problems: BTreeSet<String> = overrides
        .entries
        .iter()
        .filter(|o| {
            dictionaries.contains(o.dictionary.as_str()) && !entities.contains(o.entity.as_str())
        })
        .map(|o| {
            format!(
                "{}: no entity `{}` in any People & places index",
                o.dictionary, o.entity
            )
        })
        .collect();
    for index in of(ResourceType::PeopleAndPlaces) {
        for dictionary in of(ResourceType::Dictionary) {
            let headwords: Vec<obs_model::KeyEntry> = dictionary
                .keys
                .iter()
                .filter(|k| k.key.scheme() == KeyScheme::Headword)
                .cloned()
                .collect();
            let (table, found) = obs_engine::aliases::build(
                &index.manifest,
                &index.entities,
                &dictionary.manifest,
                &headwords,
                overrides,
            );
            problems.extend(found.into_iter().filter(|p| !p.contains(": no entity `")));
            let path = obs_engine::aliases::write_table(dir, &table)
                .with_context(|| format!("cannot write an alias cache in {}", dir.display()))?;
            written.push(path);
        }
    }
    if !problems.is_empty() {
        bail!(
            "starter/aliases.toml has {} override(s) the Starter Resources cannot satisfy:\n{}",
            problems.len(),
            list_failures(&problems.into_iter().collect::<Vec<_>>())
        );
    }
    Ok(written)
}

/// The published change record of a CC BY source (`CHANGES-<id>.md`, #20 decision 4).
/// CC BY 4.0 §3(a)(1) asks for the credit, a link to the licence and an indication of the
/// changes made; STEPBible also asks that the changes be recorded and made available. The
/// record names every pinned file, so the edition changed is exact.
pub fn changes_record(source: &Source) -> String {
    let mut s = format!(
        "# Changes we made: {}\n\n\
         {} ({}) ships in the Open Bible Software Starter library as the Resource `{}`. \
         This record states the changes we made to the source, as its licence asks.\n\n",
        source.title, source.title, source.abbreviation, source.resource_id
    );
    let licence = match obs_model::cc_by_license_url(&source.spdx) {
        Some(url) => format!("{} ({url})", source.spdx),
        None => source.spdx.clone(),
    };
    let _ = writeln!(s, "- Licence: {licence}");
    if let Some(e) = &source.edition {
        let _ = writeln!(s, "- Edition: {e}");
    }
    if let Some(p) = &source.publisher {
        let _ = writeln!(s, "- Publisher: {p}");
    }
    let origin = source
        .homepage
        .as_ref()
        .or(source.origin_url.as_ref())
        .unwrap_or(&source.url);
    let _ = writeln!(s, "- Source: {origin}");
    let _ = write!(s, "\n## Attribution\n\n{}\n", source.attribution.trim());
    s.push_str("\n## Pinned downloads\n\n");
    for (url, sha) in source.files() {
        let _ = writeln!(s, "- {url} (SHA-256 `{sha}`)");
    }
    let _ = write!(s, "\n## Changes we made\n\n{}\n", source.changes.trim());
    s
}

fn build_one(
    source: &Source,
    files: &[Vec<u8>],
    previous: Option<&ReleaseManifest>,
    changes: &IdChanges,
) -> Result<Built> {
    let id = &source.resource_id;
    let (mut resource, mut report) =
        import(source, files).with_context(|| format!("{id}: import failed"))?;

    let units: Vec<String> = resource
        .units
        .iter()
        .map(|u| u.id.as_str().to_string())
        .collect();
    let resolved = ids::aliases_for(id, changes, &units)?;
    let aliases = &resolved.map;
    for (old, new) in aliases {
        let old = UnitId::new(old.clone()).map_err(|e| anyhow!("{id}: alias `{old}`: {e}"))?;
        let new = UnitId::new(new.clone()).map_err(|e| anyhow!("{id}: alias `{new}`: {e}"))?;
        resource.unit_aliases.insert(old, new);
    }
    let diff = ids::diff(
        previous.and_then(|p| p.get(id)),
        id,
        &units,
        aliases,
        &resource.manifest.content_hash,
    )?;

    // Manifest metadata is not part of the content hash, so filling it in after the build
    // keeps the hash an Import would compute.
    let m = &mut resource.manifest;
    m.revision = diff.revision;
    m.edition.clone_from(&source.edition);
    m.publisher.clone_from(&source.publisher);
    m.source_url = Some(
        source
            .homepage
            .clone()
            .or_else(|| source.origin_url.clone())
            .unwrap_or_else(|| source.url.clone()),
    );
    m.license.clone_from(&source.spdx);
    m.attribution.clone_from(&source.attribution);
    m.changes = Some(source.changes.clone());
    m.origin = Origin::Starter;
    resource.validate().map_err(|e| anyhow!("{id}: {e}"))?;
    report.license_missing = false;
    Ok(Built {
        resource,
        report,
        diff,
        shadowed_aliases: resolved.shadowed,
    })
}

/// Run `source`'s importer on its fetched files (the main file, then any extra files).
fn import(source: &Source, files: &[Vec<u8>]) -> Result<(Resource, ImportReport)> {
    let extras = match source.importer.as_str() {
        "oshb-bdb" => 1,
        "tahot" | "tagnt" => source.extra_files.len(),
        // `modern.jsonl`, then TIPNR (to match places to its ids).
        "openbible-geocoding" => 2,
        // `events.json`, `People.csv`, then TIPNR (to give people and places its ids).
        "theographic-events" => 3,
        _ => 0,
    };
    if files.len() != 1 + extras {
        bail!(
            "importer `{}` reads {} extra file(s); the lock lists {}",
            source.importer,
            extras,
            files.len().saturating_sub(1)
        );
    }
    if !source.skip_columns.is_empty() && source.importer != "stepbible-tbes" {
        bail!("skip_columns applies only to the `stepbible-tbes` importer");
    }
    match source.importer.as_str() {
        "usfm" => return import_usfm_zip(source, &files[0]),
        "tahot" => return import_stepbible(source, files, StepbibleText::Tahot),
        "tagnt" => return import_stepbible(source, files, StepbibleText::Tagnt),
        "openbible-cross-references" => return import_openbible_zip(source, &files[0]),
        "usfm-parallel-passages" => return import_usfm_parallels(source, &files[0]),
        "tipnr" => return import_tipnr_text(source, &files[0]),
        "sword-zip" => return import_sword_zip(source, &files[0]),
        "openbible-geocoding" => return import_openbible_geo(source, files),
        "theographic-events" => return import_theographic(source, files),
        "wikisource-mhc" | "jfb-html" | "easton-zld" | "smith-rawld" => {
            return study::import(source, &files[0]);
        }
        _ => {}
    }
    let text = |i: usize| -> Result<&str> {
        let bytes: &[u8] = &files[i];
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
        std::str::from_utf8(bytes).with_context(|| format!("file {} is not UTF-8 text", i + 1))
    };
    let o = LexiconOptions {
        resource_id: source.resource_id.clone(),
        title: source.title.clone(),
        abbreviation: source.abbreviation.clone(),
        source_file: source_file_name(source),
        tool_version: format!("xtask starter {}", env!("CARGO_PKG_VERSION")),
    };
    let converted = match source.importer.as_str() {
        "stepbible-tbes" => import_tbes(&o, text(0)?, &source.skip_columns),
        "strongs-hebrew" => import_strongs_hebrew(&o, text(0)?),
        "strongs-greek" => import_strongs_greek(&o, text(0)?),
        "oshb-bdb" => import_bdb(&o, text(0)?, text(1)?),
        "abbott-smith" => import_abbott_smith(&o, text(0)?),
        other => bail!("unknown importer `{other}`"),
    };
    converted.map_err(|e| anyhow!("{e}"))
}

/// The publisher's file name, for the manifest and the report.
fn source_file_name(source: &Source) -> String {
    let origin = source.origin_url.as_deref().unwrap_or(&source.url);
    let name = origin.rsplit('/').next().unwrap_or(origin);
    name.replace("%20", " ")
}

/// A zip of USFM books (eBible, berean.bible) → a Bible.
fn import_usfm_zip(source: &Source, bytes: &[u8]) -> Result<(Resource, ImportReport)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).context("not a zip archive")?;
    let mut names: Vec<(String, usize)> = Vec::new();
    for i in 0..zip.len() {
        let entry = zip.by_index(i).context("cannot read the zip directory")?;
        let name = entry
            .name()
            .context("a zip entry name is not valid")?
            .into_owned();
        let lower = name.to_ascii_lowercase();
        if entry.is_file() && (lower.ends_with(".usfm") || lower.ends_with(".sfm")) {
            names.push((name, i));
        }
    }
    names.sort();
    let source_file = source_file_name(source);
    let mut import = UsfmImport::new(UsfmOptions {
        resource_id: source.resource_id.clone(),
        title: source.title.clone(),
        abbreviation: source.abbreviation.clone(),
        language: source.language.clone(),
        source_file,
        tool_version: format!("xtask starter {}", env!("CARGO_PKG_VERSION")),
        ..UsfmOptions::default()
    });
    let mut skipped = BTreeSet::new();
    for (name, index) in names {
        let mut text = String::new();
        zip.by_index(index)
            .with_context(|| format!("cannot open {name} in the zip"))?
            .read_to_string(&mut text)
            .with_context(|| format!("{name} is not UTF-8 text"))?;
        let code = usfm_book_code(&text).unwrap_or_default();
        if source
            .skip_books
            .iter()
            .any(|b| b.eq_ignore_ascii_case(&code))
        {
            skipped.insert(code);
            continue;
        }
        let base = name.rsplit('/').next().unwrap_or(&name).to_string();
        import.add_book(&base, text).map_err(|e| anyhow!("{e}"))?;
    }
    let missing: Vec<&String> = source
        .skip_books
        .iter()
        .filter(|b| !skipped.contains(&b.to_ascii_uppercase()))
        .collect();
    if !missing.is_empty() {
        bail!("skip_books lists books the source does not have: {missing:?}");
    }
    import.finish().map_err(|e| anyhow!("{e}"))
}

/// STEPBible's TAHOT or TAGNT tab-separated files → a Hebrew or Greek Bible.
fn import_stepbible(
    source: &Source,
    files: &[Vec<u8>],
    text: StepbibleText,
) -> Result<(Resource, ImportReport)> {
    let names: Vec<String> = source
        .files()
        .enumerate()
        .map(|(i, (url, _))| {
            let named = if i == 0 {
                source.origin_url.as_deref().unwrap_or(url)
            } else {
                url
            };
            fetch::file_name(named)
        })
        .collect();
    let mut import = StepbibleImport::new(StepbibleOptions {
        text,
        resource_id: source.resource_id.clone(),
        title: source.title.clone(),
        abbreviation: source.abbreviation.clone(),
        language: source.language.clone(),
        source_file: names.join("; "),
        tool_version: format!("xtask starter {}", env!("CARGO_PKG_VERSION")),
    });
    for (bytes, name) in files.iter().zip(&names) {
        let text = String::from_utf8(bytes.clone())
            .with_context(|| format!("{name} is not UTF-8 text"))?;
        import.add_file(name, text);
    }
    import.finish().map_err(|e| anyhow!("{e}"))
}

fn dataset_options(source: &Source) -> DatasetOptions {
    let origin = source.origin_url.as_deref().unwrap_or(&source.url);
    let source_file = origin.rsplit('/').next().unwrap_or(origin);
    DatasetOptions {
        resource_id: source.resource_id.clone(),
        title: source.title.clone(),
        abbreviation: source.abbreviation.clone(),
        language: source.language.clone(),
        // GitHub raw URLs encode the spaces of STEPBible's file names.
        source_file: source_file.replace("%20", " "),
        tool_version: format!("xtask starter {}", env!("CARGO_PKG_VERSION")),
    }
}

/// OpenBible.info's zip holding `cross_references.txt` → a Cross-reference set.
fn import_openbible_zip(source: &Source, bytes: &[u8]) -> Result<(Resource, ImportReport)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).context("not a zip archive")?;
    let mut text = String::new();
    zip.by_name("cross_references.txt")
        .context("the zip has no cross_references.txt")?
        .read_to_string(&mut text)
        .context("cross_references.txt is not UTF-8 text")?;
    import_openbible_cross_references(&text, &dataset_options(source)).map_err(|e| anyhow!("{e}"))
}

/// A zip of USFM books (the BSB's) → a Cross-reference set of their parallel passage
/// references (`\r`, #212). The Bible's own Starter entry pins the same zip.
fn import_usfm_parallels(source: &Source, bytes: &[u8]) -> Result<(Resource, ImportReport)> {
    let books = usfm_zip_books(bytes).map_err(|e| anyhow!("{e}"))?;
    import_usfm_parallel_passages(&books, &dataset_options(source)).map_err(|e| anyhow!("{e}"))
}

/// STEPBible's TIPNR text file → a People & places index.
/// A CrossWire `rawzip` package (`mods.d/<name>.conf` plus its `modules/` files) → its one
/// module through the app's SWORD Import, as Library → Import would read it. The package is
/// unpacked into a scratch folder, removed afterwards.
fn import_sword_zip(source: &Source, bytes: &[u8]) -> Result<(Resource, ImportReport)> {
    let dir = std::env::temp_dir().join(format!(
        "obs-starter-sword-{}-{}",
        source.resource_id,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let result = unpack_and_import_sword(source, bytes, &dir);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn unpack_and_import_sword(
    source: &Source,
    bytes: &[u8],
    dir: &Path,
) -> Result<(Resource, ImportReport)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).context("not a zip archive")?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).context("cannot read the zip directory")?;
        // Only paths that stay inside the scratch folder.
        let Some(name) = entry.enclosed_name() else {
            bail!("zip entry {:?} leaves the package", entry.name().ok());
        };
        let path = dir.join(name);
        if entry.is_dir() {
            std::fs::create_dir_all(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = Vec::new();
        entry.read_to_end(&mut out)?;
        std::fs::write(&path, out).with_context(|| format!("cannot write {}", path.display()))?;
    }
    let sword = obs_formats::SwordSource::open(dir).map_err(|e| anyhow!("{e}"))?;
    let module = sword
        .default_module()
        .context("the package must hold exactly one SWORD module")?;
    let options = obs_formats::SwordOptions {
        resource_id: source.resource_id.clone(),
        title: source.title.clone(),
        abbreviation: source.abbreviation.clone(),
        language: source.language.clone(),
        source_file: source_file_name(source),
        tool_version: format!("xtask starter {}", env!("CARGO_PKG_VERSION")),
    };
    obs_formats::import_sword(&sword, &module, &options).map_err(|e| anyhow!("{e}"))
}

fn import_tipnr_text(source: &Source, bytes: &[u8]) -> Result<(Resource, ImportReport)> {
    let text = std::str::from_utf8(bytes).context("the TIPNR file is not UTF-8 text")?;
    import_tipnr(text, &dataset_options(source)).map_err(|e| anyhow!("{e}"))
}

/// OpenBible.info's `ancient.jsonl` and `modern.jsonl` → a People & places index with Place
/// locations (#150), its places matched to the TIPNR file given third.
fn import_openbible_geo(source: &Source, files: &[Vec<u8>]) -> Result<(Resource, ImportReport)> {
    let text = |i: usize, what: &str| {
        std::str::from_utf8(&files[i]).with_context(|| format!("{what} is not UTF-8 text"))
    };
    let tipnr = import_tipnr(text(2, "the TIPNR file")?, &dataset_options(source))
        .map_err(|e| anyhow!("TIPNR (for matching places): {e}"))?
        .0;
    obs_formats::import_openbible_geocoding(
        text(0, "ancient.jsonl")?,
        text(1, "modern.jsonl")?,
        Some(&tipnr),
        &dataset_options(source),
    )
    .map_err(|e| anyhow!("{e}"))
}

/// Theographic's `Events.csv`, `events.json` and `People.csv` → a People & places index
/// carrying the Timeline's Events dataset (#185), its people and places matched to the TIPNR
/// file given fourth.
fn import_theographic(source: &Source, files: &[Vec<u8>]) -> Result<(Resource, ImportReport)> {
    let text = |i: usize, what: &str| {
        std::str::from_utf8(&files[i]).with_context(|| format!("{what} is not UTF-8 text"))
    };
    let tipnr = import_tipnr(text(3, "the TIPNR file")?, &dataset_options(source))
        .map_err(|e| anyhow!("TIPNR (for matching people and places): {e}"))?
        .0;
    obs_formats::import_theographic_events(
        text(0, "Events.csv")?,
        text(1, "events.json")?,
        text(2, "People.csv")?,
        Some(&tipnr),
        &dataset_options(source),
    )
    .map_err(|e| anyhow!("{e}"))
}

/// Remove the outputs of an earlier build (only files this task writes).
fn clean_outputs(out: &Path) -> Result<()> {
    std::fs::create_dir_all(out).with_context(|| format!("cannot create {}", out.display()))?;
    for entry in std::fs::read_dir(out)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() && (name == SEARCH_DIR || name == ALIAS_DIR) {
            std::fs::remove_dir_all(&path)
                .with_context(|| format!("cannot remove {}", path.display()))?;
            continue;
        }
        if path.is_file()
            && (name.ends_with(".obr")
                || name.ends_with(".obr.partial")
                || (name.starts_with("CHANGES-") && name.ends_with(".md"))
                || name == MANIFEST_FILE
                || name == REPORT_FILE)
        {
            std::fs::remove_file(&path)
                .with_context(|| format!("cannot remove {}", path.display()))?;
        }
    }
    Ok(())
}

/// Bytes of every file under `dir`, including the `search/` and `aliases/` caches.
fn dir_bytes(dir: &Path) -> Result<u64> {
    let mut total = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() {
            total += meta.len();
        } else if meta.is_dir() {
            total += dir_bytes(&entry.path())?;
        }
    }
    Ok(total)
}

fn mb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_000_000.0)
}

/// Structural failures as a bulleted list, capped so a broken Resource does not flood the log.
fn list_failures(failures: &[String]) -> String {
    const SHOWN: usize = 50;
    let mut s: String = failures
        .iter()
        .take(SHOWN)
        .map(|f| format!("- {f}\n"))
        .collect();
    if failures.len() > SHOWN {
        let _ = writeln!(s, "- …and {} more", failures.len() - SHOWN);
    }
    s
}

/// One snapshot's status, for the report and the failure message.
fn describe(status: &Status, file: &Path) -> String {
    match status {
        Status::Unchanged => "unchanged".to_string(),
        Status::Changed(_) => format!("changed (see the diff against {})", file.display()),
        Status::New => format!("new: {} is not committed yet", file.display()),
        Status::Written => format!("written to {}", file.display()),
    }
}

/// Fail unless `a` and `b` hold the same files with the same bytes.
fn compare_dirs(a: &Path, b: &Path) -> Result<()> {
    // Every file, by its path relative to the output directory (`search/` included).
    let list = |d: &Path| -> Result<BTreeMap<String, PathBuf>> {
        let mut m = BTreeMap::new();
        for entry in walkdir::WalkDir::new(d) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let rel = entry.path().strip_prefix(d).unwrap_or(entry.path());
                m.insert(
                    rel.to_string_lossy().replace('\\', "/"),
                    entry.path().to_path_buf(),
                );
            }
        }
        Ok(m)
    };
    let (la, lb) = (list(a)?, list(b)?);
    if la.keys().ne(lb.keys()) {
        bail!(
            "the two builds wrote different files: {:?} vs {:?}",
            la.keys().collect::<Vec<_>>(),
            lb.keys().collect::<Vec<_>>()
        );
    }
    let mut differ = Vec::new();
    for (name, pa) in &la {
        if std::fs::read(pa)? != std::fs::read(&lb[name])? {
            differ.push(name.as_str());
        }
    }
    if !differ.is_empty() {
        bail!(
            "the build is not deterministic; these files differ: {}",
            differ.join(", ")
        );
    }
    Ok(())
}

/// Sizes of the prebuilt caches: each Resource's Search index (in Resource order) and all
/// alias caches together.
struct CacheBytes<'a> {
    search: &'a [u64],
    aliases: u64,
}

fn build_report(
    built: &[Built],
    manifest: &ReleaseManifest,
    previous: Option<&ReleaseManifest>,
    caches: CacheBytes<'_>,
    budget: u64,
    coverage: &checks::Coverage,
    snapshots: &[goldens::Checked],
) -> String {
    let mut s = String::from("# Starter library build report\n\n");
    let obr_bytes: u64 = manifest.resources.iter().map(|r| r.bytes).sum();
    let index_bytes: u64 = caches.search.iter().sum();
    let alias_bytes = caches.aliases;
    let _ = writeln!(
        s,
        "Total: {} Resource(s), {} MB of Resource files plus {} MB of prebuilt Search index \
         caches (`{SEARCH_DIR}/`, index version {}) plus {} MB of People & places alias caches \
         (`{ALIAS_DIR}/`, {alias_bytes} bytes) = {} MB; size budget {} MB for the Starter set \
         plus its caches.\n",
        manifest.resources.len(),
        mb(obr_bytes),
        mb(index_bytes),
        obs_search::INDEX_VERSION,
        mb(alias_bytes),
        mb(obr_bytes + index_bytes + alias_bytes),
        budget / 1_000_000
    );
    match previous {
        Some(_) => s.push_str("Stable ids diffed against the previous release manifest.\n\n"),
        None => s.push_str("No previous release manifest given: every Resource is new.\n\n"),
    }
    s.push_str(
        "| Resource | Revision | Size (MB) | Search index (MB) | Units | Verses | Notes | Tagged words | Unmapped | Absent | Content hash |\n\
         |---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for (i, (b, r)) in built.iter().zip(&manifest.resources).enumerate() {
        let vm = b.report.verse_map;
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} | `{}` |",
            r.resource_id,
            r.revision,
            mb(r.bytes),
            mb(caches.search.get(i).copied().unwrap_or(0)),
            r.units.len(),
            b.report.verses,
            b.report.notes,
            b.report.tagged_words,
            vm.map_or(0, |v| v.unmapped),
            vm.map_or(0, |v| v.absent),
            r.content_hash
        );
    }
    s.push_str("\n## Changes since the previous release\n\n");
    for (b, r) in built.iter().zip(&manifest.resources) {
        let d = &b.diff;
        let prev = previous.and_then(|p| p.get(&r.resource_id));
        let line = if d.new_resource {
            "new Resource".to_string()
        } else {
            let mut parts = vec![if d.content_changed {
                "content changed (revision bumped)".to_string()
            } else {
                "content unchanged".to_string()
            }];
            if !d.added.is_empty() {
                parts.push(format!("{} unit(s) added", d.added.len()));
            }
            if !d.aliased.is_empty() {
                parts.push(format!(
                    "{} unit(s) now aliased: {}",
                    d.aliased.len(),
                    d.aliased.join(", ")
                ));
            }
            if let Some(p) = prev {
                parts.push(format!("size {} → {} MB", mb(p.bytes), mb(r.bytes)));
            }
            parts.join("; ")
        };
        let line = if b.shadowed_aliases.is_empty() {
            line
        } else {
            format!(
                "{line}; id-changes entries not shipped because the old id is live again: {}",
                b.shadowed_aliases.join(", ")
            )
        };
        let _ = writeln!(s, "- `{}`: {line}", r.resource_id);
    }
    s.push_str("\n## Structural checks\n\n");
    let _ = writeln!(
        s,
        "All passed. {} Bible(s): {} Bible Verses each mapped or Unmapped; {} Standard Verses \
         of their Bible books each present or Absent. {} Cross-reference(s) and {} Scripture \
         reference span(s) target Standard Passages. {} span(s) stay inside their units.",
        coverage.bibles,
        coverage.bible_verses,
        coverage.standard_verses,
        coverage.cross_references,
        coverage.scripture_references,
        coverage.spans
    );
    if coverage.lexicons > 0 {
        let _ = writeln!(
            s,
            "{} Tagged word(s) use {} distinct Strong's number(s); every one resolves in at \
             least one of the {} Lexicon(s).",
            coverage.tagged_words, coverage.strongs_numbers, coverage.lexicons
        );
    } else {
        let _ = writeln!(
            s,
            "{} Tagged word(s) use {} distinct Strong's number(s); the lock has no Lexicon to \
             resolve them in, so that rule was not checked.",
            coverage.tagged_words, coverage.strongs_numbers
        );
    }
    s.push_str("\n## Golden snapshots\n\n");
    if snapshots.is_empty() {
        s.push_str("None listed.\n");
    } else {
        s.push_str("| Snapshot | Status |\n|---|---|\n");
        for c in snapshots {
            let _ = writeln!(s, "| {} | {} |", c.name, describe(&c.status, &c.file));
        }
        for c in snapshots {
            if let Status::Changed(diff) = &c.status {
                let _ = write!(s, "\n### {} changed\n\n```diff\n{diff}```\n", c.name);
            }
        }
    }
    s.push_str("\n## Import reports\n");
    for b in built {
        let m = &b.resource.manifest;
        let _ = write!(
            s,
            "\n### `{}`\n\n```text\n{}  licence {}\n```\n",
            m.resource_id, b.report, m.license
        );
    }
    s
}

#[cfg(test)]
mod tests;
