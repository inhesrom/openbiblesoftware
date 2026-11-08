//! Exports never overwrite the User database or a previous good export (#252): every export
//! command (`notes.export`, `highlight_palette.export`, `search.export_csv`, `data.export`)
//! refuses the User database and SQLite's files beside it through
//! [`Session::write_export`]; an export replaces a file there whole or not at all; and Export
//! everything names each Notebook's file apart from the others however a disk folds case.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::testing::bibles::write_library;
use crate::testing::{greek_agape, kjv_agape, study};
use crate::*;

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-export-file-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_library(
            &root.join("library"),
            &[kjv_agape(), greek_agape(), study::tlx()],
        );
        Self { root }
    }

    fn db(&self) -> PathBuf {
        self.root.join("user.db")
    }

    fn file(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// A launch: the library scanned and indexed, the User database opened, one Note made.
    fn session(&self) -> Session {
        let mut s = Session::new(JobMode::Inline);
        assert!(s.library.scan(&self.root.join("library")).is_empty());
        s.library
            .set_preferred_order(vec!["kjv".into(), "tgnt".into(), "tlx".into()]);
        s.set_search_cache_dir(self.root.join("cache"));
        s.refresh_search_indexes();
        settle(&mut s);
        let (store, _) = obs_store::UserStore::open(&self.db()).unwrap();
        s.attach_user(Box::new(store));
        s.run(
            "annotation.note",
            &json!({ "passage": "Rom 8:28", "body": "all things" }),
        )
        .unwrap();
        s
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn settle(s: &mut Session) {
    for _ in 0..1000 {
        s.jobs().pump(16);
        assert!(s.poll_imports().is_empty(), "no stray events");
        if !s.search.is_building()
            && !s.bible_search_running()
            && !s.bible_search.is_reading_ahead()
        {
            return;
        }
    }
    panic!("jobs never settled");
}

/// A Search Tab showing the Analysis of `strong:G26`, so `search.export_csv` has rows.
fn analysis_tab(s: &mut Session) -> u64 {
    let out = s
        .run(
            "search.run",
            &json!({ "text": "strong:G26", "bibles": "kjv,tgnt" }),
        )
        .unwrap();
    settle(s);
    let tab = TabId(out["tab"].as_u64().unwrap());
    s.set_results_view(tab, ResultsView::Analysis);
    settle(s);
    tab.0
}

/// The bytes of `path`, or `None` when there is no file.
fn bytes(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

fn dump(s: &Session) -> String {
    s.user.as_ref().unwrap().annotation_dump().unwrap()
}

/// `command` with `params` and `path` refuses the User database and each of SQLite's files
/// beside it, by its own path and by another naming it; each is left byte-identical (or
/// still absent), and the database goes on saving and reopens whole.
fn assert_refuses_the_user_database(f: &Fixture, command: &str, params: &Value) {
    let mut s = f.session();
    let tab = (command == "search.export_csv").then(|| json!(analysis_tab(&mut s)));
    let ok = f.file("ok.export");
    s.run(command, &with_path(params, &ok, tab.clone()))
        .unwrap_or_else(|e| panic!("{command} writes another file: {e}"));
    assert!(ok.is_file(), "{command}");
    let before = dump(&s);
    let db = f.db();
    let mut dests = vec![db.clone(), f.root.join(".").join("user.db")];
    for suffix in ["-wal", "-shm", "-journal"] {
        dests.push(f.file(&format!("user.db{suffix}")));
    }
    for dest in &dests {
        let own: Vec<_> = dests.iter().map(|d| bytes(d)).collect();
        let err = s
            .run(command, &with_path(params, dest, tab.clone()))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("where your data is kept"),
            "{command} {}: {err}",
            dest.display()
        );
        let after: Vec<_> = dests.iter().map(|d| bytes(d)).collect();
        assert!(own == after, "{command} {} changed a file", dest.display());
    }
    assert_eq!(dump(&s), before);
    s.run("annotation.note", &json!({ "body": "still saving" }))
        .unwrap();
    let saved = dump(&s);
    drop(s);
    let (store, _) = obs_store::UserStore::open(&db).unwrap();
    assert_eq!(crate::UserData::annotation_dump(&store).unwrap(), saved);
}

fn with_path(params: &Value, path: &Path, tab: Option<Value>) -> Value {
    let mut p = params.clone();
    p["path"] = json!(path.to_string_lossy());
    if let Some(tab) = tab {
        p["tab"] = tab;
    }
    p
}

/// AUD-ENGINE-DATA-1: `notes.export` with the database's path wrote Markdown over it.
#[test]
fn notes_export_refuses_the_user_database_and_its_sqlite_files() {
    let f = Fixture::new("notes-guard");
    for format in ["markdown", "html", "print"] {
        assert_refuses_the_user_database(
            &f,
            "notes.export",
            &json!({ "format": format, "notebook": "all" }),
        );
    }
}

/// AUD-ENGINE-DATA-1: `highlight_palette.export` wrote JSON over the database.
#[test]
fn palette_export_refuses_the_user_database_and_its_sqlite_files() {
    let f = Fixture::new("palette-guard");
    assert_refuses_the_user_database(
        &f,
        "highlight_palette.export",
        &json!({ "id": STANDARD_PALETTE_ID }),
    );
}

/// AUD-ENGINE-DATA-1: `search.export_csv` had the same unguarded write.
#[test]
fn analysis_csv_export_refuses_the_user_database_and_its_sqlite_files() {
    let f = Fixture::new("csv-guard");
    assert_refuses_the_user_database(&f, "search.export_csv", &json!({}));
}

#[test]
fn export_everything_refuses_the_user_database_and_its_sqlite_files() {
    let f = Fixture::new("data-guard");
    assert_refuses_the_user_database(&f, "data.export", &json!({}));
}

/// Every export replaces a file already at its destination, and leaves no scratch file.
#[test]
fn each_export_replaces_a_previous_file_and_leaves_no_scratch_file() {
    let f = Fixture::new("replace");
    let mut s = f.session();
    let tab = analysis_tab(&mut s);
    let cases = [
        ("notes.export", json!({ "format": "markdown" }), "n.md"),
        (
            "highlight_palette.export",
            json!({ "id": STANDARD_PALETTE_ID }),
            "p.json",
        ),
        ("search.export_csv", json!({ "tab": tab }), "a.csv"),
        ("data.export", json!({}), "d.zip"),
    ];
    for (command, params, name) in cases {
        let dest = f.file(name);
        std::fs::write(&dest, "an older export").unwrap();
        s.run(command, &with_path(&params, &dest, None)).unwrap();
        let written = bytes(&dest).unwrap();
        assert_ne!(written, b"an older export", "{command}");
        assert!(!written.is_empty(), "{command}");
    }
    let mut left: Vec<String> = std::fs::read_dir(&f.root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect();
    left.sort();
    assert!(left.is_empty(), "scratch files left: {left:?}");
}

/// Re-exporting over a file only its owner may read keeps it that way (the replacing file
/// is not given a new file's wider permissions), for every export command.
#[cfg(unix)]
#[test]
fn an_export_keeps_the_permissions_of_the_file_it_replaces() {
    use std::os::unix::fs::PermissionsExt as _;
    let f = Fixture::new("permissions");
    let mut s = f.session();
    let tab = analysis_tab(&mut s);
    let cases = [
        ("notes.export", json!({ "format": "markdown" }), "n.md"),
        (
            "highlight_palette.export",
            json!({ "id": STANDARD_PALETTE_ID }),
            "p.json",
        ),
        ("search.export_csv", json!({ "tab": tab }), "a.csv"),
        ("data.export", json!({}), "d.zip"),
    ];
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    for (command, params, name) in cases {
        let dest = f.file(name);
        s.run(command, &with_path(&params, &dest, None)).unwrap();
        let fresh = mode(&dest);
        assert_ne!(fresh & 0o600, 0, "{command}: a new export is the owner's");
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o600)).unwrap();
        s.run(command, &with_path(&params, &dest, None)).unwrap();
        assert_eq!(mode(&dest), 0o600, "{command}");
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o640)).unwrap();
        s.run(command, &with_path(&params, &dest, None)).unwrap();
        assert_eq!(mode(&dest), 0o640, "{command}");
    }
}

