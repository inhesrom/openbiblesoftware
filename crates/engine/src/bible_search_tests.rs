//! The Search Tool's Bible mode (#59): searches over open-text fixtures built in a temp dir,
//! the Verse list in Standard order with Rust-side highlights, Scope mapping through Verse
//! maps, inline errors, recent queries, opening hits, and Word Info's hand-over.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_model::MorphemePart;
use obs_scripture::{Book, Passage, VerseMap, VerseMapEntry};
use serde_json::json;

use crate::testing::bibles::{bible, kjv, psalms, tahot, v, web, write_library};
use crate::*;

/// A temp library with KJV, WEB (Lev 19:18, Mt 5:43, Mt 22:39, Rom 13:10), a Hebrew-numbered
/// Bible `hnum` (its Gen 32:1 is Standard Gen 31:55; its Gen 32:33 is Unmapped), a long
/// `psalms` Bible for paging, and TAHOT. Every Search index is built.
fn fixture(name: &str, user: bool) -> (Session, PathBuf) {
    let root = std::env::temp_dir().join(format!("obs-bible-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("library")).unwrap();
    let hnum = [
        (
            v(Book::Genesis, 32, 1),
            "And Laban rose early, and kissed his sons, for he did love them.",
        ),
        (
            v(Book::Genesis, 32, 33),
            "An extra verse about love, found in no Standard verse.",
        ),
    ];
    write_library(
        &root.join("library"),
        &[
            kjv(),
            web(),
            bible(
                "hnum",
                "HNUM",
                &hnum,
                Some(&[Some(v(Book::Genesis, 31, 55)), None]),
                &[],
            ),
            psalms(80),
            tahot(),
        ],
    );
    let mut s = Session::new(JobMode::Inline);
    if user {
        let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
        s.attach_user(Box::new(store));
    }
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.library.set_preferred_order(
        ["kjv", "web", "hnum", "psalms", "tahot"]
            .map(String::from)
            .to_vec(),
    );
    s.set_search_cache_dir(root.join("cache"));
    s.refresh_search_indexes();
    settle(&mut s);
    (s, root)
}

/// Run every queued job and apply its events.
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
    let out = s.run("search.run", &params).unwrap();
    settle(s);
    TabId(out["tab"].as_u64().unwrap())
}

fn results(s: &Session, tab: TabId) -> std::sync::Arc<BibleResults> {
    s.search_tab(tab).unwrap().results.clone().expect("results")
}

/// Each row's reference and, per hit, its Bible and highlighted words.
/// A row as shown: its reference, and each hit's Bible and highlighted words.
type ShownRow = (String, Vec<(String, Vec<String>)>);

fn shown(s: &Session, tab: TabId) -> Vec<ShownRow> {
    let r = results(s, tab);
    let st = s.search_tab(tab).unwrap();
    r.rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let hits = row
                .hits
                .iter()
                .enumerate()
                .map(|(j, h)| {
                    let sn = st.snippet(i, j).expect("snippet of the first page");
                    (
                        r.bibles[h.bible].abbreviation.clone(),
                        sn.highlights
                            .iter()
                            .map(|x| sn.text[x.clone()].to_string())
                            .collect(),
                    )
                })
                .collect();
            (r.row_reference(row), hits)
        })
        .collect()
}

/// Give a Tab its Bible's Verse map, as the UI does when it loads the Bible.
fn load_map(s: &mut Session, tab: TabId, root: &Path, id: &str) {
    use obs_store::ResourceStore;
    let map = obs_store::ObrReader::open(&root.join(format!("library/{id}.obr")))
        .unwrap()
        .verse_map()
        .unwrap()
        .map(std::sync::Arc::new);
    s.workspace.tab_mut(tab).unwrap().verse_map = map;
}

