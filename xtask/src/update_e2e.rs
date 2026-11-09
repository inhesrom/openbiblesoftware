//! `cargo xtask update-e2e` (#276): Update now end to end, with the real app, on this OS,
//! clicked with pointer events and photographed.
//!
//! Against a mock release source (the default):
//!
//! 1. Builds the app twice from this tree: as this version (vN) and, with
//!    `OBS_BUILD_VERSION`, as the next patch version (vN+1).
//! 2. Packs each with a one-Bible Starter library into a Release archive
//!    (`cargo xtask release-archive`'s code) and unpacks vN into `target/update-e2e/install/`.
//! 3. Seeds the data folders: a User database with a preference and a Resource in the Library.
//! 4. Serves vN+1 from a mock release source on localhost (the releases API's
//!    `/releases/latest`, the archive, slowed so the download shows its progress, and
//!    `SHA256SUMS`), and runs vN with `OBS_UPDATE_URL` pointing at it. Its own launch check
//!    finds vN+1; `OBS_STEPS` clicks **Update now** and **Restart now** with pointer events
//!    at the buttons (found by their labels on screen) and takes screenshots of the notice,
//!    the download's progress and the installed update.
//! 5. When the swap is logged, while vN still runs, checks that its executable was renamed
//!    aside into `.openbible-update/old/` and vN+1 is in its place, then lets Restart now go.
//! 6. Waits for vN to close and vN+1 to start from the same folder (its log line). vN+1
//!    (with `OBS_RELAUNCH_*` as its automation) removes `.openbible-update` once vN has let go
//!    of its executable, runs its own check (nothing newer) and is photographed with Help →
//!    About; then it is stopped, and the installed copy must be vN+1, a Before-update backup
//!    taken, and the User database and the Library as they were.
//!
//! With `--real-release X.Y.Z`: one build, as 0.0.9, updates itself from this repository's
//! real published release X.Y.Z on GitHub (`OBS_UPDATE_URL` unset; no release key is
//! compiled in): the asset for this platform, its line in the real `SHA256SUMS`, the swap and
//! the relaunch into X.Y.Z, whose own start line in the log is checked (releases before 0.1.1
//! have no `--version`) and whose `OBS_SHOT` screenshot closes it. Run on demand only (the
//! `update real release` workflow), to stay clear of GitHub's rate limits.
//!
//! Screenshots are written as PNG to `target/update-e2e/shots/`. Needs a display (Linux CI:
//! `xvfb-run`). The app's network transport is the real one.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use obs_engine::starter::{MANIFEST_FILE, sha256_hex};
use obs_engine::updates::{APP_VERSION, CHECKSUMS_ASSET, UPDATE_DIR, Version, platform_target};
use serde_json::json;

use crate::release::{self, ArchiveSpec, Format};

const USAGE: &str = "usage: cargo xtask update-e2e [--real-release X.Y.Z]

Builds the app as this version and as the next patch version, installs the first from a
Release archive, serves the second from a mock release source on localhost, clicks Update
now and Restart now in the running app, and checks that the next version runs from the
same folder with the user's data intact. Screenshots go to target/update-e2e/shots/.

--real-release X.Y.Z  instead build this tree as 0.0.9 and update it from this repository's
                      real published release X.Y.Z on GitHub (network; on demand only).

Needs a display (CI: xvfb-run).";

