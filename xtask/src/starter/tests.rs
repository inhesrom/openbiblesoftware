//! End-to-end tests of the Starter build on a small zip of WEB excerpts.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use obs_store::{ObrReader, ResourceStore};

use super::*;

/// A fresh scratch directory under the target dir.
fn scratch(name: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/xtask-tests")
        .join(format!(
            "{name}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Zip the WEB USFM excerpts used by the formats tests; returns (zip path, sha256).
fn fixture_zip(dir: &Path, skip: &[&str]) -> (PathBuf, String) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("crates/formats/tests/assets/usfm/web");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&src)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let path = dir.join("web-excerpt.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        if skip.iter().any(|s| name.starts_with(s)) {
            continue;
        }
        zip.start_file(format!("usfm/{name}"), opts).unwrap();
        zip.write_all(&std::fs::read(&f).unwrap()).unwrap();
    }
    zip.finish().unwrap();
    let sha = sha256_hex(&std::fs::read(&path).unwrap());
    (path, sha)
}

/// Write a lock with one WEB entry and its evidence; returns the Config.
fn setup(dir: &Path, zip: &Path, sha: &str, evidence_names_sha: bool) -> Config {
    let starter = dir.join("starter");
    std::fs::create_dir_all(starter.join("evidence")).unwrap();
    let evidence = if evidence_names_sha {
        format!("Licence: public domain.\nsha256 {sha}\n")
    } else {
        "Licence: public domain.\n".to_string()
    };
    std::fs::write(starter.join("evidence/web.md"), evidence).unwrap();
    let lock = format!(
        r#"size_budget_mb = 400

[[source]]
resource_id = "web"
importer = "usfm"
url = "file://{}"
sha256 = "{sha}"
title = "World English Bible"
abbreviation = "WEB"
language = "en"
spdx = "LicenseRef-PublicDomain"
attribution = "The World English Bible is in the Public Domain."
changes = "Introductions omitted."
provenance = "eBible.org copyright page."
evidence = "evidence/web.md"
"#,
        zip.display().to_string().replace('\\', "/")
    );
    std::fs::write(starter.join("sources.toml"), lock).unwrap();
    Config {
        lock: starter.join("sources.toml"),
        id_changes: starter.join("id-changes.toml"),
        previous: None,
        out: dir.join("out"),
        cache: dir.join("cache"),
    }
}

#[test]
fn two_clean_builds_are_byte_identical() {
    let dir = scratch("determinism");
    let (zip, sha) = fixture_zip(&dir, &[]);
    let config = setup(&dir, &zip, &sha, true);
    build(&config).unwrap();
    let second = Config {
        out: dir.join("out2"),
        cache: dir.join("cache2"),
        ..config.clone()
    };
    build(&second).unwrap();
    compare_dirs(&config.out, &second.out).unwrap();
    let obr = std::fs::read(config.out.join("web.obr")).unwrap();
    assert_eq!(obr, std::fs::read(second.out.join("web.obr")).unwrap());

    // The manifest carries the lock's metadata.
    let reader = ObrReader::open(&config.out.join("web.obr")).unwrap();
    let m = reader.manifest();
    assert_eq!(m.origin, obs_model::Origin::Starter);
    assert_eq!(m.license, "LicenseRef-PublicDomain");
    assert_eq!(m.changes.as_deref(), Some("Introductions omitted."));
    assert_eq!(m.revision, 1);
    let report = std::fs::read_to_string(config.out.join(REPORT_FILE)).unwrap();
    assert!(report.contains("| `web` | 1 |"), "{report}");
}

#[test]
fn a_tampered_download_fails_the_hash_check() {
    let dir = scratch("tamper");
    let (zip, sha) = fixture_zip(&dir, &[]);
    let config = setup(&dir, &zip, &sha, true);
    let mut bytes = std::fs::read(&zip).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&zip, bytes).unwrap();
    let err = format!("{:#}", build(&config).unwrap_err());
    assert!(err.contains("sha256 mismatch"), "{err}");
    assert!(!config.out.join("web.obr").exists());
}

