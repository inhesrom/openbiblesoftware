//! `cargo xtask sync-e2e` (plan S29): Sync between two desktop instances of the real app,
//! each with its own User database (`OBS_USER_DB`), through one temporary Sync folder,
//! driven by `OBS_SCRIPT` commands and `OBS_STEPS` clicks and photographed.
//!
//! 1. A chooses the folder, names itself and makes the Notebook "From A" (Word forms off);
//!    it is backed up after its first sync.
//! 2. B chooses the same folder, makes "From B" and turns Word forms on; its Settings → Sync
//!    lists both devices.
//! 3. A again: its launch sync takes B's changes. It makes "After backup", then restores the
//!    Backup: the restore syncs first, the prompt "Restored from a Backup" shows, and
//!    **Catch up** brings back what changed since (B's Word forms on, "After backup").
//! 4. A restores the Backup again and chooses **Replace**: the Backup's Word forms (off) wins,
//!    and rows it never knew ("From B", "After backup") stay.
//! 5. B again: its launch sync takes A's Replace; both User databases hold the same Notebooks
//!    and Word forms.
//!
//! Each run is stopped once its last screenshot is written. Screenshots go to
//! `target/sync-e2e/shots/` as PNG; each app's log (with how long every sync took) to
//! `target/sync-e2e/<device>/logs/`. Needs a display (Linux: `xvfb-run`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde_json::json;

use crate::update_e2e;

const USAGE: &str = "usage: cargo xtask sync-e2e

Builds the app and runs it as two desktop devices, each with its own User database, syncing
through one temporary Sync folder: they converge; a Backup restored on one shows the restore
prompt, and Catch up and Replace both work. Screenshots go to target/sync-e2e/shots/.

Needs a display (CI: xvfb-run).";

/// How long one run of the app may take before the e2e fails.
const RUN_TIMEOUT: Duration = Duration::from_secs(180);
/// No Update check reaches the network: nothing answers on this port.
const NO_UPDATES: &str = "http://127.0.0.1:9";
/// The status indicator's accessible name once a sync is done.
const SYNCED: &str = "Sync: Last synced just now";
/// The synced setting the e2e changes on each side (`SYNCED_PREFERENCES`).
const WORD_FORMS: &str = "search.word_forms";

