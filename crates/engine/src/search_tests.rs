//! Search index lifecycle (#58): built at Import, rebuilt in the background when missing or
//! stale while ready indexes keep serving, and harmless to delete.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde_json::json;

use crate::*;

/// A temp root with `library/` holding two small Bibles, `kjv-a` and `kjv-b`, and an empty
/// `cache/`; the Session has scanned the library and uses the cache.
fn fixture(name: &str) -> (Session, PathBuf) {
    let root = std::env::temp_dir().join(format!("obs-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("library")).unwrap();
    for id in ["kjv-a", "kjv-b"] {
        let mut r = obs_store::sample_resource();
        r.manifest.resource_id = id.into();
        obs_store::write_obr(&r, &root.join(format!("library/{id}.obr"))).unwrap();
    }
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.set_search_cache_dir(root.join("cache"));
    (s, root)
}

fn search(s: &mut Session, q: &str) -> SearchResults {
    s.search(&SearchRequest {
        query: q.into(),
        ..Default::default()
    })
    .unwrap()
}

/// (resource id, hit count) of each searched Resource that has hits.
fn counts(r: &SearchResults) -> Vec<(&str, usize)> {
    r.resources
        .iter()
        .filter(|h| !h.hits.is_empty())
        .map(|h| (h.resource_id.as_str(), h.hits.len()))
        .collect()
}

/// Run one queued job and apply its events.
fn step(s: &mut Session) {
    s.jobs().pump(1);
    assert!(s.poll_imports().is_empty(), "no stray events");
}

fn cache_files(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root.join("cache"))
        .map(|rd| {
            rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn missing_caches_build_one_at_a_time_and_ready_ones_serve_meanwhile() {
    let (mut s, root) = fixture("initial");
    s.refresh_search_indexes();
    for id in ["kjv-a", "kjv-b"] {
        assert!(matches!(
            s.search.status(id),
            Some(IndexStatus::Indexing { .. })
        ));
    }
    let r = search(&mut s, "world");
    assert!(r.resources.is_empty());
    assert_eq!(r.indexing, ["kjv-a", "kjv-b"]);

    step(&mut s);
    assert_eq!(s.search.status("kjv-a"), Some(&IndexStatus::Ready));
    assert!(s.search.is_building(), "kjv-b is next");
    let r = search(&mut s, "world");
    assert_eq!(counts(&r), [("kjv-a", 2)]);
    assert_eq!(r.indexing, ["kjv-b"]);

    step(&mut s);
    assert!(!s.search.is_building());
    let r = search(&mut s, "world");
    assert_eq!(counts(&r), [("kjv-a", 2), ("kjv-b", 2)]);
    assert!(r.indexing.is_empty() && r.failed.is_empty());
    assert_eq!(cache_files(&root).len(), 2);

    // A second refresh (next launch) finds them: nothing to build.
    let mut again = Session::new(JobMode::Inline);
    again.library.scan(&root.join("library"));
    again.set_search_cache_dir(root.join("cache"));
    again.refresh_search_indexes();
    assert!(!again.search.is_building());
    assert_eq!(
        counts(&search(&mut again, "condemn")),
        [("kjv-a", 1), ("kjv-b", 1)]
    );
    drop((s, again));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn changing_the_index_version_rebuilds_while_the_old_indexes_keep_serving() {
    let (mut s, root) = fixture("version");
    s.refresh_search_indexes();
    step(&mut s);
    step(&mut s);
    let old = cache_files(&root);
    assert!(
        old.iter()
            .all(|n| n.ends_with(&format!(".v{INDEX_VERSION}.obsidx")))
    );

    // A new index version: every cache is stale and is rebuilt in the background, one at a
    // time. The kjv-a build is submitted but held (Inline jobs run only when pumped).
    let next = INDEX_VERSION + 1;
    s.set_search_index_version(next);
    s.refresh_search_indexes();
    assert!(s.search.is_building());
    for id in ["kjv-a", "kjv-b"] {
        assert!(matches!(
            s.search.status(id),
            Some(IndexStatus::Indexing { .. })
        ));
        assert!(
            s.search.stale_path(id).is_some(),
            "{id} keeps its old cache"
        );
    }
    // During the rebuild, searches still find everything, through the old caches.
    let r = search(&mut s, "world");
    assert_eq!(counts(&r), [("kjv-a", 2), ("kjv-b", 2)]);
    assert_eq!(r.stale, ["kjv-a", "kjv-b"]);
    assert!(r.indexing.is_empty() && r.failed.is_empty());
    let out = s.run("search.indexes", &json!({})).unwrap();
    assert_eq!(out["indexes"][0]["status"], "indexing");
    assert!(out["indexes"][0]["stale"].is_string(), "{out}");

    // kjv-a's rebuild is Ready and swapped in; kjv-b still serves from its old cache.
    step(&mut s);
    assert_eq!(s.search.status("kjv-a"), Some(&IndexStatus::Ready));
    assert_eq!(s.search.stale_path("kjv-a"), None);
    let r = search(&mut s, "world");
    assert_eq!(counts(&r), [("kjv-a", 2), ("kjv-b", 2)]);
    assert_eq!(r.stale, ["kjv-b"]);
    assert!(r.indexing.is_empty() && r.failed.is_empty());

    step(&mut s);
    assert!(!s.search.is_building());
    let r = search(&mut s, "world");
    assert_eq!(counts(&r), [("kjv-a", 2), ("kjv-b", 2)]);
    assert!(r.stale.is_empty() && r.indexing.is_empty());
    let new = cache_files(&root);
    assert_eq!(new.len(), 2, "the stale caches are removed: {new:?}");
    assert!(new.iter().all(|n| n.ends_with(&format!(".v{next}.obsidx"))));
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn after_a_version_change_a_fresh_launch_serves_the_old_caches_while_rebuilding() {
    let (mut s, root) = fixture("relaunch");
    s.refresh_search_indexes();
    step(&mut s);
    step(&mut s);
    drop(s);

    // The next release bumps the version: its first launch finds only old-version caches.
    let mut s = Session::new(JobMode::Inline);
    s.library.scan(&root.join("library"));
    s.set_search_index_version(INDEX_VERSION + 1);
    s.set_search_cache_dir(root.join("cache"));
    s.refresh_search_indexes();
    assert!(s.search.is_building());
    let r = search(&mut s, "world");
    assert_eq!(counts(&r), [("kjv-a", 2), ("kjv-b", 2)]);
    assert_eq!(r.stale, ["kjv-a", "kjv-b"]);
    step(&mut s);
    step(&mut s);
    let r = search(&mut s, "world");
    assert_eq!(counts(&r), [("kjv-a", 2), ("kjv-b", 2)]);
    assert!(r.stale.is_empty());
    assert_eq!(cache_files(&root).len(), 2);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_corrupt_cache_with_the_right_name_is_deleted_and_rebuilt() {
    let (mut s, root) = fixture("corrupt");
    s.refresh_search_indexes();
    step(&mut s);
    step(&mut s);
    let a = s.search.path("kjv-a").unwrap().to_path_buf();
    let b = s.search.path("kjv-b").unwrap().to_path_buf();
    drop(s);

    // At launch: a truncated file, and another Resource's cache under kjv-b's name.
    std::fs::write(&a, b"not a database").unwrap();
    let mut other = obs_store::sample_resource();
    other.manifest.resource_id = "kjv-c".into();
    let c = obs_store::build_search_index(
        &other,
        INDEX_VERSION,
        &root.join("elsewhere"),
        &mut |_, _| true,
    )
    .unwrap();
    std::fs::copy(&c, &b).unwrap();
    let mut s = Session::new(JobMode::Inline);
    s.library.scan(&root.join("library"));
    s.set_search_cache_dir(root.join("cache"));
    s.refresh_search_indexes();
    assert!(
        !a.exists() && !b.exists(),
        "deleted: {:?}",
        cache_files(&root)
    );
    assert!(s.search.is_building());
    step(&mut s);
    step(&mut s);
    assert_eq!(
        counts(&search(&mut s, "world")),
        [("kjv-a", 2), ("kjv-b", 2)]
    );

    // While running: a Ready cache that goes bad is deleted and rebuilt, not trusted again.
    std::fs::write(&a, b"garbage").unwrap();
    let r = search(&mut s, "world");
    assert_eq!(counts(&r), [("kjv-b", 2)]);
    assert_eq!(r.indexing, ["kjv-a"]);
    assert!(s.search.is_building(), "queued");
    assert!(!a.exists());
    step(&mut s);
    assert_eq!(
        counts(&search(&mut s, "world")),
        [("kjv-a", 2), ("kjv-b", 2)]
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_failed_build_is_retried_once_per_session_by_a_search() {
    let (mut s, root) = fixture("retry");
    let obr = root.join("library/kjv-a.obr");
    let good = std::fs::read(&obr).unwrap();
    std::fs::write(&obr, b"broken").unwrap();
    s.refresh_search_indexes();
    step(&mut s);
    assert!(matches!(
        s.search.status("kjv-a"),
        Some(IndexStatus::Failed(_))
    ));
    step(&mut s);

    // Repaired (say a transient I/O error): the next search retries it.
    std::fs::write(&obr, &good).unwrap();
    let r = search(&mut s, "world");
    assert_eq!(r.indexing, ["kjv-a"]);
    assert!(s.search.is_building());
    step(&mut s);
    assert_eq!(
        counts(&search(&mut s, "world")),
        [("kjv-a", 2), ("kjv-b", 2)]
    );

    // A build that keeps failing is retried only once per session.
    std::fs::write(&obr, b"broken").unwrap();
    std::fs::remove_file(s.search.path("kjv-a").unwrap()).unwrap();
    let r = search(&mut s, "world");
    assert_eq!(r.indexing, ["kjv-a"]);
    step(&mut s);
    let r = search(&mut s, "world");
    assert_eq!(r.failed.len(), 1, "{r:?}");
    assert!(!s.search.is_building(), "no second retry");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn removing_and_re_adding_a_resource_while_it_builds_rebuilds_it() {
    let (mut s, root) = fixture("readd");
    s.refresh_search_indexes();
    // kjv-a's build is submitted (held); remove it, then add it back.
    let entry = s.library.remove("kjv-a").unwrap();
    s.forget_search_index("kjv-a");
    s.library.add(entry);
    s.refresh_search_index("kjv-a");
    assert!(matches!(
        s.search.status("kjv-a"),
        Some(IndexStatus::Indexing { .. })
    ));
    step(&mut s); // the cancelled build
    step(&mut s); // kjv-b
    step(&mut s); // kjv-a again
    assert!(!s.search.is_building());
    assert_eq!(s.search.status("kjv-a"), Some(&IndexStatus::Ready));
    // Re-added, kjv-a is now last in the library.
    assert_eq!(
        counts(&search(&mut s, "world")),
        [("kjv-b", 2), ("kjv-a", 2)]
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_a_cache_file_or_the_whole_cache_is_harmless() {
    let (mut s, root) = fixture("deleted");
    s.refresh_search_indexes();
    step(&mut s);
    step(&mut s);
    let a = s.search.path("kjv-a").unwrap().to_path_buf();
    std::fs::remove_file(&a).unwrap();

    // The search still runs: kjv-b answers, kjv-a is rebuilt in the background.
    let r = search(&mut s, "world");
    assert_eq!(counts(&r), [("kjv-b", 2)]);
    assert_eq!(r.indexing, ["kjv-a"]);
    assert!(r.failed.is_empty());
    step(&mut s);
    assert!(a.is_file(), "rebuilt");
    assert_eq!(
        counts(&search(&mut s, "world")),
        [("kjv-a", 2), ("kjv-b", 2)]
    );

    // Even the whole cache directory.
    std::fs::remove_dir_all(root.join("cache")).unwrap();
    let r = search(&mut s, "world");
    assert!(r.resources.is_empty());
    assert_eq!(r.indexing, ["kjv-a", "kjv-b"]);
    step(&mut s);
    step(&mut s);
    assert_eq!(
        counts(&search(&mut s, "world")),
        [("kjv-a", 2), ("kjv-b", 2)]
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn prebuilt_caches_beside_the_library_are_used_without_rebuilding() {
    let (mut s, root) = fixture("prebuilt");
    let reader = obs_store::ObrReader::open(&root.join("library/kjv-a.obr")).unwrap();
    let resource = obs_store::ResourceStore::load(&reader).unwrap();
    drop(reader);
    let shipped = obs_store::build_search_index(
        &resource,
        INDEX_VERSION,
        &root.join("library/search"),
        &mut |_, _| true,
    )
    .unwrap();
    s.refresh_search_indexes();
    assert_eq!(s.search.status("kjv-a"), Some(&IndexStatus::Ready));
    assert_eq!(s.search.path("kjv-a"), Some(shipped.as_path()));
    assert!(matches!(
        s.search.status("kjv-b"),
        Some(IndexStatus::Indexing { .. })
    ));
    assert_eq!(counts(&search(&mut s, "world")), [("kjv-a", 2)]);

    // Remove from library deletes the prebuilt cache with the `.obr`.
    s.run("library.remove", &json!({"id": "kjv-a", "confirm": true}))
        .unwrap();
    assert!(!root.join("library/kjv-a.obr").exists());
    assert!(!shipped.exists(), "the prebuilt cache goes too");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn import_builds_the_cache_with_real_progress() {
    let (mut s, root) = fixture("import");
    s.refresh_search_indexes();
    step(&mut s);
    step(&mut s);
    std::fs::create_dir_all(root.join("source")).unwrap();
    std::fs::write(
        root.join("source/GEN.usfm"),
        "\\id GEN\n\\c 1\n\\p\n\\v 1 In the beginning God created the heaven and the earth.\n\\v 2 And the earth was without form, and void.\n",
    )
    .unwrap();
    let id = s
        .run("import.start", &json!({"path": root.join("source")}))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    step(&mut s);
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    assert_eq!(record.index, ImportIndexStatus::Built);
    // The Index phase's last progress: both Verses indexed.
    assert_eq!((record.done, record.total), (2, 2));
    assert_eq!(s.search.status(&id), Some(&IndexStatus::Ready));
    assert!(!s.search.is_building(), "nothing left to rebuild");
    let r = search(&mut s, "earth");
    assert_eq!(counts(&r), [(id.as_str(), 2)]);
    assert_eq!(
        counts(&search(&mut s, "\"without form\"")),
        [(id.as_str(), 1)]
    );

    // Remove from library deletes its cache.
    let path = s.search.path(&id).unwrap().to_path_buf();
    s.run("library.remove", &json!({"id": id, "confirm": true}))
        .unwrap();
    assert!(!path.exists());
    assert_eq!(s.search.status(&id), None);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_cancelled_import_index_build_leaves_no_cache() {
    let root = std::env::temp_dir().join(format!("obs-search-cancel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let indexer = Arc::new(CacheIndexer {
        dir: root.join("cache"),
        version: INDEX_VERSION,
    });
    let queue = JobQueue::threaded(1);
    let (entered, wait_entered) = mpsc::channel();
    let (release, wait_release) = mpsc::channel::<()>();
    let job = queue.submit_fn("index", move |ctx| {
        entered.send(()).unwrap();
        wait_release.recv_timeout(Duration::from_secs(10)).unwrap();
        Ok(indexer.build(&obs_store::sample_resource(), Path::new("x.obr"), ctx))
    });
    wait_entered.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(queue.cancel(job));
    release.send(()).unwrap();
    let out = loop {
        let e = queue.wait_event(Duration::from_secs(10)).unwrap();
        if let JobEventKind::Finished(out) = e.kind {
            break out;
        }
    };
    let result = out.downcast::<Result<ImportIndexStatus, String>>().unwrap();
    assert_eq!(*result, Err("cancelled".into()));
    let left = std::fs::read_dir(root.join("cache")).map_or(0, Iterator::count);
    assert_eq!(left, 0);
    drop(queue);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn without_a_cache_directory_indexes_are_unavailable_not_errors() {
    let (_, root) = fixture("nodir");
    let mut s = Session::new(JobMode::Inline);
    s.library.scan(&root.join("library"));
    s.refresh_search_indexes();
    assert_eq!(s.search.status("kjv-a"), Some(&IndexStatus::Unavailable));
    assert!(!s.search.is_building());
    let r = search(&mut s, "world");
    assert!(r.resources.is_empty());
    assert_eq!(r.failed.len(), 2);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn bad_queries_are_errors_and_status_is_a_command() {
    let (mut s, root) = fixture("command");
    s.refresh_search_indexes();
    step(&mut s);
    let err = s
        .search(&SearchRequest {
            query: "strong:X1".into(),
            ..Default::default()
        })
        .unwrap_err();
    assert!(err.to_string().contains("Strong's"), "{err}");
    let out = s.run("search.indexes", &json!({})).unwrap();
    assert_eq!(out["version"], INDEX_VERSION);
    let list = out["indexes"].as_array().unwrap();
    assert_eq!(list[0]["resource_id"], "kjv-a");
    assert_eq!(list[0]["status"], "ready");
    assert_eq!(list[1]["status"], "indexing");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}
