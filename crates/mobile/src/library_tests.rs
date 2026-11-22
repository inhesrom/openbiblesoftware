//! The Library on the device (#333, S23): the Starter library rows over the Bundled set and
//! the release, the Study set downloaded through the download states, sha256 refusals, the
//! cellular and free-space rules, Remove and Add back, the Open catalogue, Storage. Fixtures
//! from `obs_engine::testing` at `file://` URLs (CI-run on every OS); `#[ignore]`d, the real
//! Starter library (`dist/starter` after `cargo xtask starter`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_engine::JobMode;
use obs_engine::starter::MobileSet;
use obs_engine::sword_repo::file_url;
use obs_engine::testing::{
    MOCK_OPEN_NOTICE, mobile_starter_library, mock_open_catalogue, write_mobile_starter_bundle,
};

use crate::library::{download_rule, megabytes};
use crate::*;

const WIFI: Network = Network {
    is_wifi: true,
    allow_cellular: false,
};
const CELLULAR: Network = Network {
    is_wifi: false,
    allow_cellular: false,
};
const PLENTY: Option<u64> = Some(1 << 40);

/// A fresh folder for one test.
fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("obs-mobile-lib-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// The Bundled set of the fixture Starter library in `root/bundle`, the whole of it as the
/// release in `root/release`.
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

/// Booted over the fixture, with the released manifest in.
fn app(name: &str) -> (PathBuf, MobileApp) {
    let root = root(name);
    lay_out(&root);
    let mut app = boot(&root);
    assert!(app.check_starter());
    settle(&mut app);
    let view = app.starter_library();
    assert!(view.error.is_none(), "{:?}", view.error);
    (root, app)
}

/// Pump until no download is moving and no job runs.
fn settle(app: &mut MobileApp) {
    for _ in 0..100 {
        let _ = app.pump();
        let moving = (app.downloads().iter()).any(|d| {
            matches!(
                d.state,
                DownloadState::Installing | DownloadState::Indexing | DownloadState::Verifying
            )
        });
        if !moving && !app.busy() {
            return;
        }
    }
    panic!("background work did not finish");
}

/// The file a `file://` URL names, on every OS (`file:///C:/x` on Windows).
fn url_path(url: &str) -> PathBuf {
    let p = url.strip_prefix("file://").unwrap();
    if p.len() > 2 && p.as_bytes()[2] == b':' {
        PathBuf::from(&p[1..])
    } else {
        PathBuf::from(p)
    }
}

/// What the shell's background transfer does: report progress and write the file the core
/// named, then hand it back.
fn transfer(app: &mut MobileApp, r: &DownloadRequest) -> DownloadState {
    let bytes = std::fs::read(url_path(&r.url)).unwrap();
    let total = bytes.len() as u64;
    app.download_progress(&r.resource_id, r.origin, total / 2, total)
        .unwrap();
    std::fs::write(&r.file, &bytes).unwrap();
    app.download_progress(&r.resource_id, r.origin, total, total)
        .unwrap();
    app.install_downloaded(&r.resource_id, &r.file, r.origin)
        .unwrap()
}

fn requests(plan: DownloadPlan) -> Vec<DownloadRequest> {
    match plan {
        DownloadPlan::Start(r) => r,
        other => panic!("{other:?}"),
    }
}

fn ids(app: &MobileApp) -> Vec<String> {
    let mut ids: Vec<String> = app.library().into_iter().map(|r| r.resource_id).collect();
    ids.sort();
    ids
}

fn row(app: &MobileApp, id: &str) -> StarterRow {
    (app.starter_library().rows.into_iter())
        .find(|r| r.resource_id == id)
        .unwrap()
}

#[test]
fn library_rows_are_the_bundled_sets_then_the_releases_once_it_is_in() {
    let root = root("rows");
    lay_out(&root);
    let mut app = boot(&root);
    let view = app.starter_library();
    assert!(!view.released);
    let rows: Vec<String> = view.rows.iter().map(|r| r.resource_id.clone()).collect();
    assert_eq!(rows, ["bsb", "strongs-hebrew"]);
    assert!(app.check_starter());
    assert!(!app.check_starter(), "already being fetched");
    assert!(app.starter_library().checking);
    let _ = app.pump();
    let changed = app.pump();
    assert!(
        changed.contains(&Changed::Downloads) || !app.starter_library().checking,
        "{changed:?}"
    );
    settle(&mut app);
    let view = app.starter_library();
    assert!(view.released && !view.checking && !view.needs_app_update);
    assert_eq!(view.rows.len(), 7);
    let tbesg = row(&app, "tbesg");
    assert_eq!(tbesg.set, Some(MobileSet::Study));
    assert_eq!(tbesg.status, StarterRowStatus::Missing);
    assert_eq!(tbesg.license, "CC-BY-4.0");
    assert!(
        tbesg
            .features
            .iter()
            .any(|f| f == "Word Study: Lexicon entries")
    );
    assert!(tbesg.size.ends_with(" MB"), "{}", tbesg.size);
    assert_eq!(row(&app, "bsb").status, StarterRowStatus::InLibrary);
    assert_eq!(row(&app, "web").set, Some(MobileSet::More));
    assert_eq!(view.study_set.resources, 4);
    assert!(
        view.study_set.label.starts_with("4 Resources, "),
        "{}",
        view.study_set.label
    );
    std::fs::remove_dir_all(&root).unwrap();
}

/// S23's Done when: a Study set "download" from a `file://` base through the download states
/// installs, and the library lists every Resource of the Bundled and Study sets.
#[test]
fn library_study_set_downloads_through_the_states_and_installs() {
    let (root, mut app) = app("study-set");
    let plan = app
        .plan_starter_download(&StarterChoice::Set(MobileSet::Study), WIFI, false, PLENTY)
        .unwrap();
    let requests = requests(plan);
    let planned: Vec<&str> = requests.iter().map(|r| r.resource_id.as_str()).collect();
    assert_eq!(planned, ["kjv-strongs", "tbesh", "tbesg", "bdb"]);
    for r in &requests {
        assert!(!r.cellular, "Wi-Fi only by default");
        assert!(
            r.file.starts_with(root.join("app/downloads")),
            "{}",
            r.file.display()
        );
        assert!(
            r.url.ends_with(&format!("/{}.obr", r.resource_id)),
            "{}",
            r.url
        );
        assert_eq!(
            row(&app, &r.resource_id).download,
            Some(DownloadState::Queued)
        );
    }
    assert_eq!(
        app.download_banner().as_deref(),
        Some("Downloading 4 Resources")
    );
    // Asking again while they run plans nothing new.
    assert_eq!(
        app.plan_starter_download(&StarterChoice::Set(MobileSet::Study), WIFI, false, PLENTY)
            .unwrap(),
        DownloadPlan::Nothing
    );
    let first = &requests[0];
    let total = std::fs::metadata(url_path(&first.url)).unwrap().len();
    app.download_progress(&first.resource_id, first.origin, 10, total)
        .unwrap();
    assert_eq!(
        row(&app, &first.resource_id).download,
        Some(DownloadState::Downloading { done: 10, total })
    );
    assert!(
        app.download_banner()
            .unwrap()
            .starts_with("Downloading 4 Resources · "),
        "{:?}",
        app.download_banner()
    );
    for r in &requests {
        let state = transfer(&mut app, r);
        assert!(
            matches!(state, DownloadState::Indexing | DownloadState::Done),
            "{state:?}"
        );
        assert!(!r.file.exists(), "the download is deleted once installed");
    }
    settle(&mut app);
    for r in &requests {
        assert_eq!(
            row(&app, &r.resource_id).download,
            Some(DownloadState::Done)
        );
        assert_eq!(
            row(&app, &r.resource_id).status,
            StarterRowStatus::InLibrary
        );
    }
    assert_eq!(
        ids(&app),
        [
            "bdb",
            "bsb",
            "kjv-strongs",
            "strongs-hebrew",
            "tbesg",
            "tbesh"
        ]
    );
    assert_eq!(app.starter_library().study_set.resources, 0);
    assert_eq!(app.starter_library().rows.len(), 7);
    assert_eq!(app.download_banner(), None);
    // A Resource already in the library at the offered revision is not downloaded again.
    assert_eq!(
        app.plan_starter_download(&StarterChoice::One("tbesg".into()), WIFI, false, PLENTY)
            .unwrap(),
        DownloadPlan::Nothing
    );
    std::fs::remove_dir_all(&root).unwrap();
}

/// #333: "Each file is checked against its pinned sha256 before it joins the library; a
/// mismatch deletes it and offers Retry."
#[test]
fn library_a_sha256_mismatch_deletes_the_file_and_offers_retry() {
    let (root, mut app) = app("mismatch");
    let plan = app
        .plan_starter_download(&StarterChoice::One("bdb".into()), WIFI, false, PLENTY)
        .unwrap();
    let r = requests(plan).remove(0);
    std::fs::write(&r.file, b"not the release's file").unwrap();
    let state = app.install_downloaded("bdb", &r.file, r.origin).unwrap();
    let DownloadState::Failed { message, retry } = &state else {
        panic!("{state:?}");
    };
    assert!(*retry);
    assert!(message.contains("bdb.obr failed verification"), "{message}");
    assert!(!r.file.exists(), "the mismatched file is deleted");
    assert!(!ids(&app).contains(&"bdb".to_string()));
    assert_eq!(row(&app, "bdb").download, Some(state));
    assert_eq!(app.download_banner().as_deref(), Some("1 download failed"));
    // Retry: plan it again and download it again.
    let r = requests(
        app.plan_starter_download(&StarterChoice::One("bdb".into()), WIFI, false, PLENTY)
            .unwrap(),
    )
    .remove(0);
    assert_eq!(row(&app, "bdb").download, Some(DownloadState::Queued));
    transfer(&mut app, &r);
    settle(&mut app);
    assert_eq!(row(&app, "bdb").status, StarterRowStatus::InLibrary);
    // A transfer that failed in the shell is Retry too, and its partial file goes.
    let r = requests(
        app.plan_starter_download(&StarterChoice::One("tbesh".into()), WIFI, false, PLENTY)
            .unwrap(),
    )
    .remove(0);
    std::fs::write(&r.file, b"partial").unwrap();
    app.download_failed("tbesh", r.origin, "The network connection was lost.")
        .unwrap();
    assert!(!r.file.exists());
    assert_eq!(
        row(&app, "tbesh").download,
        Some(DownloadState::Failed {
            message: "The network connection was lost.".into(),
            retry: true
        })
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn library_cancel_forgets_the_download_and_deletes_its_partial_file() {
    let (root, mut app) = app("cancel");
    let r = requests(
        app.plan_starter_download(&StarterChoice::One("tbesg".into()), WIFI, false, PLENTY)
            .unwrap(),
    )
    .remove(0);
    std::fs::write(&r.file, b"part").unwrap();
    app.cancel_download("tbesg", r.origin).unwrap();
    assert!(!r.file.exists());
    assert_eq!(row(&app, "tbesg").download, None);
    assert!(app.downloads().is_empty());
    assert!(app.cancel_download("tbesg", r.origin).is_err());
    std::fs::remove_dir_all(&root).unwrap();
}

/// #333: Wi-Fi only by default; "Use cellular data" allows cellular; with it off, a download
/// over cellular shows its size and asks first. A download that won't fit says so first.
#[test]
fn library_cellular_and_free_space_rules() {
    let mb = 1_000_000;
    // Wi-Fi: start, cellular only if allowed.
    assert_eq!(download_rule(Some(mb), "BDB", WIFI, false, None), Ok(false));
    let allowed = Network {
        is_wifi: true,
        allow_cellular: true,
    };
    assert_eq!(
        download_rule(Some(mb), "BDB", allowed, false, None),
        Ok(true)
    );
    // Cellular, allowed: start.
    let cell_allowed = Network {
        is_wifi: false,
        allow_cellular: true,
    };
    assert_eq!(
        download_rule(Some(mb), "BDB", cell_allowed, false, None),
        Ok(true)
    );
    // Cellular, not allowed: ask with the size; yes starts it over cellular.
    let Err(DownloadPlan::AskCellular { bytes, message }) =
        download_rule(Some(137 * mb), "the Study set", CELLULAR, false, None)
    else {
        panic!()
    };
    assert_eq!(bytes, Some(137 * mb));
    assert!(
        message.contains("the Study set (137.0 MB) over cellular data"),
        "{message}"
    );
    assert_eq!(
        download_rule(Some(mb), "BDB", CELLULAR, true, None),
        Ok(true)
    );
    let Err(DownloadPlan::AskCellular { message, .. }) =
        download_rule(None, "WEBX", CELLULAR, false, None)
    else {
        panic!()
    };
    assert!(message.contains("its size is not known"), "{message}");
    // Free space: the download, its staged copy and room for its index.
    let Err(DownloadPlan::WontFit {
        needed,
        free,
        message,
    }) = download_rule(Some(100 * mb), "the Study set", WIFI, false, Some(200 * mb))
    else {
        panic!()
    };
    assert_eq!((needed, free), (250 * mb, 200 * mb));
    assert!(
        message.contains("needs 250.0 MB of free space"),
        "{message}"
    );
    assert_eq!(
        download_rule(Some(100 * mb), "x", WIFI, false, Some(250 * mb)),
        Ok(false)
    );
    // Won't fit is said before the cellular question.
    assert!(matches!(
        download_rule(Some(100 * mb), "x", CELLULAR, false, Some(mb)),
        Err(DownloadPlan::WontFit { .. })
    ));

    // Through the plan: nothing is queued until the rule says go.
    let (root, mut app) = app("cellular");
    let plan = app
        .plan_starter_download(
            &StarterChoice::Set(MobileSet::Study),
            CELLULAR,
            false,
            PLENTY,
        )
        .unwrap();
    let DownloadPlan::AskCellular { bytes, .. } = plan else {
        panic!("{plan:?}")
    };
    assert_eq!(bytes, Some(app.starter_library().study_set.bytes));
    assert!(app.downloads().is_empty());
    let plan = app
        .plan_starter_download(&StarterChoice::Set(MobileSet::Study), WIFI, false, Some(10))
        .unwrap();
    assert!(matches!(plan, DownloadPlan::WontFit { .. }), "{plan:?}");
    assert!(app.downloads().is_empty());
    let requests = requests(
        app.plan_starter_download(
            &StarterChoice::Set(MobileSet::Study),
            CELLULAR,
            true,
            PLENTY,
        )
        .unwrap(),
    );
    assert!(requests.iter().all(|r| r.cellular));
    std::fs::remove_dir_all(&root).unwrap();
}

/// Remove and Add back (#333): a removed Starter Resource stays removed across launches and
/// leaves the Study set; Add back of a Bundled set Resource comes from the bundle at once,
/// of another, through a download.
#[test]
fn library_removed_stays_removed_and_add_back_restores_it() {
    let (root, mut app) = app("remove");
    let r = requests(
        app.plan_starter_download(&StarterChoice::One("tbesg".into()), WIFI, false, PLENTY)
            .unwrap(),
    )
    .remove(0);
    transfer(&mut app, &r);
    settle(&mut app);
    let RemoveAnswer::Confirm(question) = app.remove_resource("tbesg", false).unwrap() else {
        panic!()
    };
    assert!(question.starts_with("Remove "), "{question}");
    assert!(ids(&app).contains(&"tbesg".to_string()));
    assert_eq!(
        app.remove_resource("tbesg", true).unwrap(),
        RemoveAnswer::Removed
    );
    assert!(!ids(&app).contains(&"tbesg".to_string()));
    assert_eq!(row(&app, "tbesg").status, StarterRowStatus::Removed);
    assert_eq!(app.starter_library().study_set.resources, 3);
    assert!(app.pump().contains(&Changed::Library));
    // A Bundled set Resource too, and not the one a reader shows.
    assert_eq!(
        app.remove_resource("strongs-hebrew", true).unwrap(),
        RemoveAnswer::Removed
    );
    let shown = app.reader(ReaderId(0)).unwrap().resource_id;
    let err = app.remove_resource(&shown, true).unwrap_err();
    assert!(err.to_string().contains("is open in the reader"), "{err}");
    drop(app);

    // Relaunch: still removed, not added back from the bundle.
    let mut app = boot(&root);
    assert!(app.check_starter());
    settle(&mut app);
    assert!(!ids(&app).contains(&"strongs-hebrew".to_string()));
    assert_eq!(
        row(&app, "strongs-hebrew").status,
        StarterRowStatus::Removed
    );
    assert_eq!(row(&app, "tbesg").status, StarterRowStatus::Removed);
    // Add back: from the bundle at once.
    let plan = app
        .plan_starter_download(
            &StarterChoice::One("strongs-hebrew".into()),
            CELLULAR,
            false,
            Some(0),
        )
        .unwrap();
    assert_eq!(plan, DownloadPlan::Nothing);
    assert_eq!(
        row(&app, "strongs-hebrew").status,
        StarterRowStatus::InLibrary
    );
    // Add back: downloaded.
    let r = requests(
        app.plan_starter_download(&StarterChoice::One("tbesg".into()), WIFI, false, PLENTY)
            .unwrap(),
    )
    .remove(0);
    transfer(&mut app, &r);
    settle(&mut app);
    assert_eq!(row(&app, "tbesg").status, StarterRowStatus::InLibrary);
    std::fs::remove_dir_all(&root).unwrap();
}

/// Get Resources' Open catalogue (#333): the same licence filter as the desktop, the file
/// downloaded by the shell, checked by its pinned licence notice and imported.
#[test]
fn library_open_catalogue_rows_filter_by_licence_and_a_download_imports() {
    let (root, mut app) = app("open");
    let sources = mock_open_catalogue(&root.join("open"), MOCK_OPEN_NOTICE);
    app.session.get_bibles.set_open_catalogue(sources);
    let rows = app.open_catalogue(LicenceFilter::FreeToShare);
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.id, "web-excerpt");
    assert_eq!(row.publisher, "from eBible.org");
    assert_eq!(row.licence_class, LicenceClass::FreeToShare);
    assert!(row.evidence_url.starts_with("https://web.archive.org/"));
    assert_eq!(row.installed, None);
    let plan = app
        .plan_open_download("web-excerpt", CELLULAR, false, PLENTY)
        .unwrap();
    assert!(
        matches!(plan, DownloadPlan::AskCellular { bytes: None, .. }),
        "{plan:?}"
    );
    let r = requests(
        app.plan_open_download("web-excerpt", WIFI, false, PLENTY)
            .unwrap(),
    )
    .remove(0);
    assert_eq!(r.origin, DownloadOrigin::Open);
    assert_eq!(r.bytes, None);
    let state = transfer(&mut app, &r);
    assert_eq!(state, DownloadState::Installing);
    settle(&mut app);
    assert!(!r.file.exists(), "deleted once read");
    let row = &app.open_catalogue(LicenceFilter::All)[0];
    assert_eq!(row.download, Some(DownloadState::Done), "{row:?}");
    let installed = row.installed.clone().unwrap();
    assert!(ids(&app).contains(&installed), "{installed}");
    assert!(
        app.plan_open_download("web-excerpt", WIFI, false, PLENTY)
            .unwrap_err()
            .to_string()
            .contains("already in your Library")
    );
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn library_storage_lists_each_resource_with_its_files() {
    assert_eq!(megabytes(26_049_999), "26.0 MB");
    assert_eq!(megabytes(26_050_000), "26.1 MB");
    assert_eq!(megabytes(647_168), "0.6 MB");
    assert_eq!(megabytes(0), "0.0 MB");
    let (root, app) = app("storage");
    let storage = app.storage();
    let listed: Vec<String> = storage.rows.iter().map(|r| r.resource_id.clone()).collect();
    assert_eq!(listed.len(), app.library().len());
    for row in &storage.rows {
        assert!(row.obr > 0 && row.search_index > 0, "{row:?}");
        assert_eq!(row.total, row.obr + row.search_index + row.aliases);
        assert!(!row.title.is_empty());
    }
    assert_eq!(
        storage.total,
        storage.rows.iter().map(|r| r.total).sum::<u64>()
    );
    assert!(storage.size.ends_with(" MB"));
    std::fs::remove_dir_all(&root).unwrap();
}

/// The real Starter library (after `cargo xtask starter`): its Bundled set installed, then
/// the Study set downloaded from `dist/starter` as the release through the download states;
/// the library lists 19 Resources and Library → Starter library 22 rows. Run by hand:
/// `cargo test -p obs-mobile --release -- --ignored --nocapture built_study_set`.
#[test]
#[ignore = "needs dist/starter from `cargo xtask starter`"]
fn library_the_built_study_set_downloads_and_installs() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/starter");
    if !dist.join(obs_engine::starter::MANIFEST_FILE).is_file() {
        eprintln!("no dist/starter; skipped");
        return;
    }
    let root = root("built-study-set");
    obs_engine::testing::copy_bundled_set(&dist, &root.join("bundle"));
    let mut app = boot(&root);
    app.set_starter_source(&file_url(&dist), None);
    assert_eq!(app.library().len(), 4);
    assert!(app.check_starter());
    settle(&mut app);
    let view = app.starter_library();
    assert_eq!(view.rows.len(), 22);
    assert_eq!(view.study_set.resources, 15);
    eprintln!("Study set: {}", view.study_set.label);
    let started = std::time::Instant::now();
    let requests = requests(
        app.plan_starter_download(&StarterChoice::Set(MobileSet::Study), WIFI, false, PLENTY)
            .unwrap(),
    );
    assert_eq!(requests.len(), 15);
    for r in &requests {
        let state = transfer(&mut app, r);
        assert!(
            !matches!(state, DownloadState::Failed { .. }),
            "{}: {state:?}",
            r.resource_id
        );
    }
    let installed = started.elapsed();
    settle(&mut app);
    eprintln!(
        "installed in {installed:?}; indexed by {:?}",
        started.elapsed()
    );
    assert_eq!(app.library().len(), 19);
    assert_eq!(app.starter_library().rows.len(), 22);
    assert!(
        app.downloads()
            .iter()
            .all(|d| d.state == DownloadState::Done)
    );
    let storage = app.storage();
    eprintln!(
        "Storage: {} for {} Resources",
        storage.size,
        storage.rows.len()
    );
    std::fs::remove_dir_all(&root).unwrap();
}