/// How long each step may take before the run fails.
const STEP_TIMEOUT: Duration = Duration::from_secs(240);
/// The same with a real download from GitHub (about 180 MB).
const REAL_STEP_TIMEOUT: Duration = Duration::from_secs(900);
/// How long the mock source takes to serve the archive, so its progress can be seen.
const MOCK_DOWNLOAD_TIME: Duration = Duration::from_secs(8);
/// The version the real-release run builds, older than any published release.
const REAL_RELEASE_FROM: &str = "0.0.9";
/// The resource id of the e2e Starter library's one Bible.
const STARTER_ID: &str = "e2e-bible";
/// The preference the e2e seeds and checks.
const MARKER_PREF: &str = "e2e.marker";
/// The log line `swap_in` writes once the new files are in place.
const SWAPPED: &str = "update installed;";
/// The log lines of an Update check or Update now that failed.
const UPDATE_FAILED: [&str; 4] = [
    "update check failed",
    "update not started",
    "update failed",
    "update not installed",
];
/// The log line of the launch clean-up once `.openbible-update` is gone.
const CLEANED_UP: &str = "removed the files the last update replaced";

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    match args {
        [] => mock_release(root),
        [flag, version] if flag == "--real-release" => {
            let to = Version::parse(version).context("--real-release takes a version, X.Y.Z")?;
            real_release(root, &to)
        }
        [other, ..] => bail!("unknown option `{other}`\n\n{USAGE}"),
    }
}

/// The run's folders under `target/update-e2e/`, emptied.
struct Work {
    dir: PathBuf,
    shots: PathBuf,
    /// Created once the swap is checked: Restart now's go-ahead.
    go: PathBuf,
}

impl Work {
    fn new(root: &Path) -> Result<Self> {
        let dir = root.join("target/update-e2e");
        if dir.exists() {
            fs::remove_dir_all(&dir).with_context(|| format!("{}", dir.display()))?;
        }
        let shots = dir.join("shots");
        fs::create_dir_all(&shots)?;
        Ok(Self {
            go: dir.join("restart.go"),
            shots,
            dir,
        })
    }

    fn shot(&self, name: &str) -> PathBuf {
        self.shots.join(format!("{name}.ppm"))
    }
}

/// The user's data: a Library with one Resource and a User database with a preference.
struct UserData {
    dir: PathBuf,
    resource: PathBuf,
    resource_bytes: Vec<u8>,
}

impl UserData {
    fn seed(work: &Work, starter: &Path) -> Result<Self> {
        let dir = work.dir.join("data");
        let library = dir.join("library");
        fs::create_dir_all(&library)?;
        let resource = library.join("users-own.obr");
        fs::copy(starter.join(format!("{STARTER_ID}.obr")), &resource)?;
        let resource_bytes = fs::read(&resource)?;
        let (db, _) = obs_store::UserStore::open(&dir.join("user.db"))?;
        db.set_preference(MARKER_PREF, "kept")?;
        Ok(Self {
            dir,
            resource,
            resource_bytes,
        })
    }

    fn logs(&self) -> PathBuf {
        self.dir.join("logs")
    }

    /// The User database kept its preference and has a Before-update backup; the Library is
    /// as it was.
    fn check_kept(&self) -> Result<()> {
        let (db, _) = obs_store::UserStore::open(&self.dir.join("user.db"))?;
        ensure!(
            db.preference(MARKER_PREF)?.as_deref() == Some("kept"),
            "the User database lost its data"
        );
        let backups = db.backups()?;
        ensure!(
            backups
                .iter()
                .any(|b| b.kind == obs_store::BackupKind::BeforeUpdate),
            "no Before-update backup was taken: {backups:?}"
        );
        ensure!(
            fs::read(&self.resource)? == self.resource_bytes,
            "the Library was changed"
        );
        Ok(())
    }
}

/// The installed app at `install/<archive folder>/`.
struct Installed {
    dir: PathBuf,
    app: PathBuf,
}

impl Installed {
    /// The old executable, renamed aside by the swap.
    fn renamed_aside(&self) -> PathBuf {
        self.dir
            .join(UPDATE_DIR)
            .join("old")
            .join(self.app.file_name().unwrap_or_default())
    }
}

