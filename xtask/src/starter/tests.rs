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
    assert!(!config.out.join("CHANGES-web.md").exists());
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
    assert_eq!(
        ids,
        [
            "kjv-strongs",
            "bsb",
            "web",
            "asv",
            "openbible-xrefs",
            "tipnr",
            "tbesh",
            "tbesg",
            "strongs-hebrew",
            "strongs-greek",
            "bdb",
            "abbott-smith",
            "mhc",
            "jfb",
            "easton",
            "smith",
        ]
    );
}

/// Zip the OpenBible.info excerpt as the publisher does; write the TIPNR excerpt as-is.
/// Returns a lock with both and their evidence.
fn dataset_setup(dir: &Path) -> Config {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("crates/formats/tests/assets");
    let xrefs = dir.join("cross-references.zip");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&xrefs).unwrap());
    zip.start_file(
        "cross_references.txt",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(&std::fs::read(assets.join("openbible/cross-references-excerpt.txt")).unwrap())
        .unwrap();
    zip.finish().unwrap();
    let tipnr = dir.join("TIPNR excerpt.txt");
    std::fs::copy(assets.join("tipnr/tipnr-excerpt.txt"), &tipnr).unwrap();

    let starter = dir.join("starter");
    std::fs::create_dir_all(starter.join("evidence")).unwrap();
    let mut lock = String::from("size_budget_mb = 400\n");
    for (id, importer, path) in [
        ("openbible-xrefs", "openbible-cross-references", &xrefs),
        ("tipnr", "tipnr", &tipnr),
    ] {
        let sha = sha256_hex(&std::fs::read(path).unwrap());
        std::fs::write(
            starter.join(format!("evidence/{id}.md")),
            format!("CC BY 4.0.\nsha256 {sha}\n"),
        )
        .unwrap();
        lock.push_str(&format!(
            r#"
[[source]]
resource_id = "{id}"
importer = "{importer}"
url = "file://{}"
sha256 = "{sha}"
title = "{id}"
abbreviation = "{id}"
language = "en"
spdx = "CC-BY-4.0"
attribution = "Credit {id}."
changes = "Converted."
provenance = "Licence page."
evidence = "evidence/{id}.md"
"#,
            path.display().to_string().replace('\\', "/")
        ));
    }
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
fn cc_by_changes_files_are_complete_deterministic_and_cleaned() {
    let dir = scratch("changes");
    let config = dataset_setup(&dir);
    build(&config).unwrap();
    let lock = Lock::load(&config.lock).unwrap();
    for source in &lock.sources {
        let path = config
            .out
            .join(format!("CHANGES-{}.md", source.resource_id));
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            format!(
                "# Changes we made: {id}\n\nResource: `{id}`\n\nLicence: CC-BY-4.0\n\n\
             ## Attribution\n\nCredit {id}.\n\n\
             ## Source\n\n{url}\n\nPinned download: {url}\n\nSHA-256: `{sha}`\n\n\
             ## Changes we made\n\nConverted.\n",
                id = source.resource_id,
                url = source.url,
                sha = source.sha256,
            )
        );
    }
    let second = Config {
        out: dir.join("out2"),
        ..config.clone()
    };
    build(&second).unwrap();
    compare_dirs(&config.out, &second.out).unwrap();

    // Rebuild removes stale notices, and rewrites current notices from the lock.
    std::fs::write(config.out.join("CHANGES-retired.md"), "stale").unwrap();
    std::fs::write(config.out.join("CHANGES-tipnr.md"), "stale").unwrap();
    build(&config).unwrap();
    compare_dirs(&config.out, &second.out).unwrap();

    // Cleanup owns only build outputs, not arbitrary Markdown files.
    std::fs::write(config.out.join("README.md"), "keep").unwrap();
    clean_outputs(&config.out).unwrap();
    assert_eq!(
        std::fs::read_to_string(config.out.join("README.md")).unwrap(),
        "keep"
    );
    assert!(!config.out.join("CHANGES-openbible-xrefs.md").exists());
    assert!(!config.out.join("CHANGES-tipnr.md").exists());
}

