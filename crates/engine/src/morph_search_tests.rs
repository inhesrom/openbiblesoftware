//! Morphology search (#145) in the Search Tool's Bible mode, over real TAGNT tagging of a few
//! Verses of Romans and John: `@aorist imperative` scoped to Romans, the matching word
//! highlighted and listed by the Analysis, criteria tied to a Strong's number or Lemma, the
//! Morphology chooser's query giving the same results as the typed one, and an ignored check
//! of the whole of Romans in the real Starter TAGNT.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_search::{MorphCategory, MorphCriteria};
use serde_json::json;

use crate::testing::bibles::write_library;
use crate::testing::tagnt_romans;
use crate::*;

/// A library of the TAGNT excerpt, indexed.
fn fixture(name: &str) -> (Session, PathBuf) {
    let root = std::env::temp_dir().join(format!("obs-morph-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_library(&root.join("library"), &[tagnt_romans()]);
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.set_search_cache_dir(root.join("cache"));
    s.refresh_search_indexes();
    settle(&mut s);
    (s, root)
}

fn settle(s: &mut Session) {
    for _ in 0..1000 {
        s.jobs().pump(16);
        assert!(s.poll_imports().is_empty(), "no stray events");
        if !s.search.is_building()
            && !s.bible_search_running()
            && !s.bible_search.is_reading_ahead()
        {
            return;
        }
    }
    panic!("jobs never settled");
}

fn cleanup(s: Session, root: PathBuf) {
    drop(s);
    let _ = std::fs::remove_dir_all(root);
}

/// Each row as shown: its reference and the highlighted words.
fn shown(s: &mut Session, text: &str, scope: &str) -> Vec<(String, Vec<String>)> {
    let out = s
        .run(
            "search.run",
            &json!({ "text": text, "scope": scope, "bibles": "tagnt-rom", "mode": "bible" }),
        )
        .unwrap_or_else(|e| panic!("{text}: {e:?}"));
    settle(s);
    let tab = TabId(out["tab"].as_u64().unwrap());
    let st = s.search_tab(tab).unwrap();
    assert!(st.error.is_none(), "{text}: {:?}", st.error);
    let r = st.results.clone().expect("results");
    r.rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let sn = st.snippet(i, 0).expect("snippet of the first page");
            let words = sn
                .highlights
                .iter()
                .map(|h| sn.text[h.clone()].to_string())
                .collect();
            (r.row_reference(row), words)
        })
        .collect()
}

fn rows(list: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
    list.iter()
        .map(|(r, w)| (r.to_string(), w.iter().map(|w| w.to_string()).collect()))
        .collect()
}

/// "All aorist imperatives in Romans": the Verses with an aorist imperative, that word
/// highlighted; Romans 15:7's aorist and its imperative are two different words.
#[test]
fn aorist_imperatives_in_romans_highlight_the_imperative() {
    let (mut s, root) = fixture("romans");
    let want = rows(&[
        ("Romans 6:13", &["παραστήσατε"]),
        ("Romans 13:14", &["ἐνδύσασθε"]),
        ("Romans 16:16", &["ἀσπάσασθε"]),
    ]);
    assert_eq!(shown(&mut s, "@aorist imperative", "Rom"), want);
    // The whole Bible adds John 15:9's μείνατε.
    let whole = shown(&mut s, "@aorist imperative", "");
    assert_eq!(whole[0], rows(&[("John 15:9", &["μείνατε"])])[0]);
    assert_eq!(whole[1..], want[..]);
    // Every imperative: 12:2 and 15:7 have present ones; 6:13 has both kinds.
    assert_eq!(
        shown(&mut s, "@imperative", "Rom"),
        rows(&[
            ("Romans 6:13", &["παριστάνετε", "παραστήσατε"]),
            ("Romans 12:2", &["συσχηματίζεσθε", "μεταμορφοῦσθε"]),
            ("Romans 13:14", &["ἐνδύσασθε", "ποιεῖσθε"]),
            ("Romans 15:7", &["προσλαμβάνεσθε"]),
            ("Romans 16:16", &["ἀσπάσασθε"]),
        ])
    );
    cleanup(s, root);
}

