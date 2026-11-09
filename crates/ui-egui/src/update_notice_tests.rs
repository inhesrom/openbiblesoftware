//! The Update check and Update now (#276) through the headless harness, against a mock
//! release source (no request reaches the network): the launch check starts a few seconds
//! after the first frame on a worker, once, and never with automatic checks off; the
//! update notice shows the release notes, Update now's progress and Restart now; Skip this
//! version persists; failures say what went wrong and offer the download page.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use obs_engine::testing::{MockRelease, MockReleases, app_exe_name, installed_archive};
use obs_engine::updates::{LAUNCH_CHECK_DELAY_SECS, Version};
use obs_formats::sword_repo::FetchError;

use super::*;
use crate::shell::update_notice::{CHECKSUM_NOTE, FIRST_RUN_NOTE};

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-update-ui-{name}-{}", std::process::id()));
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

fn release(tag: &str) -> MockRelease {
    MockRelease::with_archive(
        tag,
        "## What's new\n\nSearch is faster.\n\n- Notes are kept when a Resource is updated\n- A crash on start is fixed",
        b"new app",
    )
}

fn published(releases: &[MockRelease]) -> Arc<MockReleases> {
    let m = Arc::new(MockReleases::new());
    m.publish(releases);
    m
}

/// Launch as the desktop app does, on worker threads: the User database at `dir/user.db`,
/// the mock release source, version 1.2.3, the install in `dir/app` (when there is one) and
/// the launch check armed.
fn launch(dir: &Path, releases: &Arc<MockReleases>) -> Harness {
    let mut session = Session::new(JobMode::Threads(1));
    let (store, _) = obs_store::UserStore::open(&dir.join("user.db")).unwrap();
    session.attach_user(Box::new(store));
    session.set_fetch(releases.clone());
    session.set_update_source(MockReleases::source());
    session.set_current_version(Version::parse("1.2.3").unwrap());
    let exe = dir.join("app").join(app_exe_name());
    if exe.exists() {
        session.set_install(obs_engine::updates::Install::of_exe(&exe));
    }
    let mut shell = Shell::new(session);
    add_doc(&mut shell, crate::tests::bible());
    shell.restore_or_default();
    shell.arm_launch_update_check();
    Harness::from_shell(shell)
}

/// Frames at 60 Hz until `done`, at most `n`.
fn frames_until(h: &mut Harness, n: usize, done: impl Fn(&Harness) -> bool) {
    for _ in 0..n {
        if done(h) {
            return;
        }
        h.paced_frame();
    }
    assert!(done(h), "not done after {n} frames: {:?}", h.texts());
}

/// Frames for the launch check's delay to pass (the harness's clock moves 1/60 s a frame).
fn past_launch_delay(h: &mut Harness) {
    h.frames((LAUNCH_CHECK_DELAY_SECS * 60.0) as usize + 2);
}