/// Pack `bin` with `starter` as the Release archive of `version`.
fn archive(
    root: &Path,
    work: &Work,
    bin: &Path,
    starter: &Path,
    version: &Version,
) -> Result<PathBuf> {
    let target = platform_target().context("no Release archive is published for this platform")?;
    release::release_archive(&ArchiveSpec {
        bin: bin.to_path_buf(),
        name: format!("openbible-v{version}-{target}"),
        starter: starter.to_path_buf(),
        format: if cfg!(windows) {
            Format::Zip
        } else {
            Format::TarGz
        },
        out: work.dir.join("dist"),
        root: root.to_path_buf(),
    })
}

fn install(work: &Work, archive: &Path) -> Result<Installed> {
    let dir = release::unpack(archive, &work.dir.join("install"))?;
    let app = release::app_in(&dir)?;
    Ok(Installed { dir, app })
}

/// The clicks and screenshots of an update from `from` to `to` (`OBS_STEPS`): the notice its
/// launch check opens, Update now, the download's progress, the installed update, and Restart
/// now once the swap is checked (`work.go`).
fn update_steps(work: &Work, to: &Version, prefix: &str) -> String {
    let installed = format!("Open Bible Software {to} is installed");
    json!([
        {
            "when": format!("Open Bible Software {to} is available"),
            "shot": work.shot(&format!("{prefix}1-update-available")),
        },
        { "click": "Update now" },
        {
            "when": format!("Downloading Open Bible Software {to}"),
            "after_secs": 2.0,
            "skip_if": installed,
            "shot": work.shot(&format!("{prefix}2-downloading")),
        },
        { "when": installed, "shot": work.shot(&format!("{prefix}3-update-installed")) },
        { "click": "Restart now", "after_file": work.go },
    ])
    .to_string()
}

