#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;

use crate::get_bibles::*;
use crate::testing::MockRepos;
use crate::*;

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("obs-getbibles-{name}-{}", std::process::id()))
}

/// An inline Session pointed at mock repositories, with its catalog cache in `root`.
fn session(name: &str) -> (Session, Arc<MockRepos>, PathBuf) {
    let root = temp(name);
    let repos = Arc::new(MockRepos::new(&root.join("repos")));
    let mut s = Session::new(JobMode::Inline);
    s.set_fetch(repos.clone());
    s.get_bibles
        .set_cache_path(root.join("cache/sword-catalog.json"));
    (s, repos, root)
}

fn refresh(s: &mut Session) {
    s.run("getbibles.refresh", &json!({})).unwrap();
    s.jobs().pump(1);
    assert!(s.poll_imports().is_empty(), "no stray events");
    assert!(!s.get_bibles.is_refreshing());
}

fn listing(module: &str, repo: &str) -> BibleListing {
    BibleListing {
        module: module.into(),
        repo: repo.into(),
        title: module.into(),
        abbreviation: module.into(),
        language: "en".into(),
        size: None,
        licence: None,
        version: None,
    }
}

#[test]
fn featured_data_file_parses_with_nasb_1995_first() {
    let featured = featured_bibles();
    assert_eq!(featured.len(), 13);
    assert_eq!(
        featured[0],
        FeaturedBible {
            module: "NASB1995".into(),
            repo: "Lockman Foundation".into()
        }
    );
    let modules: Vec<&str> = featured.iter().map(|f| f.module.as_str()).collect();
    for m in ["NASB", "NETfree", "LEB", "BSB", "KJV", "ASV", "YLT"] {
        assert!(modules.contains(&m), "{m} missing from {modules:?}");
    }
}