fn cleanup(s: Session, root: PathBuf) {
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// The outline cache key of a library Bible.
fn key_of(s: &Session, id: &str) -> String {
    format!("{id}@{}", s.library.get(id).unwrap().manifest.content_hash)
}

fn outline_read(s: &Session, id: &str) -> bool {
    s.bible_search.has_outline(&key_of(s, id))
}

#[test]
fn outlines_are_read_ahead_as_soon_as_each_bible_index_is_ready() {
    // Built in the background: each outline is read once its index is built, before any
    // Search Tab exists.
    let (s, root) = fixture("read-ahead", false);
    assert!(
        s.workspace
            .tabs()
            .all(|t| t.kind != TabKind::Tool(Tool::Search))
    );
    for id in ["kjv", "web", "hnum", "psalms", "tahot"] {
        assert!(outline_read(&s, id), "{id} not read ahead");
    }
    drop(s);
    // Ready at startup (the caches exist): read ahead from the refresh, the ★ Bible first.
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.library.set_preferred_order(
        ["web", "kjv", "hnum", "psalms", "tahot"]
            .map(String::from)
            .to_vec(),
    );
    s.set_search_cache_dir(root.join("cache"));
    s.refresh_search_indexes();
    assert!(!s.search.is_building());
    assert!(s.bible_search.is_reading_ahead());
    // One outline at a time: the first pump reads only the ★ Bible's.
    s.jobs().pump(1);
    s.poll_imports();
    assert!(outline_read(&s, "web"));
    assert!(!outline_read(&s, "kjv"));
    settle(&mut s);
    for id in ["kjv", "web", "hnum", "psalms", "tahot"] {
        assert!(outline_read(&s, id), "{id} not read ahead");
    }
    cleanup(s, root);
}

#[test]
fn an_unreadable_outline_is_not_read_again_until_the_library_changes() {
    let (mut s, root) = fixture("bad-outline", false);
    let bad = root.join("bad");
    write_library(
        &bad,
        &[bible(
            "broken",
            "BRK",
            &[(v(Book::Genesis, 1, 1), "In the beginning.")],
            None,
            &[],
        )],
    );
    let file = bad.join("broken.obr");
    s.library.add_file(&file).unwrap();
    std::fs::write(&file, b"not an obr").unwrap();
    let key = key_of(&s, "broken");
    let ids = ["broken".to_string()];
    s.warm_bible_outlines(&ids);
    assert!(s.bible_search.is_reading_ahead());
    settle(&mut s);
    assert!(s.bible_search.outline_failed(&key));
    // The Search Tab asks every frame: no new read job.
    for _ in 0..3 {
        s.warm_bible_outlines(&ids);
        assert!(!s.bible_search.is_reading_ahead(), "read again");
        assert_eq!(s.jobs().pump(16), 0);
    }
    // The library changes: the next ask reads it again.
    s.library.remove("psalms");
    s.warm_bible_outlines(&ids);
    assert!(s.bible_search.is_reading_ahead());
    settle(&mut s);
    assert!(s.bible_search.outline_failed(&key));
    cleanup(s, root);
}

#[test]
fn love_thy_neighbour_in_kjv_and_web_returns_highlighted_verse_rows() {
    let (mut s, root) = fixture("phrase", false);
    let tab = search(
        &mut s,
        json!({ "text": "\"love thy neighbour\"", "bibles": "kjv,web" }),
    );
    let r = results(&s, tab);
    assert_eq!(r.summary(), "3 verses · 3 hits · KJV 3 · WEB 0");
    let hl = |t: &str| vec![("KJV".to_string(), vec![t.to_string()])];
    assert_eq!(
        shown(&s, tab),
        [
            ("Leviticus 19:18".to_string(), hl("love thy neighbour")),
            ("Matthew 5:43".to_string(), hl("love thy neighbour")),
            ("Matthew 22:39".to_string(), hl("love thy neighbour")),
        ]
    );
    let snippet = s.search_tab(tab).unwrap().snippet(0, 0).unwrap().clone();
    assert!(snippet.text.starts_with("Thou shalt not avenge"));
    assert_eq!(s.search_tab(tab).unwrap().selected, Some(0));

    // A prefix finds both spellings: rows hold every Bible that matched, in picker order.
    let tab = search(
        &mut s,
        json!({ "text": "love neighbo*", "bibles": "kjv,web", "tab": tab.0 }),
    );
    let rows = shown(&s, tab);
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[3],
        (
            "Romans 13:10".to_string(),
            vec![
                (
                    "KJV".to_string(),
                    vec!["Love", "neighbour", "love"]
                        .into_iter()
                        .map(String::from)
                        .collect()
                ),
                (
                    "WEB".to_string(),
                    vec!["Love", "neighbor", "Love"]
                        .into_iter()
                        .map(String::from)
                        .collect()
                ),
            ]
        )
    );
    cleanup(s, root);
}