/// A destination whose name is as long as a disk allows is exported to like any other: the
/// scratch file's name does not grow with the destination's.
#[test]
fn an_export_writes_to_a_name_as_long_as_the_disk_allows() {
    let f = Fixture::new("long-name");
    let mut s = f.session();
    // 84 three-byte characters plus `.md`: 255 bytes, the limit on a name on Linux and
    // macOS disks (87 UTF-16 units on Windows, well inside its limit).
    let name = format!("{}.md", "\u{8a9e}".repeat(84));
    assert_eq!(name.len(), 255);
    let dest = f.file(&name);
    std::fs::write(&dest, "an older export").unwrap();
    s.run(
        "notes.export",
        &json!({ "format": "markdown", "path": dest.to_string_lossy() }),
    )
    .unwrap();
    let written = bytes(&dest).unwrap();
    assert!(String::from_utf8(written).unwrap().contains("all things"));
}

/// The child half of [`a_failed_export_leaves_the_previous_file_byte_identical`]: run in a
/// process that may not write files larger than a block, so each export fails mid-write.
#[cfg(unix)]
const FAILURE_CHILD: &str = "OBS_EXPORT_FAILURE_CHILD";

/// AUD-ENGINE-DATA-7: a failed Export everything (or Notes export) truncated the previous
/// good export before failing. The export is retried in a child process limited to tiny
/// files (`ulimit -f`), so the write itself fails; the previous file must be byte-identical
/// and no scratch file left.
#[cfg(unix)]
#[test]
fn a_failed_export_leaves_the_previous_file_byte_identical() {
    let cases = |root: &Path| {
        [
            (
                "data.export",
                json!({ "path": root.join("previous.zip").to_string_lossy() }),
            ),
            (
                "notes.export",
                json!({ "format": "markdown", "notebook": "all",
                        "path": root.join("previous.md").to_string_lossy() }),
            ),
        ]
    };
    if let Some(root) = std::env::var_os(FAILURE_CHILD) {
        let root = PathBuf::from(root);
        let (store, _) = obs_store::UserStore::open_backup(&root.join("source.obsbackup")).unwrap();
        let mut s = Session::new(JobMode::Inline);
        s.attach_user(Box::new(store));
        for (command, params) in cases(&root) {
            let out = s.run(command, &params);
            assert!(out.is_err(), "{command} must fail here: {out:?}");
            println!("{command}: {}", out.unwrap_err());
        }
        return;
    }

    let f = Fixture::new("failed");
    let mut s = f.session();
    // Notes too large to fit a tiny file even compressed: pseudo-random words.
    let mut seed = 0x2545_f491_u32;
    for _ in 0..8 {
        let body: Vec<String> = (0..600)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                format!("{:x}", seed >> 8)
            })
            .collect();
        s.run("annotation.note", &json!({ "body": body.join(" ") }))
            .unwrap();
    }
    s.run(
        "data.back_up",
        &json!({ "path": f.file("source.obsbackup").to_string_lossy() }),
    )
    .unwrap();
    let mut previous = Vec::new();
    for (command, params) in cases(&f.root) {
        s.run(command, &params).unwrap();
        let path = PathBuf::from(params["path"].as_str().unwrap());
        let before = bytes(&path).unwrap();
        assert!(before.len() > 8192, "{command}: {} bytes", before.len());
        previous.push((path, before));
    }
    drop(s);

    let test = concat!(module_path!(), "::")
        .trim_start_matches("obs_engine::")
        .to_string()
        + "a_failed_export_leaves_the_previous_file_byte_identical";
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(r#"trap '' XFSZ; ulimit -f 1 && exec "$0" "$@""#)
        .arg(std::env::current_exe().unwrap())
        .args([test.as_str(), "--exact", "--nocapture", "--test-threads=1"])
        .env(FAILURE_CHILD, &f.root)
        .output()
        .unwrap();
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{said}");
    assert!(said.contains("1 passed"), "the child ran the test: {said}");
    for (path, before) in previous {
        let after = bytes(&path).unwrap_or_default();
        assert!(
            after == before,
            "{}: {} bytes before the failed export, {} after\n{said}",
            path.display(),
            before.len(),
            after.len()
        );
    }
    let left: Vec<String> = std::fs::read_dir(&f.root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with('.'))
        .collect();
    assert!(left.is_empty(), "scratch files left: {left:?}");
}