fn mock_release(root: &Path) -> Result<()> {
    platform_target().context("no Release archive is published for this platform")?;
    let old = Version::parse(APP_VERSION).context("this build's version is not semver")?;
    let new = Version {
        patch: old.patch + 1,
        pre: Vec::new(),
        ..old.clone()
    };
    let work = Work::new(root)?;

    // 1. Two builds of the app.
    let old_bin = build(root, Some(&old.to_string()), &work.dir.join("bin-old"))?;
    let new_bin = build(root, Some(&new.to_string()), &work.dir.join("bin-new"))?;

    // 2. Release archives, and vN installed from one.
    let starter = work.dir.join("starter");
    write_starter(&starter)?;
    let old_archive = archive(root, &work, &old_bin, &starter, &old)?;
    let new_archive = archive(root, &work, &new_bin, &starter, &new)?;
    let installed = install(&work, &old_archive)?;

    // 3. The user's data.
    let data = UserData::seed(&work, &starter)?;
    let logs = data.logs();

    // 4. The mock release source, and vN updating itself.
    let tag = format!("v{new}");
    let base = serve_release(&tag, &new_archive, MOCK_DOWNLOAD_TIME)?;
    eprintln!("update-e2e: serving {tag} at {base}");
    let restarted = work.shot("4-restarted");
    let mut cmd = command(&installed.app, &data.dir, Some(&base));
    cmd.env("OBS_STEPS", update_steps(&work, &new, ""))
        // vN+1's automation: Help → About, photographed once shown.
        .env("OBS_RELAUNCH_SCRIPT", "10 help.about")
        .env(
            "OBS_RELAUNCH_STEPS",
            json!([{ "when": "A Bible study app for open texts", "after_secs": 1.0, "shot": restarted }])
                .to_string(),
        );
    let child = cmd
        .spawn()
        .with_context(|| format!("could not launch {}", installed.app.display()))?;

    // 5. The swap, checked while vN runs; then Restart now.
    let swap = |child: &mut Child| -> Result<()> {
        ensure!(
            child.try_wait()?.is_none(),
            "v{old} exited before its swap was checked"
        );
        let aside = installed.renamed_aside();
        ensure!(
            fs::read(&aside).ok() == Some(fs::read(&old_bin)?),
            "the running v{old} executable was not renamed aside to {}",
            aside.display()
        );
        ensure!(
            fs::read(&installed.app)? == fs::read(&new_bin)?,
            "{} is not v{new} after the swap",
            installed.app.display()
        );
        eprintln!(
            "update-e2e: the running v{old} executable was renamed aside to {}, and v{new} is \
             in its place",
            aside.display()
        );
        Ok(())
    };
    run_until_restart(child, &work, &logs, STEP_TIMEOUT, &old, swap)?;

    // 6. vN+1 starts from the same folder, cleans up, checks (nothing newer), is
    //    photographed and stopped.
    let pid = wait_for(
        &logs,
        &format!("Open Bible Software {new} started"),
        None,
        STEP_TIMEOUT,
    )?;
    eprintln!("update-e2e: v{new} started (process {pid})");
    let steps = (|| {
        wait_for(&logs, CLEANED_UP, Some(pid), STEP_TIMEOUT)
            .with_context(|| format!("v{new} did not remove what the update replaced"))?;
        ensure!(
            !installed.dir.join(UPDATE_DIR).exists(),
            "{} is still there after v{new} started",
            installed.dir.join(UPDATE_DIR).display()
        );
        eprintln!(
            "update-e2e: v{new} removed {} with the old executable",
            installed.dir.join(UPDATE_DIR).display()
        );
        wait_for(&logs, "no update available", Some(pid), STEP_TIMEOUT)
            .with_context(|| format!("v{new} did not run its own Update check"))?;
        wait_for_file(&restarted, STEP_TIMEOUT)
    })();
    stop(pid);
    steps?;
    // Let the stopped process let go of its files.
    std::thread::sleep(Duration::from_secs(2));

    ensure!(
        fs::read(&installed.app)? == fs::read(&new_bin)?,
        "{} is not v{new}",
        installed.app.display()
    );
    let version = Command::new(&installed.app).arg("--version").output()?;
    let printed = String::from_utf8_lossy(&version.stdout).trim().to_string();
    ensure!(
        printed == format!("openbible {new}"),
        "{} --version printed {printed:?}",
        installed.app.display()
    );
    data.check_kept()?;
    let shots = convert_shots(&work)?;
    eprintln!(
        "update-e2e: v{old} updated itself to v{new} in {}, restarted into it, and kept the \
         User database and the Library; screenshots: {}",
        installed.dir.display(),
        shots.join(", ")
    );
    Ok(())
}