#[test]
fn every_cross_reference_target_in_the_built_obr_is_a_standard_reference() {
    use obs_scripture::{Standard, Versification};
    let dir = scratch("xrefs");
    let config = dataset_setup(&dir);
    build(&config).unwrap();
    let reader = ObrReader::open(&config.out.join("openbible-xrefs.obr")).unwrap();
    let m = reader.manifest();
    assert_eq!(m.resource_type, obs_model::ResourceType::CrossReferenceSet);
    assert_eq!(m.license, "CC-BY-4.0");
    assert_eq!(m.source_file.as_deref(), Some("cross-references.zip"));
    let resource = reader.load().unwrap();
    assert_eq!(resource.cross_references.len(), 23);
    for x in &resource.cross_references {
        for p in [x.from, x.to] {
            assert!(
                Standard.contains(p.start()) && Standard.contains(p.end()),
                "{} → {}",
                x.from,
                x.to
            );
        }
    }
    let report = std::fs::read_to_string(config.out.join(REPORT_FILE)).unwrap();
    assert!(report.contains("23 cross-reference(s)"), "{report}");
    assert!(report.contains("12 person(s) or place(s)"), "{report}");
}

#[test]
fn no_tipnr_description_text_appears_in_the_built_obr() {
    let dir = scratch("tipnr");
    let config = dataset_setup(&dir);
    build(&config).unwrap();
    let path = config.out.join("tipnr.obr");
    let reader = ObrReader::open(&path).unwrap();
    assert_eq!(
        reader.manifest().resource_type,
        obs_model::ResourceType::PeopleAndPlaces
    );
    assert_eq!(
        reader.manifest().source_file.as_deref(),
        Some("TIPNR excerpt.txt")
    );
    let abraham = reader.entity("Abraham@Gen.11.26-1Pe").unwrap().unwrap();
    assert_eq!(abraham.other_names, ["Abram"]);

    let source = std::fs::read_to_string(dir.join("TIPNR excerpt.txt")).unwrap();
    let obr = std::fs::read(&path).unwrap();
    let descriptions = obs_formats::tipnr_description_texts(&source).unwrap();
    assert!(
        obr.windows(7).any(|w| w == b"Abraham"),
        "the scan sees kept text"
    );
    let mut checked = 0;
    for (column, texts) in &descriptions {
        for t in texts {
            // Whole texts, and their first words in case a writer cut one short.
            let head: String = t.chars().take(24).collect();
            for needle in [t.as_str(), head.as_str()] {
                assert!(
                    !obr.windows(needle.len()).any(|w| w == needle.as_bytes()),
                    "{column} text in tipnr.obr: {needle}"
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 50, "only {checked} description texts checked");
}

/// Golden text/spans from the real pinned editions, round-tripped through `.obr`.
/// No network: small excerpts are checked in alongside their source/evidence notes.
#[test]
fn study_golden_sections_open_by_passage_and_dictionary_alias() {
    use obs_formats::{
        DictionaryData, StudyImport, import_dictionary, import_jfb_book, import_mhc,
    };
    use obs_model::{Key, Manifest, ResourceType, SpanKind};
    use obs_scripture::{Book, Standard, parse_reference};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let fixtures = root.join("crates/formats/tests/fixtures/study");
    let bytes = |name: &str| std::fs::read(fixtures.join(name)).unwrap();
    let text = |name: &str| std::fs::read_to_string(fixtures.join(name)).unwrap();
    let dir = scratch("study-goldens");
    for id in ["mhc", "jfb", "easton", "smith"] {
        let commentary = matches!(id, "mhc" | "jfb");
        let mut import = StudyImport::new(Manifest::new(
            id,
            id,
            id,
            if commentary {
                ResourceType::Commentary
            } else {
                ResourceType::Dictionary
            },
            "en",
        ));
        match id {
            "mhc" => import_mhc(&mut import, &text("mhc.xml")).unwrap(),
            "jfb" => {
                import_jfb_book(&mut import, Book::Genesis, &text("jfb-genesis-1.htm")).unwrap()
            }
            "easton" => import_dictionary(
                &mut import,
                DictionaryData::Easton {
                    index: &bytes("easton.idx"),
                    data: &bytes("easton.dat"),
                    block_index: &bytes("easton.zdx"),
                    blocks: &bytes("easton.zdt"),
                },
            )
            .unwrap(),
            "smith" => import_dictionary(
                &mut import,
                DictionaryData::Smith {
                    index: &bytes("smith.idx"),
                    data: &bytes("smith.dat"),
                },
            )
            .unwrap(),
            _ => unreachable!(),
        }
        let (resource, _) = import.finish().unwrap();
        let path = dir.join(format!("{id}.obr"));
        write_obr(&resource, &path).unwrap();
        let reader = ObrReader::open(&path).unwrap();
        assert_eq!(reader.load().unwrap(), resource);
        let key = if commentary {
            Key::Passage(
                parse_reference(
                    if id == "mhc" { "Gen 1:1-2" } else { "Gen 1:1" },
                    &Standard,
                    None,
                )
                .unwrap()
                .passages()[0],
            )
        } else {
            Key::Headword("Abraham".into())
        };
        let hit = reader.lookup_key(&key).unwrap().remove(0);
        let unit = reader.unit(&hit.unit).unwrap().unwrap();
        assert!(
            unit.spans
                .iter()
                .any(|s| matches!(s.kind, SpanKind::ScriptureRef { .. })),
            "{id}"
        );
        let actual = serde_json::to_string_pretty(&unit).unwrap() + "\n";
        let golden = fixtures.join(format!("golden-{id}.json"));
        if std::env::var_os("UPDATE_STUDY_GOLDENS").is_some() {
            std::fs::write(&golden, &actual).unwrap();
        }
        assert_eq!(actual, std::fs::read_to_string(golden).unwrap(), "{id}");
        if !commentary {
            let aliases = reader.lookup_key(&Key::Headword("Abram".into())).unwrap();
            assert!(aliases.iter().any(|k| k.alias && k.unit == unit.id), "{id}");
            assert!(
                aliases
                    .iter()
                    .any(|k| !k.alias && k.unit.as_str() == "Abram"),
                "original short entry stays"
            );
        }
    }
}

/// Full-edition coverage, Standard reference validation and byte determinism. Explicitly run
/// after `xtask starter` populates the pinned cache; CI's offline tests use the goldens above.
#[test]
#[ignore = "requires the full pinned Starter downloads in target/starter-cache"]
fn pinned_study_editions_have_complete_coverage_and_stable_bytes() {
    use obs_model::{Key, SpanKind};
    use obs_scripture::{Book, Standard, Verse, Versification};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let config = Config::defaults(root);
    let lock = Lock::load(&config.lock).unwrap();
    let dir = scratch("study-full");
    for source in lock
        .sources
        .iter()
        .filter(|s| matches!(s.resource_id.as_str(), "mhc" | "jfb" | "easton" | "smith"))
    {
        let bytes =
            std::fs::read(config.cache.join(&source.sha256)).expect("run xtask starter first");
        assert_eq!(sha256_hex(&bytes), source.sha256);
        let files = vec![bytes];
        let built = build_one(source, &files, None, &IdChanges::default()).unwrap();
        let resource = built.resource;
        let id = &source.resource_id;
        for unit in &resource.units {
            for span in &unit.spans {
                if let SpanKind::ScriptureRef { targets } = &span.kind {
                    for p in targets.iter() {
                        assert!(
                            Standard.contains(p.start()) && Standard.contains(p.end()),
                            "{id}: {p}"
                        );
                    }
                }
            }
        }
        if matches!(id.as_str(), "mhc" | "jfb") {
            let chapters: BTreeSet<_> = resource
                .keys
                .iter()
                .filter(|k| !k.unit.as_str().ends_with(".book-intro"))
                .filter_map(|k| match k.key {
                    Key::Passage(p) => Some((p.start().book, p.start().chapter)),
                    _ => None,
                })
                .collect();
            assert_eq!(chapters.len(), 1189, "{id}");
            assert_eq!(
                chapters
                    .iter()
                    .map(|(b, _)| b)
                    .collect::<BTreeSet<_>>()
                    .len(),
                66
            );
            let covered: BTreeSet<_> = resource
                .keys
                .iter()
                .filter(|k| !k.unit.as_str().contains("intro"))
                .flat_map(|k| match k.key {
                    Key::Passage(p) => p
                        .verses(&Standard)
                        .filter(|v| v.verse > 0)
                        .collect::<Vec<_>>(),
                    _ => vec![],
                })
                .collect();
            if id == "jfb" {
                assert_eq!(
                    covered.len(),
                    31102,
                    "missing {:?}",
                    Standard
                        .verses()
                        .filter(|v| v.verse > 0 && !covered.contains(v))
                        .collect::<Vec<_>>()
                );
            } else {
                assert_eq!(covered.len(), 31061);
                for v in 9..=43 {
                    assert!(!covered.contains(&Verse::new(Book::Luke, 18, v)));
                }
                assert!(
                    resource
                        .manifest
                        .changes
                        .as_deref()
                        .unwrap()
                        .contains("Luke 18:9-43")
                );
                assert!(
                    resource
                        .manifest
                        .attribution
                        .contains("Christian Classics Ethereal Library")
                );
            }
        }
        let first = dir.join(format!("{id}-a.obr"));
        let second = dir.join(format!("{id}-b.obr"));
        write_obr(&resource, &first).unwrap();
        let rebuilt = build_one(source, &files, None, &IdChanges::default()).unwrap();
        write_obr(&rebuilt.resource, &second).unwrap();
        assert_eq!(
            std::fs::read(first).unwrap(),
            std::fs::read(second).unwrap(),
            "{id}"
        );
    }
}

/// Write a lock with the six Lexicon fixtures (excerpts of the real sources) and their evidence.
fn setup_lexicons(dir: &Path) -> Config {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("crates/formats/tests/assets/lexicon");
    let starter = dir.join("starter");
    std::fs::create_dir_all(starter.join("evidence")).unwrap();
    let pin = |file: &str| {
        let path = assets.join(file);
        let sha = sha256_hex(&std::fs::read(&path).unwrap());
        let url = format!("file://{}", path.display().to_string().replace('\\', "/"));
        (url, sha)
    };
    let mut lock = String::from("size_budget_mb = 400\n");
    let mut evidence = String::from("Licences checked for the test fixtures.\n");
    for (id, importer, file, extra) in [
        ("tbesh", "stepbible-tbes", "tbesh-excerpt.txt", None),
        ("tbesg", "stepbible-tbes", "tbesg-excerpt.txt", None),
        (
            "strongs-hebrew",
            "strongs-hebrew",
            "strongs-hebrew-excerpt.xml",
            None,
        ),
        (
            "strongs-greek",
            "strongs-greek",
            "strongs-greek-excerpt.xml",
            None,
        ),
        (
            "bdb",
            "oshb-bdb",
            "bdb-excerpt.xml",
            Some("bdb-lexical-index-excerpt.xml"),
        ),
        (
            "abbott-smith",
            "abbott-smith",
            "abbott-smith-excerpt.xml",
            None,
        ),
    ] {
        let (url, sha) = pin(file);
        evidence.push_str(&format!("{id}: sha256 {sha}\n"));
        let spdx = if ["tbesh", "tbesg", "bdb"].contains(&id) {
            "CC-BY-4.0"
        } else {
            "LicenseRef-PublicDomain"
        };
        lock.push_str(&format!(
            r#"
[[source]]
resource_id = "{id}"
importer = "{importer}"
url = "{url}"
sha256 = "{sha}"
title = "{id}"
abbreviation = "{id}"
language = "en"
spdx = "{spdx}"
attribution = "{id} fixture Attribution."
changes = "Converted {id} fixture."
provenance = "Test fixture."
evidence = "evidence/lexicons.md"
"#
        ));
        if id == "tbesh" {
            lock.push_str("skip_columns = [\"Meaning\"]\n");
        }
        if let Some(extra) = extra {
            let (url, sha) = pin(extra);
            evidence.push_str(&format!("{id} extra file: sha256 {sha}\n"));
            lock.push_str(&format!(
                "\n[[source.extra_file]]\nurl = \"{url}\"\nsha256 = \"{sha}\"\n"
            ));
        }
    }
    std::fs::write(starter.join("evidence/lexicons.md"), evidence).unwrap();
    std::fs::write(starter.join("sources.toml"), lock).unwrap();
    Config {
        lock: starter.join("sources.toml"),
        id_changes: starter.join("id-changes.toml"),
        previous: None,
        out: dir.join("out"),
        cache: dir.join("cache"),
    }
}

/// Unit ids a Strong's number resolves to in one built Lexicon, with the extended → base
/// fallback.
fn resolve(config: &Config, id: &str, number: &str) -> Vec<String> {
    let reader = ObrReader::open(&config.out.join(format!("{id}.obr"))).unwrap();
    reader
        .lookup_strongs(number.parse().unwrap())
        .unwrap()
        .into_iter()
        .map(|e| e.unit.to_string())
        .collect()
}

#[test]
fn g5485_and_h1254a_resolve_in_the_built_lexicons() {
    let dir = scratch("lexicons");
    let config = setup_lexicons(&dir);
    build(&config).unwrap();
    assert_eq!(resolve(&config, "tbesg", "G5485"), ["G5485"]);
    assert_eq!(resolve(&config, "strongs-greek", "G5485"), ["G5485"]);
    assert_eq!(resolve(&config, "abbott-smith", "G5485"), ["χάρις"]);
    let reader = ObrReader::open(&config.out.join("abbott-smith.obr")).unwrap();
    let g1061 = reader.lookup_strongs("G1061".parse().unwrap()).unwrap();
    let full = g1061
        .iter()
        .find(|k| k.unit.as_str() == "γαμίζω")
        .expect("G1061 resolves the full definition, not only the abbreviated variant");
    assert!(
        reader
            .unit(&full.unit)
            .unwrap()
            .unwrap()
            .text
            .contains("to give in marriage")
    );
    assert_eq!(resolve(&config, "tbesh", "H1254A"), ["H1254A"]);
    assert_eq!(resolve(&config, "bdb", "H1254A"), ["b.cw.aa"]);
    // Strong's Hebrew has no H1254A entry: lookup falls back to the base number.
    assert_eq!(resolve(&config, "strongs-hebrew", "H1254A"), ["H1254"]);
    // TBESH has no H1254 entry, but its extended entries answer to their base number.
    assert_eq!(resolve(&config, "tbesh", "H1254"), ["H1254A", "H1254B"]);

    let reader = ObrReader::open(&config.out.join("tbesg.obr")).unwrap();
    assert_eq!(
        reader.lemmas_for_transliteration("charis").unwrap(),
        ["χάρις"]
    );
    let m = reader.manifest();
    assert_eq!(m.resource_type, obs_model::ResourceType::Lexicon);
    assert_eq!(m.origin, obs_model::Origin::Starter);
    let report = std::fs::read_to_string(config.out.join(REPORT_FILE)).unwrap();
    assert!(report.contains("| `abbott-smith` | 1 |"), "{report}");
    assert!(report.contains("Scripture reference(s)"), "{report}");
}

#[test]
fn lexicon_entry_text_renders_with_working_scripture_references() {
    use obs_layout::{ColorRole, SpanIndex, TextSettings, block_paragraph, unit_blocks};
    use obs_model::SpanKind;
    use obs_scripture::{Book, Passage, Standard, Verse};

    let dir = scratch("lexicon-render");
    let config = setup_lexicons(&dir);
    build(&config).unwrap();
    let reader = ObrReader::open(&config.out.join("abbott-smith.obr")).unwrap();
    let entry = &reader.lookup_strongs("G5485".parse().unwrap()).unwrap()[0];
    let unit = reader.unit(&entry.unit).unwrap().unwrap();

    // The reference is drawn as a link…
    let settings = TextSettings::default();
    let links: Vec<String> = unit_blocks(&unit)
        .iter()
        .flat_map(|b| block_paragraph(&unit, b, reader.manifest().direction, &settings).runs)
        .filter(|r| r.style.role == ColorRole::Link)
        .map(|r| r.text)
        .collect();
    assert!(links.iter().any(|t| t == "Col 4:6"), "{links:?}");

    // …and hit-testing it gives a Passage that exists in the Standard versification.
    let at = unit.text.find("Col 4:6").unwrap();
    let hit = SpanIndex::new(&unit);
    let spans = hit.at(&unit, at + 1);
    let Some(SpanKind::ScriptureRef { targets }) = spans.first().map(|s| &s.kind) else {
        panic!("no scripture_ref under Col 4:6: {spans:?}");
    };
    assert_eq!(
        targets.passages(),
        [Passage::verse(Verse::new(Book::Colossians, 4, 6))]
    );
    // Every reference in every built Lexicon entry points at real Standard Verses.
    for id in ["tbesg", "bdb", "abbott-smith"] {
        let reader = ObrReader::open(&config.out.join(format!("{id}.obr"))).unwrap();
        let mut refs = 0;
        for unit_id in reader.unit_ids().unwrap() {
            for span in reader.unit(&unit_id).unwrap().unwrap().spans {
                if let SpanKind::ScriptureRef { targets } = span.kind {
                    refs += 1;
                    for p in targets.iter() {
                        assert!(p.verses(&Standard).next().is_some(), "{id} {unit_id}: {p}");
                    }
                }
            }
        }
        assert!(refs > 0, "{id} has no Scripture references");
    }
}

#[test]
fn extra_files_are_pinned_and_covered_by_the_evidence() {
    let dir = scratch("extra-file");
    let config = setup_lexicons(&dir);
    let evidence = config.lock.parent().unwrap().join("evidence/lexicons.md");
    let text = std::fs::read_to_string(&evidence).unwrap();
    let kept: String = text
        .lines()
        .filter(|l| !l.contains("extra file"))
        .map(|l| format!("{l}\n"))
        .collect();
    std::fs::write(&evidence, kept).unwrap();
    let err = format!("{:#}", build(&config).unwrap_err());
    assert!(err.contains("bdb: evidence"), "{err}");

    // The BDB converter needs its lexical index.
    let dir = scratch("extra-file-missing");
    let config = setup_lexicons(&dir);
    let lock = std::fs::read_to_string(&config.lock).unwrap();
    let cut = lock.find("\n[[source.extra_file]]").unwrap();
    let rest = &lock[cut + 1..];
    let next = rest.find("\n[[source]]").unwrap();
    std::fs::write(&config.lock, format!("{}{}", &lock[..cut], &rest[next..])).unwrap();
    let err = format!("{:#}", build(&config).unwrap_err());
    assert!(
        err.contains("reads 1 extra file(s); the lock lists 0"),
        "{err}"
    );
}

#[test]
fn cc_by_changes_artifacts_record_lock_metadata_and_are_deterministic() {
    let dir = scratch("changes");
    let config = setup_lexicons(&dir);
    let lock = Lock::load(&config.lock).unwrap();
    build(&config).unwrap();
    for source in &lock.sources {
        let path = config
            .out
            .join(format!("CHANGES-{}.md", source.resource_id));
        if ["tbesh", "tbesg", "bdb"].contains(&source.resource_id.as_str()) {
            assert_eq!(
                std::fs::read_to_string(path).unwrap(),
                format!(
                    "# Changes we made: {}\n\nResource: `{}`\n\nLicence: CC-BY-4.0\n\n## Attribution\n\n{} fixture Attribution.\n\n## Source\n\n{}\n\nPinned download: {}\n\nSHA-256: `{}`\n\n## Changes we made\n\nConverted {} fixture.\n",
                    source.title,
                    source.resource_id,
                    source.resource_id,
                    source.url,
                    source.url,
                    source.sha256,
                    source.resource_id,
                )
            );
        } else {
            assert!(
                !path.exists(),
                "public-domain source needs no changes artifact"
            );
        }
    }
    let second = Config {
        out: dir.join("out2"),
        ..config.clone()
    };
    build(&second).unwrap();
    compare_dirs(&config.out, &second.out).unwrap();
    // Reusing an output directory must remove stale generated changes files.
    std::fs::write(config.out.join("CHANGES-removed.md"), "stale").unwrap();
    build(&config).unwrap();
    compare_dirs(&config.out, &second.out).unwrap();
}