/// A Strong's number or Lemma before `@` is the same word's; words and NOT work alongside.
#[test]
fn criteria_combine_with_strongs_lemma_words_and_not() {
    let (mut s, root) = fixture("combine");
    assert_eq!(
        shown(&mut s, "strong:G3936 @aorist imperative", "Rom"),
        rows(&[("Romans 6:13", &["παραστήσατε"])])
    );
    assert_eq!(
        shown(&mut s, "lemma:προσλαμβάνω @aorist", "Rom"),
        rows(&[("Romans 15:7", &["προσελάβετο"])])
    );
    // προσλαμβάνω is never an aorist imperative, although 15:7 has both.
    assert!(shown(&mut s, "lemma:προσλαμβάνω @aorist imperative", "Rom").is_empty());
    assert_eq!(
        shown(&mut s, "@imperative -@aorist", "Rom"),
        rows(&[(
            "Romans 12:2",
            &["συσχηματίζεσθε", "μεταμορφοῦσθε"] as &[&str]
        )])
    );
    assert_eq!(
        shown(&mut s, "ἀλληλους @aorist imperative", "Rom"),
        rows(&[("Romans 16:16", &["ἀσπάσασθε", "ἀλλήλους"])])
    );
    assert_eq!(
        shown(&mut s, "G26 @noun dative", ""),
        rows(&[("John 15:9", &["ἀγάπῃ"])])
    );
    cleanup(s, root);
}

/// The Morphology chooser's query and the same criteria typed (in any order, any spelling the
/// grammar takes) give the same results.
#[test]
fn the_chooser_built_query_and_the_typed_query_give_identical_results() {
    let (mut s, root) = fixture("chooser");
    let mut chooser = MorphCriteria::default();
    chooser.set(MorphCategory::PartOfSpeech, Some("verb"));
    chooser.set(MorphCategory::Tense, Some("aorist"));
    chooser.set(MorphCategory::Mood, Some("imperative"));
    let built = chooser.query_text();
    assert_eq!(built, "@verb aorist imperative");
    let from_chooser = shown(&mut s, &built, "Rom");
    assert_eq!(from_chooser.len(), 3);
    for typed in [
        "@aorist imperative verb",
        "morph:verb aorist imperative",
        "@verb aorist imperative",
    ] {
        assert_eq!(shown(&mut s, typed, "Rom"), from_chooser, "{typed}");
    }
    // Added after a Strong's number, as the chooser adds to what is typed.
    let mut genitive = MorphCriteria::default();
    genitive.set(MorphCategory::Case, Some("dative"));
    genitive.set(MorphCategory::PartOfSpeech, Some("noun"));
    assert_eq!(
        shown(&mut s, &format!("G26 {}", genitive.query_text()), ""),
        shown(&mut s, "strong:G26 @dat noun", "")
    );
    cleanup(s, root);
}

/// The Analysis lists the matched words with their Morphology in plain English.
#[test]
fn analysis_lists_the_imperatives_found() {
    let (mut s, root) = fixture("analysis");
    let out = s
        .run(
            "search.run",
            &json!({ "text": "@aorist imperative", "scope": "Rom", "bibles": "tagnt-rom", "mode": "bible" }),
        )
        .unwrap();
    settle(&mut s);
    let tab = TabId(out["tab"].as_u64().unwrap());
    s.run("search.view", &json!({ "tab": tab.0, "value": "analysis" }))
        .unwrap();
    settle(&mut s);
    let st = s.search_tab(tab).unwrap();
    let a = st.analysis.clone().expect("the Analysis is made");
    let got: Vec<(String, String, String)> = st
        .analysis_order
        .iter()
        .map(|&i| {
            let r = &a.rows[i];
            (r.word.clone(), r.lemma.clone(), r.morph_codes.clone())
        })
        .collect();
    let s3 = |a: &str, b: &str, c: &str| (a.to_string(), b.to_string(), c.to_string());
    assert_eq!(
        got,
        [
            s3("παραστήσατε", "παρίστημι", "V-2AAM-2P"),
            s3("ἐνδύσασθε", "ἐνδύω", "V-AMM-2P"),
            s3("ἀσπάσασθε", "ἀσπάζομαι", "V-ADM-2P"),
        ]
    );
    cleanup(s, root);
}

