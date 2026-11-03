//! USFM Import against small public-domain excerpts (eBible kjv2006 and WEB) and a synthetic
//! marker-coverage file, compared with golden JSON snapshots.
//!
//! Regenerate the snapshots with `UPDATE_GOLDEN=1 cargo nextest run -p obs-formats` and review
//! the diff. Full-Bible checks run when `OBS_TEST_KJV2006_USFM` / `OBS_TEST_WEB_USFM` point at
//! unzipped eBible USFM directories.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_formats::{ImportReport, UsfmImport, UsfmOptions};
use obs_model::{Resource, SpanKind, UnitId};
use obs_scripture::{Book, Verse};
use serde_json::{Value, json};

fn fixtures(set: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/assets/usfm")
        .join(set)
}

fn import_dir(dir: &Path, id: &str) -> (Resource, ImportReport) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e == "usfm" || e == "SFM" || e == "sfm")
        })
        .collect();
    files.sort();
    let mut import = UsfmImport::new(UsfmOptions {
        resource_id: id.into(),
        title: id.to_uppercase(),
        language: "en".into(),
        source_file: id.into(),
        tool_version: "test".into(),
        ..UsfmOptions::default()
    });
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        import
            .add_book(&name, std::fs::read_to_string(&f).unwrap())
            .unwrap();
    }
    import.finish().unwrap_or_else(|e| panic!("{e}"))
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
    json!({
        "report": {
            "books": report.books.iter().map(|b| json!([b.code, b.chapters, b.verses])).collect::<Vec<_>>(),
            "verses": report.verses,
            "notes": report.notes,
            "tagged_words": report.tagged_words,
            "dropped_markup": report.dropped_markup,
            "warnings": report.warnings,
            "license_missing": report.license_missing,
            "verse_map": report.verse_map.map(|c| json!({ "unmapped": c.unmapped, "absent": c.absent })),
        },
        "content_hash": resource.manifest.content_hash,
        "verses": verses,
        "units": units,
    })
}

fn check_golden(name: &str, actual: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("usfm-{name}.json"));
    let text = serde_json::to_string_pretty(actual).unwrap() + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() || !path.exists() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
        return;
    }
    // Tolerate CRLF checkouts even if .gitattributes is bypassed.
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

#[test]
fn kjv2006_excerpt() {
    let (r, report) = import_dir(&fixtures("kjv2006"), "kjv2006");
    assert_eq!(
        verse_text(&r, Verse::new(Book::Genesis, 1, 1)),
        "In the beginning God created the heaven and the earth."
    );
    assert_eq!(
        verse_text(&r, Verse::new(Book::Psalms, 3, 0)),
        "A Psalm of David, when he fled from Absalom his son."
    );
    // Jn 3:16 has Tagged words with normalized Strong's numbers (source `G2316`, `G0025`→`G25`).
    let jhn = r.unit(&UnitId::book(Book::John)).unwrap();
    let v16 = r
        .verses
        .iter()
        .find(|v| v.verse == Verse::new(Book::John, 3, 16))
        .unwrap();
    let tagged: Vec<(String, String)> = jhn
        .spans
        .iter()
        .filter(|s| s.start >= v16.start && s.end <= v16.end)
        .filter_map(|s| match &s.kind {
            SpanKind::TaggedWord { parts } => Some((
                jhn.text[s.range()].to_string(),
                parts[0].strongs.to_string(),
            )),
            _ => None,
        })
        .collect();
    assert!(
        tagged.contains(&("God".into(), "G2316".into())),
        "{tagged:?}"
    );
    assert!(
        tagged.contains(&("loved".into(), "G25".into())),
        "{tagged:?}"
    );
    // `\nd LORD` keeps the source text and its byte offsets: the span covers exactly "LORD".
    let psa = r.unit(&UnitId::book(Book::Psalms)).unwrap();
    let lord: Vec<&str> = psa
        .spans
        .iter()
        .filter(|s| s.kind == SpanKind::DivineName)
        .map(|s| &psa.text[s.range()])
        .collect();
    assert!(
        !lord.is_empty() && lord.iter().all(|t| *t == "LORD"),
        "{lord:?}"
    );
    assert!(report.license_missing);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    check_golden("kjv2006", &snapshot(&r, &report));
}

