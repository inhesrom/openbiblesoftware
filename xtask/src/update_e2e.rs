//! `cargo xtask update-e2e` (#276): Update now end to end, with the real app, on this OS.
//!
//! 1. Builds the app twice from this tree: as this version (vN) and, with
//!    `OBS_BUILD_VERSION`, as the next patch version (vN+1).
//! 2. Packs each with a one-Bible Starter library into a Release archive
//!    (`cargo xtask release-archive`'s code) and unpacks vN into `target/update-e2e/install/`.
//! 3. Seeds the data folders: a User database with a preference and a Resource in the Library.
//! 4. Serves vN+1 from a mock release source on localhost (the releases API's
//!    `/releases/latest`, the archive and `SHA256SUMS`), and runs vN with `OBS_UPDATE_URL`
//!    pointing at it and a script that clicks Help → Check for updates…, Update now and
//!    Restart now (`OBS_SCRIPT`; Update now and Restart now wait for the check and the
//!    install).
//! 5. Waits for vN to close and vN+1 to start from the same folder (its log line), lets it
//!    run its own check (nothing newer), then stops it, and checks that the installed copy is
//!    vN+1, a Before-update backup was taken, and the User database and the Library are as
//!    they were.
//!
//! Needs a display (Linux CI: `xvfb-run`). The app's network transport is the real one; the
//! only server it talks to is the mock on 127.0.0.1.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use obs_engine::starter::{MANIFEST_FILE, sha256_hex};
use obs_engine::updates::{APP_VERSION, CHECKSUMS_ASSET, Version, platform_target};

use crate::release::{self, ArchiveSpec, Format};

const USAGE: &str = "usage: cargo xtask update-e2e

Builds the app as this version and as the next patch version, installs the first from a
Release archive, serves the second from a mock release source on localhost, runs the app's
Check for updates…, Update now and Restart now, and checks that the next version runs from
the same folder with the user's data intact. Needs a display (CI: xvfb-run).";

/// How long each step may take before the run fails.
const STEP_TIMEOUT: Duration = Duration::from_secs(240);
/// The resource id of the e2e Starter library's one Bible.
const STARTER_ID: &str = "e2e-bible";
/// The preference the e2e seeds and checks.
const MARKER_PREF: &str = "e2e.marker";

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return Ok(());
    }
    if let Some(other) = args.first() {
        bail!("unknown option `{other}`\n\n{USAGE}");
    }
    let target = platform_target().context("no Release archive is published for this platform")?;
    let old = Version::parse(APP_VERSION).context("this build's version is not semver")?;
    let new = Version {
        patch: old.patch + 1,
        pre: Vec::new(),
        ..old.clone()
    };
    let work = root.join("target/update-e2e");
    if work.exists() {
        fs::remove_dir_all(&work).with_context(|| format!("{}", work.display()))?;
    }
    fs::create_dir_all(&work)?;

    // 1. Two builds of the app.
    let old_bin = build(root, None, &work.join("bin-old"))?;
    let new_bin = build(root, Some(&new.to_string()), &work.join("bin-new"))?;

    // 2. Release archives, and vN installed from one.
    let starter = work.join("starter");
    write_starter(&starter)?;
    let format = if cfg!(windows) {
        Format::Zip
    } else {
        Format::TarGz
    };
    let archive = |bin: &Path, version: &Version| {
        release::release_archive(&ArchiveSpec {
            bin: bin.to_path_buf(),
            name: format!("openbible-v{version}-{target}"),
            starter: starter.clone(),
            format,
            out: work.join("dist"),
            root: root.to_path_buf(),
        })
    };
    let old_archive = archive(&old_bin, &old)?;
    let new_archive = archive(&new_bin, &new)?;
    let install = release::unpack(&old_archive, &work.join("install"))?;
    let app = release::app_in(&install)?;

    // 3. The user's data.
    let data = work.join("data");
    let library = data.join("library");
    fs::create_dir_all(&library)?;
    let users_resource = library.join("users-own.obr");
    fs::copy(starter.join(format!("{STARTER_ID}.obr")), &users_resource)?;
    let resource_bytes = fs::read(&users_resource)?;
    {
        let (db, _) = obs_store::UserStore::open(&data.join("user.db"))?;
        db.set_preference(MARKER_PREF, "kept")?;
    }

    // 4. The mock release source, and vN running the update.
    let tag = format!("v{new}");
    let base = serve_release(&tag, &new_archive)?;
    eprintln!("update-e2e: serving {tag} at {base}");
    let logs = data.join("logs");
    let mut child = command(&app, &data, &base)
        .env(
            "OBS_SCRIPT",
            "30 help.check_for_updates; 31 update.install; 32 update.restart",
        )
        .spawn()
        .with_context(|| format!("could not launch {}", app.display()))?;
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > STEP_TIMEOUT {
            let _ = child.kill();
            bail!(
                "v{old} did not update and restart within {}s; its log:\n{}",
                STEP_TIMEOUT.as_secs(),
                log_text(&logs)
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    ensure!(
        status.success(),
        "v{old} exited with {status}:\n{}",
        log_text(&logs)
    );

    // 5. vN+1 starts from the same folder, checks (nothing newer) and is stopped.
    let pid = wait_for(&logs, &format!("Open Bible Software {new} started"))?;
    eprintln!("update-e2e: v{new} started (process {pid})");
    let checked = wait_for(&logs, "no update available");
    stop(pid);
    checked.with_context(|| format!("v{new} did not run its own Update check"))?;
    // Let the stopped process let go of its files.
    std::thread::sleep(Duration::from_secs(2));

    ensure!(
        fs::read(&app)? == fs::read(&new_bin)?,
        "{} is not v{new}",
        app.display()
    );
    let version = Command::new(&app).arg("--version").output()?;
    let printed = String::from_utf8_lossy(&version.stdout).trim().to_string();
    ensure!(
        printed == format!("openbible {new}"),
        "{} --version printed {printed:?}",
        app.display()
    );
    let (db, _) = obs_store::UserStore::open(&data.join("user.db"))?;
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
        fs::read(&users_resource)? == resource_bytes,
        "the Library was changed"
    );
    eprintln!(
        "update-e2e: v{old} updated itself to v{new} in {}, restarted into it, and kept the \
         User database and the Library",
        install.display()
    );
    Ok(())
}

