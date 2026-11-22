//! Background work (S25, #333 "Storage"): what a background processing task carries on (a
//! downloaded Resource's install and Search index, an Import, the alias caches), stepped
//! under a deadline; Library → Storage saying "Still being indexed" until the index is
//! built; work cut short by an expiration or a relaunch picked up again. Fixtures from
//! `obs_engine::testing` at `file://` URLs, jobs inline (CI-run on every OS).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use obs_engine::JobMode;
use obs_engine::starter::MobileSet;
use obs_engine::sword_repo::file_url;
use obs_engine::testing::{mobile_starter_library, write_mobile_starter_bundle};

use crate::*;

const WIFI: Network = Network {
    is_wifi: Some(true),
    allow_cellular: false,
};
const PLENTY: Option<u64> = Some(1 << 40);

fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("obs-mobile-bg-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// The fixture Starter library: its Bundled set in `root/bundle`, all of it as the release in
/// `root/release`.
fn lay_out(root: &Path) {
    let library = mobile_starter_library();
    let bundled: Vec<_> = (library.iter().filter(|(_, s)| *s == MobileSet::Bundled))
        .cloned()
        .collect();
    write_mobile_starter_bundle(&root.join("bundle"), &bundled);
    write_mobile_starter_bundle(&root.join("release"), &library);
}

fn boot(root: &Path) -> MobileApp {
    let mut app = MobileApp::boot_with(
        Paths::under(&root.join("app")),
        Some(&root.join("bundle")),
        JobMode::Inline,
    )
    .unwrap();
    assert!(app.problems().is_empty(), "{:?}", app.problems());
    app.set_starter_source(&file_url(&root.join("release")), None);
    app
}

/// A step with all the time it needs.
fn finish(app: &mut MobileApp) {
    let pending = app.step_background(Instant::now() + Duration::from_secs(60));
    assert!(!pending, "{:?}", app.background_work());
}

/// Booted over the fixture with the released manifest in and nothing left to do.
fn app(name: &str) -> (PathBuf, MobileApp) {
    let root = root(name);
    lay_out(&root);
    let mut app = boot(&root);
    assert!(app.check_starter());
    for _ in 0..20 {
        let _ = app.pump();
    }
    assert!(app.starter_library().released);
    finish(&mut app);
    let _ = app.pump();
    (root, app)
}

fn url_path(url: &str) -> PathBuf {
    let p = url.strip_prefix("file://").unwrap();
    if p.len() > 2 && p.as_bytes()[2] == b':' {
        PathBuf::from(&p[1..])
    } else {
        PathBuf::from(p)
    }
}

/// Download Starter Resources `ids` as the shell's transfers do, and hand the files in.
fn download(app: &mut MobileApp, ids: &[&str]) {
    for id in ids {
        let choice = StarterChoice::One((*id).to_string());
        let DownloadPlan::Start(requests) = app
            .plan_starter_download(&choice, WIFI, false, PLENTY)
            .unwrap()
        else {
            panic!("{id} not planned");
        };
        for r in requests {
            std::fs::write(&r.file, std::fs::read(url_path(&r.url)).unwrap()).unwrap();
            app.install_downloaded(&r.resource_id, &r.file, r.origin)
                .unwrap();
        }
    }
}

fn storage_row(app: &MobileApp, id: &str) -> StorageRow {
    (app.storage().rows.into_iter())
        .find(|r| r.resource_id == id)
        .unwrap()
}

/// S25's Done when: after a download the core asks for a background task, Storage says the
/// Resource is still being indexed until the task's steps have built it, then that it is
/// ready, and the pump reports the change.
#[test]
fn storage_says_still_being_indexed_until_the_background_work_builds_it() {
    let (root, mut app) = app("index");
    assert_eq!(
        app.pending_background_work(),
        None,
        "nothing left at launch"
    );
    let bsb = app.search_index_status("bsb").unwrap();
    assert_eq!(bsb.state, SearchIndexState::Ready);
    assert_eq!(bsb.label, "Search index ready");
    assert!(bsb.searchable && bsb.note.is_none());
    assert_eq!(app.search_index_status("tbesg"), None, "not in the library");

    download(&mut app, &["tbesg"]);
    let request = app.pending_background_work().unwrap();
    assert_eq!(request, INDEX_REQUEST);
    assert!(!request.requires_external_power && !request.requires_network);
    assert_eq!(app.background_work().installs, 1);

    // The task's first steps run out of time at once: one job step each (the install).
    let mut building = None;
    for _ in 0..10 {
        assert!(app.step_background(Instant::now()));
        if let Some(index) = app.search_index_status("tbesg") {
            building = Some(index);
            break;
        }
    }
    let index = building.expect("installed, its index queued");
    assert_eq!(index.state, SearchIndexState::Building);
    assert_eq!(index.label, "Still being indexed");
    assert!(!index.searchable);
    assert_eq!(
        index.note.as_deref(),
        Some("TBESG is still being indexed. Search will find it once it is ready.")
    );
    assert!(app.background_work().search_indexes);
    assert_eq!(storage_row(&app, "tbesg").index, Some(index));

    finish(&mut app);
    assert_eq!(app.pending_background_work(), None);
    let row = storage_row(&app, "tbesg");
    assert_eq!(row.index.as_ref().unwrap().state, SearchIndexState::Ready);
    assert!(row.search_index > 0, "{row:?}");
    assert!(app.pump().contains(&Changed::Indexes));
    std::fs::remove_dir_all(&root).unwrap();
}

/// A step whose deadline has passed runs at most one job step, so the shell can stop at the
/// task's expiration; work is left for the next.
#[test]
fn background_a_step_stops_at_its_deadline_after_one_job_step() {
    let (root, mut app) = app("deadline");
    download(&mut app, &["tbesg", "tbesh", "bdb"]);
    let began = Instant::now();
    let mut steps = 0;
    while app.step_background(began) {
        steps += 1;
        assert!(steps < 100, "{:?}", app.background_work());
    }
    // Three installs (each a job) and three index builds (one at a time), a step each at
    // least.
    assert!(steps >= 5, "{steps} steps");
    for id in ["tbesg", "tbesh", "bdb"] {
        assert_eq!(
            app.search_index_status(id).unwrap().state,
            SearchIndexState::Ready
        );
    }
    std::fs::remove_dir_all(&root).unwrap();
}

/// The task expires before the index is built and the app is closed: the next launch has
/// the work again and asks for a task; once it is built, a launch finds the index and has
/// no index to build.
#[test]
fn background_work_cut_short_is_pending_again_after_a_relaunch() {
    let (root, mut app) = app("relaunch");
    download(&mut app, &["tbesg"]);
    while app.search_index_status("tbesg").is_none() {
        assert!(app.step_background(Instant::now()));
    }
    assert!(
        app.pending_background_work().is_some(),
        "expired, work left"
    );
    app.on_background().unwrap();
    drop(app);

    let mut app = boot(&root);
    assert_eq!(app.pending_background_work(), Some(INDEX_REQUEST));
    assert!(
        app.background_work().search_indexes,
        "queued again at launch"
    );
    assert_eq!(
        storage_row(&app, "tbesg").index.unwrap().label,
        "Still being indexed"
    );
    finish(&mut app);
    drop(app);

    let app = boot(&root);
    // Only the launch's own alias refresh, which every launch makes.
    assert_eq!(
        app.background_work(),
        BackgroundWork {
            alias_caches: true,
            ..BackgroundWork::default()
        }
    );
    assert_eq!(
        app.search_index_status("tbesg").unwrap().state,
        SearchIndexState::Ready
    );
    std::fs::remove_dir_all(&root).unwrap();
}

/// A People & places index and a Dictionary added mid-run: their alias cache is built in the
/// background (the boot's refresh knew neither) and counted in Storage.
#[test]
fn background_alias_caches_are_built_for_a_pair_added_mid_run() {
    let root = root("aliases");
    let mut app =
        MobileApp::boot_with(Paths::under(&root.join("app")), None, JobMode::Inline).unwrap();
    finish(&mut app);
    for (name, resource) in [
        ("tipnr.obr", obs_engine::testing::tipnr()),
        ("easton.obr", obs_engine::testing::dictionary("easton")),
    ] {
        let file = root.join(name);
        obs_store::write_obr(&resource, &file).unwrap();
        let dest = app.import_destination(name).unwrap();
        std::fs::copy(&file, &dest).unwrap();
        app.start_import(&dest).unwrap();
    }
    let _ = app.pump();
    assert!(app.pending_background_work().is_some());
    let mut saw_aliases = false;
    while app.step_background(Instant::now()) {
        saw_aliases |= app.background_work().alias_caches;
    }
    assert!(
        saw_aliases,
        "the pair's alias refresh ran as background work"
    );
    let caches: Vec<_> = std::fs::read_dir(&app.paths().alias_cache)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(caches.len(), 1, "{caches:?}");
    assert!(storage_row(&app, "tipnr").aliases > 0);
    assert!(storage_row(&app, "easton").aliases > 0);
    std::fs::remove_dir_all(&root).unwrap();
}
