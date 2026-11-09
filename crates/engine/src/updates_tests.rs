//! The Update check and Update now (#276, ADR 0017) through the Session, against a mock
//! release source ([`crate::testing::MockReleases`]): no request reaches the network.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::json;

use crate::sword_repo::{Fetch, FetchError};
use crate::testing::{
    MockRelease, MockReleases, app_exe_name, installed_archive, release_archive_bytes,
};
use crate::updates::*;
use crate::*;

/// A temp folder with an installed Release archive (`app/`), a library folder and a User
/// database.
struct Lab {
    root: PathBuf,
}

impl Lab {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-updates-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("library")).unwrap();
        std::fs::write(root.join("library/kjv.obr"), b"a Resource").unwrap();
        Self { root }
    }

    fn app(&self) -> PathBuf {
        self.root.join("app")
    }

    /// Launch as the app does, on `mode`'s job queue: the User database attached, the mock
    /// release source, this version `1.2.3` and the install in `app/`.
    fn launch(&self, releases: &Arc<MockReleases>, mode: JobMode) -> Session {
        let mut s = Session::new(mode);
        let (store, _) = obs_store::UserStore::open(&self.root.join("user.db")).unwrap();
        s.attach_user(Box::new(store));
        s.set_fetch(releases.clone());
        s.set_update_source(MockReleases::source());
        s.set_current_version(Version::parse("1.2.3").unwrap());
        if self.app().join(app_exe_name()).exists() {
            s.set_install(Install::of_exe(&self.app().join(app_exe_name())));
        }
        s
    }

    /// Every file under the library folder and the User database's settings, to compare.
    fn data(&self, s: &Session) -> DataState {
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(self.root.join("library"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read(e.path()).unwrap(),
                )
            })
            .collect();
        files.sort();
        let prefs = s.user.as_ref().unwrap().preferences().unwrap();
        (files, prefs)
    }
}