pub fn run(root: &Path, args: &[String]) -> Result<()> {
    match args {
        [] => {}
        [a] if a == "--help" || a == "-h" => {
            println!("{USAGE}");
            return Ok(());
        }
        _ => bail!("unexpected arguments {args:?}\n\n{USAGE}"),
    }
    let work = root.join("target/sync-e2e");
    if work.exists() {
        fs::remove_dir_all(&work).with_context(|| format!("removing {}", work.display()))?;
    }
    let shots = work.join("shots");
    let folder = work.join("folder");
    fs::create_dir_all(&shots)?;
    fs::create_dir_all(&folder)?;
    let app = update_e2e::build(root, None, &work.join("bin"))?;
    let a = work.join("a");
    let b = work.join("b");
    let backup = work.join("a.obsbackup");
    let folder_param = json!({ "path": folder.to_string_lossy() }).to_string();
    let backup_param = json!({ "path": backup.to_string_lossy() }).to_string();
    let shot = |name: &str| shots.join(format!("{name}.ppm"));

    // 1. A joins with "From A" and Word forms off, and is backed up once synced.
    eprintln!("sync-e2e: A joins");
    run_app(
        &app,
        &a,
        &format!(
            "3 notebook.create {{\"name\":\"From A\"}}; \
             4 search.word_forms {{\"value\":false}}; \
             5 sync.device_name {{\"name\":\"Mac A\"}}; \
             6 sync.choose_folder {folder_param}; \
             60 data.back_up {backup_param}"
        ),
        &json!([
            { "click": SYNCED, "after_secs": 2.0 },
            { "when": "Synced devices", "after_secs": 1.0, "shot": shot("1-a-joined") },
        ]),
        &shot("1-a-joined"),
    )?;
    ensure!(backup.is_file(), "A's Backup was not written");

    // 2. B joins with "From B" and turns Word forms on.
    eprintln!("sync-e2e: B joins");
    run_app(
        &app,
        &b,
        &format!(
            "3 notebook.create {{\"name\":\"From B\"}}; \
             5 sync.device_name {{\"name\":\"Mac B\"}}; \
             6 sync.choose_folder {folder_param}; \
             60 search.word_forms {{\"value\":true}}; \
             70 sync.now"
        ),
        &json!([
            { "click": SYNCED, "after_secs": 5.0 },
            { "when": "Synced devices", "after_secs": 1.0, "shot": shot("2-b-joined") },
        ]),
        &shot("2-b-joined"),
    )?;
    let after_b = State::read(&b)?;
    ensure!(
        after_b.has(&["From A", "From B"]) && after_b.word_forms.as_deref() == Some("true"),
        "B did not take A's Notebook: {after_b:?}"
    );

    // 3. A takes B's changes at launch, makes "After backup", restores and catches up.
    eprintln!("sync-e2e: A restores its Backup and catches up");
    run_app(
        &app,
        &a,
        &format!(
            "40 notebook.create {{\"name\":\"After backup\"}}; \
             80 data.restore {backup_param}"
        ),
        &json!([
            { "when": "Restored from a Backup", "after_secs": 1.0, "shot": shot("3-a-restore-prompt") },
            { "click": "Catch up" },
            { "click": SYNCED, "after_secs": 1.0 },
            { "when": "Synced devices", "after_secs": 1.0, "shot": shot("4-a-caught-up") },
        ]),
        &shot("4-a-caught-up"),
    )?;
    let caught_up = State::read(&a)?;
    ensure!(
        caught_up.has(&["From A", "From B", "After backup"])
            && caught_up.word_forms.as_deref() == Some("true"),
        "Catch up did not bring back what changed since the Backup: {caught_up:?}"
    );
    ensure!(
        caught_up.folder.as_deref() == Some(&*folder.to_string_lossy()),
        "the restore did not keep A's Sync folder: {caught_up:?}"
    );

    // 4. A restores the Backup again and replaces what is in the folder.
    eprintln!("sync-e2e: A restores its Backup and replaces");
    run_app(
        &app,
        &a,
        &format!("40 data.restore {backup_param}"),
        &json!([
            { "when": "Restored from a Backup", "after_secs": 1.0 },
            { "click": "Replace" },
            { "click": SYNCED, "after_secs": 1.0 },
            { "when": "Synced devices", "after_secs": 1.0, "shot": shot("5-a-replaced") },
        ]),
        &shot("5-a-replaced"),
    )?;
    let replaced = State::read(&a)?;
    ensure!(
        replaced.word_forms.as_deref() == Some("false"),
        "Replace did not make the Backup's Word forms win: {replaced:?}"
    );
    ensure!(
        replaced.has(&["From A", "From B", "After backup"]),
        "Replace dropped a row the Backup never knew: {replaced:?}"
    );

    // 5. B takes A's Replace at launch: both hold the same.
    eprintln!("sync-e2e: B takes the Replace");
    run_app(
        &app,
        &b,
        "",
        &json!([
            { "click": SYNCED, "after_secs": 2.0 },
            { "when": "Synced devices", "after_secs": 1.0, "shot": shot("6-b-converged") },
        ]),
        &shot("6-b-converged"),
    )?;
    let (a_now, b_now) = (State::read(&a)?, State::read(&b)?);
    ensure!(
        a_now.notebooks == b_now.notebooks && a_now.word_forms == b_now.word_forms,
        "A and B did not converge:\nA {a_now:?}\nB {b_now:?}"
    );

    for name in fs::read_dir(&shots)?
        .filter_map(Result::ok)
        .map(|e| e.path())
    {
        if name.extension().is_some_and(|e| e == "ppm") {
            update_e2e::ppm_to_png(&name, &name.with_extension("png"))?;
            fs::remove_file(&name)?;
        }
    }
    for (device, dir) in [("A", &a), ("B", &b)] {
        let syncs: Vec<String> = update_e2e::log_text(&dir.join("logs"))
            .lines()
            .filter(|l| l.contains("synced in "))
            .map(str::to_string)
            .collect();
        eprintln!("sync-e2e: {device}'s syncs:");
        for line in syncs {
            eprintln!("  {line}");
        }
    }
    eprintln!(
        "sync-e2e: converged: {:?}, Word forms {:?}; screenshots in {}",
        a_now.notebooks,
        a_now.word_forms,
        shots.display()
    );
    Ok(())
}

/// Run the app on the data in `data` with `script` and `steps`, until `last` is written.
fn run_app(
    app: &Path,
    data: &Path,
    script: &str,
    steps: &serde_json::Value,
    last: &Path,
) -> Result<()> {
    fs::create_dir_all(data)?;
    let mut cmd = update_e2e::command(app, data, Some(NO_UPDATES));
    if !script.is_empty() {
        cmd.env("OBS_SCRIPT", script);
    }
    cmd.env("OBS_STEPS", steps.to_string());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("could not launch {}", app.display()))?;
    let started = Instant::now();
    let logs = data.join("logs");
    while !last.exists() {
        if let Some(status) = child.try_wait()? {
            bail!(
                "the app exited ({status}) before {} was written:\n{}",
                last.display(),
                update_e2e::log_text(&logs)
            );
        }
        if started.elapsed() > RUN_TIMEOUT {
            let _ = child.kill();
            bail!(
                "{} was not written within {}s:\n{}",
                last.display(),
                RUN_TIMEOUT.as_secs(),
                update_e2e::log_text(&logs)
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    // Written whole: the screenshot is renamed into place.
    std::thread::sleep(Duration::from_secs(1));
    let _ = child.kill();
    child.wait()?;
    Ok(())
}

/// What one device's User database holds of what the e2e checks.
#[derive(Debug)]
struct State {
    notebooks: BTreeSet<String>,
    word_forms: Option<String>,
    folder: Option<String>,
}

impl State {
    fn read(data: &Path) -> Result<Self> {
        let db: PathBuf = data.join("user.db");
        let (store, _) =
            obs_store::UserStore::open(&db).with_context(|| format!("opening {}", db.display()))?;
        Ok(Self {
            notebooks: store.notebooks()?.into_iter().map(|n| n.name).collect(),
            word_forms: store.preference(WORD_FORMS)?,
            folder: store.preference(obs_engine::SYNC_FOLDER_PREF)?,
        })
    }

    fn has(&self, names: &[&str]) -> bool {
        names.iter().all(|n| self.notebooks.contains(*n))
    }
}