fn real_release(root: &Path, to: &Version) -> Result<()> {
    let target = platform_target().context("no Release archive is published for this platform")?;
    let from = Version::parse(REAL_RELEASE_FROM).context("REAL_RELEASE_FROM")?;
    ensure!(
        obs_engine::updates::RELEASE_PUBLIC_KEY.is_none(),
        "a release key is compiled in, and {to} was published before release signing: this \
         run checks the update from an unsigned release"
    );
    let work = Work::new(root)?;
    let bin = build(root, Some(REAL_RELEASE_FROM), &work.dir.join("bin-old"))?;
    let starter = work.dir.join("starter");
    write_starter(&starter)?;
    let installed = install(&work, &archive(root, &work, &bin, &starter, &from)?)?;
    let data = UserData::seed(&work, &starter)?;
    let logs = data.logs();

    let restarted = work.shot(&format!("real-4-restarted-{to}"));
    let mut cmd = command(&installed.app, &data.dir, None);
    cmd.env("OBS_STEPS", update_steps(&work, to, "real-"))
        // The release's own screenshot automation photographs it and closes it.
        .env("OBS_RELAUNCH_SHOT", &restarted)
        .env("OBS_RELAUNCH_SHOT_FRAME", "240");
    let child = cmd
        .spawn()
        .with_context(|| format!("could not launch {}", installed.app.display()))?;
    let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
    let asset = format!("openbible-v{to}-{target}.{ext}");
    let swap = |child: &mut Child| -> Result<()> {
        ensure!(
            child.try_wait()?.is_none(),
            "v{from} exited before its swap was checked"
        );
        let log = log_text(&logs);
        for line in [
            format!("an update is available: {to}"),
            format!("downloading the update {asset}"),
            format!("{asset} matches its SHA-256 in {CHECKSUMS_ASSET}"),
        ] {
            ensure!(log.contains(&line), "no log line {line:?}:\n{log}");
        }
        let aside = installed.renamed_aside();
        ensure!(
            fs::read(&aside).ok() == Some(fs::read(&bin)?),
            "the running v{from} executable was not renamed aside to {}",
            aside.display()
        );
        ensure!(
            fs::read(&installed.app)? != fs::read(&bin)?,
            "{} is still v{from} after the swap",
            installed.app.display()
        );
        let manifest = fs::read_to_string(installed.dir.join("starter").join(MANIFEST_FILE))?;
        ensure!(
            !manifest.contains(STARTER_ID),
            "the Starter library beside the app is not the release's"
        );
        eprintln!(
            "update-e2e: picked {asset}, matched it against the release's {CHECKSUMS_ASSET}, \
             renamed the running v{from} aside and put {to} in its place"
        );
        Ok(())
    };
    let ran = run_until_restart(child, &work, &logs, REAL_STEP_TIMEOUT, &from, swap);
    let relaunched = ran.and_then(|()| {
        // Its own start line (releases before 0.1.1 log no process id, nor have --version).
        wait_for(
            &logs,
            &format!("Open Bible Software {to} started on"),
            None,
            REAL_STEP_TIMEOUT,
        )?;
        eprintln!("update-e2e: {to} started from {}", installed.dir.display());
        wait_for_file(&restarted, STEP_TIMEOUT)
    });
    // It closes itself after its screenshot; make sure.
    std::thread::sleep(Duration::from_secs(5));
    stop_app_at(&installed.app);
    relaunched?;
    data.check_kept()?;
    let shots = convert_shots(&work)?;
    eprintln!(
        "update-e2e: v{from} updated itself from the published release {to} ({asset}) in {} \
         and restarted into it; screenshots: {}",
        installed.dir.display(),
        shots.join(", ")
    );
    Ok(())
}