#[test]
fn strongs_h1254_in_tahot_highlights_the_right_morphemes() {
    let (mut s, root) = fixture("tahot", false);
    let tab = search(&mut s, json!({ "text": "strong:H1254", "bibles": "tahot" }));
    let rows = shown(&s, tab);
    assert_eq!(
        rows,
        [
            (
                "Genesis 1:1".to_string(),
                vec![("TAHOT".to_string(), vec!["בָּרָ֣א".to_string()])]
            ),
            (
                "Genesis 1:27".to_string(),
                // The root only: the prefix וַ (H9001) stays unmarked.
                vec![("TAHOT".to_string(), vec!["יִּבְרָ֨א".to_string()])]
            ),
        ]
    );
    assert!(results(&s, tab).bibles[0].rtl);
    // An extended number matches exactly; the homograph letter must agree.
    let tab = search(
        &mut s,
        json!({ "text": "H1254B", "bibles": "tahot", "tab": tab.0 }),
    );
    assert!(results(&s, tab).rows.is_empty());
    // The prefix itself, by number.
    let tab = search(
        &mut s,
        json!({ "text": "H9001", "bibles": "tahot", "tab": tab.0 }),
    );
    assert_eq!(
        shown(&s, tab),
        [(
            "Genesis 1:27".to_string(),
            vec![("TAHOT".to_string(), vec!["וַ".to_string()])]
        )]
    );
    // KJV-style Strong's tags highlight the tagged English word.
    let tab = search(
        &mut s,
        json!({ "text": "H7453 thyself", "bibles": "kjv", "tab": tab.0 }),
    );
    assert_eq!(shown(&s, tab)[0].1[0].1, ["neighbour", "thyself"]);
    cleanup(s, root);
}

#[test]
fn the_scope_maps_through_each_verse_map_and_unmapped_verses_need_whole_bible() {
    let (mut s, root) = fixture("scope", false);
    let refs = |s: &Session, tab| -> Vec<String> {
        let r = results(s, tab);
        r.rows.iter().map(|row| r.row_reference(row)).collect()
    };
    let tab = search(&mut s, json!({ "text": "love", "bibles": "kjv,hnum" }));
    assert_eq!(
        refs(&s, tab),
        [
            "Genesis 31:55",
            "Genesis 32:33 (HNUM, no Standard equivalent)",
            "Leviticus 19:18",
            "Matthew 5:43",
            "Matthew 22:39",
            "Romans 13:10",
        ]
    );
    let r = results(&s, tab);
    assert_eq!(r.scope, "Whole Bible");
    // The Hebrew-numbered verse shows under its Standard row with its own number.
    assert_eq!(r.rows[0].hits[0].native, v(Book::Genesis, 32, 1));

    for (scope, want) in [
        ("Old Testament", vec!["Genesis 31:55", "Leviticus 19:18"]),
        ("NT", vec!["Matthew 5:43", "Matthew 22:39", "Romans 13:10"]),
        // The Standard Gen 31 holds HNUM's Gen 32:1, never its Unmapped Gen 32:33.
        ("Gen 31", vec!["Genesis 31:55"]),
        ("Mt 22; Rom", vec!["Matthew 22:39", "Romans 13:10"]),
        ("Jn 3", vec![]),
    ] {
        s.run("search.scope", &json!({ "tab": tab.0, "value": scope }))
            .unwrap();
        settle(&mut s);
        assert_eq!(refs(&s, tab), want, "{scope}");
        assert_eq!(results(&s, tab).scope, scope);
    }
    s.run(
        "search.scope",
        &json!({ "tab": tab.0, "value": "Whole Bible" }),
    )
    .unwrap();
    settle(&mut s);
    assert_eq!(refs(&s, tab).len(), 6);
    assert_eq!(s.search_tab(tab).unwrap().scope, "");
    cleanup(s, root);
}

