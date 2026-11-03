//! SWORD Import against small fixture modules (KJV excerpts with OSIS and GBF markup, and a
//! ThML commentary, a Greek lexicon and an OSIS dictionary written for these tests), compared
//! with golden JSON snapshots.
//!
//! The modules under `tests/assets/sword/modules/` are built from `mods.d/*.conf` and
//! `src/*.imp` by `tests/sword_fixture`. Regenerate them and the snapshots with
//! `UPDATE_GOLDEN=1 cargo nextest run -p obs-formats` and review the diff.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod sword_fixture;

use std::path::Path;

use obs_formats::{ImportError, ImportReport, SwordOptions, SwordSource, import_sword};
use obs_model::{Key, KeyScheme, Resource, ResourceType, SpanKind, UnitId};
use obs_scripture::{Book, Passage, Standard, Verse};
use serde_json::{Value, json};
use sword_fixture::{build, confs, read_imp, root, verse_keys};
use sword_modules::SwordLibrary;

fn update() -> bool {
    std::env::var_os("UPDATE_GOLDEN").is_some()
}

fn import(module: &str) -> (Resource, ImportReport) {
    let source = SwordSource::open(&root()).unwrap();
    let options = SwordOptions {
        tool_version: "test".into(),
        ..SwordOptions::default()
    };
    import_sword(&source, module, &options).unwrap_or_else(|e| panic!("{e}"))
}

/// The committed module files hold exactly what their `.imp` sources say.
#[test]
fn fixture_modules_match_their_sources() {
    if update() {
        for conf in confs() {
            for (path, bytes) in build(&conf) {
                let path = root().join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, bytes).unwrap();
            }
        }
    }
    let lib = SwordLibrary::open(root()).unwrap();
    let norm = |s: &str| String::from_utf8(sword_modules::normalize_entry(s.as_bytes())).unwrap();
    for conf in confs() {
        let name = conf.name();
        let expected = read_imp(&name.to_ascii_lowercase());
        let module = lib.open_module(name).unwrap();
        if let Some(m) = module.as_verse() {
            let actual: Vec<_> = m
                .iter_spans()
                .map(|s| {
                    let s = s.unwrap();
                    (s.first, s.last, s.entry.raw)
                })
                .collect();
            let expected: Vec<_> = expected
                .iter()
                .map(|(k, text)| {
                    let (first, last) = verse_keys(k);
                    (first, last, norm(text))
                })
                .collect();
            assert_eq!(actual, expected, "{name}: rebuild with UPDATE_GOLDEN=1");
        } else if let Some(d) = module.as_dictionary() {
            let actual: Vec<(String, String)> = (0..d.len())
                .map(|i| {
                    let body = match d.link_at(i).unwrap() {
                        Some(target) => format!("@LINK {target}"),
                        None => d.entry_at(i).unwrap().unwrap().raw,
                    };
                    (d.key_at(i).unwrap(), body)
                })
                .collect();
            let mut expected: Vec<(String, String)> =
                expected.iter().map(|(k, v)| (k.clone(), norm(v))).collect();
            expected.sort();
            assert_eq!(actual, expected, "{name}: rebuild with UPDATE_GOLDEN=1");
        } else {
            panic!("{name}: unexpected module kind");
        }
    }
}

fn snapshot(resource: &Resource, report: &ImportReport) -> Value {
    let units: Vec<Value> = resource
        .units
        .iter()
        .map(|u| {
            let spans: Vec<Value> = u
                .spans
                .iter()
                .map(|s| {
                    let mut v = serde_json::to_value(&s.kind).unwrap();
                    v["at"] = json!(format!("{}..{}", s.start, s.end));
                    v["text"] = json!(&u.text[s.range()]);
                    v
                })
                .collect();
            json!({ "id": u.id, "kind": u.kind, "title": u.title, "text": u.text, "spans": spans })
        })
        .collect();
    let verses: Vec<Value> = resource
        .verses
        .iter()
        .map(|v| {
            let unit = resource.unit(&v.unit).unwrap();
            json!({
                "verse": v.verse.to_string(),
                "last": v.last,
                "text": &unit.text[v.range()],
            })
        })
        .collect();
    let keys: Vec<Value> = resource
        .keys
        .iter()
        .map(|k| json!([k.key.to_string(), k.unit, k.alias]))
        .collect();
    let m = &resource.manifest;
    json!({
        "manifest": {
            "resource_id": m.resource_id,
            "title": m.title,
            "abbreviation": m.abbreviation,
            "resource_type": m.resource_type,
            "language": m.language,
            "key_schemes": m.key_schemes,
            "license": m.license,
            "attribution": m.attribution,
            "source_url": m.source_url,
            "source_format": m.source_format,
            "source_file": m.source_file,
        },
        "report": {
            "source_format": report.source_format,
            "books": report.books.iter().map(|b| json!([b.code, b.chapters, b.verses])).collect::<Vec<_>>(),
            "verses": report.verses,
            "entries": report.entries,
            "notes": report.notes,
            "tagged_words": report.tagged_words,
            "dropped_markup": report.dropped_markup,
            "warnings": report.warnings,
            "license_missing": report.license_missing,
            "versification_hint": report.versification_hint,
            "verse_map": report.verse_map.map(|c| json!({ "unmapped": c.unmapped, "absent": c.absent })),
        },
        "content_hash": m.content_hash,
        "keys": keys,
        "verses": verses,
        "units": units,
    })
}