/// Wait while `child` (version `from`) updates itself: once the swap is logged, `check_swap`
/// runs (the child still running) and Restart now gets its go-ahead; then the child must
/// close by itself, successfully.
fn run_until_restart(
    mut child: Child,
    work: &Work,
    logs: &Path,
    timeout: Duration,
    from: &Version,
    check_swap: impl Fn(&mut Child) -> Result<()>,
) -> Result<()> {
    let started = Instant::now();
    let mut swapped = false;
    let status = loop {
        if !swapped && log_text(logs).contains(SWAPPED) {
            if let Err(e) = check_swap(&mut child) {
                let _ = child.kill();
                return Err(e.context(log_text(logs)));
            }
            fs::write(&work.go, b"")?;
            swapped = true;
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        // The app keeps running after a failed update; no need to wait for the timeout.
        let log = log_text(logs);
        if let Some(line) = log
            .lines()
            .find(|l| UPDATE_FAILED.iter().any(|f| l.contains(f)))
        {
            let _ = child.kill();
            bail!("v{from} could not update: {line}\n{log}");
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            bail!(
                "v{from} did not update and restart within {}s; its log:\n{}",
                timeout.as_secs(),
                log_text(logs)
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    ensure!(
        status.success(),
        "v{from} exited with {status}:\n{}",
        log_text(logs)
    );
    ensure!(
        swapped,
        "v{from} closed without installing the update:\n{}",
        log_text(logs)
    );
    Ok(())
}

/// Build the app as `version` and copy it into `dir`.
fn build(root: &Path, version: Option<&str>, dir: &Path) -> Result<PathBuf> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(&cargo);
    cmd.args(["build", "--locked", "-p", "openbible"])
        .current_dir(root);
    match version {
        Some(v) => cmd.env("OBS_BUILD_VERSION", v),
        None => cmd.env_remove("OBS_BUILD_VERSION"),
    };
    let status = cmd.status().context("could not run cargo")?;
    ensure!(status.success(), "cargo build -p openbible failed");
    let name = format!("openbible{}", std::env::consts::EXE_SUFFIX);
    fs::create_dir_all(dir)?;
    let to = dir.join(&name);
    fs::copy(root.join("target/debug").join(&name), &to)?;
    Ok(to)
}

/// A Starter library of one tiny public-domain Bible (John 1:1, KJV wording).
fn write_starter(dir: &Path) -> Result<()> {
    use obs_formats::{UsfmImport, UsfmOptions};
    fs::create_dir_all(dir)?;
    let mut import = UsfmImport::new(UsfmOptions {
        resource_id: STARTER_ID.into(),
        title: "Update test Bible".into(),
        abbreviation: "UTB".into(),
        language: "en".into(),
        source_file: "e2e.usfm".into(),
        tool_version: "xtask update-e2e".into(),
        ..UsfmOptions::default()
    });
    import
        .add_book(
            "JHN.usfm",
            "\\id JHN\n\\c 1\n\\p\n\\v 1 In the beginning was the Word.\n".into(),
        )
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let (mut resource, _) = import.finish().map_err(|e| anyhow::anyhow!("{e}"))?;
    resource.manifest.license = "LicenseRef-PD".into();
    let obr = dir.join(format!("{STARTER_ID}.obr"));
    obs_store::write_obr(&resource, &obr)?;
    let bytes = fs::read(&obr)?;
    let manifest = json!({
        "format": 1,
        "resources": [{
            "resource_id": STARTER_ID,
            "revision": resource.manifest.revision,
            "content_hash": resource.manifest.content_hash,
            "file_sha256": sha256_hex(&bytes),
            "bytes": bytes.len(),
            "title": resource.manifest.title,
            "abbreviation": resource.manifest.abbreviation,
            "language": "en",
            "license": "LicenseRef-PD",
        }],
    });
    fs::write(dir.join(MANIFEST_FILE), manifest.to_string())?;
    Ok(())
}

/// Serve release `tag` with `archive` and its SHA256SUMS on localhost, as GitHub's releases
/// API and download URLs would (`OBS_UPDATE_URL`'s layout), the archive spread over
/// `download_time`; returns the base URL.
fn serve_release(tag: &str, archive: &Path, download_time: Duration) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let base = format!("http://{}", listener.local_addr()?);
    let name = archive
        .file_name()
        .context("archive name")?
        .to_string_lossy()
        .into_owned();
    let bytes = fs::read(archive)?;
    let sums = format!("{}  {name}\n", sha256_hex(&bytes));
    let url = |asset: &str| format!("{base}/download/{tag}/{asset}");
    let latest = json!({
        "tag_name": tag,
        "name": format!("Open Bible Software {tag}"),
        "body": "## Update test\n\n- Served from a mock release source on localhost.",
        "draft": false,
        "prerelease": false,
        "html_url": format!("{base}/releases/tag/{tag}"),
        "assets": [
            { "name": name, "size": bytes.len(), "browser_download_url": url(&name) },
            { "name": CHECKSUMS_ASSET, "size": sums.len(), "browser_download_url": url(CHECKSUMS_ASSET) },
        ],
    })
    .to_string();
    let archive_path = format!("/download/{tag}/{name}");
    let files: Arc<Vec<(String, Vec<u8>)>> = Arc::new(vec![
        ("/releases/latest".into(), latest.into_bytes()),
        (archive_path.clone(), bytes),
        (
            format!("/download/{tag}/{CHECKSUMS_ASSET}"),
            sums.into_bytes(),
        ),
    ]);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let files = Arc::clone(&files);
            let archive_path = archive_path.clone();
            std::thread::spawn(move || {
                let Ok(clone) = stream.try_clone() else {
                    return;
                };
                let mut reader = BufReader::new(clone);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h == "\r\n" || h.is_empty() {
                        break;
                    }
                }
                let path = line.split_whitespace().nth(1).unwrap_or_default();
                let found = files.iter().find(|(p, _)| p == path);
                let _ = match found {
                    Some((p, body)) => write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .and_then(|()| {
                        if *p != archive_path {
                            return stream.write_all(body);
                        }
                        // Slowly, so the download's progress shows.
                        const PARTS: u32 = 80;
                        let chunk = body.len().div_ceil(PARTS as usize).max(1);
                        for part in body.chunks(chunk) {
                            stream.write_all(part)?;
                            std::thread::sleep(download_time / PARTS);
                        }
                        Ok(())
                    }),
                    None => stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    ),
                };
            });
        }
    });
    Ok(base)
}

