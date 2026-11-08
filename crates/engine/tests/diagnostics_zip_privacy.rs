//! Typed text never reaches a Diagnostics zip (#249): with the app's logger at its most
//! detailed Log level, a Note and a search holding `PRIVATE_NOTE`, the windowing library's
//! warning quoting it as text being typed (an IME event) and a panic whose message holds it,
//! no file in the zip holds it, though the Note is in the User database. Its own test binary,
//! so the process logger and panic hook are this test's alone.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Read;

use obs_engine::diagnostics::{LOG_FILE, ZIP_README, install_panic_hook, start_logging};
use obs_engine::{JobMode, LogLevel, Session};
use serde_json::json;

const TYPED: &str = "PRIVATE_NOTE";

#[test]
fn typed_text_never_reaches_any_file_in_the_diagnostics_zip() {
    let root = std::env::temp_dir().join(format!("obs-zip-privacy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let logs = root.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    start_logging(&logs, LogLevel::Debug).unwrap();
    install_panic_hook(logs.clone());

    let mut s = Session::new(JobMode::Inline);
    s.set_log_dir(logs.clone());
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    // The most detailed Log level, saved as the user would choose it: every command logged.
    s.run("app.log_level", &json!({ "value": "debug" }))
        .unwrap();
    // A Note and a search, typed by the user.
    s.run(
        "annotation.note",
        &json!({ "passage": "Jn 3:16", "body": format!("my {TYPED} text") }),
    )
    .unwrap();
    let _ = s.run("search.run", &json!({ "text": TYPED }));
    let _ = s.run(
        "annotation.note",
        &json!({ "passage": TYPED, "body": TYPED }),
    );
    // Text being typed in an input method, as egui-winit 0.37 logs it when it ignores the
    // event's range (src/lib.rs:809, at Warn).
    log::warn!(
        target: "egui_winit",
        "ignoring Preedit(\"{TYPED}\", Some((0, 99)))'s range because it is invalid"
    );
    // A panic whose message holds it, as an `expect` on a Note's text might.
    let panicked = std::thread::Builder::new()
        .name("worker".into())
        .spawn(|| panic!("invalid note: {TYPED}"))
        .unwrap()
        .join();
    assert!(panicked.is_err());
    let reports = s.crash_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    let report = reports[0].path.file_name().unwrap().to_str().unwrap();

    let dest = root.join("openbible-report.zip");
    let out = s
        .run(
            "help.save_diagnostics_zip",
            &json!({ "path": dest, "report": report }),
        )
        .unwrap();
    assert_eq!(out["path"], dest.display().to_string());

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    let mut names = Vec::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut text = String::new();
        f.read_to_string(&mut text).unwrap();
        assert!(!text.contains(TYPED), "{}:\n{text}", f.name());
        if f.name() == LOG_FILE {
            // The log saw all of it: the commands by id, the warning by where it was logged,
            // the panic by its location.
            for line in [
                "command annotation.note",
                "command search.run",
                "egui_winit: (message withheld; logged at ",
                "panicked at ",
            ] {
                assert!(text.contains(line), "{line}:\n{text}");
            }
        }
        names.push(f.name().to_string());
    }
    assert_eq!(names, [ZIP_README, report, LOG_FILE]);
    // The Note itself was saved, in the User database the zip does not hold.
    let notes = format!("{:?}", s.annotations(None).unwrap());
    assert!(notes.contains(TYPED), "{notes}");
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}
