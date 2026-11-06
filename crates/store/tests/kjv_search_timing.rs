//! Whole-Bible search timings on the real KJV (#58, targets from #19): words and Strong's in
//! <= 100 ms, phrases in <= 250 ms. Stemmed words (Word forms) keep the word target;
//! `NEAR/n`, `BEFORE/n` and `WITHIN n verses` the phrase target (#180). Needs the Starter library's `kjv-strongs.obr`
//! (`$OBS_LIBRARY`, else `~/.local/share/OpenBible/library`) and is skipped without it.
//! Measure a release build:
//!
//! ```text
//! cargo test --release -p obs-store --test kjv_search_timing -- --ignored --nocapture
//! ```
//!
//! `obs-cli search <kjv-strongs.obr>` runs the same set from the command line.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use obs_model::Key;
use obs_scripture::{Book, Verse};
use obs_search::{INDEX_VERSION, NoLexicon, SearchIndex, SearchOptions, compile, parse};
use obs_store::{IndexKey, ObrReader, ResourceStore, SqliteSearchIndex, build_search_index};

fn kjv() -> Option<PathBuf> {
    let dir = std::env::var_os("OBS_LIBRARY")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".local/share/OpenBible/library"))
        })?;
    let path = dir.join("kjv-strongs.obr");
    path.is_file().then_some(path)
}

/// A query of the timing set: `~` before a word turns Word forms on for the whole query.
fn compiled(q: &str) -> obs_search::Compiled {
    let stemmed = q.contains('~');
    let parsed = parse(&q.replace('~', "")).unwrap();
    let parsed = if stemmed {
        parsed.with_word_forms()
    } else {
        parsed
    };
    compile(&parsed, &NoLexicon).unwrap()
}