/// The library folder's files and the User database's settings.
type DataState = (Vec<(String, Vec<u8>)>, Vec<(String, String)>);

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn settle(s: &mut Session) {
    for _ in 0..200 {
        s.jobs().pump(64);
        let _ = s.poll_imports();
        if !s.updates().is_busy() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("the update job did not finish");
}

fn release(tag: &str) -> MockRelease {
    MockRelease::with_archive(tag, "## Fixes\n\n- Notes are kept.", b"new app")
}

fn published(releases: &[MockRelease]) -> Arc<MockReleases> {
    let m = Arc::new(MockReleases::new());
    m.publish(releases);
    m
}

fn v(text: &str) -> Version {
    Version::parse(text).unwrap()
}

#[test]
fn versions_compare_as_semver() {
    assert_eq!(v("v1.2.3"), v("1.2.3"));
    assert_eq!(v("1.2.3+build.5"), v("1.2.3"));
    assert_eq!(v("0.10.0").to_string(), "0.10.0");
    assert_eq!(v("1.0.0-rc.1").to_string(), "1.0.0-rc.1");
    for bad in [
        "1.2",
        "1.2.3.4",
        "01.2.3",
        "1.2.x",
        "",
        "v",
        "1.2.3-",
        "1.2.3-a..b",
        "dev-abc",
    ] {
        assert_eq!(Version::parse(bad), None, "{bad}");
    }
    // The semver spec's own precedence example, then numbers compared as numbers.
    let ordered = [
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
        "1.0.1",
        "1.2.0",
        "1.10.0",
        "2.0.0",
    ];
    for w in ordered.windows(2) {
        assert!(v(w[0]) < v(w[1]), "{} < {}", w[0], w[1]);
    }
    assert!(v("1.0.0-rc.1").is_prerelease() && !v("1.0.0").is_prerelease());
    // This build's own version parses.
    assert_eq!(Version::current().to_string(), APP_VERSION);
}

#[test]
fn the_newest_release_skips_drafts_older_versions_and_prereleases_unless_asked() {
    let mut beta = release("v1.4.0-beta.1");
    beta.prerelease = true;
    let mut draft = release("v9.0.0");
    draft.draft = true;
    // GitHub's flag alone marks a pre-release too.
    let mut flagged = release("v1.3.5");
    flagged.prerelease = true;
    let list: Vec<serde_json::Value> = [
        release("v1.2.2"),
        release("v1.2.3"),
        release("v1.3.0"),
        beta,
        draft,
        flagged,
        release("not-a-version"),
    ]
    .iter()
    .map(|r| {
        let m = MockReleases::new();
        m.publish(std::slice::from_ref(r));
        serde_json::from_str(
            &String::from_utf8(m.get(&MockReleases::list_url(), &mut |_, _| true).unwrap())
                .unwrap(),
        )
        .unwrap()
    })
    .map(|v: serde_json::Value| v[0].clone())
    .collect();
    let releases = parse_releases(&serde_json::Value::Array(list).to_string()).unwrap();
    assert_eq!(releases.len(), 6, "a tag that is not a version is left out");
    let current = v("1.2.3");
    let newest = |pre| newest_release(&releases, &current, pre).map(|r| r.tag.as_str());
    assert_eq!(newest(false), Some("v1.3.0"));
    assert_eq!(newest(true), Some("v1.4.0-beta.1"));
    // Never the same or an older version.
    assert_eq!(newest_release(&releases, &v("1.3.0"), false), None);
    assert_eq!(newest_release(&releases, &v("2.0.0"), true), None);
    // `/releases/latest` answers one object.
    let one = parse_releases(r#"{"tag_name": "v2.0.0", "body": "x", "assets": []}"#).unwrap();
    assert_eq!(one[0].version, v("2.0.0"));
    assert_eq!(one[0].name, "v2.0.0");
    assert!(parse_releases("not json").is_err());
}

#[test]
fn release_notes_become_blocks_to_draw() {
    let blocks = release_notes_blocks(
        "# Open Bible 1.3\n\nFixes **two** bugs and [a crash](https://x).\n\n- Notes are kept\n- Search is faster\n\n```\ncode\n```\n",
    );
    assert_eq!(
        blocks,
        [
            NotesBlock::Heading("Open Bible 1.3".into()),
            NotesBlock::Paragraph("Fixes two bugs and a crash.".into()),
            NotesBlock::Item("Notes are kept".into()),
            NotesBlock::Item("Search is faster".into()),
            NotesBlock::Code("code".into()),
        ]
    );
}

#[test]
fn with_automatic_checks_off_a_launch_makes_no_request() {
    let lab = Lab::new("auto-off");
    let releases = published(&[release("v1.3.0")]);
    let mut s = lab.launch(&releases, JobMode::Inline);
    s.run("updates.auto_check", &json!({ "on": false }))
        .unwrap();
    drop(s);
    // The next launch reads the setting and stays offline.
    let mut s = lab.launch(&releases, JobMode::Inline);
    assert!(!s.updates().auto_check());
    assert_eq!(s.launch_update_check(), None);
    settle(&mut s);
    assert!(releases.requests().is_empty(), "{:?}", releases.requests());
    assert!(!s.updates().notice_shown());
    // Checking by hand still works.
    s.run("help.check_for_updates", &json!({})).unwrap();
    settle(&mut s);
    assert_eq!(releases.requests(), [MockReleases::latest_url()]);
}

#[test]
fn with_automatic_checks_on_a_launch_checks_once_off_the_calling_thread() {
    let lab = Lab::new("auto-on");
    let releases = published(&[release("v1.3.0")]);
    let mut s = lab.launch(&releases, JobMode::Threads(1));
    assert!(s.updates().auto_check(), "on by default");
    assert!(s.updates().launch_check_pending());
    // A slow network: the request waits, the caller does not.
    releases.hold(true);
    let started = std::time::Instant::now();
    assert!(s.launch_update_check().is_some());
    assert!(started.elapsed() < std::time::Duration::from_millis(500));
    // Once per launch, however often it is asked.
    assert_eq!(s.launch_update_check(), None);
    assert!(matches!(
        s.updates().status(),
        UpdateStatus::Checking { manual: false }
    ));
    releases.hold(false);
    settle(&mut s);
    assert_eq!(releases.requests(), [MockReleases::latest_url()]);
    assert_ne!(releases.request_threads()[0], std::thread::current().id());
    match s.updates().status() {
        UpdateStatus::Available(r) => assert_eq!(r.tag, "v1.3.0"),
        other => panic!("{other:?}"),
    }
    assert!(s.updates().notice_shown());
    assert!(s.updates().first_run_note());
    s.run("updates.explained", &json!({})).unwrap();
    drop(s);
    let s = lab.launch(&releases, JobMode::Inline);
    assert!(!s.updates().first_run_note());
}

#[test]
fn prereleases_are_offered_only_when_included() {
    let lab = Lab::new("pre");
    let mut beta = release("v1.4.0-rc.1");
    beta.prerelease = true;
    let releases = published(&[release("v1.3.0"), beta]);
    let mut s = lab.launch(&releases, JobMode::Inline);
    s.check_for_updates(true);
    settle(&mut s);
    assert!(matches!(s.updates().status(), UpdateStatus::Available(r) if r.tag == "v1.3.0"));
    s.run("updates.prereleases", &json!({ "on": true }))
        .unwrap();
    s.check_for_updates(true);
    settle(&mut s);
    assert!(matches!(s.updates().status(), UpdateStatus::Available(r) if r.tag == "v1.4.0-rc.1"));
    assert_eq!(
        releases.requests(),
        [MockReleases::latest_url(), MockReleases::list_url()]
    );
}

#[test]
fn skip_this_version_persists_and_quiets_the_launch_check_for_that_version_only() {
    let lab = Lab::new("skip");
    let releases = published(&[release("v1.3.0")]);
    let mut s = lab.launch(&releases, JobMode::Inline);
    s.launch_update_check();
    settle(&mut s);
    assert!(s.updates().notice_shown());
    s.run("update.skip", &json!({})).unwrap();
    assert!(!s.updates().notice_shown());
    drop(s);

    let mut s = lab.launch(&releases, JobMode::Inline);
    assert_eq!(s.updates().skipped(), Some("v1.3.0"));
    s.launch_update_check();
    settle(&mut s);
    assert!(
        !s.updates().notice_shown(),
        "a skipped version is not offered at launch"
    );
    // Help → Check for updates still shows it.
    s.check_for_updates(true);
    settle(&mut s);
    assert!(s.updates().notice_shown());
    drop(s);

    // A newer version than the skipped one is offered again.
    let releases = published(&[release("v1.3.0"), release("v1.4.0")]);
    let mut s = lab.launch(&releases, JobMode::Inline);
    s.launch_update_check();
    settle(&mut s);
    assert!(s.updates().notice_shown());
}

#[test]
fn up_to_date_offline_and_rate_limited_checks_say_so_in_plain_words() {
    let lab = Lab::new("plain");
    let releases = published(&[release("v1.2.3")]);
    let mut s = lab.launch(&releases, JobMode::Inline);
    s.check_for_updates(true);
    settle(&mut s);
    assert_eq!(s.updates().status(), &UpdateStatus::UpToDate);
    assert!(s.updates().notice_shown());

    releases.fail(
        &MockReleases::latest_url(),
        FetchError::Network("failed to lookup address information".into()),
    );
    s.check_for_updates(true);
    settle(&mut s);
    let UpdateStatus::Failed { error, release } = s.updates().status().clone() else {
        panic!("{:?}", s.updates().status());
    };
    assert_eq!((error.clone(), release), (UpdateError::Offline, None));
    assert!(error.message().starts_with("Could not reach GitHub"));
    assert_eq!(s.updates().download_page(), MockReleases::source().page);

    releases.fail(&MockReleases::latest_url(), FetchError::RateLimited);
    s.check_for_updates(true);
    settle(&mut s);
    assert!(matches!(
        s.updates().status(),
        UpdateStatus::Failed {
            error: UpdateError::RateLimited,
            ..
        }
    ));
    assert!(UpdateError::RateLimited.message().contains("limiting"));

    // No release published yet.
    releases.fail(&MockReleases::latest_url(), FetchError::NotFound);
    s.check_for_updates(true);
    settle(&mut s);
    assert!(matches!(
        s.updates().status(),
        UpdateStatus::Failed {
            error: UpdateError::NoRelease,
            ..
        }
    ));

    // A launch check that cannot reach GitHub stays quiet.
    let mut s = lab.launch(&releases, JobMode::Inline);
    releases.fail(
        &MockReleases::latest_url(),
        FetchError::Network("offline".into()),
    );
    s.launch_update_check();
    settle(&mut s);
    assert_eq!(s.updates().status(), &UpdateStatus::Idle);
    assert!(!s.updates().notice_shown());
}

#[test]
fn check_for_updates_while_the_launch_check_runs_explains_its_failure() {
    let lab = Lab::new("upgrade");
    let releases = published(&[release("v1.3.0")]);
    let mut s = lab.launch(&releases, JobMode::Threads(1));
    // A slow network that then fails.
    releases.hold(true);
    assert!(s.launch_update_check().is_some());
    // Help → Check for updates while it runs: no second request, but it is now the user's.
    s.run("help.check_for_updates", &json!({})).unwrap();
    assert!(s.updates().notice_shown());
    assert_eq!(
        s.updates().status(),
        &UpdateStatus::Checking { manual: true }
    );
    releases.fail(
        &MockReleases::latest_url(),
        FetchError::Network("failed to lookup address information".into()),
    );
    releases.hold(false);
    settle(&mut s);
    assert_eq!(
        s.updates().status(),
        &UpdateStatus::Failed {
            error: UpdateError::Offline,
            release: None
        }
    );
    assert!(s.updates().notice_shown());
    assert_eq!(releases.requests(), [MockReleases::latest_url()]);
}

#[test]
fn replaced_files_a_failed_rollback_left_are_kept_and_block_another_update() {
    if platform_target().is_none() {
        return;
    }
    let lab = Lab::new("rollback-kept");
    installed_archive(&lab.app(), b"old app");
    let install = Install::of_exe(&lab.app().join(app_exe_name()));
    assert_eq!(install.check_updatable(), Ok(()));
    let work = lab.app().join(UPDATE_DIR);
    std::fs::create_dir_all(work.join("old")).unwrap();
    std::fs::create_dir_all(work.join("new")).unwrap();
    std::fs::write(work.join("old").join(app_exe_name()), b"older app").unwrap();
    std::fs::write(work.join(ROLLBACK_INCOMPLETE), b"could not put back").unwrap();
    assert_eq!(
        install.check_updatable(),
        Err(UpdateError::RollbackIncomplete(work.join("old")))
    );
    assert!(
        UpdateError::RollbackIncomplete(work.join("old"))
            .message()
            .contains("could not be put back")
    );
    // The launch clean-up keeps them and removes only the unpacked download.
    clean_up_update_folder(&lab.app());
    assert_eq!(
        std::fs::read(work.join("old").join(app_exe_name())).unwrap(),
        b"older app"
    );
    assert!(!work.join("new").exists());
}

/// Check, then Update now, and let it finish.
fn update(s: &mut Session) {
    s.check_for_updates(true);
    settle(s);
    assert!(
        matches!(s.updates().status(), UpdateStatus::Available(_)),
        "{:?}",
        s.updates().status()
    );
    s.run("update.install", &json!({})).unwrap();
    settle(s);
}

fn kept_update_backups(s: &Session) -> usize {
    (s.kept_backups().iter())
        .filter(|b| b.kind == obs_store::BackupKind::BeforeUpdate)
        .count()
}

#[test]
fn update_now_backs_up_then_replaces_the_installed_copy_and_nothing_else() {
    if platform_target().is_none() {
        return;
    }
    let lab = Lab::new("install");
    installed_archive(&lab.app(), b"old app");
    let next = release("v1.3.0");
    let releases = published(std::slice::from_ref(&next));
    let mut s = lab.launch(&releases, JobMode::Threads(1));
    s.run("updates.prereleases", &json!({ "on": false }))
        .unwrap();
    let data_before = lab.data(&s);
    assert_eq!(s.updates().cannot_update(), None);
    update(&mut s);
    let UpdateStatus::Installed {
        release,
        exe,
        backup,
        verification,
    } = s.updates().status().clone()
    else {
        panic!("{:?}", s.updates().status());
    };
    assert_eq!(release.tag, "v1.3.0");
    assert_eq!(verification, Verification::ChecksumOnly);
    assert_eq!(exe, lab.app().join(app_exe_name()));
    // The new copy is in place: executable, Starter library and the rest.
    assert_eq!(std::fs::read(&exe).unwrap(), b"new app");
    let manifest =
        std::fs::read_to_string(lab.app().join("starter/starter-manifest.json")).unwrap();
    assert!(!manifest.contains("old"), "{manifest}");
    assert_eq!(
        Install::of_exe(&exe).kind,
        InstallKind::Archive,
        "the new copy can update itself too"
    );
    // The old one waits beside it until the next launch.
    let old = lab.app().join(UPDATE_DIR).join("old");
    assert_eq!(std::fs::read(old.join(app_exe_name())).unwrap(), b"old app");
    // Requests: the check, then the checksums and the archive for this platform only.
    let (name, _) = next.archive();
    assert_eq!(
        releases.requests(),
        [
            MockReleases::latest_url(),
            MockReleases::asset_url("v1.3.0", CHECKSUMS_ASSET),
            MockReleases::asset_url("v1.3.0", name),
        ]
    );
    // A Before-update backup was taken; the Library, the User database and the settings are
    // as they were.
    let backup = backup.unwrap();
    assert!(backup.is_file());
    assert_eq!(kept_update_backups(&s), 1);
    assert_eq!(lab.data(&s), data_before);
    // A later check (the launch check) leaves the installed update waiting for Restart now.
    assert_eq!(s.check_for_updates(false), None);
    assert!(matches!(
        s.updates().status(),
        UpdateStatus::Installed { .. }
    ));
    // Restart now starts the new executable.
    s.workspace.take_effects();
    s.run("update.restart", &json!({})).unwrap();
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::Restart(exe.clone()))
    );
    // The next launch removes what was replaced.
    clean_up_update_folder(&lab.app());
    assert!(!lab.app().join(UPDATE_DIR).exists());
    assert_eq!(std::fs::read(&exe).unwrap(), b"new app");
}

