//! `cargo xtask starter`: build the Starter library from `starter/sources.toml` (#38, #20).
//!
//! 1. Load the lock and `starter/id-changes.toml` and run the provenance gate.
//! 2. Fetch every source into the download cache and verify its sha256.
//! 3. Run the same Import parsers and `ResourceBuilder` as Import (USFM for Bibles, the
//!    lexicon converters for Lexicons), then fill in the manifest from the lock (licence,
//!    Attribution, changes, `origin = starter`).
//! 4. Diff unit ids against the previous release manifest; a removed id fails the build unless
//!    `id-changes.toml` maps it (shipped as `unit_alias`).
//! 5. Write `dist/starter/<id>.obr`, `CHANGES-<id>.md` for CC BY sources,
//!    `starter-manifest.json` and `starter-build-report.md`, and check the size budget.
//!
//! Builds are deterministic: the same lock and code give byte-identical outputs.
//! `--check-determinism` builds a second time and compares every file.

mod fetch;
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
    import_strongs_greek, import_strongs_hebrew, import_tbes, import_tipnr, usfm_book_code,
};
use obs_model::{Origin, Resource, UnitId};
use obs_store::write_obr;

use fetch::{fetch, sha256_hex};
use ids::{IdDiff, MANIFEST_FORMAT, ReleaseManifest, ReleaseResource};
use lock::{IdChanges, Lock, Source};

pub const MANIFEST_FILE: &str = "starter-manifest.json";
pub const REPORT_FILE: &str = "starter-build-report.md";

const USAGE: &str = "usage: cargo xtask starter [--out DIR] [--cache DIR] [--previous MANIFEST] [--check-determinism]

  --out DIR            where the outputs go (default dist/starter)
  --cache DIR          download cache (default target/starter-cache)
  --previous MANIFEST  the previous release's starter-manifest.json, for the stable-id diff
  --check-determinism  build twice and fail unless every output is byte-identical";

/// Inputs and outputs of one build.
#[derive(Debug, Clone)]
pub struct Config {
    pub lock: PathBuf,
    pub id_changes: PathBuf,
    pub previous: Option<PathBuf>,
    pub out: PathBuf,
    pub cache: PathBuf,
}

impl Config {
    pub fn defaults(root: &Path) -> Self {
        Self {
            lock: root.join("starter/sources.toml"),
            id_changes: root.join("starter/id-changes.toml"),
            previous: None,
            out: root.join("dist/starter"),
            cache: root.join("target/starter-cache"),
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

    clean_outputs(&config.out)?;
    let mut released = Vec::with_capacity(built.len());
    for b in &built {
        let id = &b.resource.manifest.resource_id;
        let path = config.out.join(format!("{id}.obr"));
        write_obr(&b.resource, &path)
            .with_context(|| format!("cannot write {}", path.display()))?;
        let bytes = std::fs::read(&path)?;
        released.push(ReleaseResource {
            resource_id: id.clone(),
            revision: b.resource.manifest.revision,
            content_hash: b.resource.manifest.content_hash.clone(),
            file_sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
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
    for source in &lock_file.sources {
        if source
            .spdx
            .split(|c: char| c.is_whitespace() || c == '(' || c == ')')
            .any(|license| license.starts_with("CC-BY-"))
        {
            let text = format!(
                "# Changes we made: {}\n\nResource: `{}`\n\nLicence: {}\n\n\
                 ## Attribution\n\n{}\n\n\
                 ## Source\n\n{}\n\nPinned download: {}\n\nSHA-256: `{}`\n\n\
                 ## Changes we made\n\n{}\n",
                source.title,
                source.resource_id,
                source.spdx,
                source.attribution,
                source.origin_url.as_ref().unwrap_or(&source.url),
                source.url,
                source.sha256,
                source.changes,
            );
            let path = config
                .out
                .join(format!("CHANGES-{}.md", source.resource_id));
            std::fs::write(&path, text)
                .with_context(|| format!("cannot write {}", path.display()))?;
        }
    }
    let manifest = ReleaseManifest {
        format: MANIFEST_FORMAT,
        resources: released,
    };
    let mut json = serde_json::to_string_pretty(&manifest)?;
    json.push('\n');
    std::fs::write(config.out.join(MANIFEST_FILE), json)?;

    let obr_bytes: u64 = manifest.resources.iter().map(|r| r.bytes).sum();
    let budget = lock_file.size_budget_mb.saturating_mul(1_000_000);
    let report = build_report(&built, &manifest, previous.as_ref(), obr_bytes, budget);
    std::fs::write(config.out.join(REPORT_FILE), &report)?;
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
        "tipnr" => return import_tipnr_text(source, &files[0]),
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

/// STEPBible's TIPNR text file → a People & places index.
fn import_tipnr_text(source: &Source, bytes: &[u8]) -> Result<(Resource, ImportReport)> {
    let text = std::str::from_utf8(bytes).context("the TIPNR file is not UTF-8 text")?;
    import_tipnr(text, &dataset_options(source)).map_err(|e| anyhow!("{e}"))
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

fn dir_bytes(dir: &Path) -> Result<u64> {
    let mut total = 0;
    for entry in std::fs::read_dir(dir)? {
        let meta = entry?.metadata()?;
        if meta.is_file() {
            total += meta.len();
        }
    }
    Ok(total)
}

fn mb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_000_000.0)
}

/// Fail unless `a` and `b` hold the same files with the same bytes.
fn compare_dirs(a: &Path, b: &Path) -> Result<()> {
    let list = |d: &Path| -> Result<BTreeMap<String, PathBuf>> {
        let mut m = BTreeMap::new();
        for entry in std::fs::read_dir(d)? {
            let p = entry?.path();
            if p.is_file() {
                m.insert(
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    p,
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

fn build_report(
    built: &[Built],
    manifest: &ReleaseManifest,
    previous: Option<&ReleaseManifest>,
    obr_bytes: u64,
    budget: u64,
) -> String {
    let mut s = String::from("# Starter library build report\n\n");
    let _ = writeln!(
        s,
        "Total: {} Resource(s), {} MB of Resource files; size budget {} MB for the Starter \
         set plus Search index caches (caches are added by a later build step).\n",
        manifest.resources.len(),
        mb(obr_bytes),
        budget / 1_000_000
    );
    match previous {
        Some(_) => s.push_str("Stable ids diffed against the previous release manifest.\n\n"),
        None => s.push_str("No previous release manifest given: every Resource is new.\n\n"),
    }
    s.push_str(
        "| Resource | Revision | Size (MB) | Units | Verses | Notes | Tagged words | Unmapped | Absent | Content hash |\n\
         |---|---|---|---|---|---|---|---|---|---|\n",
    );
    for (b, r) in built.iter().zip(&manifest.resources) {
        let vm = b.report.verse_map;
        let _ = writeln!(
            s,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | `{}` |",
            r.resource_id,
            r.revision,
            mb(r.bytes),
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