/// Build the app (as `version`, else this tree's version) and copy it into `dir`.
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
    let manifest = serde_json::json!({
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
/// API and download URLs would (`OBS_UPDATE_URL`'s layout); returns the base URL.
fn serve_release(tag: &str, archive: &Path) -> Result<String> {
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
    let latest = serde_json::json!({
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
    let files: Arc<Vec<(String, Vec<u8>)>> = Arc::new(vec![
        ("/releases/latest".into(), latest.into_bytes()),
        (format!("/download/{tag}/{name}"), bytes),
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
                    Some((_, body)) => write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .and_then(|()| stream.write_all(body)),
                    None => stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    ),
                };
            });
        }
    });
    Ok(base)
}

/// The app at `app` with every data path under `data` and the mock release source at `base`.
fn command(app: &Path, data: &Path, base: &str) -> Command {
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
        .env("OBS_UPDATE_URL", base)
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env_remove("OBS_STARTER_DIR")
        .env_remove("OBS_INPUT")
        .env_remove("OBS_SHOT")
        .stdin(Stdio::null());
    cmd
}

fn log_text(logs: &Path) -> String {
    fs::read_to_string(logs.join(obs_engine::diagnostics::LOG_FILE)).unwrap_or_default()
}

/// Wait for a log line containing `text`; returns the process id of the launch it belongs
/// to (the last "started … (process N)" line before it).
fn wait_for(logs: &Path, text: &str) -> Result<u32> {
    let started = Instant::now();
    loop {
        let log = log_text(logs);
        let mut pid = None;
        for line in log.lines() {
            if let Some(p) = line
                .split_once("(process ")
                .and_then(|(_, rest)| rest.split_once(')'))
                .and_then(|(n, _)| n.parse().ok())
            {
                pid = Some(p);
            }
            if line.contains(text)
                && let Some(pid) = pid
            {
                return Ok(pid);
            }
        }
        if started.elapsed() > STEP_TIMEOUT {
            bail!(
                "no log line with {text:?} within {}s:\n{log}",
                STEP_TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
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
