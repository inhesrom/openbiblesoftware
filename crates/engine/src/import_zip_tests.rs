//! The Mobile app's zip and `.obr` Imports (`crate::import_zip`, #333 "Import"; plan S26):
//! where a SWORD module may sit in a zip, what is never unpacked, the bounds, USFM zips, the
//! temp folder removed whatever happens, and a `.obr` moved into the library.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::Write;
use std::path::{Path, PathBuf};

use obs_model::ResourceType;

use crate::import_zip::{
    IMPORTS_DIR, TempFolder, USFM_BESIDE_SWORD, ZIP_IMPORT_ENTRIES, ZIP_IMPORT_LIMIT, ZipBounds,
    ZipKind, read_usfm_zip, sword_layout, unpack_sword_zip, zip_kind,
};
use crate::testing::{sword_module_files, web_usfm_files, zip_files};
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

fn kjv_module(prefix: &str) -> Vec<(String, Vec<u8>)> {
    sword_module_files("obskjvosis.conf", "texts/ztext/obskjvosis", prefix)
}

fn notes_module(prefix: &str) -> Vec<(String, Vec<u8>)> {
    sword_module_files("obsnotes.conf", "comments/rawcom/obsnotes", prefix)
}

fn web_books(prefix: &str) -> Vec<(String, Vec<u8>)> {
    web_usfm_files(prefix)
}

fn options() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