#[test]
fn a_tampered_cache_entry_is_downloaded_again() {
    let dir = scratch("cache");
    let (zip, sha) = fixture_zip(&dir, &[]);
    let config = setup(&dir, &zip, &sha, true);
    std::fs::create_dir_all(&config.cache).unwrap();
    std::fs::write(config.cache.join(&sha), b"not the zip").unwrap();
    build(&config).unwrap();
    assert_eq!(
        sha256_hex(&std::fs::read(config.cache.join(&sha)).unwrap()),
        sha
    );
}

#[test]
fn removing_a_unit_id_fails_unless_id_changes_maps_it() {
    let dir = scratch("ids");
    let (zip, sha) = fixture_zip(&dir, &[]);
    let config = setup(&dir, &zip, &sha, true);
    build(&config).unwrap();
    let previous = dir.join("previous-manifest.json");
    std::fs::copy(config.out.join(MANIFEST_FILE), &previous).unwrap();

    // The next release drops 3 John.
    let next = dir.join("next");
    std::fs::create_dir_all(&next).unwrap();
    let (zip2, sha2) = fixture_zip(&next, &["3JN"]);
    let mut config2 = setup(&next, &zip2, &sha2, true);
    config2.previous = Some(previous);
    let err = format!("{:#}", build(&config2).unwrap_err());
    assert!(
        err.contains("3JN") && err.contains("id-changes.toml"),
        "{err}"
    );

    std::fs::write(
        &config2.id_changes,
        "[[alias]]\nresource_id = \"web\"\nold = \"3JN\"\nnew = \"MAT\"\n",
    )
    .unwrap();
    build(&config2).unwrap();
    let reader = ObrReader::open(&config2.out.join("web.obr")).unwrap();
    assert_eq!(
        reader.manifest().revision,
        2,
        "changed content bumps the revision"
    );
    let manifest = ReleaseManifest::load(&config2.out.join(MANIFEST_FILE)).unwrap();
    assert_eq!(
        manifest.resources[0]
            .unit_aliases
            .get("3JN")
            .map(String::as_str),
        Some("MAT")
    );
}

#[test]
fn the_provenance_gate_refuses_entries_without_evidence() {
    let dir = scratch("evidence");
    let (zip, sha) = fixture_zip(&dir, &[]);
    let config = setup(&dir, &zip, &sha, false);
    let err = format!("{:#}", build(&config).unwrap_err());
    assert!(err.contains("does not name the pinned sha256"), "{err}");
    std::fs::remove_file(config.lock.parent().unwrap().join("evidence/web.md")).unwrap();
    let err = format!("{:#}", build(&config).unwrap_err());
    assert!(
        err.contains("evidence file `evidence/web.md` is missing"),
        "{err}"
    );
}

#[test]
fn the_provenance_gate_refuses_ccel_hosted_files() {
    let dir = scratch("ccel");
    let (zip, sha) = fixture_zip(&dir, &[]);
    let config = setup(&dir, &zip, &sha, true);
    let lock = std::fs::read_to_string(&config.lock).unwrap().replace(
        "evidence = ",
        "origin_url = \"https://www.ccel.org/ccel/henry/mhc.zip\"\nevidence = ",
    );
    std::fs::write(&config.lock, lock).unwrap();
    let err = format!("{:#}", build(&config).unwrap_err());
    assert!(err.contains("ccel.org"), "{err}");
}

#[test]
fn the_size_budget_is_enforced() {
    let dir = scratch("budget");
    let (zip, sha) = fixture_zip(&dir, &[]);
    let config = setup(&dir, &zip, &sha, true);
    let lock = std::fs::read_to_string(&config.lock)
        .unwrap()
        .replace("size_budget_mb = 400", "size_budget_mb = 0");
    std::fs::write(&config.lock, lock).unwrap();
    let err = format!("{:#}", build(&config).unwrap_err());
    assert!(err.contains("size budget"), "{err}");
}

#[test]
fn the_real_lock_passes_the_provenance_gate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let config = Config::defaults(root);
    let lock_file = Lock::load(&config.lock).unwrap();
    let changes = IdChanges::load(&config.id_changes).unwrap();
    lock::check(&lock_file, config.lock.parent().unwrap(), &changes).unwrap();
    let ids: Vec<&str> = lock_file
        .sources
        .iter()
        .map(|s| s.resource_id.as_str())
        .collect();
    assert_eq!(ids, ["kjv-strongs", "bsb", "web", "asv"]);
}
