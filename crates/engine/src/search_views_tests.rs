//! Search results Grid, Chart and Analysis views (#146) over a Strong's-tagged KJV excerpt
//! and a tagged Greek excerpt, both searched for `strong:G26`: the Grid's heat map, a Grid
//! cell or Chart bar filtering the Verse list, the Analysis table sorted by every column, and
//! its CSV export; and an ignored check against the real Starter KJV.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use obs_scripture::{Book, Verse};
use serde_json::json;

use crate::search_views::csv_field;
use crate::testing::bibles::write_library;
use crate::testing::{greek_agape, kjv_agape, study};
use crate::*;

/// KJV (★) and TGNT, and the Test Lexicon (G26's headword ἀγάπη), indexed.
fn fixture(name: &str) -> (Session, PathBuf) {
    let root = std::env::temp_dir().join(format!("obs-search-views-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    write_library(
        &root.join("library"),
        &[kjv_agape(), greek_agape(), study::tlx()],
    );
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.library
        .set_preferred_order(vec!["kjv".into(), "tgnt".into(), "tlx".into()]);
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

/// Search `strong:G26` in KJV and TGNT.
fn search_g26(s: &mut Session) -> TabId {
    let out = s
        .run(
            "search.run",
            &json!({ "text": "strong:G26", "bibles": "kjv,tgnt" }),
        )
        .unwrap();
    settle(s);
    TabId(out["tab"].as_u64().unwrap())
}

fn tab_state(s: &Session, tab: TabId) -> &SearchTab {
    s.search_tab(tab).unwrap()
}

fn references(s: &Session, tab: TabId) -> Vec<String> {
    let r = tab_state(s, tab).results.clone().unwrap();
    r.rows.iter().map(|row| r.row_reference(row)).collect()
}

fn cleanup(s: Session, root: PathBuf) {
    drop(s);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn strong_g26_grid_has_john_and_1_john_standing_out_and_a_cell_filters_the_verse_list() {
    let (mut s, root) = fixture("grid");
    let tab = search_g26(&mut s);
    assert_eq!(
        tab_state(&s, tab).results.as_ref().unwrap().summary(),
        "10 verses · 13 hits · KJV 10 · TGNT 3"
    );
    s.run("search.view", &json!({ "tab": tab.0, "value": "grid" }))
        .unwrap();
    assert_eq!(tab_state(&s, tab).view, ResultsView::Grid);

    let grid = HitGrid::new(tab_state(&s, tab).all_results().unwrap());
    assert_eq!(
        grid.books,
        [
            Book::John,
            Book::Romans,
            Book::FirstCorinthians,
            Book::FirstJohn
        ]
    );
    assert_eq!(grid.bibles, ["KJV", "TGNT"]);
    assert_eq!(grid.counts, [[4, 1], [1, 0], [1, 0], [4, 2]]);
    assert_eq!(grid.max, 4);
    // John and 1 John stand out: the hottest cells, every other book a quarter as hot.
    assert_eq!(grid.hottest(0.5), [Book::John, Book::FirstJohn]);
    assert!((grid.heat(grid.count(Book::Romans, 0)) - 0.25).abs() < 1e-6);
    assert_eq!(grid.heat(grid.count(Book::Romans, 1)), 0.0);

    // Clicking the 1 John × KJV cell: the Verse list shows its four Verses, KJV hits only.
    let out = s
        .run(
            "search.filter",
            &json!({ "tab": tab.0, "book": "1John", "bible": "kjv" }),
        )
        .unwrap();
    assert_eq!(out["rows"], 4);
    assert_eq!(out["filter"], "1 John · KJV");
    let st = tab_state(&s, tab);
    assert_eq!(st.view, ResultsView::Verses, "the filtered list is shown");
    assert_eq!(st.selected, Some(0));
    let r = st.results.clone().unwrap();
    assert!(
        r.rows
            .iter()
            .all(|row| row.hits.iter().all(|h| h.bible == 0))
    );
    assert_eq!(r.summary(), "4 verses · 4 hits · KJV 4 · TGNT 0");
    assert_eq!(
        references(&s, tab),
        ["1 John 4:7", "1 John 4:8", "1 John 4:16", "1 John 4:18"]
    );
    // The filtered list's snippets are fetched and highlighted as before.
    s.request_search_snippets(tab, 0..4);
    settle(&mut s);
    let sn = tab_state(&s, tab).snippet(1, 0).unwrap().clone();
    assert_eq!(
        sn.text,
        "He that loveth not knoweth not God; for God is love."
    );
    assert_eq!(
        sn.highlights
            .iter()
            .map(|h| &sn.text[h.clone()])
            .collect::<Vec<_>>(),
        ["love"]
    );
    // Opening a filtered row goes to that Verse.
    let (_, opened) = s.open_search_hit(tab, 2, 0, false).unwrap();
    let opened = opened.unwrap();
    assert_eq!(
        s.workspace.tab(opened).unwrap().history.current(),
        Some(Verse::new(Book::FirstJohn, 4, 16))
    );
    // The Grid still counts every result.
    assert_eq!(
        HitGrid::new(tab_state(&s, tab).all_results().unwrap()).counts,
        grid.counts
    );

    // The John row (any Bible) and then no filter.
    s.run("search.filter", &json!({ "tab": tab.0, "book": "John" }))
        .unwrap();
    assert_eq!(
        references(&s, tab),
        ["John 13:35", "John 15:9", "John 15:13", "John 17:26"]
    );
    let out = s.run("search.filter", &json!({ "tab": tab.0 })).unwrap();
    assert_eq!(out["rows"], 10);
    assert_eq!(out["filter"], serde_json::Value::Null);
    assert!(tab_state(&s, tab).filter.is_none());

    // A new search clears the filter.
    s.run(
        "search.filter",
        &json!({ "tab": tab.0, "book": "Romans", "bible": "kjv" }),
    )
    .unwrap();
    let tab = search_g26(&mut s);
    assert!(tab_state(&s, tab).filter.is_none());
    assert_eq!(tab_state(&s, tab).row_count(), 10);
    assert!(
        s.run("search.filter", &json!({ "tab": tab.0, "book": "Nowhere" }))
            .is_err()
    );
    cleanup(s, root);
}

#[test]
fn chart_counts_verses_by_book_or_by_chapter_and_a_bar_filters_the_verse_list() {
    let (mut s, root) = fixture("chart");
    let tab = search_g26(&mut s);
    let results = tab_state(&s, tab).all_results().unwrap().clone();
    let bars: Vec<(String, usize)> = chart_bars(&results, ChartBy::Book)
        .into_iter()
        .map(|b| (b.label, b.count))
        .collect();
    assert_eq!(
        bars,
        [
            ("John".to_string(), 4),
            ("Romans".to_string(), 1),
            ("1 Corinthians".to_string(), 1),
            ("1 John".to_string(), 4),
        ]
    );
    // By chapter within John: all 21 chapters, so the gaps show.
    let out = s
        .run("search.chart", &json!({ "tab": tab.0, "value": "John" }))
        .unwrap();
    let bars = out["bars"].as_array().unwrap();
    assert_eq!(bars.len(), 21);
    let counts: Vec<u64> = bars.iter().map(|b| b["count"].as_u64().unwrap()).collect();
    assert_eq!(counts[12], 1);
    assert_eq!(counts[14], 2);
    assert_eq!(counts[16], 1);
    assert_eq!(counts.iter().sum::<u64>(), 4);
    assert_eq!(bars[14]["label"], "John 15");
    let st = tab_state(&s, tab);
    assert_eq!(st.view, ResultsView::Chart);
    assert_eq!(st.chart, ChartBy::Chapter(Book::John));
    // Clicking the John 15 bar.
    let bar = chart_bars(&results, ChartBy::Chapter(Book::John))
        .into_iter()
        .nth(14)
        .unwrap();
    assert_eq!(bar.filter.label(&results), "John 15");
    s.set_results_filter(tab, Some(bar.filter));
    assert_eq!(references(&s, tab), ["John 15:9", "John 15:13"]);
    assert_eq!(tab_state(&s, tab).view, ResultsView::Verses);
    cleanup(s, root);
}

/// The Analysis rows as (reference, Bible, word, lemma, morphology) in `order`.
fn table(a: &SearchAnalysis, order: &[usize]) -> Vec<[String; 5]> {
    order
        .iter()
        .map(|&i| {
            let r = &a.rows[i];
            [
                r.reference.clone(),
                r.bible.clone(),
                r.word.clone(),
                r.lemma.clone(),
                r.morphology.clone(),
            ]
        })
        .collect()
}

fn analysis(s: &Session, tab: TabId) -> (Arc<SearchAnalysis>, Vec<usize>) {
    let st = tab_state(s, tab);
    (
        st.analysis.clone().expect("the Analysis is made"),
        st.analysis_order.to_vec(),
    )
}

#[test]
fn analysis_lists_each_matched_word_with_its_lemma_and_morphology() {
    let (mut s, root) = fixture("analysis");
    let tab = search_g26(&mut s);
    assert!(
        tab_state(&s, tab).analysis.is_none(),
        "made only when shown"
    );
    s.run("search.view", &json!({ "tab": tab.0, "value": "analysis" }))
        .unwrap();
    assert!(tab_state(&s, tab).analysis_running());
    settle(&mut s);
    assert!(!tab_state(&s, tab).analysis_running());
    let (a, order) = analysis(&s, tab);
    let rows = table(&a, &order);
    // 13 KJV words (charity twice in 1 Cor 13:13, love twice in 1 Jn 4:16 and 4:18) and 4
    // TGNT words; loved (G25) in John 17:26 is not one of them.
    assert_eq!(rows.len(), 17);
    let s_ = |x: [&str; 5]| x.map(String::from);
    assert_eq!(
        rows[0],
        s_(["John 13:35", "KJV", "love", "ἀγάπη", ""]),
        "a KJV word borrows its Lemma from the Lexicon"
    );
    assert_eq!(
        rows[1],
        s_(["John 13:35", "TGNT", "ἀγάπην", "ἀγάπη", "N-ASF"])
    );
    assert!(rows.iter().all(|r| r[2] != "loved"));
    let cor: Vec<&[String; 5]> = rows
        .iter()
        .filter(|r| r[0] == "1 Corinthians 13:13")
        .collect();
    assert_eq!(cor.len(), 2);
    assert!(cor.iter().all(|r| r[2] == "charity"));
    let dsf = rows.iter().find(|r| r[4] == "N-DSF").unwrap();
    assert_eq!(dsf[0], "1 John 4:16");
    assert_eq!(dsf[2], "ἀγάπῃ");
    // The context is the hit's text around the word.
    let i = order
        .iter()
        .copied()
        .find(|&i| a.rows[i].reference == "John 15:13")
        .unwrap();
    assert_eq!(a.rows[i].before(), "Greater ");
    assert!(a.rows[i].after().starts_with(" hath no man"));
    // The table's short Context: a few words each side.
    let (text, word) = a.rows[i].excerpt(3, 4);
    assert_eq!(text, "Greater love hath no man than …");
    assert_eq!(&text[word], "love");
    let j = order
        .iter()
        .copied()
        .find(|&i| a.rows[i].reference == "1 Corinthians 13:13")
        .unwrap();
    let (text, word) = a.rows[j].excerpt(3, 4);
    assert_eq!(
        text,
        "… abideth faith, hope, charity, these three; but the …"
    );
    assert_eq!(&text[word], "charity");
    let (text, _) = a.rows[j + 1].excerpt(3, 4);
    assert_eq!(text, "… of these is charity.");
    // A row opens its Verse, whatever the Verse list's filter.
    s.set_results_filter(
        tab,
        Some(ResultsFilter {
            book: Book::Romans,
            chapter: None,
            bible: None,
        }),
    );
    let (_, opened) = s
        .open_search_hit_in(tab, a.rows[i].row, a.rows[i].hit, false, true)
        .unwrap();
    assert_eq!(
        s.workspace.tab(opened.unwrap()).unwrap().history.current(),
        Some(Verse::new(Book::John, 15, 13))
    );
    // The Analysis is kept (the results are the same).
    s.set_results_view(tab, ResultsView::Analysis);
    assert!(!tab_state(&s, tab).analysis_running());
    assert!(Arc::ptr_eq(
        &tab_state(&s, tab).analysis.clone().unwrap(),
        &a
    ));

    // A new search while the Analysis is shown makes the new one.
    s.run(
        "search.run",
        &json!({ "tab": tab.0, "text": "strong:G26", "bibles": "tgnt" }),
    )
    .unwrap();
    settle(&mut s);
    let (a, order) = analysis(&s, tab);
    assert_eq!(table(&a, &order).len(), 4);
    cleanup(s, root);
}

#[test]
fn analysis_sorts_by_every_column_both_ways() {
    let (mut s, root) = fixture("sort");
    let tab = search_g26(&mut s);
    s.set_results_view(tab, ResultsView::Analysis);
    settle(&mut s);
    let (a, canonical) = analysis(&s, tab);
    assert_eq!(canonical, (0..a.rows.len()).collect::<Vec<_>>());
    for column in AnalysisColumn::ALL {
        for descending in [false, true] {
            let out = s
                .run(
                    "search.sort",
                    &json!({ "tab": tab.0, "column": column.id(), "descending": descending }),
                )
                .unwrap();
            assert_eq!(out["column"], column.id());
            let (_, order) = analysis(&s, tab);
            assert_eq!(order.len(), a.rows.len());
            let key = |i: usize| -> (String, usize) {
                let r = &a.rows[i];
                let k = match column {
                    AnalysisColumn::Reference => String::new(),
                    AnalysisColumn::Context => {
                        format!("{}\u{1}{}", r.after().trim_start(), r.before()).to_lowercase()
                    }
                    c => r.cell(c).to_lowercase(),
                };
                (k, i)
            };
            for w in order.windows(2) {
                let (ka, ia) = key(w[0]);
                let (kb, ib) = key(w[1]);
                let ok = match (column, descending) {
                    (AnalysisColumn::Reference, false) => ia < ib,
                    (AnalysisColumn::Reference, true) => ia > ib,
                    // Empty cells last, whichever way.
                    _ if ka.is_empty() != kb.is_empty() => kb.is_empty(),
                    (_, false) => ka < kb || (ka == kb && ia < ib),
                    (_, true) => ka > kb || (ka == kb && ia < ib),
                };
                assert!(ok, "{column:?} descending={descending}: {ka:?} then {kb:?}");
            }
        }
    }
    let first = |s: &Session| {
        let (a, order) = analysis(s, tab);
        table(&a, &order)[0].clone()
    };
    s.run(
        "search.sort",
        &json!({ "tab": tab.0, "column": "bible", "descending": true }),
    )
    .unwrap();
    assert_eq!(first(&s)[1], "TGNT");
    s.run(
        "search.sort",
        &json!({ "tab": tab.0, "column": "word", "descending": false }),
    )
    .unwrap();
    assert_eq!(first(&s)[2], "charity");
    // Clicking the sorted column again turns it round.
    let out = s
        .run("search.sort", &json!({ "tab": tab.0, "column": "word" }))
        .unwrap();
    assert_eq!(out["descending"], true);
    assert_eq!(first(&s)[2], "ἀγάπῃ");
    s.run(
        "search.sort",
        &json!({ "tab": tab.0, "column": "morphology", "descending": true }),
    )
    .unwrap();
    assert_eq!(first(&s)[4], "N-NSF");
    // Ascending too, the KJV's words (no Morphology) come last.
    s.run(
        "search.sort",
        &json!({ "tab": tab.0, "column": "morphology", "descending": false }),
    )
    .unwrap();
    assert_eq!(first(&s)[4], "N-ASF");
    s.run(
        "search.sort",
        &json!({ "tab": tab.0, "column": "reference", "descending": true }),
    )
    .unwrap();
    assert_eq!(first(&s)[0], "1 John 4:18");
    assert!(
        s.run("search.sort", &json!({ "tab": tab.0, "column": "colour" }))
            .is_err()
    );
    cleanup(s, root);
}

/// A strict RFC 4180 reader: records of fields, CRLF line ends, quoted fields with doubled
/// quotes.
fn read_csv(text: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') if field.is_empty() => quoted = true,
            (false, ',') => record.push(std::mem::take(&mut field)),
            (false, '\r') => {
                assert_eq!(chars.next(), Some('\n'), "CR without LF");
                record.push(std::mem::take(&mut field));
                records.push(std::mem::take(&mut record));
            }
            (false, '\n') => panic!("LF without CR"),
            (false, c) => field.push(c),
        }
    }
    assert!(
        !quoted && field.is_empty() && record.is_empty(),
        "unterminated record"
    );
    records
}

#[test]
fn analysis_exports_as_csv_a_spreadsheet_opens() {
    let (mut s, root) = fixture("csv");
    let tab = search_g26(&mut s);
    let path = root.join("g26.csv");
    // Not before the Analysis is made.
    let err = s
        .run(
            "search.export_csv",
            &json!({ "tab": tab.0, "path": path.to_string_lossy() }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("Analysis"), "{err}");
    s.set_results_view(tab, ResultsView::Analysis);
    settle(&mut s);
    s.run(
        "search.sort",
        &json!({ "tab": tab.0, "column": "bible", "descending": true }),
    )
    .unwrap();
    let out = s
        .run(
            "search.export_csv",
            &json!({ "tab": tab.0, "path": path.to_string_lossy() }),
        )
        .unwrap();
    assert_eq!(out["records"], 17);
    let text = std::fs::read_to_string(&path).unwrap();
    // UTF-8 with a byte-order mark, so spreadsheets read the Greek as UTF-8.
    let body = text.strip_prefix('\u{feff}').expect("a byte-order mark");
    let records = read_csv(body);
    assert_eq!(records.len(), 18);
    assert_eq!(
        records[0],
        [
            "Reference",
            "Bible",
            "Word",
            "Lemma",
            "Morphology",
            "Context"
        ]
    );
    assert!(records.iter().all(|r| r.len() == 6));
    // In the order shown: TGNT first.
    assert_eq!(
        records[1][..5],
        ["John 13:35", "TGNT", "ἀγάπην", "ἀγάπη", "N-ASF"]
    );
    assert!(records[1][5].starts_with("ἐν τούτῳ γνώσονται πάντες ὅτι ἐμοὶ μαθηταί ἐστε,"));
    // A context with commas is quoted and reads back whole.
    let cor = records
        .iter()
        .find(|r| r[0] == "1 Corinthians 13:13")
        .unwrap();
    assert_eq!(
        cor[5],
        "And now abideth faith, hope, charity, these three; but the greatest of these is charity."
    );
    assert!(body.contains(
        "\"And now abideth faith, hope, charity, these three; but the greatest of these is charity.\""
    ));
    cleanup(s, root);
}

#[test]
fn csv_fields_are_quoted_and_never_read_as_formulas() {
    assert_eq!(csv_field("love"), "love");
    assert_eq!(csv_field("a, b"), "\"a, b\"");
    assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
    assert_eq!(csv_field("two\nlines"), "\"two\nlines\"");
    assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
    assert_eq!(csv_field("+1"), "'+1");
    assert_eq!(csv_field("@x"), "'@x");
    assert_eq!(csv_field("-1,2"), "\"'-1,2\"");
    assert_eq!(csv_field(""), "");
}

#[test]
fn results_views_have_ids_and_labels() {
    for v in ResultsView::ALL {
        assert_eq!(ResultsView::from_id(v.id()), Some(v));
    }
    assert_eq!(
        ResultsView::ALL.map(ResultsView::label),
        ["Verses", "Grid", "Chart", "Analysis"]
    );
    for c in AnalysisColumn::ALL {
        assert_eq!(AnalysisColumn::from_id(c.id()), Some(c));
    }
    assert_eq!(ResultsView::default(), ResultsView::Verses);
}

/// The real Starter KJV and TAGNT: `strong:G26`'s Grid (1 John stands out) and Analysis, and
/// `strong:G25`'s Grid (John and 1 John stand out). Run with
/// `cargo test --release -p obs-engine real_starter_g26 -- --ignored --nocapture`.
#[test]
#[ignore = "needs the Starter library in the platform data folder"]
fn real_starter_g26_grid_and_analysis() {
    let Some(dir) = default_library_dir().filter(|d| d.join("kjv-strongs.obr").is_file()) else {
        eprintln!("no Starter library: skipped");
        return;
    };
    let mut s = Session::new(JobMode::Inline);
    let mut ids = Vec::new();
    for f in [
        "kjv-strongs.obr",
        "tagnt.obr",
        "strongs-greek.obr",
        "abbott-smith.obr",
    ] {
        if dir.join(f).is_file() {
            let e = s.library.add_file(&dir.join(f)).unwrap();
            if e.manifest.resource_type == obs_model::ResourceType::Bible {
                ids.push(e.manifest.resource_id.clone());
            }
        }
    }
    s.set_search_cache_dir(std::env::temp_dir().join("obs-real-search-cache"));
    s.refresh_search_indexes();
    settle(&mut s);
    let out = s
        .run(
            "search.run",
            &json!({ "text": "strong:G26", "bibles": ids.join(",") }),
        )
        .unwrap();
    settle(&mut s);
    let tab = TabId(out["tab"].as_u64().unwrap());
    let results = tab_state(&s, tab).all_results().unwrap().clone();
    eprintln!("{}", results.summary());
    let grid = HitGrid::new(&results);
    eprintln!("{:<16} {:?}", "", grid.bibles);
    for (b, c) in grid.books.iter().zip(&grid.counts) {
        eprintln!("{:<16} {c:?}", b.name());
    }
    eprintln!("hottest (≥ 0.5): {:?}", grid.hottest(0.5));
    let started = std::time::Instant::now();
    s.set_results_view(tab, ResultsView::Analysis);
    settle(&mut s);
    let (a, order) = analysis(&s, tab);
    eprintln!(
        "Analysis: {} words in {:?}",
        a.rows.len(),
        started.elapsed()
    );
    for r in table(&a, &order).iter().take(6) {
        eprintln!("{r:?}");
    }
    // The noun ἀγάπη clusters in 1 John (14 Verses), then 1 Corinthians (11).
    assert_eq!(grid.hottest(1.0), [Book::FirstJohn]);
    assert!(grid.count(Book::FirstJohn, 0) > grid.count(Book::John, 0));

    // The verb ἀγαπάω is where John and 1 John both stand out.
    let out = s
        .run(
            "search.run",
            &json!({ "tab": tab.0, "text": "strong:G25", "bibles": ids.join(",") }),
        )
        .unwrap();
    settle(&mut s);
    let tab = TabId(out["tab"].as_u64().unwrap());
    let grid = HitGrid::new(tab_state(&s, tab).all_results().unwrap());
    eprintln!("strong:G25 hottest (≥ 0.5): {:?}", grid.hottest(0.5));
    for (b, c) in grid.books.iter().zip(&grid.counts) {
        eprintln!("{:<16} {c:?}", b.name());
    }
    assert_eq!(grid.hottest(0.5)[..2], [Book::John, Book::FirstJohn]);
}