fn write_zip(path: &Path, files: &[(String, Vec<u8>)]) {
    std::fs::write(path, zip_files(files)).unwrap();
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
    assert_eq!(added.outcome, ObrCopy::Added);
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

    // A greater revision of the same Resource replaces its file and tells the readers.
    sample.manifest.revision += 1;
    obs_store::write_obr(&sample, &picked).unwrap();
    let added = s.add_obr_copy(&picked).unwrap();
    assert_eq!(added.outcome, ObrCopy::Replaced);
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
    let refused = s.add_obr_copy(&picked).unwrap_err();
    assert!(matches!(refused, ObrRefused::Unreadable(_)), "{refused:?}");
    assert!(
        refused.message().starts_with("That is not a Resource file"),
        "{}",
        refused.message()
    );
    assert!(picked.is_file());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// `resource` with other text (the same length, so its spans still fit), its content hash
/// made again.
fn with_other_text(mut resource: obs_model::Resource) -> obs_model::Resource {
    let unit = resource
        .units
        .iter_mut()
        .find(|u| u.text.contains("LORD"))
        .unwrap();
    unit.text = unit.text.replacen("LORD", "Lord", 1);
    resource.manifest.content_hash = resource.content_hash();
    resource
}

#[test]
fn an_obr_copy_replaces_a_resource_only_with_a_greater_revision_and_an_identical_file_changes_nothing()
 {
    let (mut s, root) = fixture("obr-revisions");
    let mut library = obs_store::sample_resource();
    library.manifest.revision = 3;
    library.manifest.content_hash = library.content_hash();
    let dest = root.join("library/kjv-sample.obr");
    obs_store::write_obr(&library, &dest).unwrap();
    s.library.scan(&root.join("library"));
    let before = std::fs::read(&dest).unwrap();
    let picked = root.join("picked/kjv-sample.obr");
    let unchanged = |s: &mut Session| {
        assert_eq!(
            std::fs::read(&dest).unwrap(),
            before,
            "the library's file kept"
        );
        assert_eq!(s.library.get("kjv-sample").unwrap().manifest.revision, 3);
        assert!(
            s.workspace
                .take_effects()
                .iter()
                .all(|e| !matches!(e, Effect::Replaced(_)))
        );
        assert!(picked.is_file(), "left where it is");
    };

    // The same file again: nothing changes, and the outcome says so.
    obs_store::write_obr(&library, &picked).unwrap();
    let added = s.add_obr_copy(&picked).unwrap();
    assert_eq!(added.outcome, ObrCopy::AlreadyThere);
    assert_eq!(added.manifest.revision, 3);
    unchanged(&mut s);

    // The same revision with other content is refused.
    let other = with_other_text(library.clone());
    assert_ne!(other.manifest.content_hash, library.manifest.content_hash);
    obs_store::write_obr(&other, &picked).unwrap();
    let refused = s.add_obr_copy(&picked).unwrap_err();
    assert_eq!(
        refused,
        ObrRefused::NotNewer {
            abbreviation: "KJV".into(),
            revision: 3,
            offered: 3
        }
    );
    assert_eq!(
        refused.message(),
        "Your Library already has KJV revision 3. This file is revision 3 too, with other \
         content, so nothing was changed."
    );
    unchanged(&mut s);

    // A lower revision is refused, whatever its content.
    let mut lower = other.clone();
    lower.manifest.revision = 2;
    obs_store::write_obr(&lower, &picked).unwrap();
    let refused = s.add_obr_copy(&picked).unwrap_err();
    assert_eq!(
        refused.message(),
        "Your Library already has KJV revision 3. This file is an older revision (2), so \
         nothing was changed."
    );
    lower.manifest.content_hash = library.manifest.content_hash.clone();
    obs_store::write_obr(&lower, &picked).unwrap();
    assert!(matches!(
        s.add_obr_copy(&picked),
        Err(ObrRefused::NotNewer { offered: 2, .. })
    ));
    unchanged(&mut s);

    // A greater revision replaces it, in place.
    let mut greater = other;
    greater.manifest.revision = 4;
    obs_store::write_obr(&greater, &picked).unwrap();
    let added = s.add_obr_copy(&picked).unwrap();
    assert_eq!(added.outcome, ObrCopy::Replaced);
    assert_eq!(s.library.get("kjv-sample").unwrap().path, dest);
    assert_eq!(s.library.get("kjv-sample").unwrap().manifest.revision, 4);
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::Replaced("kjv-sample".into()))
    );
    assert!(!picked.exists(), "moved in");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_desktop_open_catalogue_not_a_zip_error_keeps_the_generic_words() {
    // `obs_formats::usfm_zip`'s error, as the desktop's Open catalogue Import reports a
    // download that is not a zip.
    assert_eq!(
        failure_message("WEB from eBible.org: archive: not a zip archive: invalid Zip archive"),
        "The app could not read this source, so nothing was added to your Library."
    );
    // The Mobile app's own.
    assert_eq!(
        failure_message("page.zip: this file is not a zip archive: invalid Zip archive"),
        "That file is not a zip archive, so nothing was added to your Library."
    );
}

#[test]
fn a_component_with_a_colon_anywhere_refuses_the_whole_zip() {
    refused_entry("drive-later", |zip| {
        zip.start_file("modules/C:/escaped", options()).unwrap();
        zip.write_all(b"out").unwrap();
    });
}

