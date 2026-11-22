//! Import on the device (#333 "Import", S26): a USFM zip, a SWORD module zip, a USFM file and
//! a `.obr` handed in through the file the core named, each with its Import report in
//! Rust's words; refusals; the copies deleted; nothing left in `imports/` at the next boot.
//! Fixtures from `obs_engine::testing` (CI-run on every OS).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_engine::JobMode;
use obs_engine::testing::{sword_module_files, web_usfm_files, zip_files};

use crate::*;

fn root(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("obs-mobile-import-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn boot(root: &Path) -> MobileApp {
    MobileApp::boot_with(Paths::under(&root.join("app")), None, JobMode::Inline).unwrap()
}

/// What the shell does with a picked file: ask where, copy it there, hand it in.
fn pick(app: &mut MobileApp, name: &str, bytes: &[u8]) -> (u64, PathBuf) {
    let dest = app.import_destination(name).unwrap();
    assert!(dest.starts_with(app.imports_folder()), "{}", dest.display());
    assert_eq!(dest.file_name().unwrap().to_string_lossy(), name);
    std::fs::write(&dest, bytes).unwrap();
    (app.start_import(&dest).unwrap(), dest)
}

/// Pump until the Import's report stops running.
fn settle(app: &mut MobileApp, import: u64) -> ImportReportView {
    for _ in 0..100 {
        let _ = app.pump();
        let view = app.import_report(import).unwrap();
        if view.state != ImportState::Running && !app.busy() {
            return view;
        }
    }
    panic!("the Import did not finish");
}

fn section<'a>(view: &'a ImportReportView, title: &str) -> &'a ReportSection {
    (view.sections.iter())
        .find(|s| s.title.starts_with(title))
        .unwrap_or_else(|| panic!("no {title} in {view:#?}"))
}

