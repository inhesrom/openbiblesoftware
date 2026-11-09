//! A forced panic in a test build writes a Crash report to the Log folder (#219): the panic,
//! where it happened, a backtrace, the app version, the OS and the recent log lines, with the
//! home folder written as `~`; neither a library's log message quoting typed text nor text
//! the code put in the panic message reaches it, nor the log file, nor the GitHub issue
//! address. The next launch finds it as a new report to offer
//! (`ui-egui`'s `report_problem_tests` show the offer and the review), and Save report as
//! zip… saves it with the log file and a README in one zip that holds neither text (#249).
//!
//! Only test (debug) builds panic on `OBS_FORCE_PANIC`; a release build would start the
//! normal app, so the test is compiled for debug builds only.
#![cfg(debug_assertions)]
#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn report_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// How long the forced panic may take before the app is stopped and the test fails.
const PANIC_WAIT: Duration = Duration::from_secs(120);

#[test]
fn a_forced_panic_writes_a_crash_report_the_next_launch_offers() {
    let root = std::env::temp_dir().join(format!("obs-crash-report-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let logs = home.join("logs");
    std::fs::create_dir_all(&home).unwrap();
    let stderr_path = root.join("stderr.txt");
    let mut child = Command::new(env!("CARGO_BIN_EXE_openbible"))
        .env("HOME", &home)
        .env("OBS_LOG_DIR", &logs)
        .env("OBS_FORCE_PANIC", "1")
        .env("OBS_USER_DB", root.join("user.db"))
        .env("OBS_LIBRARY", root.join("library"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&stderr_path).unwrap())
        .spawn()
        .expect("run openbible");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > PANIC_WAIT {
            let _ = child.kill();
            let _ = child.wait();
            panic!("openbible did not stop within {PANIC_WAIT:?} of a forced panic");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stderr = std::fs::read_to_string(&stderr_path).unwrap();
    assert!(!status.success(), "the app stopped: {stderr}");
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
    assert!(text.contains("Panic: (message withheld)\n"), "{text}");
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
    // The windowing library's warning is there, where it was logged from, without the text.
    assert!(
        text.contains("WARN  egui_winit: (message withheld; logged at main.rs:"),
        "{text}"
    );
    assert!(!text.contains("PRIVATE_NOTE"), "{text}");
    let url = obs_engine::diagnostics::issue_url(&report.text);
    assert!(!url.contains("PRIVATE_NOTE"), "{url}");
    // The log file has the launch and the panic too, and not the typed text.
    let log = std::fs::read_to_string(logs.join(obs_engine::diagnostics::LOG_FILE)).unwrap();
    assert!(
        log.contains("started on") && log.contains("panicked at"),
        "{log}"
    );
    assert!(!log.contains("PRIVATE_NOTE"), "{log}");

    // Save report as zip…: the report, the log file and the README, and no typed text. (As
    // `help.save_diagnostics_zip` makes it, with the app's home folder: this test's differs.)
    let name = report.path.file_name().unwrap().to_str().unwrap();
    let files = obs_engine::diagnostics::diagnostics_zip_entries(
        &logs,
        Some(report),
        home.to_str(),
        report_time(),
    );
    let dest = root.join("openbible-report.zip");
    let saved = obs_engine::diagnostics::write_diagnostics_zip(&dest, &files).unwrap();
    assert_eq!(saved, dest);
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    let mut entries = Vec::new();
    for i in 0..zip.len() {
        use std::io::Read;
        let mut f = zip.by_index(i).unwrap();
        let mut text = String::new();
        f.read_to_string(&mut text).unwrap();
        entries.push((f.name().to_string(), text));
    }
    let names: Vec<&str> = entries.iter().map(|e| e.0.as_str()).collect();
    assert_eq!(
        names,
        [
            obs_engine::diagnostics::ZIP_README,
            name,
            obs_engine::diagnostics::LOG_FILE
        ]
    );
    assert_eq!(entries[1].1, report.text);
    assert!(entries[2].1.contains("panicked at"), "{}", entries[2].1);
    // Windows writes the Log folder with backslashes.
    let log_in_zip = entries[2].1.replace('\\', "/");
    assert!(log_in_zip.contains("logging to ~/logs"), "{log_in_zip}");
    let readme = &entries[0].1;
    assert!(
        readme.contains(&format!("- {name}: the crash report")),
        "{readme}"
    );
    for (name, text) in &entries {
        assert!(!text.contains("PRIVATE_NOTE"), "{name}: {text}");
        assert!(!text.contains(home.to_str().unwrap()), "{name}: {text}");
    }
    // Nothing else was made: no User database, no library (it stopped before them).
    assert!(!root.join("user.db").exists());
    std::fs::remove_dir_all(&root).unwrap();
}
