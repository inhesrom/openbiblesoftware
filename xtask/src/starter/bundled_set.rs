//! `cargo xtask ios bundled-set`: the Mobile app's Bundled set (#333, CONTEXT.md § Mobile).
//!
//! Copies the Starter Resources whose `mobile_set` is `bundled` out of a built Starter library
//! (`dist/starter/`) into the folder the iOS app ships (`apps/ios/Bundled/starter/`), laid out
//! exactly as a Starter bundle: each `<id>.obr`, its prebuilt Search index caches in `search/`,
//! the alias caches in `aliases/` involving it, its `CHANGES-<id>.md`, and a
//! `starter-manifest.json` listing only these Resources. Every file's sha256 and size must
//! match the committed `apps/ios/bundled-set.lock` (`--update-lock` rewrites it), and the
//! folder must stay within [`MAX_BYTES`]. `--verify` checks a folder already copied, for the
//! app's build.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use super::fetch::sha256_hex;
use super::ids::{MANIFEST_FORMAT, ReleaseManifest};
use super::lock::MobileSet;
use super::{ALIAS_DIR, MANIFEST_FILE, SEARCH_DIR, clean_outputs, mb};

/// The Bundled set folder's cap. It catches a Study set Resource bundled by mistake (each is
/// 3–26 MB); the Bundled set itself measured 43.1 MB with its Search indexes (2026-11).
pub const MAX_BYTES: u64 = 50_000_000;

/// The committed lock, relative to the workspace root.
pub const LOCK_FILE: &str = "apps/ios/bundled-set.lock";

const USAGE: &str =
    "usage: cargo xtask ios bundled-set [--from DIR] [--out DIR] [--update-lock | --verify]

  --from DIR     a built Starter library (default dist/starter, from `cargo xtask starter`)
  --out DIR      where the Bundled set goes (default apps/ios/Bundled/starter)
  --update-lock  rewrite apps/ios/bundled-set.lock from this copy instead of failing on a
                 difference (commit the result for review)
  --verify       copy nothing: check the Bundled set already in --out against the lock";

const LOCK_HEADER: &str = "\
# The Mobile app's Bundled set (#333): every file `cargo xtask ios bundled-set` copies from
# dist/starter/ into apps/ios/Bundled/starter/, with its sha256 and size. The command fails
# when a fresh copy differs; `cargo xtask ios bundled-set --update-lock` rewrites this file.
";

/// Inputs and outputs of one copy.
#[derive(Debug, Clone)]
pub struct Options {
    pub from: PathBuf,
    pub out: PathBuf,
    pub lock: PathBuf,
    pub update_lock: bool,
    /// Only check the folder at `out`; copy nothing.
    pub verify: bool,
    pub max_bytes: u64,
}

impl Options {
    pub fn defaults(root: &Path) -> Self {
        Self {
            from: root.join("dist/starter"),
            out: root.join("apps/ios/Bundled/starter"),
            lock: root.join(LOCK_FILE),
            update_lock: false,
            verify: false,
            max_bytes: MAX_BYTES,
        }
    }
}

/// `apps/ios/bundled-set.lock`: the Bundled set's Resources and every file of its folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundledSetLock {
    /// Bytes of every file together.
    pub bytes: u64,
    #[serde(rename = "resource", default)]
    pub resources: Vec<LockedResource>,
    #[serde(rename = "file", default)]
    pub files: Vec<LockedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedResource {
    pub resource_id: String,
    pub revision: u32,
    /// sha256 of the `.obr` file.
    pub file_sha256: String,
    /// Size of the `.obr` file.
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedFile {
    /// Relative to the Bundled set folder, `/`-separated.
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

impl BundledSetLock {
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        toml::from_str(&raw)
            .with_context(|| format!("{} is not a Bundled set lock", path.display()))
    }

    pub fn to_toml(&self) -> Result<String> {
        Ok(format!("{LOCK_HEADER}\n{}", toml::to_string(self)?))
    }
}

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    let mut options = Options::defaults(root);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || {
            it.next()
                .map(PathBuf::from)
                .ok_or_else(|| anyhow!("`{a}` needs a value\n\n{USAGE}"))
        };
        match a.as_str() {
            "--from" => options.from = value()?,
            "--out" => options.out = value()?,
            "--update-lock" => options.update_lock = true,
            "--verify" => options.verify = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            other => bail!("unknown option `{other}`\n\n{USAGE}"),
        }
    }
    if options.verify && options.update_lock {
        bail!("`--verify` and `--update-lock` do not go together\n\n{USAGE}");
    }
    let lock = if options.verify {
        verify(&options)?
    } else {
        copy(&options)?
    };
    for r in &lock.resources {
        eprintln!(
            "bundled-set: {:<16} r{:<3} {:>6} MB",
            r.resource_id,
            r.revision,
            mb(r.bytes)
        );
    }
    eprintln!(
        "bundled-set: {} Resource(s), {} file(s), {} MB of {} MB in {}; {} {}",
        lock.resources.len(),
        lock.files.len(),
        mb(lock.bytes),
        options.max_bytes / 1_000_000,
        options.out.display(),
        if options.update_lock {
            "wrote"
        } else {
            "matches"
        },
        options.lock.display()
    );
    Ok(())
}