#[test]
fn a_sword_conf_whose_data_path_leaves_the_temp_folder_is_refused() {
    let (mut s, root) = fixture("sword-data-path");
    // The module's data where `DataPath=../../../outside/` would find it, from
    // `<library>/imports/<id>.sword/`: without the check it would be read from there.
    for (name, bytes) in kjv_module("") {
        if let Some(rest) = name.strip_prefix("modules/texts/ztext/obskjvosis/") {
            let out = root.join("outside").join(rest);
            std::fs::create_dir_all(out.parent().unwrap()).unwrap();
            std::fs::write(out, bytes).unwrap();
        }
    }
    for (name, data_path) in [
        ("up", "../../../outside/"),
        ("absolute", &*format!("{}/", root.join("outside").display())),
    ] {
        let files: Vec<(String, Vec<u8>)> = kjv_module("")
            .into_iter()
            .map(|(n, bytes)| {
                if n.ends_with(".conf") {
                    let conf = String::from_utf8(bytes).unwrap().replace(
                        "DataPath=./modules/texts/ztext/obskjvosis/",
                        &format!("DataPath={data_path}"),
                    );
                    assert!(conf.contains(data_path));
                    (n, conf.into_bytes())
                } else {
                    (n, bytes)
                }
            })
            .collect();
        let zip = root.join(format!("picked/{name}.zip"));
        write_zip(&zip, &files);
        let id = import_zip(&mut s, &zip);
        let (error, message) = failed_with(&s, &id);
        assert!(error.contains("DataPath"), "{error}");
        assert_eq!(
            message,
            "This zip's SWORD module keeps its text outside the module's own folder in \
             modules/, so nothing was added to your Library.",
            "{error}"
        );
        assert_eq!(imports_left(&root), Vec::<String>::new());
    }
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// `zip` with the central directory's uncompressed size of entry `name` set to `size`: what
/// the zip declares, not what it holds.
fn declare_size(zip: &mut [u8], name: &str, size: u32) {
    let mut at = 0;
    let mut patched = false;
    while let Some(i) = zip[at..]
        .windows(4)
        .position(|w| w == [0x50, 0x4b, 0x01, 0x02])
    {
        let h = at + i;
        let name_len = u16::from_le_bytes([zip[h + 28], zip[h + 29]]) as usize;
        if &zip[h + 46..h + 46 + name_len] == name.as_bytes() {
            zip[h + 24..h + 28].copy_from_slice(&size.to_le_bytes());
            patched = true;
        }
        at = h + 4;
    }
    assert!(patched, "no {name} in the central directory");
}

#[test]
fn a_zip_that_holds_more_than_it_declares_is_refused_as_it_is_read_and_leaves_no_temp_folder() {
    let (s, root) = fixture("sword-declared");
    let files = kjv_module("");
    let total: u64 = files.iter().map(|(_, b)| b.len() as u64).sum();
    let (big, big_bytes) = files.iter().max_by_key(|(_, b)| b.len()).unwrap();
    let mut bytes = zip_files(&files);
    declare_size(&mut bytes, big, 1);
    let zip = root.join("picked/lying.zip");
    std::fs::write(&zip, bytes).unwrap();
    // What it declares fits; what it holds does not.
    let bounds = ZipBounds {
        bytes: total - big_bytes.len() as u64 + 1,
        entries: files.len(),
    };
    let open = || zip::ZipArchive::new(std::fs::File::open(&zip).unwrap()).unwrap();
    assert!(sword_layout(&mut open(), bounds).is_ok());
    let library = root.join("library");
    let err = crate::import::parse_sword_zip(&zip, &library, "kjv-1", bounds, || false)
        .unwrap_err()
        .to_string();
    assert_eq!(
        err,
        format!("this zip unpacks to more than {} bytes", bounds.bytes)
    );
    assert!(failure_message(&format!("lying.zip: {err}")).starts_with("This zip is too large"));
    assert!(library.join(IMPORTS_DIR).is_dir(), "it was unpacked there");
    assert_eq!(imports_left(&root), Vec::<String>::new());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_temp_folder_is_removed_after_a_panic_too() {
    let root = std::env::temp_dir().join(format!("obs-import-zip-panic-{}", std::process::id()));
    let temp = root.join(IMPORTS_DIR).join("kjv-1.sword");
    std::fs::create_dir_all(temp.join("modules")).unwrap();
    std::fs::write(temp.join("modules/file"), b"unpacked").unwrap();
    let caught = std::panic::catch_unwind(|| {
        let _guard = TempFolder(temp.clone());
        panic!("the importer panicked");
    });
    assert!(caught.is_err());
    assert!(!temp.exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_sword_zip_whose_module_the_importer_refuses_fails_after_unpacking_and_leaves_no_temp_folder() {
    let (mut s, root) = fixture("sword-unsupported");
    let zip = root.join("picked/hebrewnotes.zip");
    let files = sword_module_files("obshebrewnotes.conf", "comments/rawcom/obshebrewnotes", "");
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
