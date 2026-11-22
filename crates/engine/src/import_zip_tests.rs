//! The Mobile app's zip and `.obr` Imports (`crate::import_zip`, #333 "Import"; plan S26):
//! where a SWORD module may sit in a zip, what is never unpacked, the bounds, USFM zips, the
//! temp folder removed whatever happens, and a `.obr` moved into the library.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::{Path, PathBuf};

use obs_model::ResourceType;

use crate::import_zip::{
    IMPORTS_DIR, USFM_BESIDE_SWORD, ZIP_IMPORT_ENTRIES, ZIP_IMPORT_LIMIT, ZipBounds, ZipKind,
    read_usfm_zip, sword_layout, unpack_sword_zip, zip_kind,
};
use crate::*;

fn fixture(name: &str) -> (Session, PathBuf) {
    let root = std::env::temp_dir().join(format!("obs-import-zip-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("library")).unwrap();
    std::fs::create_dir_all(root.join("picked")).unwrap();
    let mut s = Session::new(JobMode::Inline);
    s.library.scan(&root.join("library"));
    let (user, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(user));
    (s, root)
}

fn sword_assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../formats/tests/assets/sword")
}

/// Every file under `dir`, as (path relative to `dir` with `/`, bytes), sorted.
fn files_under(dir: &Path, prefix: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let rel = path
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((format!("{prefix}{rel}"), std::fs::read(&path).unwrap()));
            }
        }
    }
    out.sort();
    out
}

/// The ObsKjvOsis test module's files (its conf and its zText data) under `prefix`.
fn kjv_module(prefix: &str) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![(
        format!("{prefix}mods.d/obskjvosis.conf"),
        std::fs::read(sword_assets().join("mods.d/obskjvosis.conf")).unwrap(),
    )];
    files.extend(files_under(
        &sword_assets().join("modules/texts/ztext/obskjvosis"),
        &format!("{prefix}modules/texts/ztext/obskjvosis/"),
    ));
    files
}

/// The ObsNotes test module's files under `prefix`.
fn notes_module(prefix: &str) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![(
        format!("{prefix}mods.d/obsnotes.conf"),
        std::fs::read(sword_assets().join("mods.d/obsnotes.conf")).unwrap(),
    )];
    files.extend(files_under(
        &sword_assets().join("modules/comments/rawcom/obsnotes"),
        &format!("{prefix}modules/comments/rawcom/obsnotes/"),
    ));
    files
}

fn web_books(prefix: &str) -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../formats/tests/assets/usfm/web");
    files_under(&dir, prefix)
}

fn options() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

