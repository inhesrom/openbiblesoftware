//! The log never names what the user imported (#219): a failed Search index build of an
//! imported file is logged by its kind of work, not by its Resource id, which is made from
//! the file's name. Its own test binary, so the process logger is this test's alone.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use obs_engine::diagnostics::{
    LOG_FILE, issue_url, problem_report, recent_log_lines, start_logging,
};
use obs_engine::{ImportPhase, IndexStatus, JobMode, LogLevel, Session};
use serde_json::json;

const MARKER: &str = "private-note";

#[test]
fn a_failed_index_of_an_imported_file_is_logged_without_its_name() {
    let root = std::env::temp_dir().join(format!("obs-log-privacy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for dir in ["library", "source", "logs"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    start_logging(&root.join("logs"), LogLevel::Debug).unwrap();

    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.set_search_cache_dir(root.join("cache"));
    let file = root.join(format!("source/{MARKER}.usfm"));
    std::fs::write(
        &file,
        "\\id GEN\n\\c 1\n\\p\n\\v 1 In the beginning God created the heaven and the earth.\n",
    )
    .unwrap();
    let id = s.run("import.start", &json!({"path": file})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        id.contains(MARKER),
        "the Resource id comes from the file name: {id}"
    );
    s.jobs().pump(1);
    assert!(s.poll_imports().is_empty());
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);

    // The Resource file breaks and its index is gone: the rebuild fails.
    let obr = s.library.get(&id).unwrap().path.clone();
    std::fs::write(&obr, b"broken").unwrap();
    std::fs::remove_file(s.search.path(&id).unwrap()).unwrap();
    s.refresh_search_indexes();
    for _ in 0..20 {
        if !matches!(s.search.status(&id), Some(IndexStatus::Indexing { .. })) {
            break;
        }
        s.jobs().pump(1);
        let _ = s.poll_imports();
    }
    assert!(
        matches!(s.search.status(&id), Some(IndexStatus::Failed(_))),
        "{:?}",
        s.search.status(&id)
    );

    let log = std::fs::read_to_string(root.join("logs").join(LOG_FILE)).unwrap();
    assert!(log.contains("job Search index failed"), "{log}");
    let report = problem_report(&recent_log_lines(200), None);
    assert!(report.contains("job Search index failed"), "{report}");
    for text in [&log, &report, &issue_url(&report)] {
        assert!(!text.contains(MARKER), "{text}");
    }
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}