#[test]
#[allow(clippy::single_range_in_vec_init)]
fn ordinal_ranges_follow_the_verse_map() {
    let outline = BibleOutline::new(
        &[
            (v(Book::Genesis, 31, 54), 54),
            (v(Book::Genesis, 32, 1), 1),
            (v(Book::Genesis, 32, 2), 2),
            (v(Book::Genesis, 32, 33), 33),
            (v(Book::Exodus, 1, 1), 1),
        ],
        Some(&VerseMap::new(
            vec![
                (v(Book::Genesis, 31, 54), Some(v(Book::Genesis, 31, 54))),
                (v(Book::Genesis, 32, 1), Some(v(Book::Genesis, 31, 55))),
                (v(Book::Genesis, 32, 2), Some(v(Book::Genesis, 32, 1))),
                (v(Book::Genesis, 32, 33), None),
                (v(Book::Exodus, 1, 1), Some(v(Book::Exodus, 1, 1))),
            ]
            .into_iter()
            .map(|(n, s)| VerseMapEntry {
                verse: n,
                last: n.verse,
                standard: s.map(Passage::verse),
            })
            .collect(),
            Vec::new(),
            &obs_scripture::Standard,
        )),
    );
    let ranges = |text: &str| outline.ordinal_ranges(&parse_scope(text).unwrap());
    assert_eq!(ranges(""), None, "Whole Bible: everything, Unmapped too");
    assert_eq!(ranges("Gen 31"), Some(vec![0..=1]));
    assert_eq!(ranges("Gen 32"), Some(vec![2..=2]));
    assert_eq!(ranges("Gen 31:55; Exod"), Some(vec![1..=1, 4..=4]));
    assert_eq!(ranges("Gen"), Some(vec![0..=2]));
    assert_eq!(ranges("NT"), Some(vec![]));
}

#[test]
fn query_scope_and_bible_errors_are_inline() {
    let (mut s, root) = fixture("errors", false);
    let out = s
        .run("search.run", &json!({ "text": "strong:X1" }))
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(out["error"]["field"], "query");
    let e = s.search_tab(tab).unwrap().error.clone().unwrap();
    assert_eq!(e.field, ErrorField::Query);
    assert!(e.message.contains("not a Strong's number"), "{}", e.message);
    assert_eq!(e.span, Some(7..9));
    assert!(!s.bible_search_running(), "nothing was searched");

    let out = s
        .run(
            "search.run",
            &json!({ "text": "love", "scope": "Hezekiah 3", "tab": tab.0 }),
        )
        .unwrap();
    assert_eq!(out["error"]["field"], "scope");
    let e = s.search_tab(tab).unwrap().error.clone().unwrap();
    assert!(e.message.contains("Hezekiah"), "{}", e.message);

    // Removing the only Bible leaves nothing to search.
    s.run("search.scope", &json!({ "tab": tab.0, "value": "" }))
        .unwrap();
    let only = s.search_tab(tab).unwrap().bibles(&s.library);
    assert_eq!(only, ["kjv"], "the ★ Bible by default");
    let out = s
        .run("search.bible", &json!({ "tab": tab.0, "id": "kjv" }))
        .unwrap();
    assert_eq!(out["error"]["field"], "bibles");
    // Adding one back reruns the search.
    s.run("search.bible", &json!({ "tab": tab.0, "id": "web" }))
        .unwrap();
    settle(&mut s);
    assert_eq!(results(&s, tab).summary(), "4 verses · WEB 4");
    assert!(s.search_tab(tab).unwrap().error.is_none());
    cleanup(s, root);
}

