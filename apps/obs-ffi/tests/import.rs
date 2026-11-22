//! Import across the FFI (`docs/dev/mobile-library.md` § Import) as Swift drives it: ask
//! where to copy a picked file, copy it there, hand it in, poll until its report is done.
//! Jobs run on threads, as in the app.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use obs_engine::testing::{sword_module_files, web_usfm_files, zip_files};
use obs_ffi::{Changed, ImportReport, ImportState, MobileApp};

fn root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("obs-ffi-import-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// What the shell does: copy the picked bytes where the core says, hand them in, and poll
/// until the report stops running.
fn import(app: &MobileApp, name: &str, bytes: &[u8]) -> ImportReport {
    let dest = app.import_destination(name.into()).unwrap();
    std::fs::write(&dest, bytes).unwrap();
    let import = app.start_import(dest).unwrap();
    let started = Instant::now();
    let mut heard = false;
    loop {
        heard |= app.poll_changed().unwrap().contains(&Changed::Imports);
        let report = app.import_report(import).unwrap();
        if report.state != ImportState::Running {
            assert!(heard, "the pump reported the Import");
            return report;
        }
        assert!(started.elapsed() < Duration::from_secs(30), "timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_usfm_zip_and_a_sword_zip_import_through_the_ffi_with_their_reports() {
    let root = root("zips");
    let app = MobileApp::boot(root.join("app").to_string_lossy().into(), None, None).unwrap();
    let usfm = import(&app, "web_usfm.zip", &zip_files(&web_usfm_files("")));
    assert_eq!(usfm.state, ImportState::Complete, "{usfm:#?}");
    assert_eq!(usfm.heading, "Import complete");
    assert_eq!(usfm.sections[0].title, "Summary");
    let sword = import(
        &app,
        "KJV.zip",
        &zip_files(&sword_module_files(
            "obskjvosis.conf",
            "texts/ztext/obskjvosis",
            "",
        )),
    );
    assert_eq!(sword.state, ImportState::Complete, "{sword:#?}");
    let ids: Vec<String> = (app.library().unwrap().into_iter())
        .map(|r| r.resource_id)
        .collect();
    assert!(ids.contains(usfm.resource_id.as_ref().unwrap()), "{ids:?}");
    assert!(ids.contains(sword.resource_id.as_ref().unwrap()), "{ids:?}");
    let refused = import(&app, "notes.obr", b"not a Resource");
    assert_eq!(refused.state, ImportState::Failed);
    assert!(refused.failure.is_some() && refused.details.is_some());
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
