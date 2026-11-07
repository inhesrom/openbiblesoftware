//! A forced panic in a test build writes a Crash report to the Log folder (#219): the panic,
//! where it happened, a backtrace, the app version, the OS and the recent log lines, with the
//! home folder written as `~`. The next launch finds it as a new report to offer
//! (`ui-egui`'s `report_problem_tests` show the offer and the review).
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::process::Command;

#[test]
fn a_forced_panic_writes_a_crash_report_the_next_launch_offers() {
    let root = std::env::temp_dir().join(format!("obs-crash-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let logs = home.join("logs");
    std::fs::create_dir_all(&home).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_openbible"))
        .env("HOME", &home)
        .env("OBS_LOG_DIR", &logs)
        .env("OBS_FORCE_PANIC", "1")
        .env("OBS_USER_DB", root.join("user.db"))
        .env("OBS_LIBRARY", root.join("library"))
        .output()
        .expect("run openbible");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "the app stopped: {stderr}");
    assert!(stderr.contains("crash report written to"), "{stderr}");

    // The next launch's Session finds it as a new Crash report.
    let mut session = obs_engine::Session::new(obs_engine::JobMode::Inline);
    session.set_log_dir(logs.clone());
    let reports = session.crash_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    let report = &reports[0];
    assert!(report.is_new);
    // Windows writes source paths with backslashes.
    let text = &report.text.replace('\\', "/");
    assert!(
        text.starts_with("Open Bible Software crash report\n"),
        "{text}"
    );
    assert!(text.contains(&format!("Version: {}\n", env!("CARGO_PKG_VERSION"))));
    assert!(text.contains(&format!(
        "OS: {}\n",
        obs_engine::diagnostics::os_description()
    )));
    assert!(text.contains("Thread: main\n"), "{text}");
    assert!(
        text.contains("Panic: forced panic (OBS_FORCE_PANIC)\n"),
        "{text}"
    );
    assert!(text.contains("At: apps/openbible/src/main.rs:"), "{text}");
    // The backtrace starts where the panic was raised, not in the panic machinery.
    assert!(text.contains("\nBacktrace:\n"), "{text}");
    assert!(text.contains(": openbible::start_diagnostics"), "{text}");
    assert!(!text.contains("install_panic_hook"), "{text}");
    // The log line naming the Log folder is there, with the home folder as `~`.
    assert!(text.contains("logging to ~/logs"), "{text}");
    assert!(
        text.contains("panicked at apps/openbible/src/main.rs"),
        "{text}"
    );
    assert!(!report.text.contains(home.to_str().unwrap()), "{text}");
    // The log file has the launch and the panic too.
    let log = std::fs::read_to_string(logs.join(obs_engine::diagnostics::LOG_FILE)).unwrap();
    assert!(
        log.contains("started on") && log.contains("panicked at"),
        "{log}"
    );
    // Nothing else was made: no User database, no library (it stopped before them).
    assert!(!root.join("user.db").exists());
    std::fs::remove_dir_all(&root).unwrap();
}