fn check_golden(name: &str, actual: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("sword-{name}.json"));
    let text = serde_json::to_string_pretty(actual).unwrap() + "\n";
    if update() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    assert!(
        expected == text,
        "golden snapshot {} differs; rerun with UPDATE_GOLDEN=1 and review the diff",
        path.display()
    );
}

fn verse_text(r: &Resource, verse: Verse) -> &str {
    let v = r
        .verses
        .iter()
        .find(|v| v.verse == verse)
        .unwrap_or_else(|| panic!("{verse} missing"));
    &r.unit(&v.unit).unwrap().text[v.range()]
}

fn spans_of<'a>(r: &'a Resource, unit: &str, name: &str) -> Vec<&'a str> {
    let u = r.unit(&UnitId::new(unit).unwrap()).unwrap();
    u.spans
        .iter()
        .filter(|s| s.kind.name() == name)
        .map(|s| &u.text[s.range()])
        .collect()
}

/// Acceptance: a public-domain SWORD Bible imports, with sensible dropped-markup counts.
#[test]
fn public_domain_osis_bible_imports_with_sensible_dropped_markup() {
    let (r, report) = import("ObsKjvOsis");
    assert_eq!(r.manifest.resource_type, ResourceType::Bible);
    assert_eq!(r.manifest.license, "LicenseRef-PD");
    assert_eq!(
        verse_text(&r, Verse::new(Book::Genesis, 1, 1)),
        "In the beginning God created the heaven and the earth."
    );
    // Psalm titles fill the title slot, whether they lead verse 1 or the chapter introduction.
    assert_eq!(
        verse_text(&r, Verse::new(Book::Psalms, 3, 0)),
        "A Psalm of David, when he fled from Absalom his son."
    );
    assert_eq!(
        verse_text(&r, Verse::new(Book::Psalms, 3, 1)),
        "LORD, how are they increased that trouble me! many are they that rise up against me."
    );
    assert_eq!(
        verse_text(&r, Verse::new(Book::Psalms, 4, 0)),
        "To the chief Musician on Neginoth, A Psalm of David."
    );
    // A pre-verse heading stays outside its verse; a milestoned quote spans two verses.
    assert!(verse_text(&r, Verse::new(Book::John, 3, 16)).starts_with("For God so loved"));
    assert_eq!(spans_of(&r, "JHN", "heading"), ["God’s love for the world"]);
    let woj = spans_of(&r, "JHN", "woj");
    assert_eq!(woj.len(), 1);
    assert!(woj[0].starts_with("For God") && woj[0].ends_with("might be saved."));
    assert_eq!(spans_of(&r, "PSA", "divine_name"), ["LORD"]);
    assert_eq!(spans_of(&r, "GEN", "added"), ["was"]);
    let jhn = r.unit(&UnitId::book(Book::John)).unwrap();
    let world = jhn
        .spans
        .iter()
        .find_map(|s| match &s.kind {
            SpanKind::TaggedWord { parts } if &jhn.text[s.range()] == "the world" => Some(parts),
            _ => None,
        })
        .unwrap();
    let codes: Vec<String> = world.iter().map(|p| p.strongs.to_string()).collect();
    assert_eq!(codes, ["G3588", "G2889"]);
    assert_eq!(world[1].morph.as_deref(), Some("N-ASM"));
    let xref_books: Vec<Book> = jhn
        .spans
        .iter()
        .find_map(|s| match &s.kind {
            SpanKind::Xref { targets } => Some(targets.iter().map(|p| p.start().book).collect()),
            _ => None,
        })
        .unwrap();
    assert_eq!(xref_books, [Book::Romans, Book::FirstJohn]);
    // Only markup with no place in the span vocabulary is dropped, each counted once.
    let dropped: Vec<(&str, u64)> = report
        .dropped_markup
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(dropped, [("<figure>", 1), ("<q>", 1), ("<seg>", 1)]);
    assert_eq!(report.notes, 2);
    assert_eq!(report.versification_hint.as_deref(), Some("KJV"));
    assert_eq!(report.verse_map.map(|c| c.unmapped), Some(0));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    check_golden("kjv-osis", &snapshot(&r, &report));
}

