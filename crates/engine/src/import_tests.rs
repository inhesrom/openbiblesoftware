#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use obs_model::ResourceType;
use serde_json::json;

use crate::*;

fn fixture(name: &str, mode: JobMode) -> (Session, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("obs-import-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("source")).unwrap();
    std::fs::create_dir_all(root.join("library")).unwrap();
    std::fs::write(
        root.join("source/GEN.usfm"),
        "\\id GEN\n\\c 1\n\\p\n\\v 1 In the beginning.\n\\ip Introduction dropped.\n",
    )
    .unwrap();
    std::fs::write(
        root.join("source/JHN.usfm"),
        "\\id JHN\n\\c 1\n\\p\n\\v 1 In the beginning was the Word.\n",
    )
    .unwrap();
    let mut s = Session::new(mode);
    s.library.scan(&root.join("library"));
    let (user, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(user));
    (s, root)
}

fn start(s: &mut Session, path: &Path) -> String {
    s.run("import.start", &json!({"path": path})).unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}
fn finish(s: &mut Session) {
    s.jobs().pump(1);
    assert!(s.poll_imports().is_empty());
    assert!(!s.imports.is_running());
}

#[test]
fn folder_import_has_complete_report_is_last_and_survives_restart_then_remove_deletes_it() {
    let (mut s, root) = fixture("complete", JobMode::Inline);
    let mut sample = obs_store::sample_resource();
    sample.manifest.resource_id = "z-existing".into();
    let path = root.join("library/z-existing.obr");
    obs_store::write_obr(&sample, &path).unwrap();
    s.library.add_file(&path).unwrap();
    let id = start(&mut s, &root.join("source"));
    assert!(
        s.library.get(&id).is_none(),
        "no parsing in command/UI call"
    );
    let tab = s
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::ImportReport(id.clone()))
        .unwrap()
        .id;
    assert_eq!(
        s.workspace.tab(tab).unwrap().kind.header_box(),
        HeaderBox::Keyless
    );
    assert!(!s.workspace.tab(tab).unwrap().kind.has_link_chip());
    s.workspace.close(tab); // Closing report must not cancel Import.
    finish(&mut s);
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete);
    assert_eq!(record.index, ImportIndexStatus::Deferred);
    let report = record.report.as_ref().unwrap();
    assert_eq!(report.books.len(), 2);
    assert_eq!(report.source_files, ["GEN.usfm", "JHN.usfm"]);
    assert_eq!(report.verses, 2);
    assert!(report.license_missing);
    assert!(!report.dropped_markup.is_empty());
    assert!(report.verse_map.unwrap().absent > 0);
    assert_eq!(report.verse_map.unwrap().unmapped, 0);
    assert_eq!(
        s.library
            .of_type(ResourceType::Bible)
            .last()
            .unwrap()
            .manifest
            .resource_id,
        id
    );
    let out = s.library.get(&id).unwrap().path.clone();
    assert!(out.exists());
    s.run("import.report", &json!({"id": id})).unwrap();
    s.run("resource.open", &json!({"id": id})).unwrap();
    s.run("resource.about", &json!({"id": id})).unwrap();
    s.save_last_session().unwrap();
    drop(s);
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    let (user, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(user));
    assert_eq!(
        s.imports
            .report(&id)
            .unwrap()
            .report
            .as_ref()
            .unwrap()
            .verses,
        2
    );
    assert_eq!(
        s.library
            .of_type(ResourceType::Bible)
            .last()
            .unwrap()
            .manifest
            .resource_id,
        id
    );
    assert!(s.restore_last_session());
    s.run("library.remove", &json!({"id": id})).unwrap();
    assert!(out.exists(), "confirmation required");
    s.run("library.remove", &json!({"id": id, "confirm": true}))
        .unwrap();
    assert!(!out.exists());
    assert!(s.library.get(&id).is_none());
    assert!(s.imports.report(&id).is_none());
    assert!(!s.workspace.tabs().any(|t| matches!(&t.kind, TabKind::Resource(r) | TabKind::About(r) | TabKind::ImportReport(r) if r == &id)));
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_import_is_visible_and_never_adds_a_resource_or_overwrites_existing_file() {
    let (mut s, root) = fixture("errors", JobMode::Inline);
    let id = start(&mut s, &root.join("missing.usfm"));
    finish(&mut s);
    assert_eq!(s.imports.report(&id).unwrap().phase, ImportPhase::Failed);
    assert!(s.imports.report(&id).unwrap().error.is_some());
    assert!(s.library.entries().is_empty());
    // A file not scanned into the library still cannot be overwritten.
    let collision = root.join("library/source-1.obr");
    std::fs::write(&collision, b"untouched").unwrap();
    let id = start(&mut s, &root.join("source"));
    finish(&mut s);
    assert_eq!(s.imports.report(&id).unwrap().phase, ImportPhase::Failed);
    assert_eq!(std::fs::read(&collision).unwrap(), b"untouched");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn repeated_imports_have_distinct_ids_and_files() {
    let (mut s, root) = fixture("repeat", JobMode::Inline);
    let a = start(&mut s, &root.join("source"));
    let b = start(&mut s, &root.join("source"));
    s.jobs().pump(2);
    s.poll_imports();
    assert_ne!(a, b);
    assert_ne!(
        s.library.get(&a).unwrap().path,
        s.library.get(&b).unwrap().path
    );
    assert_eq!(s.library.entries().len(), 2);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[derive(Debug)]
struct BlockingIndexer {
    entered: mpsc::Sender<()>,
    release: Mutex<mpsc::Receiver<()>>,
}
impl ImportIndexer for BlockingIndexer {
    fn build(
        &self,
        _: &obs_model::Resource,
        _: &Path,
        ctx: &JobContext,
    ) -> Result<ImportIndexStatus, String> {
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        ctx.progress(1, 1);
        Ok(ImportIndexStatus::Built)
    }
}

#[test]
fn import_phases_report_real_progress_and_commands_stay_responsive_while_worker_waits() {
    let (mut s, root) = fixture("worker", JobMode::Threads(1));
    let (entered, rx) = mpsc::channel();
    let (tx, release) = mpsc::channel();
    s.imports.set_indexer(Arc::new(BlockingIndexer {
        entered,
        release: Mutex::new(release),
    }));
    let id = start(&mut s, &root.join("source"));
    rx.recv_timeout(Duration::from_secs(10)).unwrap();
    // Inspect real events before Index finishes: Parse reports 0,1,2 of two Bible books.
    let events = s.jobs().drain_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, JobEventKind::Phase(p) if p == "Parse"))
    );
    let parse: Vec<_> = events
        .iter()
        .filter_map(|e| match e.kind {
            JobEventKind::Progress { done, total } => Some((done, total)),
            _ => None,
        })
        .collect();
    assert_eq!(parse, [(0, 2), (1, 2), (2, 2)]);
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.kind, JobEventKind::Phase(p) if p == "Index"))
    );
    assert!(!events.iter().any(|e| e.kind.is_terminal()));
    let before = Instant::now();
    s.run("library.open", &json!({})).unwrap();
    s.run("reference.parse", &json!({"text": "Jn 3:16"}))
        .unwrap();
    assert!(before.elapsed() < Duration::from_secs(1));
    tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while s.imports.is_running() && Instant::now() < deadline {
        s.poll_imports();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        s.imports.report(&id).unwrap().index,
        ImportIndexStatus::Built
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unrelated_job_events_are_returned_to_the_host() {
    let mut s = Session::new(JobMode::Inline);
    s.jobs().submit_fn("other", |_| Ok(42u32));
    s.jobs().pump(1);
    assert!(s.poll_imports().iter().any(
        |e| matches!(&e.kind, JobEventKind::Finished(out) if out.downcast_ref::<u32>() == Some(&42))
    ));
}

fn sword_assets() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../formats/tests/assets/sword")
}

