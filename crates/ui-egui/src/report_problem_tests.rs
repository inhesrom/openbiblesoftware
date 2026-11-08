//! Report a problem (#219) through the headless harness: a Crash report left by the last run
//! is offered at launch; the user reads the whole report before anything leaves the computer,
//! and only Copy report or Open GitHub issue… (which sends it to GitHub in the new-issue
//! form's address, as the window says first; submitting the form publishes it) takes it
//! anywhere. Not now is not offered again; Help → Report a problem… shows the newest
//! report (or a problem report made now) and Help → Open log folder shows the Log folder. The
//! Last session is saved within a second of a change, so a crash keeps the Workspace.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use obs_engine::diagnostics::{self, NEW_ISSUE_URL, PanicFacts};

use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-report-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// A Crash report as the panic hook writes it, left in `logs` by the last run.
fn crashed_last_time(logs: &Path) -> String {
    let facts = PanicFacts {
        message: diagnostics::panic_summary("index out of bounds: the len is 3 but the index is 7"),
        location: Some("crates/ui-egui/src/shell.rs:120:9".into()),
        thread: "main".into(),
        backtrace: "   0: obs_ui_egui::shell::Shell::ui\n             at ./crates/ui-egui/src/shell.rs:120:9".into(),
        when: 1_790_000_000,
    };
    let lines = ["2026-09-21 14:13:20 UTC INFO  openbible: Open Bible Software started".into()];
    let text = diagnostics::crash_report_text(&facts, &lines, None);
    diagnostics::write_crash_report(logs, facts.when, &text).unwrap();
    text
}

/// Launch on `db` with `logs` as the Log folder, as the app does: the Last session first,
/// then the offer.
fn launch(db: &Path, logs: &Path) -> Harness {
    let mut h = Harness::launch(Some(db));
    h.shell.st.session.set_log_dir(logs.to_path_buf());
    h.shell.offer_crash_report();
    h.frames(3);
    h
}

fn shows_report(h: &Harness, text: &str) -> bool {
    h.texts().iter().any(|t| t.trim_end() == text.trim_end())
}