/// Copy the Bundled set from `options.from` into `options.out`, then check its size and its
/// lock (or rewrite the lock with `update_lock`). Returns the lock of what was copied.
pub fn copy(options: &Options) -> Result<BundledSetLock> {
    let from = &options.from;
    let manifest_path = from.join(MANIFEST_FILE);
    let manifest = ReleaseManifest::load(&manifest_path).with_context(|| {
        format!(
            "no built Starter library in {} (run `cargo xtask starter`)",
            from.display()
        )
    })?;
    let bundled: Vec<_> = manifest
        .resources
        .iter()
        .filter(|r| r.mobile_set == Some(MobileSet::Bundled))
        .cloned()
        .collect();
    if bundled.is_empty() {
        bail!(
            "{} lists no Resource with `mobile_set` `bundled` (rebuild it with `cargo xtask \
             starter`)",
            manifest_path.display()
        );
    }

    let out = &options.out;
    clean_outputs(out)?;
    for r in &bundled {
        let id = &r.resource_id;
        let name = format!("{id}.obr");
        let bytes = std::fs::read(from.join(&name))
            .with_context(|| format!("cannot read {}", from.join(&name).display()))?;
        if sha256_hex(&bytes) != r.file_sha256 {
            bail!(
                "{} does not match the sha256 its manifest lists (rebuild it with `cargo xtask \
                 starter`)",
                from.join(&name).display()
            );
        }
        write(&out.join(&name), &bytes)?;
        for cache in obs_store::search_index_files(&from.join(SEARCH_DIR), id) {
            copy_into(&cache, &out.join(SEARCH_DIR))?;
        }
        // Same rule as the app's bundle copy: either side of the pair.
        if let Ok(rd) = std::fs::read_dir(from.join(ALIAS_DIR)) {
            for e in rd {
                let e = e?;
                let file = e.file_name().to_string_lossy().into_owned();
                if file.starts_with(&format!("{id}.")) || file.contains(&format!("--{id}.")) {
                    copy_into(&e.path(), &out.join(ALIAS_DIR))?;
                }
            }
        }
        let changes = obs_model::changes_record_file(id);
        if from.join(&changes).is_file() {
            copy_into(&from.join(&changes), out)?;
        }
    }
    let subset = ReleaseManifest {
        format: MANIFEST_FORMAT,
        resources: bundled,
    };
    let mut json = serde_json::to_string_pretty(&subset)?;
    json.push('\n');
    write(&out.join(MANIFEST_FILE), json.as_bytes())?;

    let lock = lock_of(out, &subset)?;
    if options.update_lock {
        check_size(&lock, options.max_bytes)?;
        if let Some(dir) = options.lock.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("cannot create {}", dir.display()))?;
        }
        write(&options.lock, lock.to_toml()?.as_bytes())?;
        return Ok(lock);
    }
    check(options, lock)
}

/// Check the Bundled set already in `options.out` against its size cap and the committed
/// lock, without copying anything (for the app's build, which has no `dist/starter/`).
pub fn verify(options: &Options) -> Result<BundledSetLock> {
    let manifest = ReleaseManifest::load(&options.out.join(MANIFEST_FILE)).with_context(|| {
        format!(
            "no Bundled set in {} (run `cargo xtask ios bundled-set`)",
            options.out.display()
        )
    })?;
    let lock = lock_of(&options.out, &manifest)?;
    check(options, lock)
}