#[test]
fn sword_conf_file_imports_through_the_app_import_with_the_conf_licence() {
    let (mut s, root) = fixture("sword-conf", JobMode::Inline);
    let conf = sword_assets().join("mods.d/obskjvosis.conf");
    let id = start(&mut s, &conf);
    assert_eq!(id, "obskjvosis-1");
    assert!(s.library.get(&id).is_none(), "no parsing in the command");
    assert!(
        s.workspace
            .tabs()
            .any(|t| t.kind == TabKind::ImportReport(id.clone()))
    );
    finish(&mut s);
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    let report = record.report.as_ref().unwrap();
    assert!(report.source_format.starts_with("sword"));
    assert!(report.verses > 0);
    assert!(!report.license_missing);
    let entry = s.library.get(&id).unwrap();
    assert_eq!(entry.manifest.resource_type, ResourceType::Bible);
    assert_eq!(entry.manifest.abbreviation, "KJV-O");
    assert!(entry.path.starts_with(root.join("library")));
    assert_eq!(
        s.library
            .of_type(ResourceType::Bible)
            .last()
            .unwrap()
            .manifest
            .resource_id,
        id
    );
    // Windows cannot delete files the Session still holds open (user.db).
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sword_folder_imports_the_named_module_and_asks_for_one_when_several() {
    let (mut s, root) = fixture("sword-folder", JobMode::Inline);
    let id = s
        .run(
            "import.start",
            &json!({"path": sword_assets(), "module": "ObsNotes"}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(id, "obsnotes-1");
    finish(&mut s);
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    assert!(record.source.ends_with("(ObsNotes)"));
    assert_eq!(
        s.library.get(&id).unwrap().manifest.resource_type,
        ResourceType::Commentary
    );

    // A folder of several modules and no module named: the report says which to choose,
    // and nothing lands in the Library.
    let id = start(&mut s, &sword_assets());
    finish(&mut s);
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Failed);
    let error = record.error.as_deref().unwrap();
    assert!(error.contains("several modules"), "{error}");
    assert!(error.contains("ObsKjvOsis"), "{error}");
    assert!(s.library.get(&id).is_none());
    assert!(!root.join(format!("library/{id}.obr")).exists());
    // Windows cannot delete files the Session still holds open (user.db).
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn import_source_detects_sword_by_conf_file_or_mods_d_folder() {
    let assets = sword_assets();
    assert!(matches!(
        ImportSource::detect(assets.clone(), None),
        ImportSource::Sword { module: None, .. }
    ));
    assert!(matches!(
        ImportSource::detect(assets.join("mods.d/obsnotes.conf"), None),
        ImportSource::Sword { .. }
    ));
    assert_eq!(
        ImportSource::detect("/x/GEN.usfm".into(), None),
        ImportSource::Usfm("/x/GEN.usfm".into())
    );
}

/// LIBRARY-10 (#307): every report Tab was "Import report"; two side by side could not be
/// told apart. It names the source while it runs and the Resource once it is in.
#[test]
fn import_report_tabs_name_what_was_imported() {
    let (mut s, root) = fixture("report-title", JobMode::Inline);
    let title = |s: &Session, id: &str| {
        s.workspace
            .tabs()
            .find(|t| t.kind == TabKind::ImportReport(id.into()))
            .map(|t| t.title.clone())
    };
    let id = start(&mut s, &root.join("source/GEN.usfm"));
    assert_eq!(title(&s, &id).as_deref(), Some("Import report · GEN"));
    finish(&mut s);
    let abbreviation = s.library.get(&id).unwrap().manifest.abbreviation.clone();
    assert_eq!(
        title(&s, &id),
        Some(format!("Import report · {abbreviation}"))
    );
    let other = start(&mut s, &root.join("source/JHN.usfm"));
    assert_eq!(title(&s, &other).as_deref(), Some("Import report · JHN"));
    finish(&mut s);
    assert_ne!(title(&s, &id), title(&s, &other));
    // Opened again from About: the same name.
    for t in s.workspace.tabs().map(|t| t.id).collect::<Vec<_>>() {
        s.workspace.close(t);
    }
    s.run("import.report", &json!({"id": id})).unwrap();
    assert_eq!(
        title(&s, &id),
        Some(format!("Import report · {abbreviation}"))
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// LIBRARY-13 (#307): a failed Import says what went wrong in plain words; the system's
/// error ("os error 2", "no \id line") is only the Details.
#[test]
fn failed_imports_say_what_went_wrong_in_plain_words() {
    let (mut s, root) = fixture("failure-words", JobMode::Inline);
    std::fs::write(root.join("README.md"), "# Not a Bible\n").unwrap();
    for (path, plain) in [
        (
            root.join("missing/foo.usfm"),
            "That file or folder does not exist",
        ),
        (root.join("README.md"), "That is not a USFM file"),
    ] {
        let id = start(&mut s, &path);
        finish(&mut s);
        let record = s.imports.report(&id).unwrap();
        assert_eq!(record.phase, ImportPhase::Failed);
        let error = record.error.as_deref().unwrap();
        let message = failure_message(error);
        assert!(message.starts_with(plain), "{error:?} reads {message:?}");
        for raw in ["os error", "\\id", "README.md", "foo.usfm"] {
            assert!(!message.contains(raw), "{message:?}");
        }
    }
    assert_eq!(
        failure_message("x: Import cancelled"),
        "The Import was cancelled."
    );
    assert!(
        failure_message("KJV from CrossWire: download from CrossWire: timed out")
            .starts_with("The download did not finish")
    );
    // A local file's error starts with its path; one in a Downloads folder was not a download.
    for local in [
        "/Users/me/Downloads/x.usfm: invalid data",
        "C:\\Users\\me\\Downloads\\x.usfm: invalid data",
    ] {
        assert_eq!(
            failure_message(local),
            "The app could not read this source, so nothing was added to your Library.",
            "{local}"
        );
    }
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}