/// The installed copy, byte for byte.
fn installed_files(app: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(app).unwrap().filter_map(Result::ok) {
        let p = e.path();
        if p.is_file() {
            out.push((
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read(&p).unwrap(),
            ));
        }
    }
    out.push((
        "starter-manifest".into(),
        std::fs::read(app.join("starter/starter-manifest.json")).unwrap(),
    ));
    out.sort();
    out
}

#[test]
fn a_tampered_download_is_rejected_and_the_installed_copy_kept() {
    if platform_target().is_none() {
        return;
    }
    let lab = Lab::new("tampered");
    installed_archive(&lab.app(), b"old app");
    let next = release("v1.3.0");
    let releases = published(std::slice::from_ref(&next));
    // Someone swapped the archive after SHA256SUMS was written.
    let (name, _) = next.archive();
    let target = platform_target().unwrap();
    releases.serve(
        &MockReleases::asset_url("v1.3.0", name),
        release_archive_bytes(&archive_asset_name("v1.3.0", target), b"evil app"),
    );
    let mut s = lab.launch(&releases, JobMode::Inline);
    let before = installed_files(&lab.app());
    update(&mut s);
    let UpdateStatus::Failed { error, release } = s.updates().status().clone() else {
        panic!("{:?}", s.updates().status());
    };
    assert_eq!(error, UpdateError::ChecksumMismatch);
    assert!(error.message().contains("Nothing was changed"));
    assert_eq!(
        release.unwrap().tag,
        "v1.3.0",
        "Try again and the download page stay"
    );
    assert!(s.updates().notice_shown());
    assert_eq!(installed_files(&lab.app()), before);
    assert!(
        !lab.app().join(UPDATE_DIR).exists(),
        "nothing unpacked is left"
    );
    assert_eq!(
        kept_update_backups(&s),
        0,
        "no backup before a failed download"
    );
}