#[test]
fn the_launch_check_runs_once_on_a_worker_after_the_first_frame_and_never_delays_it() {
    let dir = folder("launch");
    let releases = published(&[release("v1.3.0")]);
    // A slow network: every request waits until released.
    releases.hold(true);
    let mut h = launch(&dir, &releases);
    // The first frames come and go with no request.
    assert!(releases.requests().is_empty());
    h.frames((LAUNCH_CHECK_DELAY_SECS * 60.0) as usize - 10);
    assert!(releases.requests().is_empty(), "not before the delay");
    // The frame that starts the check returns at once, though the request hangs.
    let started = std::time::Instant::now();
    h.frames(12);
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    frames_until(&mut h, 120, |_| releases.requests().len() == 1);
    assert_ne!(
        releases.request_threads()[0],
        std::thread::current().id(),
        "off the UI thread"
    );
    releases.hold(false);
    frames_until(&mut h, 300, |h| {
        h.find("Open Bible Software 1.3.0 is available").is_some()
    });
    // Exactly one check this launch.
    h.frames(400);
    assert_eq!(releases.requests(), [MockReleases::latest_url()]);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn with_automatic_checks_off_a_launch_makes_no_request() {
    let dir = folder("off");
    let releases = published(&[release("v1.3.0")]);
    let mut h = launch(&dir, &releases);
    h.shell
        .run("updates.auto_check", &json!({ "on": false }))
        .unwrap();
    drop(h);
    let mut h = launch(&dir, &releases);
    past_launch_delay(&mut h);
    h.frames(300);
    assert!(releases.requests().is_empty(), "{:?}", releases.requests());
    assert!(h.find("Update checks").is_none());
    assert!(h.find("Update available").is_none());
    // Settings → Updates shows it off.
    assert!(!h.shell.st.session.updates().auto_check());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_notice_shows_the_release_notes_and_skip_this_version_persists() {
    let dir = folder("notice");
    let releases = published(&[release("v1.3.0")]);
    let mut h = launch(&dir, &releases);
    past_launch_delay(&mut h);
    frames_until(&mut h, 300, |h| h.find("Update available").is_some());
    h.frames(5);
    for text in [
        "Open Bible Software 1.3.0 is available",
        "You have 1.2.3.",
        "What's new",
        "Search is faster.",
        "· Notes are kept when a Resource is updated",
        "· A crash on start is fixed",
        "Later",
        "Skip this version",
    ] {
        assert!(h.find(text).is_some(), "{text:?} not in {:?}", h.texts());
    }
    // Not installed from an archive here: the download page instead of Update now.
    assert!(h.find("Update now").is_none());
    assert!(h.find("Open the download page").is_some());
    preview(&h, "update-notice-not-updatable-kittest");
    h.click_text("Skip this version");
    assert!(h.find("Update available").is_none());
    // The first launch that checks says so once.
    assert!(h.find(FIRST_RUN_NOTE).is_some(), "{:?}", h.texts());
    h.click_text("OK");
    assert!(h.find(FIRST_RUN_NOTE).is_none());
    drop(h);

    // The next launch remembers both.
    let mut h = launch(&dir, &releases);
    assert_eq!(h.shell.st.session.updates().skipped(), Some("v1.3.0"));
    past_launch_delay(&mut h);
    frames_until(&mut h, 300, |h| !h.shell.st.session.updates().is_busy());
    h.frames(5);
    assert_eq!(releases.requests().len(), 2, "the launch checked");
    assert!(
        h.find("Update available").is_none(),
        "a skipped version stays quiet"
    );
    assert!(h.find(FIRST_RUN_NOTE).is_none());
    // Help → Check for updates… still shows it.
    h.open_menu("Help");
    h.click_text("Check for updates…");
    frames_until(&mut h, 300, |h| h.find("Update available").is_some());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn update_now_shows_progress_then_restart_now_starts_the_new_version() {
    let dir = folder("install");
    if obs_engine::updates::platform_target().is_none() {
        return;
    }
    installed_archive(&dir.join("app"), b"old app");
    let releases = published(&[release("v1.3.0")]);
    let mut h = launch(&dir, &releases);
    let relaunched = Arc::new(Mutex::new(Vec::<PathBuf>::new()));
    let log = Arc::clone(&relaunched);
    h.shell.st.updates_ui.relaunch = Arc::new(move |exe: &Path| {
        log.lock().unwrap().push(exe.to_path_buf());
        Ok(())
    });
    // The first-run note is answered by turning nothing off.
    past_launch_delay(&mut h);
    frames_until(&mut h, 300, |h| h.find("Update now").is_some());
    h.frames(5);
    assert!(h.find(CHECKSUM_NOTE).is_some(), "{:?}", h.texts());
    preview(&h, "update-notice-kittest");
    // A slow download shows its progress and Cancel.
    releases.hold(true);
    h.click_text("Update now");
    frames_until(&mut h, 60, |h| {
        h.find("Downloading Open Bible Software 1.3.0…").is_some()
    });
    assert!(h.find("Cancel").is_some());
    preview(&h, "update-downloading-kittest");
    releases.hold(false);
    frames_until(&mut h, 600, |h| h.find("Update installed").is_some());
    h.frames(3);
    assert!(
        h.find("Open Bible Software 1.3.0 is installed").is_some(),
        "{:?}",
        h.texts()
    );
    assert!(
        h.texts()
            .iter()
            .any(|t| t.contains("a backup of your data was taken first")),
        "{:?}",
        h.texts()
    );
    preview(&h, "update-installed-kittest");
    let exe = dir.join("app").join(app_exe_name());
    assert_eq!(std::fs::read(&exe).unwrap(), b"new app");
    h.click_text("Restart now");
    assert_eq!(*relaunched.lock().unwrap(), [exe]);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn offline_and_rate_limited_checks_say_so_and_offer_the_download_page() {
    let dir = folder("offline");
    let releases = published(&[release("v1.3.0")]);
    releases.fail(
        &MockReleases::latest_url(),
        FetchError::Network("failed to lookup address information".into()),
    );
    let mut h = launch(&dir, &releases);
    h.shell
        .run("updates.auto_check", &json!({ "on": false }))
        .unwrap();
    h.open_menu("Help");
    h.click_text("Check for updates…");
    frames_until(&mut h, 300, |h| {
        h.find("Could not check for updates").is_some()
    });
    h.frames(3);
    assert!(
        h.find("Could not reach GitHub. Check your internet connection and try again.")
            .is_some(),
        "{:?}",
        h.texts()
    );
    preview(&h, "update-offline-kittest");
    h.click_text("Open the download page");
    assert_eq!(h.opened, [MockReleases::source().page]);

    releases.fail(&MockReleases::latest_url(), FetchError::RateLimited);
    h.click_text("Try again");
    frames_until(&mut h, 300, |h| {
        h.texts()
            .iter()
            .any(|t| t.starts_with("GitHub is limiting"))
    });
    // The window settles to its new size first.
    h.frames(5);
    h.click_text("Close");
    assert!(h.find("Could not check for updates").is_none());
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_for_updates_during_the_launch_check_shows_its_failure_not_a_spinner() {
    let dir = folder("launch-then-manual");
    let releases = published(&[release("v1.3.0")]);
    releases.hold(true);
    let mut h = launch(&dir, &releases);
    past_launch_delay(&mut h);
    frames_until(&mut h, 120, |_| releases.requests().len() == 1);
    h.open_menu("Help");
    h.click_text("Check for updates…");
    frames_until(&mut h, 30, |h| h.find("Checking for updates…").is_some());
    releases.fail(
        &MockReleases::latest_url(),
        FetchError::Network("failed to lookup address information".into()),
    );
    releases.hold(false);
    frames_until(&mut h, 300, |h| {
        h.find("Could not check for updates").is_some()
    });
    h.frames(3);
    assert!(
        h.find("Could not reach GitHub. Check your internet connection and try again.")
            .is_some(),
        "{:?}",
        h.texts()
    );
    assert!(h.find("Checking for updates…").is_none());
    assert_eq!(releases.requests(), [MockReleases::latest_url()]);
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(unix)]
#[test]
fn an_install_folder_without_write_permission_offers_only_the_download_page() {
    use std::os::unix::fs::PermissionsExt;
    let dir = folder("read-only");
    if obs_engine::updates::platform_target().is_none() {
        return;
    }
    installed_archive(&dir.join("app"), b"old app");
    let releases = published(&[release("v1.3.0")]);
    /// Gives the folder its permissions back however the test ends.
    struct Writable(PathBuf);
    impl Drop for Writable {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    let _restore = Writable(dir.join("app"));
    std::fs::set_permissions(dir.join("app"), std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root may write anywhere; then there is nothing to test.
    let writable = std::fs::write(dir.join("app/probe"), b"").is_ok();
    let mut h = launch(&dir, &releases);
    h.open_menu("Help");
    h.click_text("Check for updates…");
    frames_until(&mut h, 300, |h| h.find("Update available").is_some());
    h.frames(5);
    if !writable {
        assert!(
            h.texts()
                .iter()
                .any(|t| t.contains("you do not have permission")),
            "{:?}",
            h.texts()
        );
        preview(&h, "update-not-writable-kittest");
        assert!(h.find("Update now").is_none());
        h.click_text("Open the download page");
        assert_eq!(
            h.opened,
            [format!(
                "{}/releases/tag/v1.3.0",
                obs_engine::testing::MOCK_UPDATE_BASE
            )]
        );
    }
    drop(h);
    drop(_restore);
    std::fs::remove_dir_all(&dir).unwrap();
}