#[test]
fn recent_queries_are_kept_per_mode_in_the_user_database() {
    let (mut s, root) = fixture("recent", true);
    for q in ["love", "\"love thy neighbour\"", "law", "love"] {
        search(&mut s, json!({ "text": q }));
    }
    assert_eq!(
        s.recent_searches(SearchMode::Bible),
        ["love", "law", "\"love thy neighbour\""]
    );
    assert!(s.recent_searches(SearchMode::Library).is_empty());
    drop(s);
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    let mut again = Session::new(JobMode::Inline);
    again.attach_user(Box::new(store));
    assert_eq!(again.recent_searches(SearchMode::Bible)[0], "love");
    // Notes searches are not built yet: no search runs in that mode.
    let tab = search(&mut again, json!({ "text": "grace", "mode": "notes" }));
    let st = again.search_tab(tab).unwrap();
    assert_eq!(st.mode, SearchMode::Notes);
    assert!(st.results.is_none() && st.library_results.is_none() && st.error.is_none());
    drop(again);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn snippets_arrive_a_page_at_a_time() {
    let (mut s, root) = fixture("pages", false);
    let tab = search(&mut s, json!({ "text": "law", "bibles": "psalms" }));
    assert_eq!(results(&s, tab).rows.len(), 80);
    let st = s.search_tab(tab).unwrap();
    assert!(st.snippet(bible_search::PAGE_ROWS - 1, 0).is_some());
    assert!(st.snippet(bible_search::PAGE_ROWS, 0).is_none());
    s.request_search_snippets(tab, 40..50);
    assert!(s.search_tab(tab).unwrap().is_loading(45));
    settle(&mut s);
    let st = s.search_tab(tab).unwrap();
    let sn = st.snippet(45, 0).unwrap();
    assert!(sn.text.starts_with("Verse 46:"), "{}", sn.text);
    assert_eq!(&sn.text[sn.highlights[0].clone()], "law");
    assert!(st.snippet(70, 0).is_none(), "only the pages asked for");
    cleanup(s, root);
}

#[test]
fn opening_a_hit_follows_the_link_set_else_the_latest_tab_else_a_new_one() {
    let (mut s, root) = fixture("open", false);
    let tab = search(
        &mut s,
        json!({ "text": "love", "bibles": "kjv,hnum", "scope": "Gen 31" }),
    );
    let gen31_55 = v(Book::Genesis, 31, 55);
    // No Tab of the Bible yet: a new one opens at the hit, in the Bible's own numbering.
    let out = s
        .run("search.open", &json!({ "tab": tab.0, "row": 0 }))
        .unwrap();
    assert_eq!(out["via"], "new");
    let hnum = TabId(out["tab"].as_u64().unwrap());
    load_map(&mut s, hnum, &root, "hnum");
    assert_eq!(
        s.workspace.tab(hnum).unwrap().kind,
        TabKind::Resource("hnum".into())
    );
    assert_eq!(
        s.workspace.tab(hnum).unwrap().history.current(),
        Some(v(Book::Genesis, 32, 1))
    );
    // Again: the most recently active Tab of that Bible goes there.
    s.workspace.navigate(hnum, v(Book::Genesis, 32, 2));
    s.workspace.focus(tab);
    let out = s
        .run("search.open", &json!({ "tab": tab.0, "row": 0 }))
        .unwrap();
    assert_eq!(out["via"], "tab");
    assert_eq!(out["tab"], hnum.0);
    assert_eq!(
        s.workspace.tab(hnum).unwrap().history.current(),
        Some(v(Book::Genesis, 32, 1))
    );
    // Ctrl/Cmd: always a new Tab.
    let out = s
        .run(
            "search.open",
            &json!({ "tab": tab.0, "row": 0, "split": true }),
        )
        .unwrap();
    assert_eq!(out["via"], "new");
    assert_ne!(out["tab"], hnum.0);
    // With a Link set, the set follows to the Standard Verse (each member in its numbering).
    let kjv = TabId(
        s.run("resource.open", &json!({ "id": "kjv" })).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    s.run("tab.link", &json!({ "tab": kjv.0, "set": "B" }))
        .unwrap();
    s.run("tab.link", &json!({ "tab": hnum.0, "set": "B" }))
        .unwrap();
    s.run("tab.link", &json!({ "tab": tab.0, "set": "B" }))
        .unwrap();
    s.workspace.navigate(kjv, v(Book::Exodus, 1, 1));
    s.workspace.take_effects();
    let out = s
        .run("search.open", &json!({ "tab": tab.0, "row": 0 }))
        .unwrap();
    assert_eq!(out["via"], "link");
    assert_eq!(
        s.workspace.tab(kjv).unwrap().history.current(),
        Some(gen31_55)
    );
    assert_eq!(
        s.workspace.tab(hnum).unwrap().history.current(),
        Some(v(Book::Genesis, 32, 1))
    );
    assert!(s.workspace.take_effects().iter().any(|e| matches!(
        e,
        Effect::Follow { tab, flash: true, .. } if *tab == kjv
    )));
    cleanup(s, root);
}

#[test]
fn current_book_and_chapter_presets_come_from_the_link_set_or_the_latest_bible() {
    let (mut s, root) = fixture("presets", false);
    let tab = search(&mut s, json!({ "text": "love" }));
    assert_eq!(scope_presets(s.search_context(tab)).len(), 3);
    let hnum = TabId(
        s.run("resource.open", &json!({ "id": "hnum" })).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    load_map(&mut s, hnum, &root, "hnum");
    s.workspace.navigate(hnum, v(Book::Genesis, 32, 1));
    // HNUM's Gen 32:1 is the Standard Gen 31:55.
    let presets = scope_presets(s.search_context(tab));
    let labels: Vec<&str> = presets.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Whole Bible",
            "Old Testament",
            "New Testament",
            "Current book (Genesis)",
            "Current chapter (Genesis 31)"
        ]
    );
    assert_eq!(presets[4].value, "Genesis 31");
    cleanup(s, root);
}

#[test]
fn word_info_searches_this_word_in_its_bible() {
    let (mut s, root) = fixture("word-info", false);
    assert!(!s.is_enabled("word_info.search"));
    let mut part = MorphemePart::new("H1254A".parse().unwrap());
    part.lemma = Some("בָּרָא".into());
    let mut word =
        ShownWord::new("בָּרָ֣א", true, "Genesis 1:1 (TAHOT)".into(), &[part], |_| None).unwrap();
    word.bible = Some("tahot".into());
    s.word_info.pin(word);
    assert!(s.is_enabled("word_info.search"));
    let out = s.run("word_info.search", &json!({})).unwrap();
    settle(&mut s);
    let tab = TabId(out["tab"].as_u64().unwrap());
    let st = s.search_tab(tab).unwrap();
    assert_eq!(st.query, "strong:H1254A");
    assert_eq!(st.bibles(&s.library), ["tahot"]);
    assert_eq!(results(&s, tab).rows.len(), 2);
    // The same Search Tab takes the next hand-over (Word Study's Show all will too).
    let out = s
        .run("search.run", &json!({ "text": "strong:H430" }))
        .unwrap();
    assert_eq!(out["tab"], tab.0);
    cleanup(s, root);
}

#[test]
fn search_this_word_switches_a_search_tab_left_in_another_mode_to_bible() {
    let (mut s, root) = fixture("word-info-mode", false);
    let tab = search(&mut s, json!({ "text": "love" }));
    s.run("search.mode", &json!({ "tab": tab.0, "value": "library" }))
        .unwrap();
    assert_eq!(s.search_tab(tab).unwrap().mode, SearchMode::Library);
    let mut part = MorphemePart::new("H1254A".parse().unwrap());
    part.lemma = Some("בָּרָא".into());
    let mut word =
        ShownWord::new("בָּרָ֣א", true, "Genesis 1:1 (TAHOT)".into(), &[part], |_| None).unwrap();
    word.bible = Some("tahot".into());
    s.word_info.pin(word);
    let out = s.run("word_info.search", &json!({})).unwrap();
    settle(&mut s);
    assert_eq!(out["tab"], tab.0);
    let st = s.search_tab(tab).unwrap();
    assert_eq!(st.mode, SearchMode::Bible);
    assert_eq!(st.query, "strong:H1254A");
    assert_eq!(results(&s, tab).rows.len(), 2);
    cleanup(s, root);
}

/// The real Starter KJV and WEB (and TAHOT), when this machine has the Starter library. Their
/// Search index caches are built once into a temp folder kept between runs. Run with
/// `cargo test --release -p obs-engine real_starter -- --ignored --nocapture`.
#[test]
#[ignore = "needs the Starter library in the platform data folder"]
fn real_starter_kjv_and_web_love_thy_neighbour_and_tahot_h1254() {
    let Some(dir) = default_library_dir().filter(|d| d.join("kjv-strongs.obr").is_file()) else {
        eprintln!("no Starter library: skipped");
        return;
    };
    let mut s = Session::new(JobMode::Inline);
    let mut ids = Vec::new();
    for f in ["kjv-strongs.obr", "web.obr", "tahot.obr"] {
        ids.push(
            s.library
                .add_file(&dir.join(f))
                .unwrap()
                .manifest
                .resource_id
                .clone(),
        );
    }
    s.set_search_cache_dir(std::env::temp_dir().join("obs-real-search-cache"));
    let ready = std::time::Instant::now();
    s.refresh_search_indexes();
    // The outlines are read ahead as soon as the indexes are Ready: the first search does
    // not read them.
    settle(&mut s);
    for id in &ids {
        assert!(outline_read(&s, id), "{id} not read ahead");
    }
    eprintln!(
        "indexes ready and outlines read ahead in {:?}",
        ready.elapsed()
    );

    let started = std::time::Instant::now();
    let tab = search(
        &mut s,
        json!({ "text": "\"love thy neighbour\"", "bibles": format!("{},{}", ids[0], ids[1]) }),
    );
    let r = results(&s, tab);
    eprintln!(
        "{} in {:?} (index {} ms)",
        r.summary(),
        started.elapsed(),
        r.elapsed_ms
    );
    let refs: Vec<String> = r.rows.iter().map(|row| r.row_reference(row)).collect();
    for want in [
        "Leviticus 19:18",
        "Matthew 22:39",
        "Romans 13:9",
        "Galatians 5:14",
    ] {
        assert!(
            refs.iter().any(|x| x == want),
            "{want} missing from {refs:?}"
        );
    }
    let rows = shown(&s, tab);
    let lev = rows.iter().find(|(r, _)| r == "Leviticus 19:18").unwrap();
    assert_eq!(lev.1[0].0, "KJV");
    assert_eq!(lev.1[0].1, ["love thy neighbour"]);

    let tab = search(
        &mut s,
        json!({ "text": "strong:H1254", "bibles": ids[2], "tab": tab.0, "scope": "Gen 1-2" }),
    );
    let rows = shown(&s, tab);
    eprintln!("{rows:?}");
    let gen1_1 = &rows.iter().find(|(r, _)| r == "Genesis 1:1").unwrap().1[0].1;
    assert_eq!(gen1_1.len(), 1);
    assert!(gen1_1[0].starts_with('ב'), "{gen1_1:?}");
    // Gen 1:27 וַיִּבְרָא: the root without its prefix.
    let gen1_27 = &rows.iter().find(|(r, _)| r == "Genesis 1:27").unwrap().1[0].1;
    assert!(!gen1_27.is_empty());
    assert!(gen1_27.iter().all(|w| !w.starts_with('ו')), "{gen1_27:?}");
    drop(s);
}