#[test]
fn catalog_lists_bibles_of_every_repository_with_conf_licence_and_size() {
    let (mut s, repos, root) = session("fetch");
    assert!(s.get_bibles.catalog().is_none());
    refresh(&mut s);
    assert_eq!(s.get_bibles.offline(), None);
    let catalog = s.get_bibles.catalog().unwrap();

    // Bibles only, locked modules left out, one row per module.
    let mut modules: Vec<&str> = catalog.bibles.iter().map(|b| b.module.as_str()).collect();
    modules.sort();
    assert_eq!(
        modules,
        [
            "NASB1995",
            "ObsGreekBible",
            "ObsHebrew",
            "ObsKjvGbf",
            "ObsKjvOsis"
        ]
    );
    let nasb = catalog.get("nasb1995").unwrap();
    assert_eq!(nasb.repo, "Lockman Foundation");
    assert_eq!(nasb.title, "New American Standard Bible - NASB 1995");
    assert_eq!(nasb.abbreviation, "NASB95");
    assert_eq!(
        nasb.licence.as_deref(),
        Some("Copyrighted; Permission to distribute granted to CrossWire")
    );
    // Lockman's conf has no InstallSize: the size is the package zip's, asked by HEAD.
    assert_eq!(nasb.size, Some(crate::testing::NASB_PACKAGE_SIZE));
    assert_eq!(nasb.source_label(), "from Lockman Foundation");

    // NASB 1995 heads Featured; Featured modules no repository offers are left out.
    let featured: Vec<&str> = catalog
        .featured()
        .iter()
        .map(|b| b.module.as_str())
        .collect();
    assert_eq!(featured, ["NASB1995"]);

    // A repository that cannot be read is reported, and does not fail the rest.
    assert_eq!(catalog.failed.len(), 1);
    assert_eq!(catalog.failed[0].0, "Broken");
    assert_eq!(
        catalog
            .repo("CrossWire")
            .unwrap()
            .repository()
            .sources
            .len(),
        1
    );
    // Every byte came from the repositories (the master list, then each file:// repo).
    let fetched = repos.fetched.lock().unwrap().clone();
    assert_eq!(fetched[0], obs_formats::sword_repo::MASTER_REPO_LIST_URL);
    assert!(
        fetched[1..]
            .iter()
            .all(|u| u.starts_with("file://") || u.starts_with("HEAD file://")),
        "{fetched:?}"
    );
    // Only Featured Bibles without a conf size are asked (here NASB1995 alone).
    let heads: Vec<&String> = fetched.iter().filter(|u| u.starts_with("HEAD ")).collect();
    assert_eq!(heads.len(), 1, "{heads:?}");
    assert!(heads[0].ends_with("/lockman/packages/NASB1995.zip"));
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_same_module_in_several_repositories_shows_once_from_the_preferred_source() {
    let (mut s, _repos, root) = session("dedupe");
    refresh(&mut s);
    let catalog = s.get_bibles.catalog().unwrap();
    // ObsKjvOsis is in CrossWire and eBible.org: CrossWire wins over listing order.
    let kjv = catalog.get("ObsKjvOsis").unwrap();
    assert_eq!(kjv.repo, "CrossWire");
    assert_ne!(kjv.title, "eBible copy");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();

    // The Featured list's repository beats CrossWire; otherwise the first listed wins.
    let featured = [FeaturedBible {
        module: "NET".into(),
        repo: "Bible.org".into(),
    }];
    let kept = dedupe(
        vec![
            listing("NET", "CrossWire"),
            listing("net", "Bible.org"),
            listing("X", "eBible.org"),
            listing("X", "IBT"),
            listing("Y", "IBT"),
            listing("Y", "CrossWire"),
        ],
        &featured,
    );
    let got: Vec<(&str, &str)> = kept
        .iter()
        .map(|b| (b.module.as_str(), b.repo.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("net", "Bible.org"),
            ("X", "eBible.org"),
            ("Y", "CrossWire")
        ]
    );
}

#[test]
fn all_bibles_search_and_language_filter() {
    let (mut s, _repos, root) = session("filter");
    refresh(&mut s);
    let catalog = s.get_bibles.catalog().unwrap();
    assert_eq!(catalog.all("", None).len(), 5);
    let greek: Vec<&str> = catalog
        .all("", Some("grc"))
        .iter()
        .map(|b| b.module.as_str())
        .collect();
    assert_eq!(greek, ["ObsGreekBible"]);
    let found: Vec<&str> = catalog
        .all("nasb", None)
        .iter()
        .map(|b| b.module.as_str())
        .collect();
    assert_eq!(found, ["NASB1995"], "abbreviation/title search");
    assert!(catalog.all("nasb", Some("grc")).is_empty());
    let langs = catalog.languages();
    assert_eq!(langs[0], ("en".to_string(), 4));
    assert!(langs.contains(&("grc".to_string(), 1)));

    // The same through the command.
    let out = s
        .run(
            "getbibles.list",
            &json!({"query": "greek", "language": "grc"}),
        )
        .unwrap();
    assert_eq!(out["all"].as_array().unwrap().len(), 1);
    assert_eq!(out["featured"][0]["module"], "NASB1995");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn offline_the_cached_catalog_is_shown_with_the_reason() {
    let (mut s, repos, root) = session("offline");
    refresh(&mut s);
    let fetched = s.get_bibles.catalog().unwrap().clone();
    assert!(root.join("cache/sword-catalog.json").is_file());

    // A new Session (next app start) opens the cache with no network at all.
    repos.set_offline(true);
    let mut s = Session::new(JobMode::Inline);
    s.set_fetch(repos.clone());
    s.get_bibles
        .set_cache_path(root.join("cache/sword-catalog.json"));
    assert_eq!(s.get_bibles.catalog(), Some(&fetched));
    refresh(&mut s);
    let why = s.get_bibles.offline().unwrap();
    assert!(why.contains("repository list"), "{why}");
    assert_eq!(s.get_bibles.catalog(), Some(&fetched), "cache kept");
    let out = s.run("getbibles.list", &json!({})).unwrap();
    assert!(out["offline"].as_str().is_some());
    assert_eq!(out["featured"][0]["module"], "NASB1995");

    // Back online, a refresh clears the message.
    repos.set_offline(false);
    refresh(&mut s);
    assert_eq!(s.get_bibles.offline(), None);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn without_an_app_transport_nothing_reaches_the_network() {
    let root = temp("no-transport");
    let mut s = Session::new(JobMode::Inline);
    s.get_bibles.set_cache_path(root.join("sword-catalog.json"));
    refresh(&mut s);
    let why = s.get_bibles.offline().unwrap();
    assert!(why.contains("no network transport"), "{why}");
    assert!(s.get_bibles.catalog().is_none());
    let _ = std::fs::remove_dir_all(root);
}
