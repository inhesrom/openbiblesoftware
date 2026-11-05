//! Library → Starter library… (#131) through the shell's real-event headless harness: the
//! rows with type, size, licence and features; Add back after a removal; and the one-click
//! download, verified, when there is no bundle.

use std::path::{Path, PathBuf};

use obs_engine::starter::StarterStatus;
use obs_engine::testing::{starter_set, write_starter_bundle};

use super::*;

struct Lab {
    root: PathBuf,
}

impl Lab {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-shell-starter-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("library")).unwrap();
        Self { root }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Launch with a User database and library folder in `lab`, syncing the Starter library from
/// `bundle` as the app does.
fn launch(lab: &Lab, bundle: Option<&Path>) -> Harness {
    let mut h = Harness::launch(Some(&lab.root.join("user.db")));
    let s = &mut h.shell.st.session;
    s.library.set_directory(lab.root.join("library"));
    s.set_search_cache_dir(lab.root.join("cache"));
    let sync = s.sync_starter_library(bundle);
    assert!(sync.problems.is_empty(), "{:?}", sync.problems);
    settle(&mut h);
    h
}

/// Run queued jobs (inline mode) and draw.
fn settle(h: &mut Harness) {
    for _ in 0..20 {
        h.shell.st.session.jobs().pump(64);
        h.frames(2);
        let s = &h.shell.st.session;
        if s.starter.download().is_none()
            && !s.starter.is_checking()
            && !s.search.is_building()
            && !s.reanchoring.is_busy()
        {
            break;
        }
    }
    h.frames(2);
}

fn open_starter_library(h: &mut Harness) {
    h.open_menu("Library");
    h.click_text("Starter library…");
    settle(h);
    assert!(
        h.panels()
            .iter()
            .any(|(_, t)| t.iter().any(|t| t == "Starter library")),
        "{:?}",
        h.panels()
    );
}

/// The control labelled `label` on the row whose title is `title`.
fn row_button(h: &Harness, title: &str, label: &str) -> Pos2 {
    let row = h
        .find(title)
        .unwrap_or_else(|| panic!("{title:?} not on screen: {:?}", h.texts()));
    h.shapes
        .iter()
        .filter(|(r, t)| {
            t == label && r.center().y > row.top() - 4.0 && r.center().y < row.top() + 50.0
        })
        .map(|(r, _)| r.center())
        .next()
        .unwrap_or_else(|| panic!("no {label:?} on row {title:?}: {:?}", h.texts()))
}

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

const TBESG: &str = "Brief lexicon of Extended Strongs for Greek";
const BDB: &str = "Brown-Driver-Briggs";

#[test]
fn the_starter_library_tab_lists_type_size_licence_and_features_and_add_back_restores() {
    let lab = Lab::new("tab");
    let bundle = lab.root.join("starter");
    write_starter_bundle(&bundle, &starter_set());
    let mut h = launch(&lab, Some(&bundle));
    // First run: every Starter Resource is in the Library.
    for id in ["kjv-strongs", "tbesh", "tbesg", "bdb", "strongs-hebrew"] {
        assert!(h.shell.st.session.library.get(id).is_some(), "{id}");
    }
    open_starter_library(&mut h);
    assert!(h.find("IN YOUR LIBRARY · 5").is_some(), "{:?}", h.texts());
    assert!(h.find("NOT IN YOUR LIBRARY · 0").is_none());
    assert!(h.find(TBESG).is_some(), "{:?}", h.texts());
    let size = h
        .shell
        .st
        .session
        .starter
        .manifest()
        .unwrap()
        .get("tbesg")
        .unwrap()
        .bytes;
    let facts = format!("Lexicon · TBESG · {:.1} MB", size as f64 / 1e6);
    assert!(h.find(&facts).is_some(), "{facts}: {:?}", h.texts());
    assert!(h.find("CC BY").is_some(), "licence badge");
    assert!(h.find("Public domain").is_some(), "the KJV's badge");
    assert!(
        h.find("Word Study: Lexicon entries · Word Info: Gloss")
            .is_some(),
        "{:?}",
        h.texts()
    );
    assert!(h.find("In Library").is_some());
    assert!(
        h.find("Every Starter Resource you have not removed is in your Library.")
            .is_some()
    );

    // Removed: listed as such, with Add back.
    h.shell
        .run("library.remove", &json!({ "id": "bdb", "confirm": true }));
    h.frames(3);
    assert!(h.find("Removed").is_some(), "{:?}", h.texts());
    // Listed first, under its own heading.
    let not_in = h.find("NOT IN YOUR LIBRARY · 1").unwrap();
    let have = h.find("IN YOUR LIBRARY · 4").unwrap();
    let bdb = h.find(BDB).unwrap();
    assert!(not_in.top() < bdb.top() && bdb.top() < have.top());
    preview(&h, "starter-library");
    let add_back = row_button(&h, BDB, "Add back");
    h.click(add_back);
    settle(&mut h);
    assert!(h.shell.st.session.library.get("bdb").is_some());
    assert!(h.find("Removed").is_none(), "{:?}", h.texts());
}