#[test]
fn a_release_without_checksums_or_with_foreign_asset_urls_is_not_installed() {
    if platform_target().is_none() {
        return;
    }
    let lab = Lab::new("no-sums");
    installed_archive(&lab.app(), b"old app");
    let mut next = release("v1.3.0");
    next.assets.retain(|(n, _)| n != CHECKSUMS_ASSET);
    let releases = published(&[next]);
    let mut s = lab.launch(&releases, JobMode::Inline);
    update(&mut s);
    assert!(matches!(
        s.updates().status(),
        UpdateStatus::Failed {
            error: UpdateError::NoChecksums,
            ..
        }
    ));
    // An asset served from anywhere but the release downloads is never fetched.
    let next = release("v1.4.0");
    let mut json: serde_json::Value = serde_json::from_slice(
        &published(std::slice::from_ref(&next))
            .get(&MockReleases::latest_url(), &mut |_, _| true)
            .unwrap(),
    )
    .unwrap();
    json["assets"][0]["browser_download_url"] = json!("https://elsewhere.invalid/app.tar.gz");
    releases.serve(&MockReleases::latest_url(), json.to_string());
    let asked = releases.requests().len();
    update(&mut s);
    assert!(matches!(
        s.updates().status(),
        UpdateStatus::Failed {
            error: UpdateError::NoDownload,
            ..
        }
    ));
    assert_eq!(releases.requests().len(), asked + 1, "only the check");
    assert_eq!(
        std::fs::read(lab.app().join(app_exe_name())).unwrap(),
        b"old app"
    );
}

