//! Whole-Bible search timings on the real KJV (#58, targets from #19): words and Strong's in
//! <= 100 ms, phrases in <= 250 ms. Needs the Starter library's `kjv-strongs.obr`
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

use obs_search::{Compiled, INDEX_VERSION, NoLexicon, SearchIndex, SearchOptions, to_fts5};
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
        // (query, target ms, minimum hits as a sanity check)
        ("love", 100, 250),
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
            let compiled = to_fts5(q, &NoLexicon).unwrap();
            assert!(matches!(compiled, Compiled::Match(_)), "{q}");
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
