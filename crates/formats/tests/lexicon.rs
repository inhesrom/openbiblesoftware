//! Lexicon converters against small excerpts of the real sources, compared with golden JSON
//! snapshots ("golden entries", #41).
//!
//! Regenerate the snapshots with `UPDATE_GOLDEN=1 cargo nextest run -p obs-formats` and review
//! the diff. The TBESH meaning column and the Strong's Hebrew sense outline are blanked in the
//! fixtures: the Starter library does not ship them (see `starter/evidence/`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use obs_formats::{
    ImportReport, LexiconOptions, import_abbott_smith, import_bdb, import_strongs_greek,
    import_strongs_hebrew, import_tbes,
};
use obs_model::{Key, Resource, ResourceType, SpanKind};
use obs_scripture::{Book, Passage, StrongsNumber, Verse};
use serde_json::{Value, json};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/assets/lexicon")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn options(id: &str) -> LexiconOptions {
    LexiconOptions {
        resource_id: id.into(),
        title: id.to_uppercase(),
        abbreviation: id.to_uppercase(),
        source_file: id.into(),
        tool_version: "test".into(),
    }
}

fn tbesh() -> (Resource, ImportReport) {
    import_tbes(
        &options("tbesh"),
        &fixture("tbesh-excerpt.txt"),
        &["Meaning".to_string()],
    )
    .unwrap()
}

fn tbesg() -> (Resource, ImportReport) {
    import_tbes(&options("tbesg"), &fixture("tbesg-excerpt.txt"), &[]).unwrap()
}

fn strongs_hebrew() -> (Resource, ImportReport) {
    import_strongs_hebrew(
        &options("strongs-hebrew"),
        &fixture("strongs-hebrew-excerpt.xml"),
    )
    .unwrap()
}

fn strongs_greek() -> (Resource, ImportReport) {
    import_strongs_greek(
        &options("strongs-greek"),
        &fixture("strongs-greek-excerpt.xml"),
    )
    .unwrap()
}

fn bdb() -> (Resource, ImportReport) {
    import_bdb(
        &options("bdb"),
        &fixture("bdb-excerpt.xml"),
        &fixture("bdb-lexical-index-excerpt.xml"),
    )
    .unwrap()
}

fn abbott_smith() -> (Resource, ImportReport) {
    import_abbott_smith(
        &options("abbott-smith"),
        &fixture("abbott-smith-excerpt.xml"),
    )
    .unwrap()
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
            json!({ "id": u.id, "title": u.title, "text": u.text, "spans": spans })
        })
        .collect();
    let keys: Vec<Value> = resource
        .keys
        .iter()
        .map(|k| json!([k.key.to_string(), k.unit, k.alias]))
        .collect();
    let transliterations: Vec<Value> = resource
        .transliterations
        .iter()
        .map(|t| json!([t.transliteration, t.lemma]))
        .collect();
    json!({
        "report": {
            "entries": report.entries,
            "scripture_refs": report.scripture_refs,
            "strongs_links": report.tagged_words,
            "dropped_markup": report.dropped_markup,
            "warnings": report.warnings,
        },
        "content_hash": resource.manifest.content_hash,
        "keys": keys,
        "transliterations": transliterations,
        "units": units,
    })
}

fn check_golden(name: &str, actual: &Value) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("lexicon-{name}.json"));
    let text = serde_json::to_string_pretty(actual).unwrap() + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() || !path.exists() {
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

#[test]
fn golden_entries() {
    for (name, (resource, report)) in [
        ("tbesh", tbesh()),
        ("tbesg", tbesg()),
        ("strongs-hebrew", strongs_hebrew()),
        ("strongs-greek", strongs_greek()),
        ("bdb", bdb()),
        ("abbott-smith", abbott_smith()),
    ] {
        let m = &resource.manifest;
        assert_eq!(m.resource_type, ResourceType::Lexicon, "{name}");
        assert!(m.declares(obs_model::KeyScheme::StrongsNumber), "{name}");
        assert!(m.declares(obs_model::KeyScheme::Lemma), "{name}");
        assert!(!resource.transliterations.is_empty(), "{name}");
        check_golden(name, &snapshot(&resource, &report));
    }
}

/// Unit ids with a Strong's number Key equal to `n`.
fn units_for(r: &Resource, n: &str) -> Vec<String> {
    let n: StrongsNumber = n.parse().unwrap();
    r.keys
        .iter()
        .filter(|k| k.key == Key::StrongsNumber(n))
        .map(|k| k.unit.to_string())
        .collect()
}

#[test]
fn g5485_and_h1254a_are_keys() {
    assert_eq!(units_for(&tbesg().0, "G5485"), ["G5485"]);
    assert_eq!(units_for(&strongs_greek().0, "G5485"), ["G5485"]);
    assert_eq!(units_for(&abbott_smith().0, "G5485"), ["χάρις"]);
    assert_eq!(units_for(&tbesh().0, "H1254A"), ["H1254A"]);
    assert_eq!(units_for(&bdb().0, "H1254A"), ["b.cw.aa"]);
    // Plain Strong's has only the base number; Lexicon lookup falls back to it.
    assert_eq!(units_for(&strongs_hebrew().0, "H1254"), ["H1254"]);
    // Extended entries also answer to their base number, as a secondary Key.
    let (bdb, _) = bdb();
    let base: Vec<(String, bool)> = bdb
        .keys
        .iter()
        .filter(|k| k.key.to_string() == "H1254")
        .map(|k| (k.unit.to_string(), k.alias))
        .collect();
    assert_eq!(base, [("b.cw.aa".into(), true), ("b.cx.aa".into(), true)]);
}

#[test]
fn transliterations_index_the_lemmas() {
    let (r, _) = tbesg();
    assert!(
        r.transliterations
            .iter()
            .any(|t| t.transliteration == "charis" && t.lemma == "χάρις")
    );
    let (r, _) = bdb();
    assert!(
        r.transliterations
            .iter()
            .any(|t| t.transliteration == "bara" && t.lemma == "בָּרָא")
    );
}

#[test]
fn entry_text_carries_scripture_references_and_strongs_links() {
    let (r, _) = abbott_smith();
    let unit = r.units.iter().find(|u| u.id.as_str() == "χάρις").unwrap();
    let col_4_6 = Passage::verse(Verse::new(Book::Colossians, 4, 6));
    let span = unit
        .spans
        .iter()
        .find(|s| matches!(&s.kind, SpanKind::ScriptureRef { targets } if targets.passages() == [col_4_6]))
        .expect("Col 4:6 is a scripture_ref");
    assert_eq!(&unit.text[span.range()], "Col 4:6");
    assert!(unit.spans.iter().any(|s| matches!(
        &s.kind,
        SpanKind::TaggedWord { parts } if parts[0].strongs.to_string() == "H2580"
    )));

    let (r, _) = strongs_greek();
    let unit = r.units.iter().find(|u| u.id.as_str() == "G5485").unwrap();
    let link = unit
        .spans
        .iter()
        .find(|s| matches!(s.kind, SpanKind::TaggedWord { .. }))
        .unwrap();
    assert_eq!(&unit.text[link.range()], "G5463");
}

#[test]
fn the_tbesh_meaning_column_is_left_out_when_skipped() {
    let (r, _) = tbesh();
    assert!(r.units.iter().all(|u| !u.text.contains("omitted")));
    let (with, _) = import_tbes(&options("tbesh"), &fixture("tbesh-excerpt.txt"), &[]).unwrap();
    assert!(with.units.iter().any(|u| u.text.contains("omitted")));
}
