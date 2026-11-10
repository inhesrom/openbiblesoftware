//! The Command box's Search preview (#295): the debounced query, the ★ Bible's first five
//! Verses with the match highlighted and how many match in all, no Bible, an index still being
//! built, and opening a hit.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::json;

use crate::testing::bibles::{kjv, psalms, write_library};
use crate::*;

/// A temp library of `resources` in this Preferred order; with `index`, every Search index
/// is built.
fn fixture(name: &str, resources: &[obs_model::Resource], index: bool) -> (Session, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("obs-search-preview-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("library")).unwrap();
    write_library(&root.join("library"), resources);
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.library.set_preferred_order(
        resources
            .iter()
            .map(|r| r.manifest.resource_id.clone())
            .collect(),
    );
    s.set_search_cache_dir(root.join("cache"));
    if index {
        s.refresh_search_indexes();
        settle(&mut s);
    }
    (s, root)
}

/// Run every queued job and apply its events.
fn settle(s: &mut Session) {
    for _ in 0..1000 {
        s.jobs().pump(16);
        assert!(s.poll_imports().is_empty(), "no stray events");
        if !s.search.is_building()
            && !s.search_preview_running()
            && !s.bible_search.is_reading_ahead()
        {
            return;
        }
    }
    panic!("jobs never settled");
}

fn hits(s: &Session) -> std::sync::Arc<PreviewHits> {
    match s.search_preview() {
        SearchPreview::Hits(h) => h.clone(),
        other => panic!("no hits: {other:?}"),
    }
}

/// Each hit's reference and highlighted words.
fn shown(h: &PreviewHits) -> Vec<(String, Vec<String>)> {
    h.hits
        .iter()
        .map(|hit| {
            let sn = &hit.snippet;
            let words = sn.highlights.iter().map(|r| sn.text[r.clone()].to_string());
            (hit.reference.clone(), words.collect())
        })
        .collect()
}

#[test]
fn the_preview_searches_once_the_typing_has_stopped() {
    let (mut s, root) = fixture("debounce", &[psalms(12), kjv()], true);
    // Keys 30 ms apart: nothing is searched while they come.
    s.update_search_preview(Some("l"), 0.0);
    s.update_search_preview(Some("lo"), 0.03);
    s.update_search_preview(Some("lov"), 0.06);
    s.update_search_preview(Some("love"), 0.09);
    s.update_search_preview(Some("love"), 0.15);
    settle(&mut s);
    assert_eq!(s.search_preview_query(), None);
    assert_eq!(s.search_preview(), &SearchPreview::Off);
    let due = s.search_preview_due_in(0.15).unwrap();
    assert!((due - 0.04).abs() < 1e-9, "{due}");
    // The typing stopped at 0.09: one search, of the last text, 100 ms later.
    s.update_search_preview(Some("love"), 0.19);
    assert_eq!(s.search_preview_query(), Some("love"));
    assert!(
        s.search_preview_running(),
        "a background job, not the caller's thread"
    );
    assert_eq!(s.search_preview_due_in(0.19), None);
    settle(&mut s);
    assert_eq!(hits(&s).query, "love");
    // The same text again searches nothing more.
    s.update_search_preview(Some("love"), 0.5);
    assert!(!s.search_preview_running());

    // While the next text waits, the last hits stay; then they are replaced.
    s.update_search_preview(Some("law"), 1.0);
    assert_eq!(hits(&s).query, "love");
    s.update_search_preview(Some("law"), 1.1);
    settle(&mut s);
    assert_eq!(hits(&s).query, "law");

    // The dropdown closed: nothing to show, and nothing searched for the old text.
    s.update_search_preview(None, 1.2);
    assert_eq!(s.search_preview(), &SearchPreview::Off);
    assert_eq!(s.search_preview_query(), None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_preview_shows_the_first_five_verses_of_the_star_bible_and_how_many_match() {
    let (mut s, root) = fixture("hits", &[psalms(12), kjv()], true);
    s.update_search_preview(Some("love"), 0.0);
    s.update_search_preview(Some("love"), PREVIEW_DEBOUNCE);
    settle(&mut s);
    let h = hits(&s);
    assert_eq!(h.bible, "PSA", "the ★ Bible only");
    assert_eq!(h.total, 12);
    assert_eq!(PREVIEW_HITS, 5);
    let love = vec!["love".to_string()];
    assert_eq!(
        shown(&h),
        (1..=5)
            .map(|v| (format!("Psalms 119:{v}"), love.clone()))
            .collect::<Vec<_>>()
    );
    assert!(
        h.hits[0]
            .snippet
            .text
            .starts_with("Verse 1: O how love I thy law!")
    );

    // Another ★ Bible is searched next time.
    s.library
        .set_preferred_order(["kjv", "psalms"].map(String::from).to_vec());
    s.update_search_preview(Some("neighbour"), 1.0);
    s.update_search_preview(Some("neighbour"), 1.0 + PREVIEW_DEBOUNCE);
    settle(&mut s);
    let h = hits(&s);
    assert_eq!(h.bible, "KJV");
    assert_eq!(h.total, h.hits.len());
    assert_eq!(shown(&h)[0].1, ["neighbour"]);

    // A query the Search Tool would refuse previews nothing.
    s.update_search_preview(Some("-love"), 2.0);
    s.update_search_preview(Some("-love"), 2.0 + PREVIEW_DEBOUNCE);
    settle(&mut s);
    assert_eq!(s.search_preview(), &SearchPreview::Off);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn without_a_bible_the_preview_says_so() {
    let (mut s, root) = fixture("no-bible", &[], true);
    s.update_search_preview(Some("love"), 0.0);
    s.update_search_preview(Some("love"), PREVIEW_DEBOUNCE);
    settle(&mut s);
    assert_eq!(s.search_preview(), &SearchPreview::NoBible);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn while_the_star_bible_is_indexed_the_preview_says_so_and_fills_in_when_ready() {
    let (mut s, root) = fixture("indexing", &[psalms(12)], false);
    s.refresh_search_indexes();
    assert!(s.search.is_building());
    s.update_search_preview(Some("love"), 0.0);
    s.update_search_preview(Some("love"), PREVIEW_DEBOUNCE);
    assert_eq!(
        s.search_preview(),
        &SearchPreview::Indexing("PSA".to_string())
    );
    settle(&mut s);
    assert!(!s.search.is_building());
    // The next frame finds the index ready and searches again, the text unchanged.
    s.update_search_preview(Some("love"), 1.0);
    settle(&mut s);
    assert_eq!(hits(&s).total, 12);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn opening_a_preview_hit_goes_to_its_verse_in_a_bible() {
    let (mut s, root) = fixture("open", &[psalms(12), kjv()], true);
    let err = s.run("command_box.open_hit", &json!({ "hit": 0 }));
    assert!(err.is_err(), "no preview yet");
    s.update_search_preview(Some("love"), 0.0);
    s.update_search_preview(Some("love"), PREVIEW_DEBOUNCE);
    settle(&mut s);
    let out = s.run("command_box.open_hit", &json!({ "hit": 2 })).unwrap();
    assert_eq!(out["verse"], "Ps.119.3");
    let tab = TabId(out["tab"].as_u64().unwrap());
    let t = s.workspace.tab(tab).unwrap();
    assert_eq!(t.kind, TabKind::Resource("psalms".into()));
    assert_eq!(s.workspace.focused(), Some(tab));
    assert!(s.run("command_box.open_hit", &json!({ "hit": 5 })).is_err());
    let _ = std::fs::remove_dir_all(root);
}