#[test]
fn a_crash_report_is_offered_at_launch_and_read_before_anything_leaves_the_computer() {
    let dir = folder("offer");
    let logs = dir.join("logs");
    let report = crashed_last_time(&logs);
    let mut h = launch(&dir.join("user.db"), &logs);

    // The offer: nothing has been sent.
    assert!(
        h.find("Open Bible Software closed unexpectedly").is_some(),
        "{:?}",
        h.texts()
    );
    assert!(
        h.find("Nothing has been sent. You can read the report first.")
            .is_some()
    );
    h.frames(30);
    preview(&h, "report-problem-offer-kittest");

    // Report a problem… shows the whole report, and still nothing has left.
    h.click_text("Report a problem…");
    h.frames(2);
    assert!(h.find("Report a problem").is_some());
    assert!(shows_report(&h, &report), "{:?}", h.texts());
    assert!(h.opened.is_empty() && h.copied.is_empty());
    // Before either button is clicked, the window says what each does with the report.
    assert!(
        h.find(crate::shell::report_problem::REVIEW_NOTE).is_some(),
        "{:?}",
        h.texts()
    );
    assert!(crate::shell::report_problem::REVIEW_NOTE.contains("sends it to GitHub"));
    assert!(crate::shell::report_problem::REVIEW_NOTE.contains("public issue only if you submit"));
    preview(&h, "report-problem-review-kittest");
    // Offered once: it is no longer new.
    let reports = h.shell.st.session.crash_reports();
    assert_eq!(reports.len(), 1);
    assert!(!reports[0].is_new);

    // Copy report copies exactly what was shown.
    h.click_text("Copy report");
    assert_eq!(h.copied, std::slice::from_ref(&report));
    assert!(h.opened.is_empty());

    // Open GitHub issue… opens the issue form, prefilled; the user submits it there.
    h.click_text("Open GitHub issue…");
    assert_eq!(h.opened.len(), 1);
    let url = &h.opened[0];
    assert!(url.starts_with(&format!(
        "{NEW_ISSUE_URL}?title=Crash%3A%20index%20out%20of%20bounds"
    )));
    assert!(
        url.contains("Panic%3A%20index%20out%20of%20bounds"),
        "{url}"
    );
    let toast = h.shell.st.toast.as_ref().map(|t| t.0.clone()).unwrap();
    assert!(
        toast.starts_with("Sent to GitHub's new-issue form") && toast.contains("public"),
        "{toast}"
    );

    h.click_text("Close");
    assert!(h.shell.st.report_ui.shown.is_none());
    drop(h);

    // The next launch does not offer it again.
    let h = launch(&dir.join("user.db"), &logs);
    assert!(h.find("Open Bible Software closed unexpectedly").is_none());
    assert!(h.shell.st.report_ui.shown.is_none());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn not_now_is_not_offered_again_and_help_report_a_problem_shows_it_to_delete() {
    let dir = folder("not-now");
    let logs = dir.join("logs");
    let report = crashed_last_time(&logs);
    let mut h = launch(&dir.join("user.db"), &logs);
    h.click_text("Not now");
    assert!(h.find("Open Bible Software closed unexpectedly").is_none());
    assert!(h.opened.is_empty() && h.copied.is_empty());
    drop(h);

    let mut h = launch(&dir.join("user.db"), &logs);
    assert!(h.shell.st.report_ui.shown.is_none(), "offered once only");
    h.open_menu("Help");
    h.frames(30);
    preview(&h, "report-problem-help-menu-kittest");
    h.click_text("Report a problem…");
    h.frames(2);
    assert!(shows_report(&h, &report), "{:?}", h.texts());
    h.click_text("Delete report");
    assert!(h.shell.st.session.crash_reports().is_empty());
    assert!(h.shell.st.report_ui.shown.is_none());
    assert!(h.opened.is_empty() && h.copied.is_empty());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn without_a_crash_report_help_report_a_problem_shows_a_problem_report() {
    let dir = folder("problem");
    let logs = dir.join("logs");
    let mut h = launch(&dir.join("user.db"), &logs);
    assert!(h.shell.st.report_ui.shown.is_none(), "nothing to offer");
    h.open_menu("Help");
    h.click_text("Report a problem…");
    h.frames(2);
    let text = h
        .texts()
        .into_iter()
        .find(|t| t.starts_with("Open Bible Software problem report"))
        .unwrap_or_else(|| panic!("{:?}", h.texts()))
        .to_string();
    assert!(text.contains(&format!("Version: {}", env!("CARGO_PKG_VERSION"))));
    assert!(text.contains("Recent log lines:"));
    assert!(h.find("Delete report").is_none(), "nothing to delete");
    h.click_text("Open GitHub issue…");
    assert!(
        h.opened[0].contains("title=Problem%20report"),
        "{:?}",
        h.opened
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn help_open_log_folder_shows_the_log_folder() {
    let dir = folder("open-folder");
    let logs = dir.join("logs");
    let mut h = launch(&dir.join("user.db"), &logs);
    let shown = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&shown);
    h.shell.st.report_ui.open_folder = Arc::new(move |p: &Path| {
        log.lock().unwrap().push(p.to_path_buf());
        Ok(None)
    });
    h.open_menu("Help");
    h.click_text("Open log folder");
    assert_eq!(*shown.lock().unwrap(), std::slice::from_ref(&logs));
    assert!(logs.is_dir());
    // A folder that cannot be shown says where it is.
    h.shell.st.report_ui.open_folder = Arc::new(|_: &Path| Err("no file manager".into()));
    h.shell.run("help.open_log_folder", &json!({}));
    h.frames(1);
    let toast = h.shell.st.toast.as_ref().map(|t| t.0.clone()).unwrap();
    assert!(toast.contains(&logs.display().to_string()), "{toast}");

    // A file manager that starts but then fails says so too, without holding up the window.
    h.shell.st.toast = None;
    let (tx, rx) = std::sync::mpsc::channel();
    let rx = Mutex::new(Some(rx));
    h.shell.st.report_ui.open_folder = Arc::new(move |_: &Path| Ok(rx.lock().unwrap().take()));
    h.shell.run("help.open_log_folder", &json!({}));
    h.frames(2);
    assert!(h.shell.st.toast.is_none(), "still running");
    assert!(
        h.repaint_delay < std::time::Duration::MAX,
        "a frame is asked for to see it exit"
    );
    tx.send(Err("xdg-open failed: exit status: 4".into()))
        .unwrap();
    h.frames(1);
    let toast = h.shell.st.toast.as_ref().map(|t| t.0.clone()).unwrap();
    assert!(
        toast.contains("exit status: 4") && toast.contains(&logs.display().to_string()),
        "{toast}"
    );
    assert!(h.shell.st.report_ui.opening.is_none());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn a_file_manager_that_exits_with_an_error_is_reported() {
    let dir = folder("opener-exit");
    let rx = crate::shell::report_problem::open_folder_with("false", &dir, true)
        .unwrap()
        .unwrap();
    let outcome = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert!(
        outcome
            .as_ref()
            .is_err_and(|e| e.starts_with("false failed: exit status: 1")),
        "{outcome:?}"
    );
    let rx = crate::shell::report_problem::open_folder_with("true", &dir, true)
        .unwrap()
        .unwrap();
    assert_eq!(
        rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap(),
        Ok(())
    );
    assert!(
        crate::shell::report_problem::open_folder_with("obs-no-such-file-manager", &dir, true)
            .is_err()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_last_session_is_saved_within_a_second_of_a_change_so_a_crash_keeps_it() {
    let dir = folder("session");
    let db = dir.join("user.db");
    let logs = dir.join("logs");
    let mut h = launch(&db, &logs);
    // A save was just made; a change now is saved by a frame the app asks for, even idle.
    h.shell.run("resource.open", &json!({ "id": "heb-test" }));
    h.frame();
    assert!(h.shell.st.save_due);
    assert!(
        h.repaint_delay <= std::time::Duration::from_secs(1),
        "{:?}",
        h.repaint_delay
    );
    // Only the frames the app asks for come, each when asked; the save is made within a
    // second of the change.
    let changed = h.time;
    for _ in 0..10 {
        if !h.shell.st.save_due {
            break;
        }
        assert!(
            h.repaint_delay < std::time::Duration::MAX,
            "a frame is asked for"
        );
        h.time += h.repaint_delay.as_secs_f64();
        h.idle_frame();
    }
    assert!(!h.shell.st.save_due);
    assert!(h.time - changed <= 1.1, "{}", h.time - changed);
    assert_eq!(
        h.repaint_delay,
        std::time::Duration::MAX,
        "nothing left to save"
    );
    // The app stops without quitting (a crash): no closing save.
    drop(h);

    let h = launch(&db, &logs);
    let titles: Vec<String> = h.panels().into_iter().flat_map(|(_, t)| t).collect();
    assert!(titles.iter().any(|t| t == "HEB"), "{titles:?}");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Frames until `done` holds (the save dialog answers on its own thread).
fn until(h: &mut Harness, what: &str, done: impl Fn(&Harness) -> bool) {
    for _ in 0..300 {
        if done(h) {
            h.frames(2);
            return;
        }
        h.paced_frame();
    }
    panic!("never: {what}; on screen: {:?}", h.texts());
}

/// The files of the zip at `path`, `(name, text)` in order.
fn unzip(path: &Path) -> Vec<(String, String)> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut f = zip.by_index(i).unwrap();
            let mut text = String::new();
            f.read_to_string(&mut text).unwrap();
            (f.name().to_string(), text)
        })
        .collect()
}

/// What the open window says its zip will hold, by name.
fn listed(h: &Harness) -> Vec<String> {
    let zip = match &h.shell.st.report_ui.shown {
        Some(crate::shell::report_problem::Shown::Review { zip, .. })
        | Some(crate::shell::report_problem::Shown::Diagnostics { zip }) => zip,
        _ => panic!("no zip window"),
    };
    let names: Vec<String> = zip.iter().map(|f| f.name.clone()).collect();
    // Each is on screen, with what it is.
    for f in zip {
        let line = h
            .texts()
            .into_iter()
            .find(|t| t.starts_with(&format!("{} — {} (", f.name, f.about)));
        assert!(line.is_some(), "{} not listed: {:?}", f.name, h.texts());
    }
    assert!(
        h.find(crate::shell::report_problem::ZIP_NOTE).is_some(),
        "{:?}",
        h.texts()
    );
    names
}

type Calls = Arc<Mutex<Vec<PathBuf>>>;
/// What the save dialog was asked: the suggested file name and folder.
type Asked = Arc<Mutex<Vec<(String, Option<PathBuf>)>>>;

/// The save dialog answers `dest` and records what it was asked; the file manager records
/// what it was asked to show.
fn save_to(h: &mut Harness, dest: &Path) -> (Asked, Calls) {
    let asked = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&asked);
    let dest = dest.to_path_buf();
    h.shell.st.report_ui.save_dialog = Arc::new(move |name: &str, folder: Option<&Path>| {
        log.lock()
            .unwrap()
            .push((name.to_string(), folder.map(Path::to_path_buf)));
        Some(dest.clone())
    });
    let shown = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&shown);
    h.shell.st.report_ui.reveal_file = Arc::new(move |p: &Path| {
        log.lock().unwrap().push(p.to_path_buf());
        Ok(None)
    });
    (asked, shown)
}

#[test]
fn save_report_as_zip_lists_what_it_saves_then_saves_it_and_shows_it() {
    let dir = folder("zip");
    let logs = dir.join("logs");
    let report = crashed_last_time(&logs);
    std::fs::write(
        logs.join(diagnostics::LOG_FILE),
        "2026-09-21 14:13:20 UTC INFO  openbible: Open Bible Software started\n",
    )
    .unwrap();
    let mut h = launch(&dir.join("user.db"), &logs);
    h.click_text("Report a problem…");
    h.frames(2);

    // Before anything is saved, the window lists exactly what the zip will hold.
    assert!(
        crate::shell::report_problem::REVIEW_NOTE.contains("Save report as zip…"),
        "the note says what it does before it is clicked"
    );
    let names = listed(&h);
    let crash = h.shell.st.session.crash_reports()[0].path.clone();
    let crash_name = crash
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .replace(".seen", "");
    assert_eq!(
        names,
        [
            diagnostics::ZIP_README.to_string(),
            crash_name.clone(),
            diagnostics::LOG_FILE.to_string()
        ]
    );
    h.frames(30);
    preview(&h, "report-problem-zip-review-kittest");

    let dest = dir.join("saved/openbible-report.zip");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let (asked, shown) = save_to(&mut h, &dest);
    h.click_text("Save report as zip…");
    until(&mut h, "zip saved", |h| !h.shell.st.zip_dialog_waiting());
    // The dialog suggested a dated name, in Downloads or Desktop where there is one.
    let (name, folder) = asked.lock().unwrap()[0].clone();
    assert!(
        name.starts_with("openbible-report-") && name.ends_with(".zip"),
        "{name}"
    );
    assert_eq!(folder, diagnostics::default_zip_folder());
    // The zip holds what was listed, the report as it was shown.
    let files = unzip(&dest);
    let in_zip: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(in_zip, names);
    assert_eq!(files[1].1, report);
    // It is shown in the file manager, and the toast says where it is.
    assert_eq!(*shown.lock().unwrap(), std::slice::from_ref(&dest));
    let toast = h.shell.st.toast.as_ref().map(|t| t.0.clone()).unwrap();
    assert!(
        toast.contains(&dest.display().to_string()) && toast.contains("Nothing was sent"),
        "{toast}"
    );
    // Nothing left the computer, and the review stays open.
    assert!(h.opened.is_empty() && h.copied.is_empty());
    assert!(h.find("Save report as zip…").is_some());

    // Saving again to the same name keeps the first zip and saves beside it; a file manager
    // that cannot show it leaves the toast saying where it is.
    let first = std::fs::read(&dest).unwrap();
    h.shell.st.report_ui.reveal_file = Arc::new(|_: &Path| Err("no file manager".into()));
    h.click_text("Save report as zip…");
    until(&mut h, "zip saved again", |h| {
        !h.shell.st.zip_dialog_waiting()
    });
    assert_eq!(std::fs::read(&dest).unwrap(), first);
    let beside = dir.join("saved/openbible-report (2).zip");
    assert_eq!(
        unzip(&beside).into_iter().map(|f| f.0).collect::<Vec<_>>(),
        names
    );
    let toast = h.shell.st.toast.as_ref().map(|t| t.0.clone()).unwrap();
    assert!(
        toast.contains("no file manager") && toast.contains(&beside.display().to_string()),
        "{toast}"
    );

    // A cancelled dialog saves nothing.
    h.shell.st.report_ui.save_dialog = Arc::new(|_: &str, _: Option<&Path>| None);
    h.click_text("Save report as zip…");
    until(&mut h, "dialog cancelled", |h| {
        !h.shell.st.zip_dialog_waiting()
    });
    assert_eq!(std::fs::read_dir(dir.join("saved")).unwrap().count(), 2);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn help_save_diagnostics_as_zip_saves_the_log_files_without_a_crash_report() {
    let dir = folder("zip-logs");
    let logs = dir.join("logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join(diagnostics::LOG_FILE), "a log line\n").unwrap();
    std::fs::write(logs.join("openbible.1.log"), "an older line\n").unwrap();
    let mut h = launch(&dir.join("user.db"), &logs);
    assert!(h.shell.st.session.crash_reports().is_empty());
    h.open_menu("Help");
    h.frames(30);
    preview(&h, "report-problem-zip-help-menu-kittest");
    h.click_text("Save diagnostics as zip…");
    h.frames(2);
    assert!(
        h.find("Save diagnostics as zip").is_some(),
        "{:?}",
        h.texts()
    );
    let names = listed(&h);
    assert_eq!(
        names,
        [diagnostics::ZIP_README, "openbible.log", "openbible.1.log"]
    );
    h.frames(30);
    preview(&h, "report-problem-zip-diagnostics-kittest");

    let dest = dir.join("logs.zip");
    let (_, shown) = save_to(&mut h, &dest);
    h.click_text("Save as zip…");
    until(&mut h, "zip saved", |h| !h.shell.st.zip_dialog_waiting());
    let files = unzip(&dest);
    assert_eq!(
        files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(),
        names
    );
    assert_eq!(files[1].1, "a log line\n");
    assert!(files[0].1.contains("- openbible.1.log: an older log file"));
    assert_eq!(*shown.lock().unwrap(), std::slice::from_ref(&dest));
    assert!(
        h.shell.st.report_ui.shown.is_none(),
        "the window closes once saved"
    );
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn showing_a_file_tries_each_file_manager_in_turn() {
    use std::process::Command;
    let wait = |rx: crate::shell::report_problem::FolderShown| {
        rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap()
    };
    let run = crate::shell::report_problem::run_in_turn;
    assert_eq!(
        wait(
            run(vec![Command::new("false"), Command::new("true")])
                .unwrap()
                .unwrap()
        ),
        Ok(())
    );
    let failed = wait(
        run(vec![
            Command::new("obs-no-such-file-manager"),
            Command::new("false"),
        ])
        .unwrap()
        .unwrap(),
    );
    let failed = failed.unwrap_err();
    assert!(
        failed.starts_with("obs-no-such-file-manager: ")
            && failed.ends_with("; false failed: exit status: 1"),
        "{failed}"
    );
}