#[test]
fn gbf_bible_with_linked_verses() {
    let (r, report) = import("ObsKjvGbf");
    let v4 = r
        .verses
        .iter()
        .find(|v| v.verse == Verse::new(Book::John, 1, 4))
        .unwrap();
    assert_eq!(v4.last, 5);
    assert!(verse_text(&r, Verse::new(Book::John, 1, 4)).ends_with("comprehended it not."));
    assert_eq!(
        spans_of(&r, "JHN", "woj"),
        [
            "Verily, verily, I say unto thee, Except a man be born again, he cannot see the kingdom of God."
        ]
    );
    assert_eq!(spans_of(&r, "JHN", "heading"), ["The Word made flesh"]);
    assert_eq!(report.tagged_words, 32);
    let dropped: Vec<&str> = report.dropped_markup.keys().map(String::as_str).collect();
    assert_eq!(dropped, ["<JL>"]);
    check_golden("kjv-gbf", &snapshot(&r, &report));
}

/// Acceptance: a SWORD commentary imports, keyed by Standard Passages.
#[test]
fn thml_commentary_imports_keyed_by_passage() {
    let (r, report) = import("ObsNotes");
    assert_eq!(r.manifest.resource_type, ResourceType::Commentary);
    assert_eq!(r.manifest.key_schemes, [KeyScheme::Passage]);
    assert!(r.verses.is_empty() && r.verse_map.is_none());
    let keyed: Vec<(String, &str)> = r
        .keys
        .iter()
        .map(|k| (k.key.to_string(), k.unit.as_str()))
        .collect();
    assert_eq!(
        keyed,
        [
            ("Gen 1:1".to_string(), "GEN.1.1"),
            ("John 1:1".to_string(), "JHN.1.1"),
            ("John 1:1-51".to_string(), "JHN.1"),
            ("John 1:1-21:25".to_string(), "JHN"),
            ("John 1:4-5".to_string(), "JHN.1.4-5"),
            ("John 3:16".to_string(), "JHN.3.16"),
        ]
    );
    let john_1_4 =
        Passage::new(Verse::new(Book::John, 1, 4), Verse::new(Book::John, 1, 5)).unwrap();
    assert!(r.keys.iter().any(|k| k.key == Key::Passage(john_1_4)));
    // A relative reference resolves against the section's own Passage.
    let refs: Vec<String> = r
        .unit(&UnitId::new("JHN.1.4-5").unwrap())
        .unwrap()
        .spans
        .iter()
        .filter_map(|s| match &s.kind {
            SpanKind::ScriptureRef { targets } => Some(targets.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(refs, ["John 8:12"]);
    assert_eq!(report.entries, 6);
    let dropped: Vec<(&str, u64)> = report
        .dropped_markup
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    assert_eq!(dropped, [("<font>", 1), ("<img>", 1), ("<sup>", 1)]);
    assert!(Passage::chapter(&Standard, Book::John, 1).is_some());
    check_golden("notes-thml", &snapshot(&r, &report));
}

/// Acceptance (data side of the About Tab): DistributionLicense, TextSource and the copyright
/// keys are copied into the manifest's licence, Attribution and source URL.
#[test]
fn licence_fields_are_copied_into_the_manifest() {
    let (r, report) = import("ObsNotes");
    let m = &r.manifest;
    assert_eq!(m.license, "CC0-1.0");
    assert_eq!(
        m.attribution,
        "Licence: Creative Commons: CC0\nText source: https://github.com/inhesrom/openbiblesoftware\n\
         Copyright holder: Open Bible Software contributors\nCopyright date: 2026\n\
         Imported from the SWORD module ObsNotes."
    );
    assert_eq!(
        m.source_url.as_deref(),
        Some("https://github.com/inhesrom/openbiblesoftware")
    );
    assert!(!report.license_missing);

    let (r, _) = import("ObsKjvOsis");
    assert_eq!(r.manifest.license, "LicenseRef-PD");
    assert!(r.manifest.attribution.starts_with(
        "Licence: Public Domain\nText source: King James Version (1769), markup written for the Open Bible Software tests"
    ));
    assert_eq!(r.manifest.source_url, None);

    // No DistributionLicense: the Resource is for personal use and the report says so.
    let (r, report) = import("ObsGreek");
    assert_eq!(r.manifest.license, obs_model::LICENSE_UNKNOWN);
    assert!(report.license_missing);
    assert!(
        report
            .to_string()
            .contains("licence: none found in the source")
    );
}

#[test]
fn greek_lexicon_keyed_by_strongs_number() {
    let (r, report) = import("ObsGreek");
    assert_eq!(r.manifest.resource_type, ResourceType::Lexicon);
    let keys: Vec<String> = r.keys.iter().map(|k| k.key.to_string()).collect();
    assert_eq!(keys, ["G25", "G26"]);
    assert_eq!(
        r.unit(&UnitId::new("G26").unwrap()).unwrap().text,
        "ἀγάπη agapē: love.\nCompare φιλία."
    );
    assert_eq!(report.entries, 2);
    check_golden("greek-thml", &snapshot(&r, &report));
}

#[test]
fn dictionary_keyed_by_headword_with_link_aliases() {
    let (r, report) = import("ObsTopics");
    assert_eq!(r.manifest.resource_type, ResourceType::Dictionary);
    let keys: Vec<(String, &str, bool)> = r
        .keys
        .iter()
        .map(|k| (k.key.to_string(), k.unit.as_str(), k.alias))
        .collect();
    assert_eq!(
        keys,
        [
            ("AARON".to_string(), "AARON", false),
            ("ABRAHAM".to_string(), "ABRAHAM", false),
            ("ABRAM".to_string(), "ABRAHAM", true),
        ]
    );
    assert_eq!(report.dropped_markup.get("<index>"), Some(&1));
    check_golden("topics-osis", &snapshot(&r, &report));
}

#[test]
fn modules_are_listed_and_unknown_ones_named() {
    let source = SwordSource::open(&root()).unwrap();
    let listed: Vec<(String, Option<ResourceType>)> = source
        .modules()
        .into_iter()
        .map(|m| (m.name, m.resource_type))
        .collect();
    assert_eq!(listed.len(), 5);
    assert!(listed.contains(&("ObsNotes".into(), Some(ResourceType::Commentary))));
    assert_eq!(source.default_module(), None);
    let one = SwordSource::open(&root().join("mods.d/obsnotes.conf")).unwrap();
    assert_eq!(one.default_module().as_deref(), Some("ObsNotes"));
    let err = import_sword(&source, "Nope", &SwordOptions::default()).unwrap_err();
    assert!(matches!(err, ImportError::UnknownModule { .. }));
    assert!(err.to_string().contains("ObsKjvOsis"), "{err}");
}

/// Every importable module in a local SWORD folder imports and validates. Structure only, as
/// `docs/dev/local-copyrighted-modules.md` asks; skipped when `OBS_LOCAL_SWORD_DIR` is unset.
#[test]
fn local_sword_modules_import_when_available() {
    let Some(dir) = std::env::var_os("OBS_LOCAL_SWORD_DIR") else {
        return;
    };
    let source = SwordSource::open(Path::new(&dir)).unwrap();
    for m in source.modules() {
        if m.resource_type.is_none() {
            continue;
        }
        let (r, report) = match import_sword(&source, &m.name, &SwordOptions::default()) {
            Ok(done) => done,
            Err(ImportError::UnsupportedModule { .. }) => continue,
            Err(e) => panic!("{}: {e}", m.name),
        };
        assert!(!r.units.is_empty(), "{}", m.name);
        if r.manifest.resource_type == ResourceType::Bible {
            assert!(
                report.verses > 0 && report.verse_map.is_some(),
                "{}",
                m.name
            );
        } else {
            assert!(report.entries > 0, "{}", m.name);
        }
        let dropped: u64 = report.dropped_markup.values().sum();
        eprintln!(
            "{}: {:?}, {} verses, {} entries, {} notes, {dropped} dropped, {} warnings",
            m.name,
            r.manifest.resource_type,
            report.verses,
            report.entries,
            report.notes,
            report.warnings.len()
        );
    }
}