/// The app at `app` with every data path under `data` and the release source at `base`
/// (`None`: GitHub's).
fn command(app: &Path, data: &Path, base: Option<&str>) -> Command {
    let home = data.join("home");
    let _ = fs::create_dir_all(&home);
    let mut cmd = Command::new(app);
    cmd.current_dir(data)
        .env("OBS_LIBRARY", data.join("library"))
        .env("OBS_USER_DB", data.join("user.db"))
        .env("OBS_LOG_DIR", data.join("logs"))
        .env("OBS_SEARCH_CACHE", data.join("search-cache"))
        .env("OBS_ALIAS_CACHE", data.join("alias-cache"))
        .env("OBS_SWORD_CATALOG", data.join("sword-catalog.json"))
        .env("OBS_SWORD_DIR", data.join("sword"))
        .env("OBS_STARTER_URL", "file:///nonexistent/obs-update-e2e/")
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env_remove("OBS_STARTER_DIR")
        .env_remove("OBS_UPDATE_PUBLIC_KEY")
        .stdin(Stdio::null());
    for name in [
        "OBS_SCRIPT",
        "OBS_INPUT",
        "OBS_SHOT",
        "OBS_SHOT_FRAME",
        "OBS_STEPS",
    ] {
        cmd.env_remove(name);
    }
    match base {
        Some(base) => cmd.env("OBS_UPDATE_URL", base),
        None => cmd.env_remove("OBS_UPDATE_URL"),
    };
    cmd
}

fn log_text(logs: &Path) -> String {
    fs::read_to_string(logs.join(obs_engine::diagnostics::LOG_FILE)).unwrap_or_default()
}