#[test]
fn without_a_bundle_one_click_downloads_verifies_and_adds_the_starter_library() {
    let lab = Lab::new("download");
    let release = lab.root.join("release");
    write_starter_bundle(&release, &starter_set());
    let mut h = Harness::launch(Some(&lab.root.join("user.db")));
    {
        let s = &mut h.shell.st.session;
        s.library.set_directory(lab.root.join("library"));
        s.set_search_cache_dir(lab.root.join("cache"));
        s.starter
            .set_download_base(&obs_engine::sword_repo::file_url(&release));
        let sync = s.sync_starter_library(None);
        assert!(sync.offer_download);
    }
    open_starter_library(&mut h);
    let wanted = h.shell.st.session.starter_wanted_bytes();
    let label = format!(
        "Download Starter library (≈{} MB)",
        wanted.div_ceil(1_000_000)
    );
    assert!(h.find(&label).is_some(), "{label}: {:?}", h.texts());
    assert!(h.find("Not in Library").is_some());
    preview(&h, "starter-library-download");
    h.click_text(&label);
    // The click starts the download; its rows say so until the job is applied.
    assert!(h.shell.st.session.starter.download().is_some());
    settle(&mut h);
    let s = &h.shell.st.session;
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    assert!(
        s.starter_rows()
            .iter()
            .all(|r| r.status == StarterStatus::InLibrary)
    );
    assert_eq!(
        s.library
            .of_type(obs_model::ResourceType::Lexicon)
            .map(|e| e.manifest.resource_id.as_str())
            .collect::<Vec<_>>(),
        ["tbesh", "tbesg", "bdb", "strongs-hebrew"]
    );
    assert!(h.find(&label).is_none());
}

#[test]
fn a_download_that_fails_verification_says_so_and_retry_adds_it() {
    let lab = Lab::new("mismatch");
    let release = lab.root.join("release");
    write_starter_bundle(&release, &starter_set());
    let tbesh = release.join("tbesh.obr");
    let mut bytes = std::fs::read(&tbesh).unwrap();
    bytes[100] ^= 0xff;
    std::fs::write(&tbesh, bytes).unwrap();
    let mut h = Harness::launch(Some(&lab.root.join("user.db")));
    {
        let s = &mut h.shell.st.session;
        s.library.set_directory(lab.root.join("library"));
        s.starter
            .set_download_base(&obs_engine::sword_repo::file_url(&release));
    }
    open_starter_library(&mut h);
    let wanted = h.shell.st.session.starter_wanted_bytes();
    h.click_text(&format!(
        "Download Starter library (≈{} MB)",
        wanted.div_ceil(1_000_000)
    ));
    settle(&mut h);
    let error = h
        .texts()
        .into_iter()
        .find(|t| t.contains("failed verification"))
        .unwrap_or_else(|| panic!("{:?}", h.texts()))
        .to_string();
    assert!(error.contains("Nothing was added"), "{error}");
    assert!(
        h.shell
            .st
            .session
            .starter_rows()
            .iter()
            .all(|r| r.status == StarterStatus::Missing)
    );
    preview(&h, "starter-library-mismatch");
    write_starter_bundle(&release, &starter_set());
    h.click_text("Retry");
    settle(&mut h);
    assert_eq!(
        h.shell
            .st
            .session
            .library
            .get("tbesh")
            .map(|e| e.manifest.revision),
        Some(1)
    );
}