/// The real Starter TAGNT, when this machine has it: every Verse of Romans with an aorist
/// imperative, found by reading the codes directly (Robinson `V-` + `A` or `2A` tense +
/// `M` mood), is what `@aorist imperative` finds, within the #19 budget. Run with
/// `cargo test --release -p obs-engine real_starter_tagnt -- --ignored --nocapture`.
#[test]
#[ignore = "needs the Starter library in the platform data folder"]
fn real_starter_tagnt_aorist_imperatives_in_romans() {
    use obs_model::SpanKind;
    use obs_scripture::Book;
    use obs_store::ResourceStore;

    let Some(dir) = default_library_dir().filter(|d| d.join("tagnt.obr").is_file()) else {
        eprintln!("no Starter library: skipped");
        return;
    };
    let path = dir.join("tagnt.obr");
    // Expected, without the Morphology decoder or the index.
    let resource = obs_store::ObrReader::open(&path).unwrap().load().unwrap();
    let mut expected: Vec<(u16, u16)> = Vec::new();
    for v in resource
        .verses
        .iter()
        .filter(|v| v.verse.book == Book::Romans)
    {
        let unit = resource.units.iter().find(|u| u.id == v.unit).unwrap();
        let range = v.range();
        let aorist_imperative = unit.spans.iter().any(|s| {
            (range.start..range.end).contains(&(s.start as usize))
                && matches!(&s.kind, SpanKind::TaggedWord { parts } if parts.iter().any(|p| {
                    let code = p.morph.as_deref().unwrap_or("");
                    let tvm = code.strip_prefix("V-").and_then(|r| r.split('-').next());
                    tvm.is_some_and(|t| {
                        let t = t.strip_prefix('2').unwrap_or(t);
                        t.len() == 3 && t.starts_with('A') && t.ends_with('M')
                    })
                }))
        });
        if aorist_imperative {
            expected.push((v.verse.chapter, v.verse.verse));
        }
    }
    let mut s = Session::new(JobMode::Inline);
    let id = s
        .library
        .add_file(&path)
        .unwrap()
        .manifest
        .resource_id
        .clone();
    s.set_search_cache_dir(std::env::temp_dir().join("obs-real-search-cache"));
    s.refresh_search_indexes();
    settle(&mut s);
    let started = std::time::Instant::now();
    let out = s
        .run(
            "search.run",
            &json!({ "text": "@aorist imperative", "scope": "Rom", "bibles": id, "mode": "bible" }),
        )
        .unwrap();
    settle(&mut s);
    let elapsed = started.elapsed();
    let tab = TabId(out["tab"].as_u64().unwrap());
    let r = s.search_tab(tab).unwrap().results.clone().unwrap();
    let found: Vec<(u16, u16)> = r
        .rows
        .iter()
        .map(|row| {
            let v = row.hits[0].native;
            (v.chapter, v.verse)
        })
        .collect();
    eprintln!(
        "{} in {elapsed:?} (index {} ms): {found:?}",
        r.summary(),
        r.elapsed_ms
    );
    assert_eq!(found, expected);
    assert_eq!(found.len(), 23);
    assert!(r.elapsed_ms <= 100, "{} ms", r.elapsed_ms);
    for (query, budget) in [
        ("@verb", 100),
        ("@noun genitive", 100),
        ("strong:G26 @noun genitive", 250),
        ("lemma:ἀγαπάω @verb aorist", 250),
    ] {
        let out = s
            .run(
                "search.run",
                &json!({ "text": query, "scope": "", "bibles": id, "mode": "bible", "tab": tab.0 }),
            )
            .unwrap();
        settle(&mut s);
        let tab = TabId(out["tab"].as_u64().unwrap());
        let r = s.search_tab(tab).unwrap().results.clone().unwrap();
        eprintln!("{query}: {} (index {} ms)", r.summary(), r.elapsed_ms);
        assert!(r.elapsed_ms <= budget, "{query}: {} ms", r.elapsed_ms);
    }
}