#[test]
#[ignore = "needs the real KJV; run in release with --ignored"]
fn whole_bible_searches_meet_the_targets() {
    let Some(obr) = kjv() else {
        eprintln!("skipped: no kjv-strongs.obr in the library");
        return;
    };
    let resource = ObrReader::open(&obr).unwrap().load().unwrap();
    let dir = std::env::temp_dir().join(format!("obs-kjv-timing-{}", std::process::id()));
    let started = Instant::now();
    let path = build_search_index(&resource, INDEX_VERSION, &dir, &mut |_, _| true).unwrap();
    eprintln!(
        "built {} in {:.2} s ({:.1} MB)",
        path.display(),
        started.elapsed().as_secs_f64(),
        std::fs::metadata(&path).unwrap().len() as f64 / 1e6
    );
    let key = IndexKey::new(&resource.manifest, INDEX_VERSION);
    let set: &[(&str, u64, usize)] = &[
        // (query, target ms, minimum hits as a sanity check); `~` turns Word forms on.
        ("love", 100, 250),
        ("~love", 100, 400),
        ("~believe", 100, 250),
        ("~the", 100, 20_000),
        ("~a", 100, 6_000),
        ("~go", 100, 1_000),
        ("faith NEAR/5 works", 250, 5),
        ("faith BEFORE/3 works", 250, 1),
        ("the BEFORE/3 lord", 250, 100),
        ("faith WITHIN 2 verses works", 250, 20),
        ("the WITHIN 2 verses and", 250, 1_000),
        ("~love NEAR/3 ~god", 250, 10),
        ("the", 100, 20_000),
        ("lord god", 100, 1_000),
        ("lov*", 100, 400),
        ("strong:G26", 100, 100),
        ("H430", 100, 2_000),
        ("strong:G5485 favour", 100, 1),
        ("\"in the beginning\"", 250, 10),
        ("\"the son of man\"", 250, 80),
        ("\"the lord\"", 250, 5_000),
    ];
    let mut missed = Vec::new();
    for (q, target, min_hits) in set {
        let mut times = Vec::new();
        let mut hits = 0;
        for _ in 0..7 {
            let t = Instant::now();
            // As the engine does: open the cache, compile, collect every hit.
            let index = SqliteSearchIndex::open(&path, Some(&key)).unwrap();
            let compiled = compiled(q);
            hits = index
                .search(&compiled, &SearchOptions::default())
                .unwrap()
                .len();
            times.push(t.elapsed());
        }
        times.sort();
        let median = times[times.len() / 2];
        eprintln!(
            "{q:<24} {hits:>6} hits  median {:>6.1} ms  max {:>6.1} ms  (target {target} ms)",
            median.as_secs_f64() * 1e3,
            times[times.len() - 1].as_secs_f64() * 1e3
        );
        assert!(hits >= *min_hits, "{q}: only {hits} hits");
        if median > Duration::from_millis(*target) {
            missed.push(format!("{q}: {median:?} > {target} ms"));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(missed.is_empty(), "targets missed: {missed:?}");
}

/// The Verses a query finds in the real KJV, as (book, chapter, verse).
fn verses(index: &SqliteSearchIndex, q: &str) -> Vec<(Book, u16, u16)> {
    index
        .search(&compiled(q), &SearchOptions::default())
        .unwrap()
        .into_iter()
        .filter_map(|h| match h.key {
            Some(Key::Passage(p)) => {
                let v: Verse = p.start();
                Some((v.book, v.chapter, v.verse))
            }
            _ => None,
        })
        .collect()
}

/// #180's acceptance on the real KJV: `faith WITHIN 2 verses works` finds James 2:17-18, and
/// "love" finds "loveth" with Word forms on only. Run with
/// `cargo test --release -p obs-store --test kjv_search_timing -- --ignored --nocapture`.
#[test]
#[ignore = "needs the real KJV; run with --ignored"]
fn proximity_and_word_forms_on_the_real_kjv() {
    // Run only on request (`--ignored`), so a missing library is a failure, not a pass.
    let obr = kjv().expect("kjv-strongs.obr not found: put the Starter library in the app's data folder (or point $OBS_LIBRARY at it)");
    let resource = ObrReader::open(&obr).unwrap().load().unwrap();
    let dir = std::env::temp_dir().join(format!("obs-kjv-180-{}", std::process::id()));
    let path = build_search_index(&resource, INDEX_VERSION, &dir, &mut |_, _| true).unwrap();
    let index = SqliteSearchIndex::open(&path, None).unwrap();

    let within = verses(&index, "faith WITHIN 2 verses works");
    let james: Vec<u16> = within
        .iter()
        .filter(|(b, c, _)| *b == Book::James && *c == 2)
        .map(|v| v.2)
        .collect();
    eprintln!(
        "faith WITHIN 2 verses works: {} Verses; James 2: {james:?}",
        within.len()
    );
    assert!(james.contains(&17) && james.contains(&18), "{james:?}");
    // James 2:21 says only "works"; 2:20 and 2:22 next to it say "faith".
    assert!(james.contains(&21), "{james:?}");
    assert!(!verses(&index, "faith works").contains(&(Book::James, 2, 21)));

    let john_3_35 = (Book::John, 3, 35); // "The Father loveth the Son"
    let off = verses(&index, "love");
    let on = verses(&index, "~love");
    eprintln!("love: {} Verses; with Word forms {}", off.len(), on.len());
    assert!(!off.contains(&john_3_35));
    assert!(on.contains(&john_3_35));
    assert!(off.iter().all(|v| on.contains(v)));

    // Short words have forms too: "go" finds "goeth", "do" finds "doeth".
    let john_10_4 = (Book::John, 10, 4); // "he goeth before them"
    assert!(!verses(&index, "go").contains(&john_10_4));
    assert!(verses(&index, "~go").contains(&john_10_4));
    let john_3_21 = (Book::John, 3, 21); // "he that doeth truth"
    assert!(!verses(&index, "do").contains(&john_3_21));
    assert!(verses(&index, "~do").contains(&john_3_21));
    let _ = std::fs::remove_dir_all(&dir);
}
