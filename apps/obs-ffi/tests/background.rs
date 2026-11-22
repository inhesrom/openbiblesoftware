//! The background processing task's steps across the FFI, with jobs on threads as in the app:
//! a Resource added, the core asks for a task, each step ends by its deadline (plus one wait
//! of the pump's), other calls are answered while a step runs, and the steps end once the
//! Search index is built, which Storage then says.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use obs_ffi::{BackgroundRequest, MobileApp, SearchIndexState};

fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("obs-ffi-bg-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// One step's longest: its deadline, then at most one round of the core's work (a pump,
/// with the lock) and scheduling slack on a loaded machine.
const STEP_SLACK: Duration = Duration::from_millis(400);

#[test]
fn background_steps_end_by_their_deadline_and_build_the_index() {
    let root = root("steps");
    let app = MobileApp::boot(root.join("app").to_string_lossy().into(), None, None).unwrap();
    // Let the launch's own work (the alias refresh) finish.
    while app.step_background().unwrap() {}
    assert_eq!(app.pending_background_work().unwrap(), None);

    let file = root.join("kjv.obr");
    obs_store::write_obr(&obs_engine::testing::bibles::kjv(), &file).unwrap();
    let dest = app.import_destination("kjv.obr".into()).unwrap();
    std::fs::copy(&file, &dest).unwrap();
    app.start_import(dest).unwrap();
    assert_eq!(
        app.pending_background_work().unwrap(),
        Some(BackgroundRequest {
            requires_external_power: false,
            requires_network: false,
        })
    );

    let app = Arc::new(app);
    let other = {
        let app = Arc::clone(&app);
        std::thread::spawn(move || {
            // Answered while the steps run: the lock is free between their rounds.
            let mut slowest = Duration::ZERO;
            for _ in 0..20 {
                let asked = Instant::now();
                let _ = app.library().unwrap();
                slowest = slowest.max(asked.elapsed());
                std::thread::sleep(Duration::from_millis(5));
            }
            slowest
        })
    };
    let began = Instant::now();
    loop {
        let step = Instant::now();
        let pending = app.step_background().unwrap();
        let took = step.elapsed();
        assert!(
            took <= obs_mobile::BACKGROUND_STEP + STEP_SLACK,
            "a step took {took:?}"
        );
        if !pending {
            break;
        }
        assert!(began.elapsed() < Duration::from_secs(60), "never finished");
    }
    let slowest = other.join().unwrap();
    assert!(
        slowest < Duration::from_millis(300),
        "a call waited {slowest:?}"
    );
    let library = app.library().unwrap();
    let kjv = library.iter().find(|r| r.abbreviation == "KJV").unwrap();
    let status = app
        .search_index_status(kjv.resource_id.clone())
        .unwrap()
        .unwrap();
    assert_eq!(status.state, SearchIndexState::Ready);
    let storage = app.storage().unwrap();
    let row = (storage.rows.iter())
        .find(|r| r.resource_id == kjv.resource_id)
        .unwrap();
    assert_eq!(row.index.as_ref().unwrap().label, "Search index ready");
    std::fs::remove_dir_all(&root).unwrap();
}