fn check_size(lock: &BundledSetLock, max_bytes: u64) -> Result<()> {
    if lock.bytes <= max_bytes {
        return Ok(());
    }
    let mut why = format!(
        "the Bundled set is {} MB, over its {} MB cap:",
        mb(lock.bytes),
        max_bytes / 1_000_000
    );
    for f in &lock.files {
        let _ = write!(why, "\n  {:>8} MB  {}", mb(f.bytes), f.path);
    }
    bail!(why)
}

/// `lock` (of the folder `options.out`) is within the cap and matches the committed lock.
fn check(options: &Options, lock: BundledSetLock) -> Result<BundledSetLock> {
    check_size(&lock, options.max_bytes)?;
    if !options.lock.is_file() {
        bail!(
            "no lock at {} (write one with `cargo xtask ios bundled-set --update-lock`)",
            options.lock.display()
        );
    }
    let committed = BundledSetLock::load(&options.lock)?;
    let differences = differences(&committed, &lock);
    if !differences.is_empty() {
        bail!(
            "the Bundled set in {} does not match {}:\n  {}\nIf the change is intended, run \
             `cargo xtask ios bundled-set --update-lock` and commit the lock.",
            options.out.display(),
            options.lock.display(),
            differences.join("\n  ")
        );
    }
    Ok(lock)
}

/// The lock of the Bundled set folder `dir`, whose manifest is `manifest`.
fn lock_of(dir: &Path, manifest: &ReleaseManifest) -> Result<BundledSetLock> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(dir).sort_by_file_name() {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let bytes = std::fs::read(entry.path())
            .with_context(|| format!("cannot read {}", entry.path().display()))?;
        let rel = entry.path().strip_prefix(dir).unwrap_or(entry.path());
        files.push(LockedFile {
            path: rel.to_string_lossy().replace('\\', "/"),
            sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(BundledSetLock {
        bytes: files.iter().map(|f| f.bytes).sum(),
        resources: manifest
            .resources
            .iter()
            .map(|r| LockedResource {
                resource_id: r.resource_id.clone(),
                revision: r.revision,
                file_sha256: r.file_sha256.clone(),
                bytes: r.bytes,
            })
            .collect(),
        files,
    })
}

/// What differs between the committed lock and a fresh copy's, one line each.
fn differences(committed: &BundledSetLock, fresh: &BundledSetLock) -> Vec<String> {
    let mut out = Vec::new();
    for r in &committed.resources {
        match fresh
            .resources
            .iter()
            .find(|f| f.resource_id == r.resource_id)
        {
            None => out.push(format!(
                "{}: in the lock, not in the Bundled set",
                r.resource_id
            )),
            Some(f) if f != r => out.push(format!(
                "{}: the lock has r{} ({} bytes), the copy r{} ({} bytes)",
                r.resource_id, r.revision, r.bytes, f.revision, f.bytes
            )),
            Some(_) => {}
        }
    }
    for f in &fresh.resources {
        if !committed
            .resources
            .iter()
            .any(|r| r.resource_id == f.resource_id)
        {
            out.push(format!(
                "{}: in the Bundled set, not in the lock",
                f.resource_id
            ));
        }
    }
    for c in &committed.files {
        match fresh.files.iter().find(|f| f.path == c.path) {
            None => out.push(format!("{}: in the lock, not copied", c.path)),
            Some(f) if f != c => out.push(format!(
                "{}: sha256 {} ({} bytes) in the lock, {} ({} bytes) copied",
                c.path, c.sha256, c.bytes, f.sha256, f.bytes
            )),
            Some(_) => {}
        }
    }
    for f in &fresh.files {
        if !committed.files.iter().any(|c| c.path == f.path) {
            out.push(format!("{}: copied, not in the lock", f.path));
        }
    }
    if out.is_empty() && committed != fresh {
        out.push(format!(
            "total {} bytes in the lock, {} copied",
            committed.bytes, fresh.bytes
        ));
    }
    out
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

/// Copy `file` into the folder `dir`, keeping its name.
fn copy_into(file: &Path, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let name = file
        .file_name()
        .ok_or_else(|| anyhow!("{} has no file name", file.display()))?;
    std::fs::copy(file, dir.join(name))
        .with_context(|| format!("cannot copy {} into {}", file.display(), dir.display()))?;
    Ok(())
}