fn write_zip(path: &Path, files: &[(String, Vec<u8>)]) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in files {
        zip.start_file(name.as_str(), options()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

/// Import the zip at `path` as the Mobile app does; return the Resource id.
fn import_zip(s: &mut Session, path: &Path) -> String {
    let (id, _job) = s
        .start_import_job(ImportSource::zip(path.to_path_buf()))
        .unwrap();
    s.jobs().pump(1);
    s.poll_imports();
    assert!(!s.imports.is_running());
    id
}

/// No temp folder is left in the library's `imports/`.
fn imports_left(root: &Path) -> Vec<String> {
    match std::fs::read_dir(root.join("library").join(IMPORTS_DIR)) {
        Ok(rd) => rd
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn failed_with(s: &Session, id: &str) -> (String, &'static str) {
    let record = s.imports.report(id).unwrap();
    assert_eq!(record.phase, ImportPhase::Failed, "{record:?}");
    assert!(s.library.get(id).is_none());
    let error = record.error.clone().unwrap();
    let message = failure_message(&error);
    (error, message)
}

#[test]
fn a_sword_zip_with_mods_d_at_its_top_imports_and_leaves_no_temp_folder() {
    let (mut s, root) = fixture("sword-top");
    let zip = root.join("picked/KJV.zip");
    write_zip(&zip, &kjv_module(""));
    assert_eq!(zip_kind(&zip), ZipKind::Sword);
    let id = import_zip(&mut s, &zip);
    assert_eq!(id, "kjv-1");
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    let report = record.report.as_ref().unwrap();
    assert!(report.source_format.starts_with("sword"), "{report:?}");
    assert!(report.verses > 0);
    assert!(!report.warnings.iter().any(|w| w == USFM_BESIDE_SWORD));
    let entry = s.library.get(&id).unwrap();
    assert_eq!(entry.manifest.resource_type, ResourceType::Bible);
    assert_eq!(entry.manifest.abbreviation, "KJV-O");
    assert_eq!(imports_left(&root), Vec::<String>::new());
    // The zip itself is the caller's: the Import leaves it.
    assert!(zip.is_file());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_sword_zip_in_one_wrapper_folder_imports_resource_forks_aside() {
    let (mut s, root) = fixture("sword-wrapper");
    let zip = root.join("picked/sword.zip");
    let mut files = notes_module("sword/");
    // What macOS adds to a zip it makes: never read as a module or unpacked.
    files.push((
        "__MACOSX/sword/mods.d/._obsnotes.conf".into(),
        vec![0, 5, 22, 7],
    ));
    files.push(("sword/mods.d/._obsnotes.conf".into(), vec![0, 5, 22, 7]));
    write_zip(&zip, &files);
    let id = import_zip(&mut s, &zip);
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    assert_eq!(
        s.library.get(&id).unwrap().manifest.resource_type,
        ResourceType::Commentary
    );
    assert_eq!(imports_left(&root), Vec::<String>::new());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_sword_zip_too_deep_or_of_several_modules_is_refused_in_plain_words() {
    let (mut s, root) = fixture("sword-refused");
    let deep = root.join("picked/deep.zip");
    write_zip(&deep, &kjv_module("a/b/"));
    let id = import_zip(&mut s, &deep);
    let (error, message) = failed_with(&s, &id);
    assert!(error.contains("not where the app looks"), "{error}");
    assert!(
        message.starts_with("This zip's SWORD module is not where"),
        "{message}"
    );

    let several = root.join("picked/several.zip");
    let mut files = kjv_module("");
    files.extend(notes_module(""));
    write_zip(&several, &files);
    let id = import_zip(&mut s, &several);
    let (error, message) = failed_with(&s, &id);
    assert!(
        error.contains("several SWORD modules (obskjvosis, obsnotes)"),
        "{error}"
    );
    assert_eq!(
        message,
        "This zip holds several SWORD modules. Zip one module, then import it."
    );

    // Two wrapper folders, one module each: several modules too.
    let two = root.join("picked/two.zip");
    let mut files = kjv_module("a/");
    files.extend(notes_module("b/"));
    write_zip(&two, &files);
    let id = import_zip(&mut s, &two);
    let (error, _) = failed_with(&s, &id);
    assert!(error.contains("several SWORD modules"), "{error}");
    assert_eq!(imports_left(&root), Vec::<String>::new());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_sword_zip_with_usfm_files_imports_the_module_and_the_report_says_the_books_were_not() {
    let (mut s, root) = fixture("sword-mixed");
    let zip = root.join("picked/mixed.zip");
    let mut files = kjv_module("");
    files.extend(web_books("usfm/"));
    write_zip(&zip, &files);
    assert_eq!(zip_kind(&zip), ZipKind::Sword);
    let id = import_zip(&mut s, &zip);
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    let report = record.report.as_ref().unwrap();
    assert!(report.source_format.starts_with("sword"));
    assert!(
        report.warnings.iter().any(|w| w == USFM_BESIDE_SWORD),
        "{:?}",
        report.warnings
    );
    assert_eq!(imports_left(&root), Vec::<String>::new());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// A zip with one bad entry beside a good module: nothing is written outside the temp
/// folder, nothing joins the library, and the temp folder goes.
fn refused_entry(name: &str, add: impl FnOnce(&mut zip::ZipWriter<std::fs::File>)) {
    let (mut s, root) = fixture(name);
    let zip_path = root.join(format!("picked/{name}.zip"));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
    for (n, bytes) in kjv_module("") {
        zip.start_file(n.as_str(), options()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    add(&mut zip);
    zip.finish().unwrap();
    let id = import_zip(&mut s, &zip_path);
    let (error, message) = failed_with(&s, &id);
    assert!(
        message.starts_with("This zip holds a file the app will not unpack"),
        "{error} reads {message}"
    );
    assert!(!root.join("escaped").exists());
    assert!(!root.join("library/escaped").exists());
    assert_eq!(imports_left(&root), Vec::<String>::new());
    assert!(s.library.entries().is_empty());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_zip_slip_entry_refuses_the_whole_zip() {
    refused_entry("slip", |zip| {
        zip.start_file("modules/../../../escaped", options())
            .unwrap();
        zip.write_all(b"out").unwrap();
    });
}

#[test]
fn an_absolute_entry_refuses_the_whole_zip() {
    refused_entry("absolute", |zip| {
        zip.start_file("/tmp/escaped", options()).unwrap();
        zip.write_all(b"out").unwrap();
    });
}

#[test]
fn a_backslash_or_drive_entry_refuses_the_whole_zip() {
    refused_entry("backslash", |zip| {
        zip.start_file("..\\..\\escaped", options()).unwrap();
        zip.write_all(b"out").unwrap();
    });
    refused_entry("drive", |zip| {
        zip.start_file("C:/escaped", options()).unwrap();
        zip.write_all(b"out").unwrap();
    });
}

#[test]
fn a_symbolic_link_entry_refuses_the_whole_zip() {
    refused_entry("symlink", |zip| {
        zip.add_symlink("modules/escaped", "../../..", options())
            .unwrap();
    });
}

#[test]
fn the_unpack_bounds_are_512_mib_and_10000_entries_and_refuse_past_them() {
    assert_eq!(ZIP_IMPORT_LIMIT, 512 * 1024 * 1024);
    assert_eq!(ZIP_IMPORT_ENTRIES, 10_000);
    assert_eq!(
        ZipBounds::DEFAULT,
        ZipBounds {
            bytes: ZIP_IMPORT_LIMIT,
            entries: ZIP_IMPORT_ENTRIES
        }
    );
    // The plain words name the same bounds.
    let message = failure_message("x.zip: this zip unpacks to more than 512 MiB");
    assert!(
        message.contains("512 MiB") && message.contains("10,000"),
        "{message}"
    );
    assert_eq!(
        failure_message("x.zip: this zip has more than 10000 files"),
        message
    );

    let root = std::env::temp_dir().join(format!("obs-import-zip-bounds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("KJV.zip");
    let files = kjv_module("");
    write_zip(&path, &files);
    let bytes: u64 = files.iter().map(|(_, b)| b.len() as u64).sum();
    let open = || zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let fits = ZipBounds {
        bytes,
        entries: files.len(),
    };
    assert!(sword_layout(&mut open(), fits).is_ok());
    let dest = root.join("unpacked");
    let (sword, usfm) = unpack_sword_zip(&path, &dest, fits, || false).unwrap();
    assert_eq!(sword, dest);
    assert!(!usfm);
    assert!(dest.join("mods.d/obskjvosis.conf").is_file());
    let fewer_bytes = ZipBounds {
        bytes: bytes - 1,
        ..fits
    };
    assert_eq!(
        sword_layout(&mut open(), fewer_bytes).unwrap_err(),
        format!("this zip unpacks to more than {} bytes", bytes - 1)
    );
    let fewer_entries = ZipBounds {
        entries: files.len() - 1,
        ..fits
    };
    assert_eq!(
        sword_layout(&mut open(), fewer_entries).unwrap_err(),
        format!("this zip has more than {} files", files.len() - 1)
    );
    // Nothing is unpacked past a bound.
    let refused = root.join("refused");
    assert!(unpack_sword_zip(&path, &refused, fewer_bytes, || false).is_err());
    assert!(!refused.exists());
    // A cancelled unpack stops.
    let err = unpack_sword_zip(&path, &root.join("cancelled"), fits, || true).unwrap_err();
    assert_eq!(err.to_string(), "Import cancelled");
    // A USFM zip's file is bounded too.
    let usfm = root.join("web.zip");
    write_zip(&usfm, &web_books(""));
    let size = std::fs::metadata(&usfm).unwrap().len();
    assert!(
        read_usfm_zip(
            &usfm,
            ZipBounds {
                bytes: size,
                entries: 5
            }
        )
        .is_ok()
    );
    assert!(
        read_usfm_zip(
            &usfm,
            ZipBounds {
                bytes: size - 1,
                entries: 5
            }
        )
        .is_err()
    );
    let err = read_usfm_zip(
        &usfm,
        ZipBounds {
            bytes: size,
            entries: 4,
        },
    )
    .unwrap_err();
    assert_eq!(err.to_string(), "this zip has more than 4 files");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_usfm_zip_imports_its_books_from_any_folder() {
    let (mut s, root) = fixture("usfm");
    let zip = root.join("picked/engweb_usfm.zip");
    let mut files = web_books("engweb/");
    files.push(("engweb/copr.htm".into(), b"<p>Public Domain</p>".to_vec()));
    write_zip(&zip, &files);
    assert_eq!(zip_kind(&zip), ZipKind::Usfm);
    let id = import_zip(&mut s, &zip);
    assert_eq!(id, "engweb-usfm-1");
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    let report = record.report.as_ref().unwrap();
    assert_eq!(report.source_format, "usfm");
    assert_eq!(report.books.len(), 5, "{:?}", report.books);
    assert_eq!(report.source_files.len(), 5);
    assert!(report.verses > 0);
    let entry = s.library.get(&id).unwrap();
    assert_eq!(entry.manifest.resource_type, ResourceType::Bible);
    assert_eq!(entry.manifest.title, "engweb_usfm.zip");
    assert!(zip.is_file());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_zip_without_books_or_a_file_that_is_not_a_zip_says_so() {
    let (mut s, root) = fixture("usfm-empty");
    let empty = root.join("picked/notes.zip");
    write_zip(&empty, &[("readme.txt".into(), b"hello".to_vec())]);
    let id = import_zip(&mut s, &empty);
    let (_, message) = failed_with(&s, &id);
    assert!(
        message.starts_with("This zip holds no USFM books"),
        "{message}"
    );

    let not_zip = root.join("picked/page.zip");
    std::fs::write(&not_zip, "<html>Not found</html>").unwrap();
    assert_eq!(zip_kind(&not_zip), ZipKind::Usfm);
    let id = import_zip(&mut s, &not_zip);
    let (_, message) = failed_with(&s, &id);
    assert_eq!(
        message,
        "That file is not a zip archive, so nothing was added to your Library."
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_obr_copy_is_moved_into_the_library_and_a_second_revision_replaces_it() {
    let (mut s, root) = fixture("obr");
    let mut sample = obs_store::sample_resource();
    sample.manifest.resource_id = "shared-bible".into();
    let picked = root.join("picked/shared-bible.obr");
    obs_store::write_obr(&sample, &picked).unwrap();
    let added = s.add_obr_copy(&picked).unwrap();
    assert!(!added.replaced);
    assert_eq!(added.manifest.resource_id, "shared-bible");
    let dest = root.join("library/shared-bible.obr");
    assert!(dest.is_file());
    assert!(!picked.exists(), "moved, not copied");
    assert_eq!(s.library.get("shared-bible").unwrap().path, dest);
    assert!(
        s.workspace
            .take_effects()
            .iter()
            .all(|e| !matches!(e, Effect::Replaced(_)))
    );

    // A newer revision of the same Resource replaces its file and tells the readers.
    sample.manifest.revision += 1;
    obs_store::write_obr(&sample, &picked).unwrap();
    let added = s.add_obr_copy(&picked).unwrap();
    assert!(added.replaced);
    assert_eq!(s.library.get("shared-bible").unwrap().path, dest);
    assert_eq!(
        s.library.get("shared-bible").unwrap().manifest.revision,
        sample.manifest.revision
    );
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::Replaced("shared-bible".into()))
    );
    let names: Vec<String> = std::fs::read_dir(root.join("library"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["shared-bible.obr"], "no staged file left");

    // Another Resource's file already has the name: the next name is used.
    let mut other = obs_store::sample_resource();
    other.manifest.resource_id = "other".into();
    obs_store::write_obr(&other, &root.join("library/taken.obr")).unwrap();
    let mut taken = obs_store::sample_resource();
    taken.manifest.resource_id = "taken".into();
    obs_store::write_obr(&taken, &picked).unwrap();
    s.add_obr_copy(&picked).unwrap();
    assert_eq!(
        s.library.get("taken").unwrap().path,
        root.join("library/taken-2.obr")
    );

    // A file that is not a Resource is refused and left where it is.
    std::fs::write(&picked, "not a Resource").unwrap();
    assert!(s.add_obr_copy(&picked).is_err());
    assert!(picked.is_file());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_sword_zip_whose_module_the_importer_refuses_fails_after_unpacking_and_leaves_no_temp_folder() {
    let (mut s, root) = fixture("sword-unsupported");
    let zip = root.join("picked/hebrewnotes.zip");
    let mut files = vec![(
        "mods.d/obshebrewnotes.conf".to_string(),
        std::fs::read(sword_assets().join("mods.d/obshebrewnotes.conf")).unwrap(),
    )];
    files.extend(files_under(
        &sword_assets().join("modules/comments/rawcom/obshebrewnotes"),
        "modules/comments/rawcom/obshebrewnotes/",
    ));
    write_zip(&zip, &files);
    let id = import_zip(&mut s, &zip);
    let (error, message) = failed_with(&s, &id);
    assert_eq!(
        message, "The app cannot import this SWORD module yet.",
        "{error}"
    );
    assert!(
        root.join("library").join(IMPORTS_DIR).is_dir(),
        "it was unpacked there"
    );
    assert_eq!(imports_left(&root), Vec::<String>::new());
    assert!(!root.join(format!("library/{id}.obr")).exists());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}