#[test]
fn cancel_stops_the_download_and_changes_nothing() {
    if platform_target().is_none() {
        return;
    }
    let lab = Lab::new("cancel");
    installed_archive(&lab.app(), b"old app");
    let releases = published(&[release("v1.3.0")]);
    let mut s = lab.launch(&releases, JobMode::Threads(1));
    s.check_for_updates(true);
    settle(&mut s);
    releases.hold(true);
    s.run("update.install", &json!({})).unwrap();
    assert!(matches!(
        s.updates().status(),
        UpdateStatus::Downloading { .. }
    ));
    s.run("update.cancel", &json!({})).unwrap();
    releases.hold(false);
    settle(&mut s);
    let UpdateStatus::Failed { error, .. } = s.updates().status().clone() else {
        panic!("{:?}", s.updates().status());
    };
    assert_eq!(error, UpdateError::Cancelled);
    assert_eq!(error.message(), "Update cancelled. Nothing was changed.");
    assert_eq!(
        std::fs::read(lab.app().join(app_exe_name())).unwrap(),
        b"old app"
    );
    // Later puts the release back on offer; Update now works then.
    s.run("update.later", &json!({})).unwrap();
    assert!(matches!(s.updates().status(), UpdateStatus::Available(_)));
    s.run("update.install", &json!({})).unwrap();
    settle(&mut s);
    assert!(matches!(
        s.updates().status(),
        UpdateStatus::Installed { .. }
    ));
}