#[test]
fn web_excerpt() {
    let (r, report) = import_dir(&fixtures("web"), "web");
    // WEB 3 John has 14 verses; there is no 1:15 as in the KJV (a Verse-map case for #28).
    assert!(verse_text(&r, Verse::new(Book::ThirdJohn, 1, 14)).ends_with("speak face to face."));
    assert!(
        !r.verses
            .iter()
            .any(|v| v.verse == Verse::new(Book::ThirdJohn, 1, 15))
    );
    assert!(verse_text(&r, Verse::new(Book::Matthew, 17, 21)).starts_with("But this kind"));
    // Footnotes and cross-references become note units with zero-width refs.
    assert!(report.notes >= 2);
    assert!(
        r.units
            .iter()
            .flat_map(|u| &u.spans)
            .any(|s| matches!(s.kind, SpanKind::Xref { .. }))
    );
    check_golden("web", &snapshot(&r, &report));
}

#[test]
fn synthetic_marker_coverage() {
    let (r, report) = import_dir(&fixtures("synthetic"), "synthetic");
    let ps = |c, v| Verse::new(Book::Psalms, c, v);
    // A verse range is one Bible Verse; lettered verses belong to their number.
    let v3 = r.verses.iter().find(|v| v.verse == ps(3, 3)).unwrap();
    assert_eq!(v3.last, 4);
    assert_eq!(
        verse_text(&r, ps(3, 5)),
        "A lettered verse, continued. A duplicate verse number."
    );
    // Unknown and introduction markers are counted, not silently lost.
    for marker in ["\\zz", "\\ip"] {
        assert_eq!(report.dropped_markup.get(marker), Some(&1), "{marker}");
    }
    assert!(report.warnings.iter().any(|w| w.contains("appears twice")));
    assert!(report.warnings.iter().any(|w| w.contains("X99")));
    // Books outside the canon are kept as text without verses.
    let tob = r.unit(&UnitId::new("TOB").unwrap()).unwrap();
    assert!(tob.text.contains("Text outside the 66-book canon."));
    assert_eq!(report.books.last().unwrap().book, None);
    // Its verse counts as Unmapped in the Verse map summary.
    assert_eq!(report.verse_map.map(|c| c.unmapped), Some(1));
    assert!(r.verse_map.is_some());
    check_golden("synthetic", &snapshot(&r, &report));
}

/// Number of Bible Verses a USFM source should give: `\v` markers in canonical books plus one
/// title slot per Psalm whose `\d` comes before its first verse.
fn expected_verses(dir: &Path) -> usize {
    let mut total = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "usfm") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let code = obs_formats::usfm_book_code(&text).unwrap_or_default();
        if Book::from_usfm(&code).is_none() {
            continue;
        }
        total += text.matches("\\v ").count();
        if code == "PSA" {
            total += text
                .split("\\c ")
                .skip(1)
                .filter(|ch| match (ch.find("\\d "), ch.find("\\v ")) {
                    (Some(d), Some(v)) => d < v,
                    (Some(_), None) => true,
                    _ => false,
                })
                .count();
        }
    }
    total
}

#[test]
fn full_ebible_sources_when_available() {
    for var in ["OBS_TEST_KJV2006_USFM", "OBS_TEST_WEB_USFM"] {
        let Some(dir) = std::env::var_os(var) else {
            eprintln!("{var} not set; skipping the full-Bible check");
            continue;
        };
        let dir = PathBuf::from(dir);
        let (r, report) = import_dir(&dir, "full");
        assert_eq!(r.verses.len(), expected_verses(&dir), "{var}");
        let map = r.verse_map.as_ref().unwrap();
        // Both are KJV-numbered English Bibles: only WEB's Rom 14:24-26 doxology moves.
        assert_eq!(map.unmapped().count(), 0, "{var}");
        assert!(map.absent().len() <= 1, "{var}: {:?}", map.absent());
        assert!(
            report.warnings.is_empty(),
            "{var}: {:?}",
            &report.warnings[..report.warnings.len().min(10)]
        );
        eprintln!("{var}: {} verses, report:\n{report}", r.verses.len());
    }
}