/// PERSONAL-1: Notebooks whose file names differ only in case (or by a `/`, which becomes a
/// space) had entries that overwrite each other when the zip is extracted on a disk that
/// folds case (macOS, Windows).
#[test]
fn export_everything_names_notebooks_apart_on_a_case_insensitive_disk() {
    let f = Fixture::new("case");
    let mut s = f.session();
    let names = ["Romans", "romans", "ROMANS", "Romans study", "Romans/Study"];
    for (i, name) in names.iter().enumerate() {
        // `notebook.create` refuses a name another Notebook has in another case, so the
        // case twins are written as rows, as a database from elsewhere could hold them.
        let id = match s.run("notebook.create", &json!({ "name": name })) {
            Ok(out) => out["id"].as_str().unwrap().to_string(),
            Err(e) => {
                assert!(e.to_string().contains("already a Notebook"), "{e}");
                let now = obs_store::annotation::now_ms();
                let nb = Notebook {
                    id: format!("case-twin-{i}"),
                    name: (*name).to_string(),
                    is_default: false,
                    show_in_text: true,
                    created_at: now,
                    modified_at: now,
                };
                s.commit(
                    "New Notebook",
                    vec![Change::insert(Row::Notebook(nb.clone()))],
                )
                .unwrap();
                nb.id
            }
        };
        s.run(
            "annotation.note",
            &json!({ "body": format!("marker{i}x"), "notebook": id }),
        )
        .unwrap();
    }
    let dest = f.file("e2.zip");
    let out = s
        .run("data.export", &json!({ "path": dest.to_string_lossy() }))
        .unwrap();
    let files: Vec<String> = out["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let notes: Vec<&String> = files.iter().filter(|n| n.starts_with("notes/")).collect();
    // The default Notebook and the five made here.
    assert_eq!(notes.len(), names.len() + 1, "{notes:?}");
    let folded: HashSet<String> = files.iter().map(|n| n.to_lowercase()).collect();
    assert_eq!(folded.len(), files.len(), "{files:?}");

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&dest).unwrap()).unwrap();
    let mut found = HashSet::new();
    for name in &notes {
        let mut text = String::new();
        zip.by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        for (i, name) in names.iter().enumerate() {
            if text.contains(&format!("marker{i}x")) {
                assert!(found.insert(i), "{name} twice");
            }
        }
    }
    assert_eq!(
        found.len(),
        names.len(),
        "every Notebook's Notes: {found:?}"
    );
}