#[test]
fn a_copy_not_installed_from_an_archive_only_points_to_the_download_page() {
    let lab = Lab::new("not-archive");
    installed_archive(&lab.app(), b"old app");
    std::fs::remove_file(lab.app().join(INSTALL_MARKER)).unwrap();
    let releases = published(&[release("v1.3.0")]);
    let mut s = lab.launch(&releases, JobMode::Inline);
    assert_eq!(s.updates().cannot_update(), Some(UpdateError::NotUpdatable));
    update(&mut s);
    assert!(matches!(
        s.updates().status(),
        UpdateStatus::Failed {
            error: UpdateError::NotUpdatable,
            ..
        }
    ));
    assert!(
        UpdateError::NotUpdatable
            .message()
            .contains("download page")
    );
    assert_eq!(
        s.updates().download_page(),
        format!("{MOCK_UPDATE_BASE}/releases/tag/v1.3.0")
    );
    assert_eq!(releases.requests().len(), 1, "nothing downloaded");
}

use crate::testing::MOCK_UPDATE_BASE;

#[cfg(unix)]
#[test]
fn a_folder_without_write_permission_is_not_touched() {
    use std::os::unix::fs::PermissionsExt;
    if platform_target().is_none() {
        return;
    }
    let lab = Lab::new("read-only");
    installed_archive(&lab.app(), b"old app");
    let releases = published(&[release("v1.3.0")]);
    let mut s = lab.launch(&releases, JobMode::Inline);
    std::fs::set_permissions(lab.app(), std::fs::Permissions::from_mode(0o555)).unwrap();
    // Root may write anywhere; then there is nothing to test.
    if std::fs::write(lab.app().join("probe"), b"").is_ok() {
        std::fs::set_permissions(lab.app(), std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    assert_eq!(
        s.updates().cannot_update(),
        Some(UpdateError::NotWritable(lab.app()))
    );
    update(&mut s);
    let failed = s.updates().status().clone();
    std::fs::set_permissions(lab.app(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let UpdateStatus::Failed { error, .. } = failed else {
        panic!("{failed:?}");
    };
    assert_eq!(error, UpdateError::NotWritable(lab.app()));
    assert!(
        error.message().contains("permission"),
        "{}",
        error.message()
    );
    assert_eq!(
        std::fs::read(lab.app().join(app_exe_name())).unwrap(),
        b"old app"
    );
}

#[test]
fn never_a_downgrade_or_the_same_version() {
    let lab = Lab::new("downgrade");
    installed_archive(&lab.app(), b"old app");
    let releases = published(&[release("v1.2.3")]);
    let mut s = lab.launch(&releases, JobMode::Inline);
    s.check_for_updates(true);
    settle(&mut s);
    assert_eq!(s.updates().status(), &UpdateStatus::UpToDate);
    assert!(s.run("update.install", &json!({})).is_err());
    s.set_current_version(v("1.4.0"));
    let releases = published(&[release("v1.3.0")]);
    s.set_fetch(releases.clone());
    s.check_for_updates(true);
    settle(&mut s);
    assert_eq!(s.updates().status(), &UpdateStatus::UpToDate);
}

#[test]
fn a_script_can_run_update_now_and_restart_before_the_check_answers() {
    if platform_target().is_none() {
        return;
    }
    let lab = Lab::new("queued");
    installed_archive(&lab.app(), b"old app");
    let releases = published(&[release("v1.3.0")]);
    let mut s = lab.launch(&releases, JobMode::Threads(1));
    releases.hold(true);
    s.run("help.check_for_updates", &json!({})).unwrap();
    s.run("update.install", &json!({})).unwrap();
    s.run("update.restart", &json!({})).unwrap();
    s.workspace.take_effects();
    releases.hold(false);
    settle(&mut s);
    settle(&mut s);
    assert!(matches!(
        s.updates().status(),
        UpdateStatus::Installed { .. }
    ));
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::Restart(lab.app().join(app_exe_name())))
    );
}

/// The signing fixtures: `SHA256SUMS` over `archive.bin` (as `openbible-v9.9.9-test-target.tar.gz`),
/// signed with a throwaway minisign key whose public half is `test-release.pub`.
fn signing_fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/update-signing")
            .join(name),
    )
    .unwrap()
}

