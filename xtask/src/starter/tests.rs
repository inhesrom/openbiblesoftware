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
            "tahot",
            "tagnt",
        ]
    );
    // STEPBible splits TAHOT over four files and TAGNT over two.
    let files = |id: &str| {
        lock_file
            .sources
            .iter()
            .find(|s| s.resource_id == id)
            .map(|s| s.files().count())
    };
    assert_eq!(files("tahot"), Some(4));
    assert_eq!(files("tagnt"), Some(2));
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

/// The MHC and JFB lock entries proposed by the #39 commentary-source spike, read from its
/// findings note. #44 moves them into `starter/sources.toml` with the converters and deletes
/// this test.
#[test]
fn the_proposed_commentary_lock_entries_pass_the_provenance_gate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let note = std::fs::read_to_string(root.join("docs/research/commentary-sources.md")).unwrap();
    let entries = note
        .split_once("<!-- proposed-lock")
        .and_then(|(_, rest)| rest.split_once("```toml\n"))
        .and_then(|(_, rest)| rest.split_once("```"))
        .map(|(entries, _)| entries)
        .expect("the findings note has a proposed-lock toml block");
    let proposed: Lock = toml::from_str(&format!("size_budget_mb = 400\n{entries}")).unwrap();
    let ids: Vec<&str> = proposed
        .sources
        .iter()
        .map(|s| s.resource_id.as_str())
        .collect();
    assert_eq!(ids, ["mhc", "jfb"]);

    // Check them next to the real entries, so ids must not collide either.
    let config = Config::defaults(root);
    let mut lock_file = Lock::load(&config.lock).unwrap();
    lock_file.sources.extend(proposed.sources);
    let changes = IdChanges::load(&config.id_changes).unwrap();
    let err = lock::check(&lock_file, config.lock.parent().unwrap(), &changes)
        .unwrap_err()
        .to_string();
    // The converters are #44's work; everything else must already pass the gate.
    let problems: Vec<&str> = err.lines().skip(1).map(str::trim).collect();
    assert_eq!(problems.len(), 2, "{err}");
    assert!(
        problems[0].starts_with("mhc: unknown importer `wikisource-mhc`"),
        "{err}"
    );
    assert!(
        problems[1].starts_with("jfb: unknown importer `jfb-html`"),
        "{err}"
    );
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

/// Write a lock with one TAHOT entry split over two excerpt files; returns the Config.
/// `evidence_names_all` false leaves the second file's sha256 out of the evidence.
fn setup_tahot(dir: &Path, evidence_names_all: bool) -> Config {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("crates/formats/tests/assets/stepbible");
    let mut files = Vec::new();
    for name in ["tahot-gen-deu.txt", "tahot-isa-mal.txt"] {
        let to = dir.join(name);
        std::fs::copy(src.join(name), &to).unwrap();
        let sha = sha256_hex(&std::fs::read(&to).unwrap());
        files.push((to.display().to_string().replace('\\', "/"), sha));
    }
    let starter = dir.join("starter");
    std::fs::create_dir_all(starter.join("evidence")).unwrap();
    let named = if evidence_names_all { 2 } else { 1 };
    let evidence: String = files[..named]
        .iter()
        .map(|(_, sha)| format!("sha256 {sha}\n"))
        .collect();
    std::fs::write(
        starter.join("evidence/tahot.md"),
        format!("Licence: CC BY 4.0.\n{evidence}"),
    )
    .unwrap();
    let lock = format!(
        r#"size_budget_mb = 400

[[source]]
resource_id = "tahot"
importer = "tahot"
url = "file://{}"
sha256 = "{}"
title = "Hebrew Old Testament (STEPBible TAHOT)"
abbreviation = "TAHOT"
language = "hbo"
spdx = "CC-BY-4.0"
homepage = "https://STEPBible.github.io/STEPBible-Data/"
attribution = "TAHOT, data created by www.STEPBible.org based on work at Tyndale House Cambridge (CC BY 4.0)."
changes = "Morpheme separators removed; Ketiv readings as notes."
provenance = "The licence notice in each file."
evidence = "evidence/tahot.md"

[[source.extra_file]]
url = "file://{}"
sha256 = "{}"
"#,
        files[0].0, files[0].1, files[1].0, files[1].1
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
fn a_tahot_source_in_several_files_builds_a_hebrew_bible_with_its_changes_note() {
    let dir = scratch("tahot");
    let config = setup_tahot(&dir, true);
    build(&config).unwrap();
    let reader = ObrReader::open(&config.out.join("tahot.obr")).unwrap();
    let m = reader.manifest();
    assert_eq!(m.direction, obs_model::Direction::Rtl);
    assert_eq!(m.language, "hbo");
    assert_eq!(m.license, "CC-BY-4.0");
    assert_eq!(m.source_format, "stepbible-tahot");
    assert_eq!(
        m.source_file.as_deref(),
        Some("tahot-gen-deu.txt; tahot-isa-mal.txt")
    );
    assert_eq!(
        m.source_url.as_deref(),
        Some("https://STEPBible.github.io/STEPBible-Data/")
    );
    // Both files were read: Genesis from the first, Malachi from the second.
    let report = std::fs::read_to_string(config.out.join(REPORT_FILE)).unwrap();
    assert!(report.contains("| `tahot` | 1 |"), "{report}");
    assert!(report.contains("0 unmapped verse(s)"), "{report}");
    let manifest = ReleaseManifest::load(&config.out.join(MANIFEST_FILE)).unwrap();
    let units = &manifest.resources[0].units;
    assert!(units.iter().any(|u| u == "GEN") && units.iter().any(|u| u == "MAL"));
    // CC BY: the changes are published beside the Resource.
    let changes = std::fs::read_to_string(config.out.join("CHANGES-tahot.md")).unwrap();
    assert!(changes.contains("## Changes we made"), "{changes}");
    assert!(changes.contains("Ketiv readings as notes."), "{changes}");
    assert!(changes.contains("Tyndale House Cambridge"), "{changes}");
}

#[test]
fn the_evidence_must_name_every_file_of_a_source() {
    let dir = scratch("tahot-evidence");
    let config = setup_tahot(&dir, false);
    let second = Lock::load(&config.lock).unwrap().sources[0].extra_files[0]
        .sha256
        .clone();
    let err = format!("{:#}", build(&config).unwrap_err());
    assert!(
        err.contains("does not name the pinned sha256") && err.contains(&second),
        "{err}"
    );
}

#[test]
fn a_tampered_extra_file_fails_the_hash_check() {
    let dir = scratch("tahot-tampered-extra");
    let config = setup_tahot(&dir, true);
    std::fs::write(dir.join("tahot-isa-mal.txt"), "tampered").unwrap();
    let err = format!("{:#}", build(&config).unwrap_err());
    assert!(
        err.contains("sha256 mismatch") && err.contains("tahot-isa-mal.txt"),
        "{err}"
    );
    assert!(!config.out.exists(), "no unverified Resource is published");
}

#[test]
fn a_usfm_source_takes_no_extra_files() {
    let dir = scratch("usfm-extra");
    let (zip, sha) = fixture_zip(&dir, &[]);
    let config = setup(&dir, &zip, &sha, true);
    let lock = std::fs::read_to_string(&config.lock).unwrap()
        + &format!(
            "\n[[source.extra_file]]\nurl = \"file://{}\"\nsha256 = \"{sha}\"\n",
            zip.display().to_string().replace('\\', "/")
        );
    std::fs::write(&config.lock, lock).unwrap();
    let err = format!("{:#}", build(&config).unwrap_err());
    assert!(err.contains("takes no extra files"), "{err}");
}