#[test]
fn a_usfm_zip_imports_with_its_report_and_the_copy_is_deleted() {
    let root = root("usfm-zip");
    let mut app = boot(&root);
    let zip = zip_files(&web_usfm_files("engweb/"));
    let (import, dest) = pick(&mut app, "engweb_usfm.zip", &zip);
    // Jobs run inline here, at the pump: before it, the Import is waiting to read the file.
    let running = app.import_report(import).unwrap();
    assert_eq!(running.heading, "Import in progress");
    assert_eq!(running.state, ImportState::Running);
    assert_eq!(running.status, "Reading the file…");
    assert_eq!(running.source, "engweb_usfm.zip");
    assert!(app.pump().contains(&Changed::Imports));
    let view = settle(&mut app, import);
    assert_eq!(view.state, ImportState::Complete, "{view:#?}");
    assert_eq!(view.heading, "Import complete");
    let id = view.resource_id.clone().unwrap();
    assert!(app.library().iter().any(|r| r.resource_id == id));
    let summary = section(&view, "Summary");
    assert_eq!(summary.lines[0], "engweb_usfm.zip");
    assert_eq!(summary.lines[1], "5 source files · usfm");
    assert!(
        summary.lines[2].starts_with("5 Bible books · "),
        "{summary:?}"
    );
    let licence = section(&view, "Licence");
    assert!(licence.warning);
    assert!(licence.lines[0].starts_with("Licence: unknown"));
    let files = section(&view, "Source files and Bible books");
    assert!(files.lines.len() >= 10);
    assert!(files.collapsed && !licence.collapsed && !summary.collapsed);
    assert!(view.status.starts_with("Search index"), "{}", view.status);
    assert!(
        !dest.exists() && !dest.parent().unwrap().exists(),
        "copy deleted"
    );
    // Its final report seen, the pump no longer builds it.
    assert!(app.picked.iter().all(|p| p.settled), "settled");
    assert!(!app.pump().contains(&Changed::Imports));
    // The report opens the new Bible in the reader.
    let reader = app.open(ReaderId(0), &id, None).unwrap();
    assert_eq!(reader.resource_id, id);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_sword_module_zip_imports_and_a_usfm_file_beside_it_is_named_in_the_report() {
    let root = root("sword-zip");
    let mut app = boot(&root);
    let mut files = sword_module_files("obskjvosis.conf", "texts/ztext/obskjvosis", "sword/");
    files.extend(web_usfm_files("books/"));
    let (import, _) = pick(&mut app, "KJV.zip", &zip_files(&files));
    let view = settle(&mut app, import);
    assert_eq!(view.state, ImportState::Complete, "{view:#?}");
    let id = view.resource_id.clone().unwrap();
    let resource = (app.library().into_iter())
        .find(|r| r.resource_id == id)
        .unwrap();
    assert_eq!(resource.resource_type, ResourceType::Bible);
    assert!(section(&view, "Summary").lines[1].contains("sword"));
    let licence = section(&view, "Licence");
    assert!(!licence.warning);
    assert_eq!(licence.lines, [obs_engine::import::LICENCE_RECORDED]);
    let warnings = section(&view, "Warnings");
    assert!(
        warnings
            .lines
            .contains(&obs_engine::import_zip::USFM_BESIDE_SWORD.to_string()),
        "{warnings:?}"
    );
    assert_eq!(
        std::fs::read_dir(app.imports_folder()).unwrap().count(),
        0,
        "nothing left in imports/"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_usfm_file_imports_too() {
    let root = root("usfm-file");
    let mut app = boot(&root);
    let (_, book) = web_usfm_files("").into_iter().next().unwrap();
    let (import, _) = pick(&mut app, "2KI.usfm", &book);
    let view = settle(&mut app, import);
    assert_eq!(view.state, ImportState::Complete, "{view:#?}");
    assert_eq!(section(&view, "Summary").lines[1], "1 source file · usfm");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_obr_is_added_as_it_is_and_only_a_greater_revision_replaces_it() {
    let root = root("obr");
    let mut app = boot(&root);
    let mut sample = obs_store::sample_resource();
    sample.manifest.resource_id = "shared-bible".into();
    let file = root.join("shared.obr");
    obs_store::write_obr(&sample, &file).unwrap();
    let (import, dest) = pick(&mut app, "shared.obr", &std::fs::read(&file).unwrap());
    let view = app.import_report(import).unwrap();
    assert_eq!(view.state, ImportState::Complete, "{view:#?}");
    assert_eq!(view.resource_id.as_deref(), Some("shared-bible"));
    let summary = section(&view, "Summary");
    assert_eq!(summary.lines[0], sample.manifest.title);
    assert_eq!(
        summary.lines[1],
        format!("Bible · {}", sample.manifest.abbreviation)
    );
    assert_eq!(summary.lines[2], "Added to your Library as it is.");
    assert!(app.paths().library.join("shared-bible.obr").is_file());
    assert!(!dest.parent().unwrap().exists());
    assert!(
        app.library()
            .iter()
            .any(|r| r.resource_id == "shared-bible")
    );

    sample.manifest.revision += 1;
    obs_store::write_obr(&sample, &file).unwrap();
    let (import, _) = pick(&mut app, "shared.obr", &std::fs::read(&file).unwrap());
    let view = app.import_report(import).unwrap();
    assert_eq!(
        section(&view, "Summary").lines[2],
        format!(
            "Replaced {} in your Library with revision {}.",
            sample.manifest.abbreviation, sample.manifest.revision
        )
    );

    // The same file again changes nothing, and says so.
    let (import, dest) = pick(&mut app, "shared.obr", &std::fs::read(&file).unwrap());
    let view = app.import_report(import).unwrap();
    assert_eq!(view.state, ImportState::Complete, "{view:#?}");
    assert_eq!(
        section(&view, "Summary").lines[2],
        format!(
            "Your Library already has this file ({} revision 2), so nothing was changed.",
            sample.manifest.abbreviation
        )
    );
    assert!(!dest.parent().unwrap().exists(), "the copy deleted");

    // A lower revision is refused in plain words; the library keeps revision 2.
    sample.manifest.revision = 1;
    obs_store::write_obr(&sample, &file).unwrap();
    let (import, dest) = pick(&mut app, "shared.obr", &std::fs::read(&file).unwrap());
    let view = app.import_report(import).unwrap();
    assert_eq!(view.state, ImportState::Failed, "{view:#?}");
    assert_eq!(
        view.failure.as_deref(),
        Some(
            format!(
                "Your Library already has {} revision 2. This file is an older revision (1), \
                 so nothing was changed.",
                sample.manifest.abbreviation
            )
            .as_str()
        )
    );
    assert!(view.details.as_deref().unwrap().contains("revision 2"));
    assert!(!dest.parent().unwrap().exists(), "the copy deleted");
    assert_eq!(
        app.session
            .library
            .get("shared-bible")
            .unwrap()
            .manifest
            .revision,
        2
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn refusals_say_why_in_plain_words_and_delete_the_copy() {
    let root = root("refused");
    let mut app = boot(&root);
    // A file that is not a Resource, named .obr.
    let (import, dest) = pick(&mut app, "notes.obr", b"not a Resource");
    let view = app.import_report(import).unwrap();
    assert_eq!(view.state, ImportState::Failed);
    assert_eq!(view.heading, "Import failed");
    assert!(
        view.failure
            .as_deref()
            .unwrap()
            .starts_with("That is not a Resource file"),
        "{view:#?}"
    );
    assert!(view.details.is_some());
    assert!(!dest.exists());
    // A kind the app does not import.
    let (import, _) = pick(&mut app, "notes.txt", b"hello");
    let view = app.import_report(import).unwrap();
    assert_eq!(view.failure.as_deref(), Some(IMPORT_KINDS));
    // A zip of two SWORD modules.
    let mut files = sword_module_files("obskjvosis.conf", "texts/ztext/obskjvosis", "");
    files.extend(sword_module_files(
        "obsnotes.conf",
        "comments/rawcom/obsnotes",
        "",
    ));
    let (import, _) = pick(&mut app, "two.zip", &zip_files(&files));
    let view = settle(&mut app, import);
    assert_eq!(view.state, ImportState::Failed);
    assert_eq!(
        view.failure.as_deref(),
        Some("This zip holds several SWORD modules. Zip one module, then import it.")
    );
    // The shell could not copy the picked file.
    let dest = app.import_destination("locked.usfm").unwrap();
    let import = app.import_copy_failed(&dest, "permission denied").unwrap();
    let view = app.import_report(import).unwrap();
    assert_eq!(view.state, ImportState::Failed);
    assert_eq!(
        view.failure.as_deref(),
        Some("The app could not read that file, so nothing was added to your Library.")
    );
    assert_eq!(view.details.as_deref(), Some("permission denied"));
    assert!(!dest.parent().unwrap().exists());
    // A path the core did not name is refused and left alone.
    let other = root.join("elsewhere.usfm");
    std::fs::write(&other, "\\id GEN\n").unwrap();
    assert!(app.start_import(&other).is_err());
    assert!(other.is_file());
    // No file name.
    assert!(app.import_destination("..").is_err());
    assert!(app.import_destination("").is_err());
    // Only the last component of a name is kept.
    let dest = app.import_destination("../../escape.usfm").unwrap();
    assert!(dest.starts_with(app.imports_folder()));
    assert_eq!(dest.file_name().unwrap(), "escape.usfm");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_cancelled_import_says_so_and_boot_sweeps_what_imports_left() {
    let root = root("cancel");
    let mut app = boot(&root);
    let (import, dest) = pick(&mut app, "engweb_usfm.zip", &zip_files(&web_usfm_files("")));
    app.cancel_import(import).unwrap();
    let view = settle(&mut app, import);
    assert_eq!(view.state, ImportState::Failed, "{view:#?}");
    assert_eq!(view.failure.as_deref(), Some("The Import was cancelled."));
    assert!(!dest.exists());
    // A file named but never handed in (the app was closed): gone at the next boot.
    let left = app.import_destination("left.usfm").unwrap();
    std::fs::write(&left, "\\id GEN\n").unwrap();
    drop(app);
    let app = boot(&root);
    assert!(!app.imports_folder().exists());
    assert!(app.import_report(import).is_err(), "Imports are per run");
    std::fs::remove_dir_all(root).unwrap();
}