#[test]
fn a_signed_checksum_file_is_verified_with_the_compiled_in_key() {
    let archive = signing_fixture("archive.bin");
    let sums = signing_fixture("SHA256SUMS");
    let sig = signing_fixture("SHA256SUMS.minisig");
    let key = String::from_utf8(signing_fixture("test-release.pub")).unwrap();
    let key = key.lines().nth(1).unwrap();
    let name = "openbible-v9.9.9-test-target.tar.gz";
    assert_eq!(
        verify_archive(name, &archive, &sums, Some(&sig), Some(key)),
        Ok(Verification::Signed)
    );
    // Without a key, the checksum alone.
    assert_eq!(
        verify_archive(name, &archive, &sums, None, None),
        Ok(Verification::ChecksumOnly)
    );
    // A tampered archive with a SHA256SUMS rewritten to match: the signature catches it.
    let evil = b"evil".to_vec();
    let forged = format!("{}  {name}\n", crate::starter::sha256_hex(&evil));
    assert_eq!(
        verify_archive(name, &evil, forged.as_bytes(), Some(&sig), Some(key)),
        Err(UpdateError::BadSignature)
    );
    assert_eq!(
        verify_archive(name, &evil, &sums, Some(&sig), Some(key)),
        Err(UpdateError::ChecksumMismatch)
    );
    // With a key, a missing signature is refused.
    assert_eq!(
        verify_archive(name, &archive, &sums, None, Some(key)),
        Err(UpdateError::BadSignature)
    );
    assert_eq!(
        verify_archive("other.zip", &archive, &sums, None, None),
        Err(UpdateError::NoChecksums)
    );
    // No release key is compiled in yet (ADR 0017): SHA256SUMS only, and the app says so.
    assert_eq!(RELEASE_PUBLIC_KEY, None);
    assert!(
        Verification::ChecksumOnly
            .describe()
            .contains("not set up yet")
    );
}

#[test]
fn an_archive_whose_files_would_land_outside_its_folder_is_refused() {
    let lab = Lab::new("traversal");
    let install = installed_archive(&lab.app(), b"old app");
    // A tar entry named `../escaped` (written raw: the tar crate refuses such names).
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(4);
    header.set_mode(0o644);
    {
        let raw = header.as_old_mut();
        raw.name[..10].copy_from_slice(b"../escaped");
    }
    header.set_cksum();
    let mut data = header.as_bytes().to_vec();
    data.extend_from_slice(b"evil");
    data.resize(data.len() + 508 + 1024, 0);
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut gz, &data).unwrap();
    let bytes = gz.finish().unwrap();
    let err = stage_update(&install, "x.tar.gz", &bytes).unwrap_err();
    assert!(matches!(err, UpdateError::BadArchive(_)), "{err:?}");
    assert!(!lab.app().join(UPDATE_DIR).join("escaped").exists());
    assert!(!lab.app().join("escaped").exists());
    // An archive without the app is refused too.
    let empty = release_archive_bytes("openbible-v1-x.tar.gz", b"app");
    let mut wrong = install.clone();
    wrong.exe = lab.app().join("other-app");
    assert!(matches!(
        stage_update(&wrong, "openbible-v1-x.tar.gz", &empty),
        Err(UpdateError::BadArchive(m)) if m.contains("other-app")
    ));
    assert!(!lab.app().join(UPDATE_DIR).exists());
}