/// Wait for a log line containing `text` (of launch `pid`, when given); returns the process
/// id of the launch it belongs to (the last "started … (process N)" line before it).
fn wait_for(logs: &Path, text: &str, pid: Option<u32>, timeout: Duration) -> Result<u32> {
    let started = Instant::now();
    loop {
        let log = log_text(logs);
        let mut of = None;
        for line in log.lines() {
            if let Some(p) = line
                .split_once("(process ")
                .and_then(|(_, rest)| rest.split_once(')'))
                .and_then(|(n, _)| n.parse().ok())
            {
                of = Some(p);
            }
            if line.contains(text)
                && let Some(of) = of
                && pid.is_none_or(|pid| pid == of)
            {
                return Ok(of);
            }
        }
        if started.elapsed() > timeout {
            bail!(
                "no log line with {text:?} within {}s:\n{log}",
                timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn wait_for_file(path: &Path, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    while !path.exists() {
        ensure!(
            started.elapsed() < timeout,
            "{} was not written within {}s",
            path.display(),
            timeout.as_secs()
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    // Written whole (`OBS_STEPS` renames it into place; `OBS_SHOT` may still be writing).
    std::thread::sleep(Duration::from_secs(1));
    Ok(())
}

/// Stop process `pid` (not a child of this one: the old app started it).
fn stop(pid: u32) {
    let status = if cfg!(windows) {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .stdout(Stdio::null())
            .status()
    } else {
        Command::new("kill").arg(pid.to_string()).status()
    };
    if let Err(e) = status {
        eprintln!("update-e2e: could not stop process {pid}: {e}");
    }
}

/// Stop whatever still runs from `app` (a release that logs no process id).
fn stop_app_at(app: &Path) {
    let path = app.display().to_string();
    let status = if cfg!(windows) {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Get-Process | Where-Object {{ $_.Path -eq '{}' }} | Stop-Process -Force",
                    path.replace('\'', "''")
                ),
            ])
            .stdout(Stdio::null())
            .status()
    } else {
        Command::new("pkill")
            .args(["-f", &path])
            .stdout(Stdio::null())
            .status()
    };
    if let Err(e) = status {
        eprintln!("update-e2e: could not stop {path}: {e}");
    }
}

/// Every screenshot in `work.shots` as PNG (the PPMs removed); returns their file names.
fn convert_shots(work: &Work) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let mut ppms: Vec<PathBuf> = fs::read_dir(&work.shots)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ppm"))
        .collect();
    ppms.sort();
    for ppm in ppms {
        let png = ppm.with_extension("png");
        ppm_to_png(&ppm, &png).with_context(|| format!("{}", ppm.display()))?;
        fs::remove_file(&ppm)?;
        names.push(
            png.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        );
    }
    Ok(names)
}

/// The app's `P6 w h 255\n` screenshot as an RGB PNG.
fn ppm_to_png(ppm: &Path, png: &Path) -> Result<()> {
    let bytes = fs::read(ppm)?;
    let header_end = bytes
        .iter()
        .position(|b| *b == b'\n')
        .context("no PPM header")?;
    let header = std::str::from_utf8(&bytes[..header_end])?;
    let fields: Vec<&str> = header.split_whitespace().collect();
    let [magic, w, h, max] = fields.as_slice() else {
        bail!("unexpected PPM header {header:?}");
    };
    ensure!(
        *magic == "P6" && *max == "255",
        "unexpected PPM header {header:?}"
    );
    let (w, h): (u32, u32) = (w.parse()?, h.parse()?);
    let pixels = &bytes[header_end + 1..];
    let row = w as usize * 3;
    ensure!(pixels.len() == row * h as usize, "a short PPM");
    let mut raw = Vec::with_capacity((row + 1) * h as usize);
    for line in pixels.chunks(row) {
        raw.push(0); // filter: none
        raw.extend_from_slice(line);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw)?;
    let idat = z.finish()?;
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB, no interlace
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [
        (b"IHDR", &ihdr[..]),
        (b"IDAT", &idat[..]),
        (b"IEND", &[][..]),
    ] {
        out.extend_from_slice(&u32::try_from(data.len())?.to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    fs::write(png, out)?;
    Ok(())
}

/// PNG's CRC-32 (ISO 3309).
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_png_specs_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    #[test]
    fn a_screenshot_converts_to_a_png_with_its_size() {
        let dir = std::env::temp_dir().join(format!("obs-update-e2e-png-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let ppm = dir.join("s.ppm");
        let mut bytes = b"P6 3 2 255\n".to_vec();
        bytes.extend((0..18u8).map(|i| i * 10));
        fs::write(&ppm, bytes).unwrap();
        let png = dir.join("s.png");
        ppm_to_png(&ppm, &png).unwrap();
        let out = fs::read(&png).unwrap();
        assert_eq!(&out[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&out[12..16], b"IHDR");
        assert_eq!(&out[16..24], &[0, 0, 0, 3, 0, 0, 0, 2]);
        assert_eq!(
            &out[out.len() - 12..],
            &[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82]
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
