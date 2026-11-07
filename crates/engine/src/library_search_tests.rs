//! The Search Tool's Library mode (#60): searches over open-text Commentaries, a Lexicon and a
//! Dictionary built in a temp dir; groups in Preferred order with bm25 inside each group,
//! highlighted excerpts, the Resource picker, inline errors, recent queries and opening hits
//! at their paragraph.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use obs_model::{Key, ResourceType, UnitId};
use obs_scripture::{Book, Verse};
use serde_json::json;

use crate::library_search::{
    EXCERPT_BYTES, LibraryResults, LibraryScope, excerpt, paragraph_snippet, row_label,
};
use crate::testing::bibles::{kjv, write_library};
use crate::testing::study::{self, GEN15_COVENANT};
use crate::*;

/// A temp library: KJV, two Commentaries (TCC ★, TC2), a Lexicon (TLX) and a Dictionary
/// (TBD), every Search index built.
fn fixture(name: &str, user: bool) -> (Session, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("obs-library-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("library")).unwrap();
    write_library(
        &root.join("library"),
        &[
            kjv(),
            study::tcc(),
            study::tc2(),
            study::tlx(),
            study::tbd(),
        ],
    );
    let mut s = Session::new(JobMode::Inline);
    if user {
        let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
        s.attach_user(Box::new(store));
    }
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.library.set_preferred_order(
        ["kjv", "tcc", "tc2", "tlx", "tbd"]
            .map(String::from)
            .to_vec(),
    );
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

fn search(s: &mut Session, params: serde_json::Value) -> TabId {
    let mut params = params;
    params["mode"] = json!("library");
    let out = s.run("search.run", &params).unwrap();
    settle(s);
    TabId(out["tab"].as_u64().unwrap())
}

fn results(s: &Session, tab: TabId) -> Arc<LibraryResults> {
    s.search_tab(tab)
        .unwrap()
        .library_results
        .clone()
        .expect("library results")
}

/// Each group's abbreviation, and its rows as label and highlighted words.
type ShownGroup = (String, Vec<(String, Vec<String>)>);

fn shown(s: &Session, tab: TabId) -> Vec<ShownGroup> {
    let r = results(s, tab);
    let st = s.search_tab(tab).unwrap();
    r.groups
        .iter()
        .map(|g| {
            let rows = g
                .rows
                .clone()
                .map(|i| {
                    let sn = st.snippet(i, 0).expect("snippet of the first page");
                    (
                        row_label(&r.rows[i]),
                        sn.highlights
                            .iter()
                            .map(|h| sn.text[h.clone()].to_string())
                            .collect(),
                    )
                })
                .collect();
            (g.abbreviation.clone(), rows)
        })
        .collect()
}

fn cleanup(s: Session, root: PathBuf) {
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn covenant_returns_hits_grouped_by_resource_in_preferred_order_best_first() {
    let (mut s, root) = fixture("covenant", false);
    let tab = search(&mut s, json!({ "text": "covenant" }));
    let r = results(&s, tab);
    // Commentaries, then Lexicons, then Dictionaries, each in Preferred order; KJV (a Bible)
    // is Bible mode's.
    let groups: Vec<(&str, ResourceType)> = r
        .groups
        .iter()
        .map(|g| (g.abbreviation.as_str(), g.resource_type))
        .collect();
    assert_eq!(
        groups,
        [
            ("TCC", ResourceType::Commentary),
            ("TC2", ResourceType::Commentary),
            ("TLX", ResourceType::Lexicon),
            ("TBD", ResourceType::Dictionary),
        ]
    );
    assert_eq!(r.searched, 4);
    assert_eq!(r.summary(), "9 paragraphs in 4 Resources (4 searched)");
    assert_eq!(
        r.group_header(&r.groups[0]),
        "TCC — Test Covenant Commentary · 3 hits"
    );
    // Best first (bm25: lower is better) inside each group.
    for g in &r.groups {
        let scores: Vec<f64> = g.rows.clone().map(|i| r.rows[i].score).collect();
        assert!(
            scores.windows(2).all(|w| w[0] <= w[1]),
            "{}: {scores:?}",
            g.abbreviation
        );
        assert!(r.starts_group(g.rows.start));
    }
    let shown = shown(&s, tab);
    let labels = |abbr: &str| -> Vec<String> {
        let mut l: Vec<String> = shown
            .iter()
            .find(|(a, _)| a == abbr)
            .unwrap()
            .1
            .iter()
            .map(|(l, _)| l.clone())
            .collect();
        l.sort();
        l
    };
    assert_eq!(
        labels("TCC"),
        ["Genesis 15:1-21", "Genesis 9:8-17", "Jeremiah 31:31-34"]
    );
    assert_eq!(labels("TC2"), ["Genesis 17:1-14"]);
    assert_eq!(labels("TLX"), ["G1242 · διαθήκη", "H1285 · בְּרִית"]);
    // The Covenant entry's heading is a document of its own, besides its paragraph.
    assert_eq!(labels("TBD"), ["Ark", "Covenant", "Covenant"]);
    // The short heading "Covenant" is the best Dictionary hit.
    let tbd = &r.groups[3];
    assert_eq!(r.rows[tbd.rows.start].title.as_deref(), Some("Covenant"));
    // Every row's excerpt highlights the word, whatever its case.
    for (abbr, rows) in &shown {
        for (label, words) in rows {
            assert!(!words.is_empty(), "{abbr} {label}: no highlight");
            assert!(
                words.iter().all(|w| w.eq_ignore_ascii_case("covenant")),
                "{abbr} {label}: {words:?}"
            );
        }
    }
    // A hit carries its unit, paragraph range and Key.
    let gen15 = r
        .rows
        .iter()
        .find(|row| row.title.as_deref() == Some("Genesis 15:1-21"))
        .unwrap();
    assert!(
        matches!(gen15.key, Some(Key::Passage(p)) if p.start() == Verse::new(Book::Genesis, 15, 1))
    );
    let text = s.library.get("tcc").unwrap().path.clone();
    let unit = {
        use obs_store::ResourceStore;
        obs_store::ObrReader::open(&text)
            .unwrap()
            .unit_text(&gen15.unit)
            .unwrap()
            .unwrap()
            .1
    };
    assert_eq!(
        &unit[gen15.range.start as usize..gen15.range.end as usize],
        GEN15_COVENANT
    );
    cleanup(s, root);
}

#[test]
fn groups_follow_a_changed_preferred_order() {
    let (mut s, root) = fixture("order", false);
    s.library.set_preferred_order(
        ["kjv", "tc2", "tcc", "tlx", "tbd"]
            .map(String::from)
            .to_vec(),
    );
    let tab = search(&mut s, json!({ "text": "covenant" }));
    let abbrs: Vec<String> = results(&s, tab)
        .groups
        .iter()
        .map(|g| g.abbreviation.clone())
        .collect();
    assert_eq!(abbrs, ["TC2", "TCC", "TLX", "TBD"]);
    cleanup(s, root);
}

#[test]
fn the_picker_chooses_types_or_single_resources_and_reruns_the_search() {
    let (mut s, root) = fixture("picker", false);
    let tab = search(&mut s, json!({ "text": "covenant" }));
    let abbrs = |s: &Session| -> Vec<String> {
        results(s, tab)
            .groups
            .iter()
            .map(|g| g.abbreviation.clone())
            .collect()
    };
    assert!(s.search_tab(tab).unwrap().library.is_default());
    // Leaving out Lexicons.
    let out = s
        .run(
            "search.resource_type",
            &json!({ "tab": tab.0, "value": "lexicon" }),
        )
        .unwrap();
    settle(&mut s);
    assert_eq!(out["types"], json!(["commentary", "dictionary"]));
    assert_eq!(abbrs(&s), ["TCC", "TC2", "TBD"]);
    // Leaving out one Commentary keeps the other, one by one.
    s.run("search.resource", &json!({ "tab": tab.0, "id": "tcc" }))
        .unwrap();
    settle(&mut s);
    let scope = s.search_tab(tab).unwrap().library.clone();
    assert_eq!(scope.types, [ResourceType::Dictionary]);
    assert_eq!(scope.resources, ["tc2"]);
    assert_eq!(abbrs(&s), ["TC2", "TBD"]);
    // Choosing it again makes the whole type again.
    s.run("search.resource", &json!({ "tab": tab.0, "id": "tcc" }))
        .unwrap();
    settle(&mut s);
    let scope = s.search_tab(tab).unwrap().library.clone();
    assert_eq!(
        scope.types,
        [ResourceType::Commentary, ResourceType::Dictionary]
    );
    assert!(scope.resources.is_empty());
    // A single Lexicon.
    s.run("search.resource", &json!({ "tab": tab.0, "id": "tlx" }))
        .unwrap();
    settle(&mut s);
    assert_eq!(abbrs(&s), ["TCC", "TC2", "TLX", "TBD"]);
    // Bibles are not Library Resources.
    assert!(
        s.run("search.resource", &json!({ "tab": tab.0, "id": "kjv" }))
            .is_err()
    );
    // Nothing left: an inline error under the picker.
    for ty in ["commentary", "dictionary"] {
        s.run(
            "search.resource_type",
            &json!({ "tab": tab.0, "value": ty }),
        )
        .unwrap();
    }
    let out = s
        .run("search.resource", &json!({ "tab": tab.0, "id": "tlx" }))
        .unwrap();
    assert_eq!(out["error"]["field"], "resources");
    assert_eq!(out["error"]["message"], "Choose a Resource to search");
    // A hand-over names types and Resources.
    let tab = search(
        &mut s,
        json!({ "text": "covenant", "tab": tab.0, "resources": "dictionaries, tc2" }),
    );
    assert_eq!(abbrs(&s), ["TC2", "TBD"]);
    assert!(s.search_tab(tab).unwrap().error.is_none());
    assert!(
        s.run(
            "search.run",
            &json!({ "text": "x", "mode": "library", "resources": "kjv" })
        )
        .is_err()
    );
    cleanup(s, root);
}

#[test]
fn scope_toggles_and_parsing() {
    let (s, root) = fixture("scope", false);
    let lib = &s.library;
    let mut scope = LibraryScope::default();
    assert_eq!(scope.resources(lib), ["tcc", "tc2", "tlx", "tbd"]);
    scope.toggle_type(lib, ResourceType::Commentary);
    assert_eq!(scope.resources(lib), ["tlx", "tbd"]);
    scope.toggle_resource(lib, "tc2").unwrap();
    assert_eq!(scope.resources(lib), ["tc2", "tlx", "tbd"]);
    scope.toggle_type(lib, ResourceType::Commentary);
    assert_eq!(scope, LibraryScope::default());
    let parsed = LibraryScope::parse("Lexicon, tbd, tcc", lib).unwrap();
    assert_eq!(parsed.types, [ResourceType::Lexicon]);
    assert_eq!(parsed.resources, ["tbd", "tcc"]);
    assert_eq!(parsed.resources(lib), ["tcc", "tlx", "tbd"]);
    assert!(LibraryScope::parse("bibles", lib).is_err());
    cleanup(s, root);
}

#[test]
fn query_errors_and_an_empty_library_are_inline() {
    let (mut s, root) = fixture("errors", false);
    let out = s
        .run(
            "search.run",
            &json!({ "text": "strong:X1", "mode": "library" }),
        )
        .unwrap();
    assert_eq!(out["error"]["field"], "query");
    assert!(!s.bible_search_running(), "nothing was searched");
    drop(s);
    let mut empty = Session::new(JobMode::Inline);
    let out = empty
        .run(
            "search.run",
            &json!({ "text": "covenant", "mode": "library" }),
        )
        .unwrap();
    assert_eq!(out["error"]["field"], "resources");
    assert_eq!(
        out["error"]["message"],
        "Add a Commentary, Lexicon or Dictionary to your library to search it"
    );
    drop(empty);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn recent_library_queries_are_kept_apart_from_bible_ones() {
    let (mut s, root) = fixture("recent", true);
    for q in ["covenant", "ark", "covenant"] {
        search(&mut s, json!({ "text": q }));
    }
    assert_eq!(s.recent_searches(SearchMode::Library), ["covenant", "ark"]);
    assert!(s.recent_searches(SearchMode::Bible).is_empty());
    cleanup(s, root);
}

#[test]
fn excerpts_cut_long_paragraphs_around_the_first_hit() {
    let long = format!(
        "{} the covenant is here. {}",
        "word ".repeat(200),
        "tail ".repeat(200)
    );
    let highlighter = obs_search::Highlighter::new(
        &obs_search::parse("covenant").unwrap(),
        &obs_search::NoLexicon,
    );
    let sn = paragraph_snippet(&long, 0..long.len(), &[], &highlighter);
    assert!(sn.text.len() <= EXCERPT_BYTES + 10, "{}", sn.text.len());
    assert!(sn.text.starts_with("… word"), "{}", sn.text);
    assert!(sn.text.ends_with("tail …"), "{}", sn.text);
    assert_eq!(sn.highlights.len(), 1);
    assert_eq!(&sn.text[sn.highlights[0].clone()], "covenant");
    // Short paragraphs are kept whole; headings are not dropped from a heading document.
    let short = obs_engine_snippet("Covenant", &highlighter);
    assert_eq!(short.text, "Covenant");
    assert_eq!(short.highlights, vec![0..8_usize]);
    // Highlights cut by the window are clipped to it.
    let cut = excerpt(
        Snippet {
            text: "a".repeat(50) + " " + &"b".repeat(50),
            highlights: vec![0..10, 60..101],
        },
        30,
    );
    assert!(cut.text.starts_with('a'), "{}", cut.text);
    assert_eq!(cut.highlights, vec![0..10_usize]);
}

fn obs_engine_snippet(text: &str, highlighter: &obs_search::Highlighter) -> Snippet {
    let heading = obs_model::Span {
        start: 0,
        end: u32::try_from(text.len()).unwrap(),
        kind: obs_model::SpanKind::Heading { level: 2 },
    };
    paragraph_snippet(text, 0..text.len(), &[heading], highlighter)
}

/// The Effect that scrolls a Tab to its hit, if one was queued.
fn text_jump(effects: &[Effect], tab: TabId) -> Option<(UnitId, std::ops::Range<u32>)> {
    effects.iter().find_map(|e| match e {
        Effect::NavigateText {
            tab: t,
            unit,
            range,
            ..
        } if *t == tab => Some((unit.clone(), range.clone())),
        _ => None,
    })
}

#[test]
fn opening_a_hit_goes_to_its_unit_and_paragraph_in_the_linked_latest_or_a_new_tab() {
    let (mut s, root) = fixture("open", false);
    let tab = search(&mut s, json!({ "text": "furnace" }));
    let r = results(&s, tab);
    assert_eq!(r.rows.len(), 1);
    let hit = r.rows[0].clone();
    let gen15_1 = Verse::new(Book::Genesis, 15, 1);
    s.workspace.take_effects();
    // No TCC Tab yet: a new one opens at the Key, and is scrolled to the paragraph.
    let out = s
        .run("search.open", &json!({ "tab": tab.0, "row": 0 }))
        .unwrap();
    assert_eq!(out["via"], "new");
    let tcc = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(
        s.workspace.tab(tcc).unwrap().kind,
        TabKind::Resource("tcc".into())
    );
    assert_eq!(
        s.workspace.tab(tcc).unwrap().history.current(),
        Some(gen15_1)
    );
    let effects = s.workspace.take_effects();
    assert_eq!(
        text_jump(&effects, tcc),
        Some((hit.unit.clone(), hit.range.clone()))
    );
    // The Opened effect comes first, so the UI loads the Resource before scrolling it.
    let opened = effects
        .iter()
        .position(|e| matches!(e, Effect::Opened { tab, .. } if *tab == tcc));
    let jumped = effects
        .iter()
        .position(|e| matches!(e, Effect::NavigateText { .. }));
    assert!(opened < jumped, "{effects:?}");
    // Again: the most recently active TCC Tab goes there.
    s.workspace
        .navigate(tcc, Verse::new(Book::Jeremiah, 31, 31));
    s.workspace.focus(tab);
    s.workspace.take_effects();
    let out = s
        .run("search.open", &json!({ "tab": tab.0, "row": 0 }))
        .unwrap();
    assert_eq!(out["via"], "tab");
    assert_eq!(out["tab"], tcc.0);
    assert_eq!(
        s.workspace.tab(tcc).unwrap().history.current(),
        Some(gen15_1)
    );
    assert!(text_jump(&s.workspace.take_effects(), tcc).is_some());
    // Ctrl/Cmd: always a new Tab.
    let out = s
        .run(
            "search.open",
            &json!({ "tab": tab.0, "row": 0, "split": true }),
        )
        .unwrap();
    assert_eq!(out["via"], "new");
    let other = TabId(out["tab"].as_u64().unwrap());
    assert_ne!(other, tcc);
    // With a Link set: its TCC Tab goes there (even when another TCC Tab was active later),
    // and the set's Bible follows to the Key's Passage.
    let kjv = TabId(
        s.run("resource.open", &json!({ "id": "kjv" })).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    for t in [kjv, tcc, tab] {
        s.run("tab.link", &json!({ "tab": t.0, "set": "B" }))
            .unwrap();
    }
    s.workspace.focus(other);
    s.workspace.navigate(kjv, Verse::new(Book::Exodus, 1, 1));
    s.workspace.focus(tab);
    s.workspace.take_effects();
    let out = s
        .run("search.open", &json!({ "tab": tab.0, "row": 0 }))
        .unwrap();
    assert_eq!(out["via"], "link");
    assert_eq!(out["tab"], tcc.0);
    let effects = s.workspace.take_effects();
    assert!(text_jump(&effects, tcc).is_some());
    assert_eq!(
        s.workspace.tab(kjv).unwrap().history.current(),
        Some(gen15_1)
    );
    assert!(effects.iter().any(|e| matches!(
        e,
        Effect::Follow { tab, flash: true, .. } if *tab == kjv
    )));
    cleanup(s, root);
}

#[test]
fn opening_a_dictionary_or_lexicon_hit_records_its_entry_key() {
    let (mut s, root) = fixture("open-entry", false);
    let tab = search(&mut s, json!({ "text": "\"old covenant\"" }));
    let r = results(&s, tab);
    assert_eq!(r.groups.len(), 1);
    assert_eq!(r.groups[0].abbreviation, "TBD");
    let out = s
        .run("search.open", &json!({ "tab": tab.0, "row": 0 }))
        .unwrap();
    let tbd = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(
        s.workspace.tab(tbd).unwrap().entry_history.current(),
        Some(Key::Headword("Covenant".into()))
    );
    let tab = search(&mut s, json!({ "text": "treaty", "tab": tab.0 }));
    let out = s
        .run("search.open", &json!({ "tab": tab.0, "row": 0 }))
        .unwrap();
    let tlx = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(
        s.workspace.tab(tlx).unwrap().entry_history.current(),
        Some(Key::StrongsNumber("H1285".parse().unwrap()))
    );
    cleanup(s, root);
}

/// The real Starter MHC, JFB, Easton and Smith, when this machine has the Starter library.
/// Their Search index caches are built once into a temp folder kept between runs. Run with
/// `cargo test --release -p obs-engine real_starter_library -- --ignored --nocapture`.
#[test]
#[ignore = "needs the Starter library in the platform data folder"]
fn real_starter_library_covenant_in_mhc_jfb_easton_and_smith() {
    let Some(dir) = default_library_dir().filter(|d| d.join("mhc.obr").is_file()) else {
        eprintln!("no Starter library: skipped");
        return;
    };
    let mut s = Session::new(JobMode::Inline);
    let mut ids = Vec::new();
    for f in ["mhc.obr", "jfb.obr", "easton.obr", "smith.obr"] {
        let Ok(e) = s.library.add_file(&dir.join(f)) else {
            eprintln!("{f} missing: skipped");
            return;
        };
        ids.push(e.manifest.resource_id.clone());
    }
    s.set_search_cache_dir(std::env::temp_dir().join("obs-real-search-cache"));
    let ready = std::time::Instant::now();
    s.refresh_search_indexes();
    settle(&mut s);
    eprintln!("indexes ready in {:?}", ready.elapsed());
    let started = std::time::Instant::now();
    let tab = search(&mut s, json!({ "text": "covenant" }));
    let r = results(&s, tab);
    eprintln!(
        "{} in {:?} (search {} ms)",
        r.summary(),
        started.elapsed(),
        r.elapsed_ms
    );
    let types: Vec<ResourceType> = r.groups.iter().map(|g| g.resource_type).collect();
    assert!(types.contains(&ResourceType::Commentary), "{types:?}");
    assert!(types.contains(&ResourceType::Dictionary), "{types:?}");
    assert_eq!(r.groups.len(), 4, "every Resource has a covenant hit");
    // Commentaries first, in Preferred order, then the Dictionaries.
    let order: Vec<&str> = r.groups.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(order, ids.iter().map(String::as_str).collect::<Vec<_>>());
    for g in &r.groups {
        eprintln!(
            "{} — first: {}",
            r.group_header(g),
            row_label(&r.rows[g.rows.start])
        );
        let scores: Vec<f64> = g.rows.clone().map(|i| r.rows[i].score).collect();
        assert!(scores.windows(2).all(|w| w[0] <= w[1]));
    }
    // The first page's excerpts each highlight the word.
    let st = s.search_tab(tab).unwrap();
    for i in 0..bible_search::PAGE_ROWS.min(r.rows.len()) {
        let sn = st.snippet(i, 0).unwrap();
        assert!(sn.text.len() <= library_search::EXCERPT_BYTES + 10);
        assert!(
            sn.highlights
                .iter()
                .any(|h| sn.text[h.clone()].eq_ignore_ascii_case("covenant")),
            "row {i}: {sn:?}"
        );
    }
    // Easton's Covenant entry is among its hits.
    let easton = r.groups.iter().find(|g| g.id == ids[2]).unwrap();
    assert!(
        easton
            .rows
            .clone()
            .any(|i| r.rows[i].title.as_deref() == Some("Covenant"))
    );
    drop(s);
}

/// Word forms set from the Settings Tab (#213) names no Search Tab: every open Bible and
/// Library search runs again with it, a Notes search keeps each word as written (it is not
/// run again), and the Setting's Reset brings the first results back.
#[test]
fn word_forms_from_settings_rerun_every_open_bible_and_library_search() {
    let (mut s, root) = fixture("word-forms-settings", true);
    let open = |s: &mut Session, params: serde_json::Value| {
        let mut params = params;
        params["split"] = json!(true);
        let out = s.run("search.run", &params).unwrap();
        settle(s);
        TabId(out["tab"].as_u64().unwrap())
    };
    let bible_rows = |s: &Session, tab: TabId| -> Vec<String> {
        let r = s.search_tab(tab).unwrap().results.clone().expect("results");
        r.rows.iter().map(|row| r.row_reference(row)).collect()
    };
    let library_groups = |s: &Session, tab: TabId| -> Vec<String> {
        (results(s, tab).groups.iter())
            .map(|g| g.abbreviation.clone())
            .collect()
    };
    s.run(
        "annotation.note",
        &json!({ "body": "Two covenants compared", "passage": "Jeremiah 31:31" }),
    )
    .unwrap();

    let bible = open(&mut s, json!({ "text": "work", "bibles": "kjv" }));
    let library = open(&mut s, json!({ "text": "covenants", "mode": "library" }));
    let notes = open(&mut s, json!({ "text": "covenants", "mode": "notes" }));
    assert_eq!(s.search_tab(bible).unwrap().mode, SearchMode::Bible);
    assert!(bible_rows(&s, bible).is_empty());
    assert!(library_groups(&s, library).is_empty());
    let notes_before = s.search_tab(notes).unwrap().notes.clone().unwrap();
    assert_eq!(notes_before.hits.len(), 1);

    // The Settings Tab's check box: no `tab`.
    let out = s
        .run("search.word_forms", &json!({ "value": true }))
        .unwrap();
    settle(&mut s);
    assert_eq!(out["tab"], serde_json::Value::Null);
    assert_eq!(bible_rows(&s, bible), ["Romans 13:10"]);
    assert_eq!(library_groups(&s, library), ["TCC", "TC2", "TLX", "TBD"]);
    let notes_after = s.search_tab(notes).unwrap().notes.clone().unwrap();
    assert!(
        Arc::ptr_eq(&notes_before, &notes_after),
        "the Notes search ran again"
    );

    // Reset: Word forms off again, and the searches as they first were.
    s.run("settings.reset", &json!({ "setting": "search_word_forms" }))
        .unwrap();
    settle(&mut s);
    assert!(!s.search_word_forms());
    assert!(bible_rows(&s, bible).is_empty());
    assert!(library_groups(&s, library).is_empty());
    let notes_reset = s.search_tab(notes).unwrap().notes.clone().unwrap();
    assert!(Arc::ptr_eq(&notes_before, &notes_reset));
    cleanup(s, root);
}