#[test]
fn the_check_asks_for_public_release_metadata_only() {
    let source = UpdateSource::default();
    assert_eq!(
        source.check_url(false),
        "https://api.github.com/repos/inhesrom/openbiblesoftware/releases/latest"
    );
    assert_eq!(
        source.check_url(true),
        "https://api.github.com/repos/inhesrom/openbiblesoftware/releases?per_page=20"
    );
    assert!(source.serves(
        "https://github.com/inhesrom/openbiblesoftware/releases/download/v1.0.0/SHA256SUMS"
    ));
    assert!(!source.serves("https://github.com/someone/else/releases/download/v1/x"));
    assert!(
        !source.serves("https://github.com/inhesrom/openbiblesoftware/releases/download/../../x")
    );
    assert_eq!(
        archive_asset_name("v1.0.0", "x86_64-pc-windows-msvc"),
        "openbible-v1.0.0-x86_64-pc-windows-msvc.zip"
    );
    assert_eq!(
        archive_asset_name("v1.0.0", "aarch64-apple-darwin"),
        "openbible-v1.0.0-aarch64-apple-darwin.tar.gz"
    );
}

/// `old/` held so its files cannot be removed (a read-only folder in it), as Windows holds
/// the old executable while the version that ran Restart now is still closing.
#[cfg(unix)]
fn held_old_folder(lab: &Lab) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let work = lab.app().join(UPDATE_DIR);
    let held = work.join("old").join("held");
    std::fs::create_dir_all(&held).unwrap();
    std::fs::write(held.join(app_exe_name()), b"old app, still running").unwrap();
    std::fs::create_dir_all(work.join("new")).unwrap();
    std::fs::set_permissions(&held, std::fs::Permissions::from_mode(0o555)).unwrap();
    held
}

#[cfg(unix)]
fn release_held(held: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(held, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn the_launch_clean_up_waits_for_the_old_version_to_let_go_of_its_files() {
    let lab = Lab::new("clean-up-waits");
    installed_archive(&lab.app(), b"new app");
    let held = held_old_folder(&lab);
    // Root may delete anything; then there is nothing to test.
    if std::fs::remove_file(held.join(app_exe_name())).is_ok() {
        release_held(&held);
        return;
    }
    let work = lab.app().join(UPDATE_DIR);
    let letting_go = {
        let held = held.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(400));
            release_held(&held);
        })
    };
    let started = std::time::Instant::now();
    clean_up_update_folder_at_launch(&lab.app(), std::time::Duration::from_secs(20));
    letting_go.join().unwrap();
    assert!(!work.exists(), "removed once the old version let go");
    assert!(started.elapsed() >= std::time::Duration::from_millis(400));
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(
        std::fs::read(lab.app().join(app_exe_name())).unwrap(),
        b"new app"
    );
}

#[cfg(unix)]
#[test]
fn the_launch_clean_up_gives_up_after_its_patience_and_leaves_the_rest_for_a_later_launch() {
    let lab = Lab::new("clean-up-gives-up");
    installed_archive(&lab.app(), b"new app");
    let held = held_old_folder(&lab);
    if std::fs::remove_file(held.join(app_exe_name())).is_ok() {
        release_held(&held);
        return;
    }
    let work = lab.app().join(UPDATE_DIR);
    let started = std::time::Instant::now();
    clean_up_update_folder_at_launch(&lab.app(), std::time::Duration::from_millis(500));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    // The unpacked download went at the first try; the held files stay for a later launch.
    assert!(!work.join("new").exists());
    assert!(held.join(app_exe_name()).exists());
    release_held(&held);
    clean_up_update_folder_at_launch(&lab.app(), std::time::Duration::from_millis(500));
    assert!(!work.exists());
}

#[cfg(unix)]
#[test]
fn the_launch_clean_ups_retries_never_remove_an_update_this_process_is_unpacking() {
    let lab = Lab::new("clean-up-spares-new");
    installed_archive(&lab.app(), b"new app");
    let held = held_old_folder(&lab);
    if std::fs::remove_file(held.join(app_exe_name())).is_ok() {
        release_held(&held);
        return;
    }
    let work = lab.app().join(UPDATE_DIR);
    // After the first try, while the retries wait, Update now unpacks into `new/`.
    let unpacking = {
        let (work, held) = (work.clone(), held.clone());
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            std::fs::create_dir_all(work.join("new/openbible-next")).unwrap();
            std::fs::write(work.join("new/openbible-next/openbible"), b"next app").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(300));
            release_held(&held);
        })
    };
    clean_up_update_folder_at_launch(&lab.app(), std::time::Duration::from_secs(20));
    unpacking.join().unwrap();
    assert!(!work.join("old").exists());
    assert_eq!(
        std::fs::read(work.join("new/openbible-next/openbible")).unwrap(),
        b"next app"
    );
}
