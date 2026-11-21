//! Get Resources → Books (ADR 0024, #347) in the engine, against the mock of COL's
//! repository: connect, the list, a download file by file, everything by tarball, a local
//! copy, Check for new books, plain errors.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use obs_formats::sword_repo::FetchError;
use obs_model::ResourceType;
use serde_json::json;

use crate::col_get::{ColError, SessionSecret};
use crate::testing::MockCol;
use crate::*;

const TOKEN: &str = "github_pat_UNIT0123";

struct Lab(PathBuf);

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn session(name: &str) -> (Session, Arc<MockCol>, Lab) {
    let root = std::env::temp_dir().join(format!("obs-col-get-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("library")).unwrap();
    let mut s = Session::new(JobMode::Inline);
    s.library.set_directory(root.join("library"));
    let (user, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(user));
    let col = Arc::new(MockCol::new(TOKEN));
    s.set_fetch(col.clone());
    s.col.set_api(MockCol::API);
    s.col.set_secrets(Arc::new(SessionSecret::default()));
    (s, col, Lab(root))
}

fn settle(s: &mut Session) {
    for _ in 0..200 {
        s.jobs().pump(64);
        s.poll_imports();
        if !s.col.is_loading() && !s.col.is_loading_public() && !s.imports.is_running() {
            return;
        }
    }
    panic!("never settled");
}

fn books(s: &Session) -> Vec<String> {
    let mut t: Vec<String> = s
        .library
        .of_type(ResourceType::Book)
        .map(|e| e.manifest.title.clone())
        .collect();
    t.sort();
    t
}

#[test]
fn connecting_reads_the_list_and_downloads_go_file_by_file_or_by_tarball() {
    let (mut s, col, _lab) = session("github");
    s.col_restore();
    assert!(
        col.seen.lock().unwrap().is_empty(),
        "nothing asked before Connect"
    );
    s.run("col.connect_token", &json!({ "token": TOKEN }))
        .unwrap();
    settle(&mut s);
    assert!(s.col.error().is_none(), "{:?}", s.col.error());
    let catalog = s.col.catalog().unwrap().clone();
    assert_eq!(catalog.works.len(), 5);
    assert!(
        col.seen.lock().unwrap().iter().all(|(_, auth)| *auth),
        "every request to the API carries the token"
    );
    // Filters and sizes.
    let out = s
        .run("col.filter", &json!({ "tradition": "Puritan" }))
        .unwrap();
    assert_eq!(out["shown"], 1);
    s.run("col.select", &json!({})).unwrap();
    let chosen = s.col.chosen();
    assert_eq!(chosen.len(), 1);
    let (n, bytes) = crate::col_get::size_of(&chosen);
    assert_eq!(n, 1);
    assert!(bytes > 0);
    // One work, file by file (by blob).
    s.run("col.download", &json!({ "scope": "selected" }))
        .unwrap();
    settle(&mut s);
    assert_eq!(books(&s), ["Of the Mortification of Sin"]);
    assert!(
        col.seen
            .lock()
            .unwrap()
            .iter()
            .any(|(u, _)| u.contains("/git/blobs/"))
    );
    // Everything, by tarball.
    s.run("col.filter", &json!({})).unwrap();
    s.run("col.download", &json!({ "scope": "everything" }))
        .unwrap();
    settle(&mut s);
    assert_eq!(books(&s).len(), 5);
    assert!(
        col.seen
            .lock()
            .unwrap()
            .iter()
            .any(|(u, _)| u.contains("/tarball/"))
    );
    // Each COL Book has its origin, the same id as a second download would give it.
    let owen = obs_engine_col_id("EEBO-TCP/John Owen/Owen - Mortification.md");
    let origin = s.col_books().get(&owen).cloned().unwrap();
    assert_eq!(origin.via, "github");
    assert_eq!(origin.blobs.len(), 3);
    let entry = s.library.get(&owen).unwrap();
    assert!(
        entry
            .manifest
            .attribution
            .contains("From the Christian Open Library (https://christianopenlibrary.org)")
    );

    // Check for new books: one changed, one new, one removed.
    col.put(
        "EEBO-TCP/John Owen/Owen - Mortification — Part 2.md",
        "## Chapter II\n\nThe second chapter, corrected.\n",
    );
    col.put(
        "Sermons/New.md",
        "---\ntitle: A New Sermon\n---\n\n## One\n\nText.\n",
    );
    col.remove("Sermons/Edwards - Sinners.md");
    s.run("col.refresh", &json!({})).unwrap();
    settle(&mut s);
    let changes = s.col.changes().unwrap().clone();
    assert_eq!(changes.changed.len(), 1);
    assert_eq!(changes.new, ["Sermons/New.md"]);
    assert_eq!(changes.removed.len(), 1);
    let revision = s.library.get(&owen).unwrap().manifest.revision;
    s.run("col.update", &json!({})).unwrap();
    settle(&mut s);
    assert_eq!(
        s.library.get(&owen).unwrap().manifest.revision,
        revision + 1
    );
    assert_eq!(books(&s).len(), 5, "the removed book is kept");
    assert_eq!(
        s.run("col.update", &json!({})).unwrap_err().to_string(),
        "`col.update` failed: Every COL Book is up to date"
    );
}

fn obs_engine_col_id(path: &str) -> String {
    crate::col::col_resource_id(path)
}

#[test]
fn errors_say_what_went_wrong_in_plain_words() {
    let (mut s, col, _lab) = session("errors");
    for (refuse, want) in [
        (FetchError::NotFound, ColError::NoAccess),
        (FetchError::RateLimited, ColError::RateLimited),
        (
            FetchError::Network("refused".into()),
            ColError::Offline("refused".into()),
        ),
        (FetchError::Unauthorized, ColError::BadToken),
    ] {
        *col.refuse.lock().unwrap() = Some(refuse);
        s.run("col.connect_token", &json!({ "token": TOKEN }))
            .unwrap();
        settle(&mut s);
        assert_eq!(s.col.error(), Some(&want));
        assert!(!want.message().contains(TOKEN));
    }
    // A refused token is forgotten; the others are kept to try again.
    assert!(s.col.access().is_none());
    assert!(
        s.run("col.connect_token", &json!({ "token": "not a token!" }))
            .is_err()
    );
}

#[test]
fn a_local_copy_needs_no_sign_in_and_no_network() {
    let (mut s, col, lab) = session("local");
    let copy = lab.0.join("copy");
    for (path, bytes) in crate::testing::col_sample_files() {
        let file = copy.join(&path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    }
    s.run("col.connect_local", &json!({ "path": copy }))
        .unwrap();
    settle(&mut s);
    assert!(s.col.error().is_none(), "{:?}", s.col.error());
    assert_eq!(s.col.catalog().unwrap().works.len(), 5);
    s.run(
        "col.download",
        &json!({ "paths": "Creeds/Westminster Confession.md" }),
    )
    .unwrap();
    settle(&mut s);
    assert_eq!(books(&s), ["The Westminster Confession of Faith"]);
    assert!(col.seen.lock().unwrap().is_empty(), "nothing fetched");
    // The same blob SHAs as the repository's: up to date.
    s.run("col.refresh", &json!({})).unwrap();
    settle(&mut s);
    assert_eq!(s.col.changes().unwrap().current, 1);
    // A folder that is not a copy.
    std::fs::create_dir_all(lab.0.join("empty")).unwrap();
    s.run("col.connect_local", &json!({ "path": lab.0.join("empty") }))
        .unwrap();
    settle(&mut s);
    assert!(matches!(s.col.error(), Some(ColError::NotAClone(_))));
}
#[test]
fn search_christian_open_library_opens_the_website_and_sends_nothing() {
    let (mut s, col, _lab) = session("website");
    assert!(s.col_search_link(), "on by default");
    let out = s
        .run("col.search_website", &json!({ "text": "Romans 8:28" }))
        .unwrap();
    let url = "https://christianopenlibrary.org/#q=Romans%208%3A28";
    assert_eq!(out["url"], url);
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::OpenUrl(url.into()))
    );
    assert!(
        col.seen.lock().unwrap().is_empty(),
        "the app fetched nothing"
    );
    assert!(
        s.run("col.search_website", &json!({ "text": "  " }))
            .is_err()
    );
    // The Setting hides it, and is kept.
    s.run("col.search_link", &json!({ "on": false })).unwrap();
    assert!(!s.col_search_link());
    let setting = crate::settings::SETTINGS
        .iter()
        .find(|st| st.id == "col_search_link")
        .unwrap();
    assert_eq!(setting.section, crate::settings::SettingsSection::Search);
    assert_eq!(s.setting_is_default("col_search_link"), Some(false));
    s.col = Default::default();
    assert!(!s.col_search_link(), "read back from the User database");
}
