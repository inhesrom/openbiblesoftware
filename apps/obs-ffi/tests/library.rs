//! The Library across the FFI (`docs/dev/mobile-library.md`) as Swift drives it: a
//! `Transport` the shell implements (here, a Rust one serving a fixture release at a made-up
//! `https://` mirror), the released manifest, the Study set planned, transferred by "the
//! shell" and installed. Jobs run on threads, as in the app.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use obs_engine::starter::MobileSet as EngineSet;
use obs_engine::testing::{mobile_starter_library, write_mobile_starter_bundle};
use obs_ffi::{
    DownloadOrigin, DownloadPlan, DownloadState, MobileApp, MobileSet, Network, StarterChoice,
    StarterRowStatus, Transport, TransportError,
};

const MIRROR: &str = "https://mirror.test/starter/";

fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("obs-ffi-lib-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// The shell's transport: answers [`MIRROR`] URLs from a folder, 404 for anything else, and
/// remembers what it was asked.
struct Mirror {
    dir: PathBuf,
    asked: Mutex<Vec<String>>,
}

impl Transport for Mirror {
    fn get(&self, url: String) -> Result<Vec<u8>, TransportError> {
        self.asked.lock().unwrap().push(url.clone());
        let name = url.strip_prefix(MIRROR).ok_or(TransportError::NotFound)?;
        std::fs::read(self.dir.join(name)).map_err(|_| TransportError::NotFound)
    }
}

fn wait_until(app: &MobileApp, done: impl Fn(&MobileApp) -> bool) {
    let started = Instant::now();
    while !done(app) {
        assert!(started.elapsed() < Duration::from_secs(30), "timed out");
        let _ = app.poll_changed().unwrap();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_study_set_is_fetched_through_the_shells_transport_and_installed() {
    let root = root("study-set");
    let library = mobile_starter_library();
    let bundled: Vec<_> = (library.iter().filter(|(_, s)| *s == EngineSet::Bundled))
        .cloned()
        .collect();
    write_mobile_starter_bundle(&root.join("bundle"), &bundled);
    write_mobile_starter_bundle(&root.join("release"), &library);
    let app = MobileApp::boot(s(&root.join("app")), Some(s(&root.join("bundle"))), None).unwrap();
    assert!(app.problems().unwrap().is_empty(), "{:?}", app.problems());
    let mirror = Arc::new(Mirror {
        dir: root.join("release"),
        asked: Mutex::new(Vec::new()),
    });
    app.set_transport(mirror.clone()).unwrap();
    app.set_starter_source(MIRROR.into(), None).unwrap();
    assert_eq!(app.starter_library().unwrap().rows.len(), 2);
    assert!(app.check_starter().unwrap());
    wait_until(&app, |a| a.starter_library().unwrap().released);
    assert_eq!(
        *mirror.asked.lock().unwrap(),
        [format!("{MIRROR}starter-manifest.json")]
    );
    let view = app.starter_library().unwrap();
    assert_eq!(view.rows.len(), 7);
    assert_eq!(view.study_set.resources, 4);
    let wifi = Network {
        is_wifi: Some(true),
        allow_cellular: false,
    };
    let plan = app
        .plan_starter_download(
            StarterChoice::Set {
                set: MobileSet::Study,
            },
            wifi,
            false,
            None,
        )
        .unwrap();
    let DownloadPlan::Start { requests } = plan else {
        panic!("{plan:?}")
    };
    assert_eq!(requests.len(), 4);
    for r in &requests {
        // The shell's background transfer.
        let name = r.url.strip_prefix(MIRROR).unwrap();
        std::fs::copy(root.join("release").join(name), &r.file).unwrap();
        app.download_progress(
            r.resource_id.clone(),
            r.origin,
            r.bytes.unwrap(),
            r.bytes.unwrap(),
        )
        .unwrap();
        let state = app
            .install_downloaded(r.resource_id.clone(), r.file.clone(), r.origin)
            .unwrap();
        assert!(!matches!(state, DownloadState::Failed { .. }), "{state:?}");
    }
    wait_until(&app, |a| {
        (a.downloads().unwrap().iter()).all(|d| d.state == DownloadState::Done)
    });
    assert_eq!(app.library().unwrap().len(), 6);
    let view = app.starter_library().unwrap();
    assert!(
        (view.rows.iter())
            .filter(|r| r.set == Some(MobileSet::Study))
            .all(|r| r.status == StarterRowStatus::InLibrary)
    );
    assert_eq!(app.download_banner().unwrap(), None);
    assert_eq!(app.storage().unwrap().rows.len(), 6);
    drop(app);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_transport_error_is_the_starter_librarys_error_in_words() {
    let root = root("transport-error");
    write_mobile_starter_bundle(&root.join("bundle"), &[]);
    let app = MobileApp::boot(s(&root.join("app")), Some(s(&root.join("bundle"))), None).unwrap();
    app.set_transport(Arc::new(Mirror {
        dir: root.join("nothing-here"),
        asked: Mutex::new(Vec::new()),
    }))
    .unwrap();
    app.set_starter_source(MIRROR.into(), None).unwrap();
    app.check_starter().unwrap();
    wait_until(&app, |a| !a.starter_library().unwrap().checking);
    let error = app.starter_library().unwrap().error.unwrap();
    assert!(
        error.contains("starter-manifest.json: not found"),
        "{error}"
    );
    drop(app);
    std::fs::remove_dir_all(&root).unwrap();
}

/// S24: "Use cellular data" and the first-launch Study set offer across the FFI, with the
/// words the shell shows for download states and the licence filter.
#[test]
fn the_cellular_setting_the_study_set_offer_and_the_words_cross_the_ffi() {
    let root = root("offer");
    let library = mobile_starter_library();
    let bundled: Vec<_> = (library.iter().filter(|(_, s)| *s == EngineSet::Bundled))
        .cloned()
        .collect();
    write_mobile_starter_bundle(&root.join("bundle"), &bundled);
    write_mobile_starter_bundle(&root.join("release"), &library);
    let boot =
        || MobileApp::boot(s(&root.join("app")), Some(s(&root.join("bundle"))), None).unwrap();
    let app = boot();
    let wifi = Network {
        is_wifi: Some(true),
        allow_cellular: false,
    };
    assert!(!app.use_cellular().unwrap());
    app.set_use_cellular(true).unwrap();
    assert!(app.use_cellular().unwrap());
    let offer = app
        .study_set_offer(wifi)
        .unwrap()
        .expect("offered at the first launch");
    assert_eq!(offer.size, None);
    assert!(offer.looking_up);
    let unknown = Network {
        is_wifi: None,
        allow_cellular: false,
    };
    assert!(
        (app.study_set_offer(unknown).unwrap().unwrap().network)
            .starts_with("Checking the network.")
    );
    app.study_set_offer_shown().unwrap();
    app.set_transport(Arc::new(Mirror {
        dir: root.join("release"),
        asked: Mutex::new(Vec::new()),
    }))
    .unwrap();
    app.set_starter_source(MIRROR.into(), None).unwrap();
    assert!(app.check_starter().unwrap());
    wait_until(&app, |a| a.starter_library().unwrap().released);
    let offer = app.study_set_offer(wifi).unwrap().unwrap();
    assert_eq!(
        offer.size.as_deref(),
        Some(app.starter_library().unwrap().study_set.label.as_str())
    );
    assert!(!offer.looking_up);
    // A transfer the core does not know (still running after a relaunch) is adopted.
    app.download_progress("tbesg".into(), DownloadOrigin::Starter, 1, 2)
        .unwrap();
    let rows = app.downloads().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].state,
        DownloadState::Downloading { done: 1, total: 2 }
    );
    drop(app);
    let app = boot();
    assert!(app.use_cellular().unwrap(), "kept");
    assert_eq!(app.study_set_offer(wifi).unwrap(), None, "shown once");
    assert_eq!(
        obs_ffi::download_label(DownloadState::Downloading {
            done: 1_000_000,
            total: 2_000_000
        }),
        "Downloading · 1.0 MB of 2.0 MB"
    );
    let filters = obs_ffi::licence_filters();
    let labels: Vec<&str> = filters.choices.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Free to share", "Free for personal use", "All licences"]
    );
    assert_eq!(filters.default, obs_ffi::LicenceFilter::All);
    assert_eq!(
        obs_ffi::licence_class_label(obs_ffi::LicenceClass::PersonalUse),
        "Free for personal use"
    );
    std::fs::remove_dir_all(&root).unwrap();
}