#[test]
fn a_restored_starter_library_tab_loads_the_released_list_on_relaunch() {
    let lab = Lab::new("restored");
    let release = lab.root.join("release");
    write_starter_bundle(&release, &starter_set());
    let db = lab.root.join("user.db");
    let base = obs_engine::sword_repo::file_url(&release);
    let setup = |s: &mut obs_engine::Session| {
        s.library.set_directory(lab.root.join("library"));
        s.set_search_cache_dir(lab.root.join("cache"));
        s.starter.set_download_base(&base);
        s.sync_starter_library(None)
    };
    {
        let mut h = Harness::launch(Some(&db));
        assert!(setup(&mut h.shell.st.session).offer_download);
        open_starter_library(&mut h);
        assert!(h.shell.st.session.starter.manifest().is_some());
        // Quit with the Tab open, nothing downloaded.
        h.shell.save_session(true);
    }
    // Relaunch as the app does: the Last session restores the Tab before any command runs.
    let mut session = obs_engine::Session::new(obs_engine::JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(&db).unwrap();
    session.attach_user(Box::new(store));
    assert!(!setup(&mut session).offer_download);
    let mut shell = Shell::new(session);
    shell.restore_or_default();
    assert!(
        shell
            .st
            .session
            .workspace
            .tabs()
            .any(|t| t.kind == obs_engine::TabKind::StarterLibrary)
    );
    let mut h = Harness::from_shell(shell);
    settle(&mut h);
    let s = &h.shell.st.session;
    assert!(s.starter.manifest().is_some(), "{:?}", s.starter.error());
    let label = format!(
        "Download Starter library (≈{} MB)",
        s.starter_wanted_bytes().div_ceil(1_000_000)
    );
    assert!(h.find(&label).is_some(), "{label}: {:?}", h.texts());
    assert!(h.find(TBESG).is_some(), "{:?}", h.texts());
}

/// A Starter KJV at `revision`: revision 1 has a typo before "God created", so the fixed
/// text moves that Highlight one byte left.
fn kjv(revision: u32) -> obs_model::Resource {
    use obs_engine::testing::bibles::{bible, v};
    use obs_scripture::Book;
    let first = if revision == 1 {
        "In the beginnning God created the heaven and the earth."
    } else {
        "In the beginning God created the heaven and the earth."
    };
    let mut r = bible(
        "kjv-strongs",
        "KJV",
        &[
            (v(Book::Genesis, 1, 1), first),
            (
                v(Book::Genesis, 1, 2),
                "And the earth was without form, and void.",
            ),
        ],
        None,
        &[],
    );
    r.manifest.origin = obs_model::Origin::Starter;
    r.manifest.revision = revision;
    r.manifest.license = "LicenseRef-PublicDomain".into();
    r
}

/// The text of the open KJV Tab, and the text under its Highlight.
fn kjv_tab(h: &mut Harness, tab: u64) -> (String, String) {
    let st = &mut h.shell.st;
    let doc = st.tabs[&obs_engine::TabId(tab)]
        .view
        .as_ref()
        .unwrap()
        .doc
        .clone();
    let marks = st.marks.marks(&st.session, &doc);
    let highlights: Vec<_> = marks
        .iter()
        .filter(|m| matches!(m.kind, crate::marks::MarkKind::Highlight(_)))
        .collect();
    assert_eq!(highlights.len(), 1, "{marks:?}");
    let m = highlights[0];
    assert_eq!(m.start.unit, m.end.unit);
    let text = &doc.units[m.start.unit].text;
    (
        doc.units.iter().map(|u| u.text.as_str()).collect(),
        text[m.start.byte..m.end.byte].to_string(),
    )
}

#[test]
fn a_downloaded_revision_reloads_the_open_tab_and_its_highlight() {
    let lab = Lab::new("revision");
    let release = lab.root.join("release");
    write_starter_bundle(&release, &[kjv(1)]);
    let mut h = Harness::launch(Some(&lab.root.join("user.db")));
    {
        let s = &mut h.shell.st.session;
        s.library.set_directory(lab.root.join("library"));
        s.set_search_cache_dir(lab.root.join("cache"));
        s.starter
            .set_download_base(&obs_engine::sword_repo::file_url(&release));
    }
    open_starter_library(&mut h);
    h.shell.run("starter.add", &json!({}));
    settle(&mut h);
    let out = h
        .shell
        .run("resource.open", &json!({ "id": "kjv-strongs" }))
        .unwrap();
    let tab = out["tab"].as_u64().unwrap();
    let v1 = kjv(1);
    let at = v1.units[0].text.find("God created").unwrap();
    h.shell
        .run(
            "annotation.highlight",
            &json!({
                "resource": "kjv-strongs",
                "start_unit": v1.units[0].id.as_str(),
                "start": at,
                "end": at + "God created".len(),
            }),
        )
        .unwrap();
    h.frames(2);
    let (text, marked) = kjv_tab(&mut h, tab);
    assert!(text.contains("beginnning"), "{text}");
    assert_eq!(marked, "God created");

    // Revision 2 is released and downloaded while the Tab is open.
    write_starter_bundle(&release, &[kjv(2)]);
    h.shell.run("starter.check", &json!({}));
    settle(&mut h);
    h.shell.run("starter.add", &json!({}));
    settle(&mut h);
    assert_eq!(
        h.shell
            .st
            .session
            .library
            .get("kjv-strongs")
            .unwrap()
            .manifest
            .revision,
        2
    );
    // The open Tab reads the new text, and the Highlight is still over "God created".
    let (text, marked) = kjv_tab(&mut h, tab);
    assert!(text.contains("In the beginning God"), "{text}");
    assert!(!text.contains("beginnning"), "{text}");
    assert_eq!(marked, "God created");

    // Closed and opened again: the new text, not a copy loaded before the revision.
    h.shell.run("tab.close", &json!({ "tab": tab }));
    let out = h
        .shell
        .run("resource.open", &json!({ "id": "kjv-strongs" }))
        .unwrap();
    let tab = out["tab"].as_u64().unwrap();
    h.frames(2);
    let (text, marked) = kjv_tab(&mut h, tab);
    assert!(!text.contains("beginnning"), "{text}");
    assert_eq!(marked, "God created");
}

#[test]
fn retry_after_a_failed_add_back_adds_that_resource_back() {
    let lab = Lab::new("retry-add-back");
    let release = lab.root.join("release");
    write_starter_bundle(&release, &starter_set());
    let mut h = Harness::launch(Some(&lab.root.join("user.db")));
    {
        let s = &mut h.shell.st.session;
        s.library.set_directory(lab.root.join("library"));
        s.set_search_cache_dir(lab.root.join("cache"));
        s.starter
            .set_download_base(&obs_engine::sword_repo::file_url(&release));
    }
    open_starter_library(&mut h);
    h.shell.run("starter.add", &json!({}));
    settle(&mut h);
    h.shell
        .run("library.remove", &json!({ "id": "bdb", "confirm": true }));
    let bdb = release.join("bdb.obr");
    let good = std::fs::read(&bdb).unwrap();
    let mut bad = good.clone();
    bad[100] ^= 0xff;
    std::fs::write(&bdb, bad).unwrap();
    let tab = h.tab_at("Starter library");
    h.click(tab);
    let add_back = row_button(&h, BDB, "Add back");
    h.click(add_back);
    settle(&mut h);
    assert!(
        h.texts().iter().any(|t| t.contains("failed verification")),
        "{:?}",
        h.texts()
    );
    std::fs::write(&bdb, good).unwrap();
    h.click_text("Retry");
    settle(&mut h);
    assert!(h.shell.st.session.library.get("bdb").is_some());
    assert!(h.shell.st.session.starter.error().is_none());
}
