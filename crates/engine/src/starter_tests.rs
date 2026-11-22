//! The Starter library in the Library (#131): first run from a bundle, removals and Add back,
//! revisions, and the verified download, through the Session with real `.obr` files and a real
//! User database in a temp folder.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use obs_model::{Key, Origin, Resource, ResourceType};
use obs_scripture::Book;
use serde_json::json;

use crate::starter::{StarterManifest, StarterStatus, bundle_candidates, features};
use crate::sword_repo::{Fetch, FetchError, LocalFetch, Progress, file_url};
use crate::testing::bibles::{bible, v};
use crate::*;

/// A temp folder holding a bundle (`starter/`), a release folder (`release/`), the library
/// folder and the User database.
struct Lab {
    root: PathBuf,
}

impl Lab {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-starter-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn bundle(&self) -> PathBuf {
        self.root.join("starter")
    }

    fn release(&self) -> PathBuf {
        self.root.join("release")
    }

    fn lib(&self) -> PathBuf {
        self.root.join("library")
    }

    /// Launch as the app does: scan the library folder, attach the User database, then sync
    /// the Starter library from `bundle`.
    fn launch(&self, bundle: Option<&Path>) -> (Session, starter::StarterSync) {
        let mut s = Session::new(JobMode::Inline);
        s.set_search_cache_dir(self.root.join("cache"));
        let problems = s.library.scan(&self.lib());
        assert!(problems.is_empty(), "{problems:?}");
        let (store, _) = obs_store::UserStore::open(&self.root.join("user.db")).unwrap();
        s.attach_user(Box::new(store));
        let sync = s.sync_starter_library(bundle);
        settle(&mut s);
        (s, sync)
    }

    /// A session with no bundle that downloads from `release/` through `fetch`.
    fn downloader(&self, fetch: Arc<dyn Fetch>) -> Session {
        let (mut s, sync) = self.launch(None);
        assert!(sync.offer_download || s.starter.state().is_some());
        s.set_fetch(fetch);
        s.starter.set_download_base(&file_url(&self.release()));
        s.check_starter_release().unwrap();
        settle(&mut s);
        assert!(s.starter.manifest().is_some(), "{:?}", s.starter.error());
        s
    }

    /// Every file under the library folder, relative, sorted.
    fn files(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![self.lib()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.filter_map(Result::ok) {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(
                        p.strip_prefix(self.lib())
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        out.sort();
        out
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Run queued jobs and apply their results.
fn settle(s: &mut Session) {
    for _ in 0..50 {
        s.jobs().pump(64);
        let _ = s.poll_imports();
        if s.starter.download().is_none()
            && !s.starter.is_checking()
            && !s.reanchoring.is_busy()
            && !s.search.is_building()
        {
            return;
        }
    }
    panic!("background work did not finish");
}

/// A public-domain Bible with Starter id `id`: Genesis 1:1-3.
fn starter_bible(id: &str, abbreviation: &str, first: &str, revision: u32) -> Resource {
    let verses = [
        (v(Book::Genesis, 1, 1), first),
        (
            v(Book::Genesis, 1, 2),
            "And the earth was without form, and void; and darkness was upon the face of the deep.",
        ),
        (
            v(Book::Genesis, 1, 3),
            "And God said, Let there be light: and there was light.",
        ),
    ];
    let mut r = bible(id, abbreviation, &verses, None, &[]);
    r.manifest.origin = Origin::Starter;
    r.manifest.revision = revision;
    r.manifest.license = "LicenseRef-PublicDomain".into();
    r
}

fn starter_set() -> Vec<Resource> {
    testing::starter_set()
}

fn ids(s: &Session, ty: ResourceType) -> Vec<String> {
    s.library
        .of_type(ty)
        .map(|e| e.manifest.resource_id.clone())
        .collect()
}

fn lexicon_entries(s: &mut Session, number: &str) -> Vec<LexiconEntry> {
    let key = Key::StrongsNumber(number.parse().unwrap());
    for _ in 0..50 {
        match s.guide_section(&key, GuideSection::LexiconEntries) {
            SectionStatus::Ready(d) => match &*d {
                SectionData::LexiconEntries(e) => return e.clone(),
                other => panic!("{other:?}"),
            },
            SectionStatus::Failed(e) => panic!("{e}"),
            SectionStatus::Loading => {
                s.jobs().pump(16);
                let _ = s.poll_imports();
            }
        }
    }
    panic!("Lexicon entries never finished");
}

#[test]
fn the_bundle_is_looked_for_in_the_documented_order() {
    let exe = Path::new("/opt/obs/bin/openbible");
    let workspace = PathBuf::from("/src/obs/dist/starter");
    assert_eq!(
        bundle_candidates(None, Some(exe), Some(workspace.clone())),
        vec![
            PathBuf::from("/opt/obs/bin/starter"),
            PathBuf::from("/opt/obs/bin/../Resources/starter"),
            workspace.clone(),
        ]
    );
    // $OBS_STARTER_DIR alone decides, so it can also say "no bundle".
    assert_eq!(
        bundle_candidates(Some("/x".into()), Some(exe), Some(workspace)),
        vec![PathBuf::from("/x")]
    );
    assert!(bundle_candidates(None, None, None).is_empty());
}

#[test]
fn first_run_adds_every_starter_resource_with_caches_records_and_preferred_order() {
    let lab = Lab::new("first-run");
    testing::write_starter_bundle(&lab.bundle(), &starter_set());
    let (mut s, sync) = lab.launch(Some(&lab.bundle()));
    assert!(sync.problems.is_empty(), "{:?}", sync.problems);
    assert_eq!(sync.added.len(), 5, "{sync:?}");
    assert!(!sync.offer_download);
    // STARTER_PREFERRED_ORDER applies, as for an Import.
    assert_eq!(
        ids(&s, ResourceType::Lexicon),
        ["tbesh", "tbesg", "bdb", "strongs-hebrew"]
    );
    assert_eq!(
        s.library
            .preferred(ResourceType::Bible)
            .unwrap()
            .manifest
            .resource_id,
        "kjv-strongs"
    );
    // The files are in the library folder, with their prebuilt Search index caches and the
    // CC BY change records; nothing is left staged.
    let files = lab.files();
    for id in ["kjv-strongs", "tbesh", "tbesg", "bdb", "strongs-hebrew"] {
        assert!(files.contains(&format!("{id}.obr")), "{files:?}");
        assert!(
            files
                .iter()
                .any(|f| f.starts_with(&format!("search/{id}."))),
            "{files:?}"
        );
    }
    assert!(files.contains(&"CHANGES-tbesg.md".to_string()), "{files:?}");
    assert!(!files.iter().any(|f| f.ends_with(".part")), "{files:?}");
    // The prebuilt caches are used as they are: nothing rebuilds.
    for id in ["kjv-strongs", "tbesg"] {
        assert!(s.search.is_ready(id), "{id}: {:?}", s.search.status(id));
        assert!(
            s.search
                .path(id)
                .unwrap()
                .starts_with(lab.lib().join("search")),
            "{:?}",
            s.search.path(id)
        );
    }
    // Word Study on G26 shows Lexicon entries.
    let entries = lexicon_entries(&mut s, "G26");
    assert!(
        entries.iter().any(|e| e.resource_id == "tbesg"),
        "{entries:?}"
    );
    // A second launch changes nothing.
    drop(s);
    let (s, sync) = lab.launch(Some(&lab.bundle()));
    assert_eq!(sync, starter::StarterSync::default());
    assert_eq!(s.library.entries().len(), 5);
}

#[test]
fn a_removed_starter_resource_stays_removed_and_add_back_restores_it() {
    let lab = Lab::new("removed");
    testing::write_starter_bundle(&lab.bundle(), &starter_set());
    let (mut s, _) = lab.launch(Some(&lab.bundle()));
    s.run("library.remove", &json!({ "id": "bdb", "confirm": true }))
        .unwrap();
    assert!(!lab.lib().join("bdb.obr").exists());
    drop(s);

    // Not added back on the next launch; listed as Removed.
    let (mut s, sync) = lab.launch(Some(&lab.bundle()));
    assert!(sync.added.is_empty(), "{sync:?}");
    assert!(s.library.get("bdb").is_none());
    let row = |s: &Session, id: &str| {
        s.starter_rows()
            .into_iter()
            .find(|r| r.resource_id == id)
            .unwrap()
    };
    assert_eq!(row(&s, "bdb").status, StarterStatus::Removed);
    assert_eq!(row(&s, "tbesh").status, StarterStatus::InLibrary);
    // "Add all missing" leaves it out too: removals are the user's choice.
    assert!(s.starter_wanted().is_empty());
    let listed = s.run("starter.list", &json!({})).unwrap();
    let bdb = listed["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "bdb")
        .unwrap()
        .clone();
    assert_eq!(bdb["status"], "removed");

    // Add back: like any Resource that left the Preferred order and came back (a re-Import),
    // it joins the bottom of its type; the order the user has is kept.
    let out = s.run("starter.add", &json!({ "id": "bdb" })).unwrap();
    assert_eq!(out["ids"], json!(["bdb"]));
    assert!(out["job"].is_null(), "from the bundle, at once");
    assert_eq!(
        ids(&s, ResourceType::Lexicon),
        ["tbesh", "tbesg", "strongs-hebrew", "bdb"]
    );
    assert!(lab.files().contains(&"bdb.obr".to_string()));
    assert!(lab.files().iter().any(|f| f.starts_with("search/bdb.")));
    assert_eq!(row(&s, "bdb").status, StarterStatus::InLibrary);
    drop(s);
    let (s, _) = lab.launch(Some(&lab.bundle()));
    assert!(s.library.get("bdb").is_some());
    assert!(!s.starter.state().unwrap().removed.contains("bdb"));
}

#[test]
fn a_starter_file_deleted_outside_the_app_is_offered_not_added_back_silently() {
    let lab = Lab::new("deleted");
    testing::write_starter_bundle(&lab.bundle(), &starter_set());
    drop(lab.launch(Some(&lab.bundle())));
    std::fs::remove_file(lab.lib().join("strongs-hebrew.obr")).unwrap();
    let (s, sync) = lab.launch(Some(&lab.bundle()));
    assert!(sync.added.is_empty());
    let wanted: Vec<String> = s
        .starter_wanted()
        .into_iter()
        .map(|i| i.resource_id)
        .collect();
    assert_eq!(wanted, ["strongs-hebrew"]);
}

#[test]
fn a_starter_resource_new_to_the_bundle_is_added_on_the_next_launch() {
    let lab = Lab::new("new-in-bundle");
    let mut set = starter_set();
    let web = starter_bible(
        "web",
        "WEB",
        "In the beginning, God created the heavens and the earth.",
        1,
    );
    testing::write_starter_bundle(&lab.bundle(), &set);
    drop(lab.launch(Some(&lab.bundle())));
    set.push(web);
    testing::write_starter_bundle(&lab.bundle(), &set);
    let (s, sync) = lab.launch(Some(&lab.bundle()));
    assert_eq!(sync.added, ["web"]);
    assert_eq!(ids(&s, ResourceType::Bible), ["kjv-strongs", "web"]);
}

#[test]
fn a_newer_starter_revision_replaces_the_old_one_and_its_annotations_still_show() {
    let lab = Lab::new("revision");
    let old = starter_bible(
        "kjv-strongs",
        "KJV",
        "In the beginnning God created the heaven and the earth.",
        1,
    );
    let web = starter_bible(
        "web",
        "WEB",
        "In the beginning, God created the heavens and the earth.",
        1,
    );
    testing::write_starter_bundle(&lab.bundle(), &[old.clone(), web.clone()]);
    let highlight = {
        let (mut s, _) = lab.launch(Some(&lab.bundle()));
        // The user ranks the WEB first: the revision must keep that.
        s.run("library.move", &json!({ "id": "web", "to": 0 }))
            .unwrap();
        let text = &old.units[0].text;
        let at = text.find("God created").unwrap();
        let out = s
            .run(
                "annotation.highlight",
                &json!({
                    "resource": "kjv-strongs",
                    "start_unit": old.units[0].id.as_str(),
                    "start": at,
                    "end": at + "God created".len(),
                }),
            )
            .unwrap();
        out["ids"][0].as_str().unwrap().to_string()
    };

    // A corrected release: the typo fixed, revision 2.
    let new = starter_bible(
        "kjv-strongs",
        "KJV",
        "In the beginning God created the heaven and the earth.",
        2,
    );
    testing::write_starter_bundle(&lab.bundle(), &[new.clone(), web]);
    let (s, sync) = lab.launch(Some(&lab.bundle()));
    assert_eq!(sync.updated, ["kjv-strongs"], "{sync:?}");
    assert!(sync.added.is_empty());
    let entry = s.library.get("kjv-strongs").unwrap();
    assert_eq!(entry.manifest.revision, 2);
    assert_eq!(entry.manifest.content_hash, new.manifest.content_hash);
    assert_eq!(entry.path, lab.lib().join("kjv-strongs.obr"));
    // Same identity: its Preferred order and its Annotation carry over.
    assert_eq!(ids(&s, ResourceType::Bible), ["web", "kjv-strongs"]);
    assert_eq!(s.library.entries().len(), 2);
    let a = s.annotation(&highlight).unwrap();
    let range = a.anchor.as_ref().unwrap().text_range().unwrap().clone();
    assert!(!range.orphaned);
    assert_eq!(range.content_hash, new.manifest.content_hash);
    let text = &new.units[0].text;
    assert_eq!(
        &text[range.start as usize..range.end as usize],
        "God created"
    );
    assert_eq!(s.annotations_referring("kjv-strongs"), 1);
    assert!(s.orphaned_annotations().unwrap().is_empty());
}

#[test]
fn rows_show_type_size_licence_and_the_features_each_powers() {
    let lab = Lab::new("rows");
    testing::write_starter_bundle(&lab.bundle(), &starter_set());
    let (s, _) = lab.launch(Some(&lab.bundle()));
    let rows = s.starter_rows();
    let tbesg = rows.iter().find(|r| r.resource_id == "tbesg").unwrap();
    assert_eq!(tbesg.resource_type, Some(ResourceType::Lexicon));
    assert_eq!(tbesg.license, "CC-BY-4.0");
    assert_eq!(tbesg.abbreviation, "TBESG");
    assert!(tbesg.bytes > 0);
    assert!(tbesg.features.contains(&"Word Study: Lexicon entries"));
    let kjv = rows
        .iter()
        .find(|r| r.resource_id == "kjv-strongs")
        .unwrap();
    assert!(
        kjv.features
            .contains(&"Word Study: Translated as and Occurrences")
    );
    assert_eq!(
        features(Some(ResourceType::Bible), "tahot", "hbo")[0],
        "Reading the Hebrew or Greek text"
    );
    assert_eq!(
        features(Some(ResourceType::Commentary), "mhc", "en"),
        ["Passage Guide: Commentaries"]
    );
}

#[test]
fn with_no_bundle_the_first_launch_offers_the_download_once() {
    let lab = Lab::new("offer");
    let (s, sync) = lab.launch(None);
    assert!(sync.offer_download);
    assert!(s.starter.state().unwrap().offered);
    drop(s);
    let (_, sync) = lab.launch(None);
    assert!(!sync.offer_download);
}

#[test]
fn one_click_downloads_verifies_and_adds_the_starter_library() {
    let lab = Lab::new("download");
    testing::write_starter_bundle(&lab.release(), &starter_set());
    let mut s = lab.downloader(Arc::new(LocalFetch));
    let manifest_bytes: u64 = s
        .starter
        .manifest()
        .unwrap()
        .resources
        .iter()
        .map(|r| r.bytes)
        .sum();
    assert_eq!(s.starter_wanted_bytes(), manifest_bytes);
    let out = s.run("starter.add", &json!({})).unwrap();
    assert!(out["job"].is_u64());
    assert_eq!(out["ids"].as_array().unwrap().len(), 5);
    assert!(
        s.starter_rows()
            .iter()
            .all(|r| r.status == StarterStatus::Adding)
    );
    settle(&mut s);
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    assert_eq!(s.library.entries().len(), 5);
    assert_eq!(
        ids(&s, ResourceType::Lexicon),
        ["tbesh", "tbesg", "bdb", "strongs-hebrew"]
    );
    assert!(
        s.starter_rows()
            .iter()
            .all(|r| r.status == StarterStatus::InLibrary)
    );
    assert_eq!(s.starter_wanted_bytes(), 0);
    // No prebuilt caches come with a download: the Search indexes are built here.
    for id in ["kjv-strongs", "tbesg"] {
        assert!(s.search.is_ready(id), "{id}: {:?}", s.search.status(id));
    }
    let entries = lexicon_entries(&mut s, "G26");
    assert!(entries.iter().any(|e| e.resource_id == "tbesg"));
}

#[test]
fn a_sha256_mismatch_fails_loudly_and_leaves_nothing_half_added() {
    let lab = Lab::new("mismatch");
    testing::write_starter_bundle(&lab.release(), &starter_set());
    // The release's BDB is not the file the manifest names.
    let bdb = lab.release().join("bdb.obr");
    let mut bytes = std::fs::read(&bdb).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&bdb, bytes).unwrap();
    let mut s = lab.downloader(Arc::new(LocalFetch));
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    let error = s.starter.error().unwrap().to_string();
    assert!(
        error.contains("bdb.obr failed verification") && error.contains("Nothing was added"),
        "{error}"
    );
    // Loudly: a message, as well as the Tab's error.
    let effects = s.workspace.take_effects();
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Message(m) if m.contains("failed verification"))),
        "{effects:?}"
    );
    assert!(s.library.entries().is_empty());
    assert!(lab.files().is_empty(), "{:?}", lab.files());
    // Retry: the same click again, once the release is right.
    testing::write_starter_bundle(&lab.release(), &starter_set());
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    assert!(s.starter.error().is_none());
    assert_eq!(s.library.entries().len(), 5);
}

/// `file://` like [`LocalFetch`], but the download is cancelled at the `n`th file, as a
/// Cancel click mid-download would.
#[derive(Debug)]
struct CancelAt {
    n: usize,
    seen: Mutex<usize>,
}

impl Fetch for CancelAt {
    fn get(&self, url: &str, progress: Progress<'_>) -> Result<Vec<u8>, FetchError> {
        let mut seen = self.seen.lock().unwrap();
        *seen += 1;
        if url.ends_with(".obr") && *seen > self.n {
            return Err(FetchError::Cancelled);
        }
        LocalFetch.get(url, progress)
    }
}

#[test]
fn a_cancelled_download_leaves_nothing_and_can_be_retried() {
    let lab = Lab::new("cancel");
    testing::write_starter_bundle(&lab.release(), &starter_set());
    // The manifest, then two `.obr` files, then the cancel.
    let mut s = lab.downloader(Arc::new(CancelAt {
        n: 3,
        seen: Mutex::new(0),
    }));
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    assert_eq!(s.starter.error(), Some("Download cancelled"));
    assert!(s.library.entries().is_empty());
    assert!(lab.files().is_empty(), "{:?}", lab.files());
    s.set_fetch(Arc::new(LocalFetch));
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    assert_eq!(s.library.entries().len(), 5);
}

#[test]
fn starter_cancel_stops_a_running_download() {
    let lab = Lab::new("cancel-cmd");
    testing::write_starter_bundle(&lab.release(), &starter_set());
    let mut s = lab.downloader(Arc::new(LocalFetch));
    s.run("starter.add", &json!({})).unwrap();
    s.run("starter.cancel", &json!({})).unwrap();
    settle(&mut s);
    assert!(s.starter.download().is_none());
    assert!(s.library.entries().is_empty());
    assert!(lab.files().is_empty(), "{:?}", lab.files());
}

#[test]
fn a_newer_released_revision_is_offered_as_an_update_and_downloaded_in_place() {
    let lab = Lab::new("release-revision");
    let v1 = starter_bible(
        "kjv-strongs",
        "KJV",
        "In the beginnning God created the heaven and the earth.",
        1,
    );
    testing::write_starter_bundle(&lab.release(), std::slice::from_ref(&v1));
    let mut s = lab.downloader(Arc::new(LocalFetch));
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    let v2 = starter_bible(
        "kjv-strongs",
        "KJV",
        "In the beginning God created the heaven and the earth.",
        2,
    );
    testing::write_starter_bundle(&lab.release(), std::slice::from_ref(&v2));
    s.check_starter_release().unwrap();
    settle(&mut s);
    assert_eq!(
        s.starter_rows()[0].status,
        StarterStatus::Outdated {
            installed: 1,
            offered: 2
        }
    );
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    assert_eq!(s.library.get("kjv-strongs").unwrap().manifest.revision, 2);
    assert_eq!(s.library.entries().len(), 1);
}

/// The KJV at `revision` and the WEB, as two Starter Bibles.
fn two_bibles(revision: u32) -> (Resource, Resource) {
    (
        starter_bible(
            "kjv-strongs",
            "KJV",
            "In the beginnning God created the heaven and the earth.",
            revision,
        ),
        starter_bible(
            "web",
            "WEB",
            "In the beginning, God created the heavens and the earth.",
            1,
        ),
    )
}

#[test]
fn a_failed_commit_of_a_fresh_download_takes_back_every_file_already_in_place() {
    let lab = Lab::new("commit-fresh");
    let (kjv, web) = two_bibles(1);
    testing::write_starter_bundle(&lab.release(), &[kjv, web]);
    let mut s = lab.downloader(Arc::new(LocalFetch));
    // The first file goes in place; something is in the way of the second.
    let order: Vec<String> = s
        .starter
        .manifest()
        .unwrap()
        .resources
        .iter()
        .map(|r| r.resource_id.clone())
        .collect();
    assert_eq!(order, ["kjv-strongs", "web"]);
    std::fs::create_dir_all(lab.lib().join("web.obr")).unwrap();
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    let error = s.starter.error().unwrap();
    assert!(error.contains("web.obr"), "{error}");
    assert!(s.library.entries().is_empty());
    // Not even the first one stays: nothing half-added, nothing staged.
    assert!(lab.files().is_empty(), "{:?}", lab.files());
    assert!(s.starter.state().is_none_or(|st| st.known.is_empty()));
}

#[test]
fn a_failed_commit_of_a_revision_restores_the_earlier_revision() {
    let lab = Lab::new("commit-revision");
    let (v1, web) = two_bibles(1);
    testing::write_starter_bundle(&lab.release(), std::slice::from_ref(&v1));
    let mut s = lab.downloader(Arc::new(LocalFetch));
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    let kjv = lab.lib().join("kjv-strongs.obr");
    let earlier = std::fs::read(&kjv).unwrap();
    // Revision 2 of the KJV and a new WEB, which cannot be put in place.
    let (v2, _) = two_bibles(2);
    testing::write_starter_bundle(&lab.release(), &[v2, web]);
    s.check_starter_release().unwrap();
    settle(&mut s);
    std::fs::create_dir_all(lab.lib().join("web.obr")).unwrap();
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    assert!(s.starter.error().unwrap().contains("web.obr"));
    // The earlier revision is back, byte for byte, and still the one in the Library.
    assert_eq!(std::fs::read(&kjv).unwrap(), earlier);
    assert_eq!(s.library.get("kjv-strongs").unwrap().manifest.revision, 1);
    assert!(s.library.get("web").is_none());
    let files = lab.files();
    assert!(
        !files
            .iter()
            .any(|f| f.ends_with(".part") || f.ends_with(".replaced")),
        "{files:?}"
    );
    assert!(
        !s.workspace
            .take_effects()
            .iter()
            .any(|e| matches!(e, Effect::Replaced(_)))
    );
    // Once the way is clear, Retry puts both in place.
    std::fs::remove_dir(lab.lib().join("web.obr")).unwrap();
    s.run("starter.retry", &json!({})).unwrap();
    settle(&mut s);
    assert_eq!(s.library.get("kjv-strongs").unwrap().manifest.revision, 2);
    assert!(s.library.get("web").is_some());
    assert!(!lab.files().iter().any(|f| f.ends_with(".replaced")));
}

#[test]
fn a_downloaded_revision_tells_the_app_to_reload_what_it_loaded_from_the_old_file() {
    let lab = Lab::new("replaced");
    let (v1, _) = two_bibles(1);
    testing::write_starter_bundle(&lab.release(), std::slice::from_ref(&v1));
    let mut s = lab.downloader(Arc::new(LocalFetch));
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    // A new Resource is not a replacement.
    assert!(
        !s.workspace
            .take_effects()
            .iter()
            .any(|e| matches!(e, Effect::Replaced(_)))
    );
    let (v2, _) = two_bibles(2);
    testing::write_starter_bundle(&lab.release(), std::slice::from_ref(&v2));
    s.check_starter_release().unwrap();
    settle(&mut s);
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    let effects = s.workspace.take_effects();
    assert!(
        effects.contains(&Effect::Replaced("kjv-strongs".into())),
        "{effects:?}"
    );
}

#[test]
fn retry_after_a_failed_or_cancelled_add_back_adds_that_resource_back() {
    let lab = Lab::new("retry-add-back");
    testing::write_starter_bundle(&lab.release(), &starter_set());
    let mut s = lab.downloader(Arc::new(LocalFetch));
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    s.run("library.remove", &json!({ "id": "bdb", "confirm": true }))
        .unwrap();
    // The only one not in the Library is removed: "add all missing" has nothing to add.
    assert!(s.starter_wanted().is_empty());

    // Cancelled: the first `.obr` fetched is cancelled.
    s.set_fetch(Arc::new(CancelAt {
        n: 0,
        seen: Mutex::new(0),
    }));
    s.run("starter.add", &json!({ "id": "bdb" })).unwrap();
    settle(&mut s);
    assert_eq!(s.starter.error(), Some("Download cancelled"));
    s.set_fetch(Arc::new(LocalFetch));
    let out = s.run("starter.retry", &json!({})).unwrap();
    assert_eq!(out["ids"], json!(["bdb"]));
    settle(&mut s);
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    assert!(s.library.get("bdb").is_some());

    // Failed: the release's BDB is not the file the manifest names.
    s.run("library.remove", &json!({ "id": "bdb", "confirm": true }))
        .unwrap();
    let bdb = lab.release().join("bdb.obr");
    let good = std::fs::read(&bdb).unwrap();
    let mut bad = good.clone();
    bad[100] ^= 0xff;
    std::fs::write(&bdb, bad).unwrap();
    s.run("starter.add", &json!({ "id": "bdb" })).unwrap();
    settle(&mut s);
    assert!(s.starter.error().unwrap().contains("failed verification"));
    std::fs::write(&bdb, good).unwrap();
    let out = s.run("starter.retry", &json!({})).unwrap();
    assert_eq!(out["ids"], json!(["bdb"]));
    settle(&mut s);
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    assert!(s.library.get("bdb").is_some());
    assert!(!s.starter.state().unwrap().removed.contains("bdb"));
}

#[test]
fn a_resource_being_downloaded_cannot_be_removed_until_the_download_ends() {
    let lab = Lab::new("remove-while-downloading");
    let (v1, _) = two_bibles(1);
    testing::write_starter_bundle(&lab.release(), std::slice::from_ref(&v1));
    let mut s = lab.downloader(Arc::new(LocalFetch));
    s.run("starter.add", &json!({})).unwrap();
    settle(&mut s);
    let (v2, _) = two_bibles(2);
    testing::write_starter_bundle(&lab.release(), std::slice::from_ref(&v2));
    s.check_starter_release().unwrap();
    settle(&mut s);
    // The newer revision's download is queued, not finished (inline jobs run when pumped).
    s.run("starter.add", &json!({})).unwrap();
    assert!(s.is_starter_downloading("kjv-strongs"));
    for confirm in [false, true] {
        let e = s
            .run(
                "library.remove",
                &json!({ "id": "kjv-strongs", "confirm": confirm }),
            )
            .unwrap_err()
            .to_string();
        assert!(e.contains("being downloaded"), "{e}");
    }
    assert!(lab.lib().join("kjv-strongs.obr").is_file());
    settle(&mut s);
    assert_eq!(s.library.get("kjv-strongs").unwrap().manifest.revision, 2);
    // Once it is in place, removing it sticks.
    s.run(
        "library.remove",
        &json!({ "id": "kjv-strongs", "confirm": true }),
    )
    .unwrap();
    assert!(s.library.get("kjv-strongs").is_none());
    assert!(s.starter.state().unwrap().removed.contains("kjv-strongs"));
    assert!(s.starter_wanted().is_empty());
}

#[test]
fn a_manifest_with_unsafe_or_repeated_ids_is_refused() {
    let item = |id: &str| {
        json!({ "resource_id": id, "revision": 1, "content_hash": "sha256:ab",
                "file_sha256": "cd", "bytes": 10 })
    };
    for id in [
        "../victim",
        "/tmp/victim",
        "a/b",
        "..",
        "KJV",
        "",
        "x\\..\\y",
    ] {
        let text = json!({ "format": 1, "resources": [item(id)] }).to_string();
        let e = StarterManifest::parse(&text).unwrap_err();
        assert!(e.contains("not a Resource id"), "{id:?}: {e}");
    }
    let text = json!({ "format": 1, "resources": [item("web"), item("web")] }).to_string();
    assert!(
        StarterManifest::parse(&text)
            .unwrap_err()
            .contains("more than once")
    );
}

#[test]
fn an_unsafe_id_never_touches_a_file_outside_the_library() {
    let lab = Lab::new("unsafe-id");
    let victim = lab.root.join("victim.obr.part");
    std::fs::write(&victim, "not yours").unwrap();
    let bytes = b"anything".to_vec();
    let item = |id: &str| starter::StarterItem {
        resource_id: id.into(),
        revision: 1,
        content_hash: String::new(),
        // The bytes match: a matching sha256 is no proof the path is safe.
        file_sha256: starter::sha256_hex(&bytes),
        bytes: bytes.len() as u64,
        title: String::new(),
        abbreviation: String::new(),
        resource_type: None,
        language: String::new(),
        license: String::new(),
        mobile_set: None,
    };
    let mut reads = 0;
    for items in [
        vec![item("../victim")],
        vec![item("web"), item("web")],
        vec![item("web"), item("/tmp/victim")],
    ] {
        let e = starter::install(&lab.lib(), &items, &mut |_| {
            reads += 1;
            Ok(bytes.clone())
        })
        .unwrap_err();
        assert!(
            e.contains("Resource id") || e.contains("more than once"),
            "{e}"
        );
    }
    assert_eq!(reads, 0, "refused before anything is read or written");
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "not yours");
    assert!(!lab.lib().exists());

    // From a release: the manifest itself is refused, and the Tab says so.
    std::fs::create_dir_all(lab.release()).unwrap();
    let manifest = json!({ "format": 1, "resources": [{
        "resource_id": "../victim", "revision": 1, "content_hash": "sha256:ab",
        "file_sha256": starter::sha256_hex(&bytes), "bytes": bytes.len() }] });
    std::fs::write(
        lab.release().join(starter::MANIFEST_FILE),
        manifest.to_string(),
    )
    .unwrap();
    let (mut s, _) = lab.launch(None);
    s.set_fetch(Arc::new(LocalFetch));
    s.starter.set_download_base(&file_url(&lab.release()));
    s.check_starter_release().unwrap();
    settle(&mut s);
    assert!(s.starter.manifest().is_none());
    assert!(s.starter.error().unwrap().contains("not a Resource id"));
    assert!(s.run("starter.add", &json!({})).is_err());
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "not yours");
}

#[test]
fn the_manifest_xtask_writes_parses_and_old_ones_still_do() {
    let old = r#"{"format":1,"resources":[{"resource_id":"web","revision":1,
        "content_hash":"sha256:ab","file_sha256":"cd","bytes":10,"units":["GEN"],
        "unit_aliases":{}}]}"#;
    let m = StarterManifest::parse(old).unwrap();
    assert_eq!(m.resources[0].resource_type, None);
    assert_eq!(m.resources[0].mobile_set, None);
    assert_eq!(m.get("web").unwrap().bytes, 10);
    // The Mobile app's set (#333), written since S04 of docs/plans/mobile-ios.md.
    let new = old.replace(r#""bytes":10,"#, r#""bytes":10,"mobile_set":"more","#);
    let m = StarterManifest::parse(&new).unwrap();
    assert_eq!(m.resources[0].mobile_set.as_deref(), Some("more"));
    assert!(
        StarterManifest::parse(r#"{"format":2,"resources":[]}"#)
            .unwrap_err()
            .contains("format 2")
    );
}

/// The Bundled set's manifest says which Mobile set each Resource is in (#333).
fn mark_mobile_set(bundle: &Path, set: &str) {
    let path = bundle.join(starter::MANIFEST_FILE);
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for r in manifest["resources"].as_array_mut().unwrap() {
        r["mobile_set"] = json!(set);
    }
    std::fs::write(&path, manifest.to_string()).unwrap();
}

/// A Bundled set as `cargo xtask ios bundled-set` writes it (#333): a Starter bundle whose
/// manifest lists only its Resources, each with `mobile_set`. The Mobile app adds it on the
/// first run as the desktop app adds its bundle, prebuilt Search index caches included.
#[test]
fn a_mobile_bundled_set_is_accepted_as_a_starter_bundle() {
    let lab = Lab::new("bundled-set");
    let mut bsb = testing::bibles::bsb_john_1();
    bsb.manifest.origin = Origin::Starter;
    bsb.manifest.license = "LicenseRef-PublicDomain".into();
    let strongs = starter_set()
        .into_iter()
        .find(|r| r.manifest.resource_id == "strongs-hebrew")
        .unwrap();
    testing::write_starter_bundle(&lab.bundle(), &[bsb, strongs]);
    mark_mobile_set(&lab.bundle(), "bundled");
    let (s, sync) = lab.launch(Some(&lab.bundle()));
    assert!(sync.problems.is_empty(), "{:?}", sync.problems);
    assert_eq!(sync.added.len(), 2, "{sync:?}");
    let manifest = s.starter.manifest().unwrap();
    assert!(
        manifest
            .resources
            .iter()
            .all(|r| r.mobile_set.as_deref() == Some("bundled")),
        "{manifest:?}"
    );
    assert_eq!(
        s.library
            .preferred(ResourceType::Bible)
            .unwrap()
            .manifest
            .resource_id,
        "bsb"
    );
    assert_eq!(ids(&s, ResourceType::Lexicon), ["strongs-hebrew"]);
    for id in ["bsb", "strongs-hebrew"] {
        assert!(s.search.is_ready(id), "{id}: {:?}", s.search.status(id));
    }
}

/// The real Bundled set (after `cargo xtask starter` and `cargo xtask ios bundled-set`) into
/// an empty data folder, as the Mobile app's first launch adds it. Run by hand:
/// `cargo test -p obs-engine --release -- --ignored --nocapture built_bundled_set`.
#[test]
#[ignore = "needs apps/ios/Bundled/starter from `cargo xtask ios bundled-set`"]
fn the_built_bundled_set_fills_an_empty_library_on_first_run() {
    let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/ios/Bundled/starter");
    let Ok(text) = std::fs::read_to_string(bundle.join(starter::MANIFEST_FILE)) else {
        eprintln!("no apps/ios/Bundled/starter; skipped");
        return;
    };
    let manifest = StarterManifest::parse(&text).unwrap();
    let mut listed: Vec<&str> = manifest
        .resources
        .iter()
        .map(|r| r.resource_id.as_str())
        .collect();
    listed.sort_unstable();
    assert_eq!(
        listed,
        ["bsb", "openbible-xrefs", "strongs-greek", "strongs-hebrew"]
    );
    let lab = Lab::new("built-bundled-set");
    let started = std::time::Instant::now();
    let (mut s, sync) = lab.launch(Some(&bundle));
    eprintln!(
        "first run: {} Resources added in {:?}",
        sync.added.len(),
        started.elapsed()
    );
    assert!(sync.problems.is_empty(), "{:?}", sync.problems);
    assert_eq!(sync.added.len(), manifest.resources.len());
    for item in &manifest.resources {
        assert_eq!(item.mobile_set.as_deref(), Some("bundled"));
        let e = s.library.get(&item.resource_id).unwrap();
        assert_eq!(e.manifest.origin, Origin::Starter);
        assert!(
            s.search.is_ready(&item.resource_id),
            "{}: prebuilt cache used",
            item.resource_id
        );
    }
    assert_eq!(
        s.library
            .preferred(ResourceType::Bible)
            .unwrap()
            .manifest
            .resource_id,
        "bsb"
    );
    assert_eq!(
        ids(&s, ResourceType::CrossReferenceSet),
        ["openbible-xrefs"]
    );
    let entries = lexicon_entries(&mut s, "G26");
    let from: Vec<&str> = entries.iter().map(|e| e.resource_id.as_str()).collect();
    assert_eq!(from, ["strongs-greek"], "{from:?}");
}

/// The real `dist/starter/` (after `cargo xtask starter`) into an empty data folder, as
/// `cargo run -p openbible` does on its first launch. Run by hand:
/// `cargo test -p obs-engine --release -- --ignored --nocapture built_starter_library`.
#[test]
#[ignore = "needs dist/starter from `cargo xtask starter`"]
fn the_built_starter_library_fills_an_empty_library_on_first_run() {
    let bundle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/starter");
    let Ok(text) = std::fs::read_to_string(bundle.join(starter::MANIFEST_FILE)) else {
        eprintln!("no dist/starter; skipped");
        return;
    };
    let manifest = StarterManifest::parse(&text).unwrap();
    let lab = Lab::new("built");
    let started = std::time::Instant::now();
    let (mut s, sync) = lab.launch(Some(&bundle));
    eprintln!(
        "first run: {} Resources added in {:?}",
        sync.added.len(),
        started.elapsed()
    );
    assert!(sync.problems.is_empty(), "{:?}", sync.problems);
    assert_eq!(sync.added.len(), manifest.resources.len());
    for item in &manifest.resources {
        let e = s.library.get(&item.resource_id).unwrap();
        assert_eq!(e.manifest.origin, Origin::Starter);
        assert!(
            s.search.is_ready(&item.resource_id),
            "{}: prebuilt cache used",
            item.resource_id
        );
    }
    for ty in [
        ResourceType::Bible,
        ResourceType::Lexicon,
        ResourceType::Commentary,
    ] {
        eprintln!("{ty:?}: {:?}", ids(&s, ty));
    }
    assert_eq!(
        s.library
            .preferred(ResourceType::Bible)
            .unwrap()
            .manifest
            .resource_id,
        "kjv-strongs"
    );
    let entries = lexicon_entries(&mut s, "G26");
    let from: Vec<&str> = entries.iter().map(|e| e.resource_id.as_str()).collect();
    eprintln!("G26 Lexicon entries: {from:?}");
    assert_eq!(from.first(), Some(&"tbesg"), "{from:?}");
    assert!(from.contains(&"abbott-smith") && from.contains(&"strongs-greek"));
}

// ---- Released leads: the Mobile app's Starter library (#333, S23 of docs/plans/mobile-ios.md)

use crate::starter::MobileSet;

/// The Mobile app's Bundled set out of `library`: the items of that set only.
fn bundled_only(library: &[(Resource, MobileSet)]) -> Vec<(Resource, MobileSet)> {
    library
        .iter()
        .filter(|(_, s)| *s == MobileSet::Bundled)
        .cloned()
        .collect()
}

/// [`LocalFetch`], reading a `file://` URL's file without its query (`releases.json?per_page=20`
/// is `releases.json`), as a web server answers the releases API.
#[derive(Debug)]
struct FileServer;

impl Fetch for FileServer {
    fn get(&self, url: &str, progress: Progress<'_>) -> Result<Vec<u8>, FetchError> {
        LocalFetch.get(url.split('?').next().unwrap_or(url), progress)
    }
}

impl Lab {
    /// Launch as the Mobile app does: the released manifest leads, downloads from
    /// `release/` (`file://`), and the releases API is `releases.json`.
    fn mobile_launch(&self) -> Session {
        let mut s = Session::new(JobMode::Inline);
        s.set_search_cache_dir(self.root.join("cache"));
        let problems = s.library.scan(&self.lib());
        assert!(problems.is_empty(), "{problems:?}");
        let (store, _) = obs_store::UserStore::open(&self.root.join("user.db")).unwrap();
        s.attach_user(Box::new(store));
        s.set_fetch(Arc::new(FileServer));
        s.starter.set_released_leads(true);
        s.starter.set_download_base(&file_url(&self.release()));
        s.starter
            .set_releases_api(&file_url(&self.root.join("releases.json")));
        let sync = s.sync_starter_library(Some(&self.bundle()));
        assert!(sync.problems.is_empty(), "{:?}", sync.problems);
        settle(&mut s);
        s
    }
}

fn row_ids(s: &Session) -> Vec<String> {
    s.starter_rows()
        .into_iter()
        .map(|r| r.resource_id)
        .collect()
}

fn status(s: &Session, id: &str) -> StarterStatus {
    s.starter_rows()
        .into_iter()
        .find(|r| r.resource_id == id)
        .unwrap()
        .status
}

/// With the switch off (the desktop), a released manifest fetched by `starter.check` changes
/// nothing: the rows, Add back and revisions are the bundle's, as before S23.
#[test]
fn with_released_leads_off_the_bundle_alone_still_makes_the_rows_add_back_and_revisions() {
    let lab = Lab::new("leads-off");
    testing::write_starter_bundle(&lab.bundle(), &starter_set());
    // A newer release: the KJV at revision 2, and a Resource the bundle lacks.
    let mut release = starter_set();
    release[0] = starter_bible(
        "kjv-strongs",
        "KJV",
        "In the beginning God created the heaven and the earth!",
        2,
    );
    release.push(starter_bible(
        "web",
        "WEB",
        "In the beginning, God created.",
        1,
    ));
    testing::write_starter_bundle(&lab.release(), &release);
    let (mut s, _) = lab.launch(Some(&lab.bundle()));
    assert!(!s.starter.released_leads());
    let rows_before = s.starter_rows();
    s.starter.set_download_base(&file_url(&lab.release()));
    s.run("starter.check", &json!({})).unwrap();
    settle(&mut s);
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    assert_eq!(s.starter_rows(), rows_before);
    assert_eq!(rows_before.len(), 5);
    assert_eq!(status(&s, "kjv-strongs"), StarterStatus::InLibrary);
    assert!(s.starter_wanted().is_empty());
    // Remove and Add back come from the bundle, at once.
    s.run("library.remove", &json!({ "id": "bdb", "confirm": true }))
        .unwrap();
    assert_eq!(status(&s, "bdb"), StarterStatus::Removed);
    let out = s.run("starter.add", &json!({ "id": "bdb" })).unwrap();
    assert!(out["job"].is_null(), "from the bundle");
    assert_eq!(s.library.get("kjv-strongs").unwrap().manifest.revision, 1);
    // An id only the release lists is still not part of this Starter library.
    let err = s
        .run("starter.add", &json!({ "id": "web" }))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("web is not part of the Starter library"),
        "{err}"
    );
    assert_eq!(s.starter.files_base(), s.starter.download_base());
}

/// S23's Done when: a Bundled set installed, then a Study set item added through the release
/// path from a `file://` base; once the released manifest is in, Library → Starter library
/// lists every Starter Resource throughout.
#[test]
fn a_bundled_set_then_a_study_set_item_from_the_release_lists_every_row_throughout() {
    let lab = Lab::new("leads-study-item");
    let library = testing::mobile_starter_library();
    testing::write_mobile_starter_bundle(&lab.bundle(), &bundled_only(&library));
    testing::write_mobile_starter_bundle(&lab.release(), &library);
    let mut s = lab.mobile_launch();
    let mut installed: Vec<String> = s
        .library
        .entries()
        .iter()
        .map(|e| e.manifest.resource_id.clone())
        .collect();
    installed.sort();
    assert_eq!(installed, ["bsb", "strongs-hebrew"]);
    // Offline: the bundle's entries.
    assert_eq!(row_ids(&s), ["bsb", "strongs-hebrew"]);
    s.check_starter_release().unwrap();
    settle(&mut s);
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    let every: Vec<String> = library
        .iter()
        .map(|(r, _)| r.manifest.resource_id.clone())
        .collect();
    assert_eq!(row_ids(&s), every);
    assert_eq!(status(&s, "bsb"), StarterStatus::InLibrary);
    assert_eq!(status(&s, "tbesg"), StarterStatus::Missing);
    let out = s.run("starter.add", &json!({ "id": "tbesg" })).unwrap();
    assert!(
        out["job"].is_u64(),
        "downloaded, not from the bundle: {out}"
    );
    assert_eq!(row_ids(&s), every);
    settle(&mut s);
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    assert_eq!(row_ids(&s), every);
    assert_eq!(status(&s, "tbesg"), StarterStatus::InLibrary);
    assert_eq!(s.library.entries().len(), 3);
    assert!(s.search.is_ready("tbesg"), "{:?}", s.search.status("tbesg"));
    // Add back of a bundled Resource still comes from the bundle.
    s.run(
        "library.remove",
        &json!({ "id": "strongs-hebrew", "confirm": true }),
    )
    .unwrap();
    assert_eq!(status(&s, "strongs-hebrew"), StarterStatus::Removed);
    let out = s
        .run("starter.add", &json!({ "id": "strongs-hebrew" }))
        .unwrap();
    assert!(out["job"].is_null(), "from the bundle: {out}");
    assert_eq!(status(&s, "strongs-hebrew"), StarterStatus::InLibrary);
}

/// The Study set is what one click adds of that set; removed ones stay out of it.
#[test]
fn the_study_set_is_the_wanted_items_of_that_set_and_removed_ones_stay_out() {
    let lab = Lab::new("leads-study-set");
    let library = testing::mobile_starter_library();
    testing::write_mobile_starter_bundle(&lab.bundle(), &bundled_only(&library));
    testing::write_mobile_starter_bundle(&lab.release(), &library);
    let mut s = lab.mobile_launch();
    s.check_starter_release().unwrap();
    settle(&mut s);
    let study = |s: &Session| -> Vec<String> {
        s.starter_wanted_set(MobileSet::Study)
            .into_iter()
            .map(|i| i.resource_id)
            .collect()
    };
    assert_eq!(study(&s), ["kjv-strongs", "tbesh", "tbesg", "bdb"]);
    assert!(s.starter_wanted_set(MobileSet::Bundled).is_empty());
    let more: Vec<String> = (s.starter_wanted_set(MobileSet::More).into_iter())
        .map(|i| i.resource_id)
        .collect();
    assert_eq!(more, ["web"]);
    s.run("starter.add", &json!({ "id": "bdb" })).unwrap();
    settle(&mut s);
    s.run("library.remove", &json!({ "id": "bdb", "confirm": true }))
        .unwrap();
    assert_eq!(study(&s), ["kjv-strongs", "tbesh", "tbesg"]);
}

/// A newer released revision of a bundled Resource is downloaded: the bundle holds only the
/// older file.
#[test]
fn a_newer_released_revision_of_a_bundled_resource_is_downloaded_not_taken_from_the_bundle() {
    let lab = Lab::new("leads-bundled-revision");
    let library = testing::mobile_starter_library();
    testing::write_mobile_starter_bundle(&lab.bundle(), &bundled_only(&library));
    let mut release = library.clone();
    let hebrew = release
        .iter_mut()
        .find(|(r, _)| r.manifest.resource_id == "strongs-hebrew")
        .unwrap();
    hebrew.0.manifest.revision = 2;
    hebrew.0.manifest.content_hash = "sha256:revised".into();
    testing::write_mobile_starter_bundle(&lab.release(), &release);
    let mut s = lab.mobile_launch();
    s.check_starter_release().unwrap();
    settle(&mut s);
    assert_eq!(
        status(&s, "strongs-hebrew"),
        StarterStatus::Outdated {
            installed: 1,
            offered: 2
        }
    );
    let out = s
        .run("starter.add", &json!({ "id": "strongs-hebrew" }))
        .unwrap();
    assert!(out["job"].is_u64(), "{out}");
    settle(&mut s);
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    assert_eq!(
        s.library.get("strongs-hebrew").unwrap().manifest.revision,
        2
    );
}

/// Files the app downloaded itself are checked and installed as a download is, and deleted
/// once read; a sha256 mismatch adds nothing.
#[test]
fn starter_files_downloaded_by_the_app_are_verified_installed_and_deleted() {
    let lab = Lab::new("leads-files");
    let library = testing::mobile_starter_library();
    testing::write_mobile_starter_bundle(&lab.bundle(), &bundled_only(&library));
    testing::write_mobile_starter_bundle(&lab.release(), &library);
    let mut s = lab.mobile_launch();
    s.check_starter_release().unwrap();
    settle(&mut s);
    let downloads = lab.root.join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let item = |id: &str| s.starter.manifest().unwrap().get(id).unwrap().clone();
    let (tbesh, bdb) = (item("tbesh"), item("bdb"));
    // A mismatch: the BDB's file is not the one the manifest names.
    let bad = downloads.join("bdb.obr");
    std::fs::write(&bad, b"not an obr").unwrap();
    let err = s
        .add_starter_from_files(std::slice::from_ref(&bdb), &[("bdb".into(), bad.clone())])
        .unwrap_err();
    assert!(err.contains("bdb.obr failed verification"), "{err}");
    assert!(!bad.exists(), "the mismatched download is deleted");
    assert!(s.library.get("bdb").is_none());
    assert!(
        !lab.files().iter().any(|f| f.starts_with("bdb")),
        "{:?}",
        lab.files()
    );
    // Retry: the right files.
    let mut files = Vec::new();
    for id in ["tbesh", "bdb"] {
        let to = downloads.join(format!("{id}.obr"));
        std::fs::copy(lab.release().join(format!("{id}.obr")), &to).unwrap();
        files.push((id.to_string(), to));
    }
    let ids = s.add_starter_from_files(&[tbesh, bdb], &files).unwrap();
    assert_eq!(ids, ["tbesh", "bdb"]);
    for (_, f) in &files {
        assert!(!f.exists(), "{} deleted once read", f.display());
    }
    settle(&mut s);
    assert_eq!(status(&s, "tbesh"), StarterStatus::InLibrary);
    assert!(s.search.is_ready("bdb"), "{:?}", s.search.status("bdb"));
}

/// A releases list as GitHub's releases API answers it, newest first: `(tag, manifest folder,
/// draft, pre-release)`.
fn write_releases(path: &Path, releases: &[(&str, &Path, bool, bool)]) {
    let list: Vec<serde_json::Value> = releases
        .iter()
        .map(|(tag, dir, draft, prerelease)| {
            json!({
                "tag_name": tag, "name": tag, "body": "", "html_url": "",
                "draft": draft, "prerelease": prerelease,
                "assets": [{
                    "name": starter::MANIFEST_FILE,
                    "browser_download_url": file_url(&dir.join(starter::MANIFEST_FILE)),
                    "size": 1,
                }],
            })
        })
        .collect();
    std::fs::write(path, serde_json::to_string(&list).unwrap()).unwrap();
}

/// A Starter library folder whose manifest says `format`.
fn with_format(dir: &Path, resources: &[(Resource, MobileSet)], format: u32) {
    testing::write_mobile_starter_bundle(dir, resources);
    let path = dir.join(starter::MANIFEST_FILE);
    let mut m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    m["format"] = json!(format);
    std::fs::write(&path, m.to_string()).unwrap();
}

/// #333: "the app reads the Starter manifest of the release its format version matches".
/// The latest release is of a newer format: the releases list is read, newest first, past
/// drafts and pre-releases, and the files come from the release found.
#[test]
fn a_newer_manifest_format_is_looked_past_to_the_release_this_app_reads_and_its_files() {
    let lab = Lab::new("leads-format");
    let library = testing::mobile_starter_library();
    testing::write_mobile_starter_bundle(&lab.bundle(), &bundled_only(&library));
    with_format(&lab.release(), &library, starter::MANIFEST_FORMAT + 1);
    let (draft, pre, older) = (
        lab.root.join("draft"),
        lab.root.join("pre"),
        lab.root.join("v0.2.0"),
    );
    for dir in [&draft, &pre, &older] {
        testing::write_mobile_starter_bundle(dir, &library);
    }
    write_releases(
        &lab.root.join("releases.json"),
        &[
            ("v0.4.0", &draft, true, false),
            ("v0.3.1-beta.1", &pre, false, true),
            ("v0.3.0", &lab.release(), false, false),
            ("v0.2.0", &older, false, false),
        ],
    );
    let mut s = lab.mobile_launch();
    s.check_starter_release().unwrap();
    settle(&mut s);
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    assert!(!s.starter.needs_app_update());
    assert_eq!(s.starter_rows().len(), library.len());
    assert_eq!(s.starter.files_base(), format!("{}/", file_url(&older)));
    // Its files, not the latest release's: those are gone.
    for (r, _) in &library {
        let _ = std::fs::remove_file(
            lab.release()
                .join(format!("{}.obr", r.manifest.resource_id)),
        );
    }
    s.run("starter.add", &json!({ "id": "tbesg" })).unwrap();
    settle(&mut s);
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    assert!(s.library.get("tbesg").is_some());
}

/// No release this app reads, within the list or before an older format: a clear "update
/// the app" state, and the rows stay the bundle's.
#[test]
fn with_no_release_of_this_format_the_app_must_be_updated() {
    let lab = Lab::new("leads-no-format");
    let library = testing::mobile_starter_library();
    testing::write_mobile_starter_bundle(&lab.bundle(), &bundled_only(&library));
    with_format(&lab.release(), &library, starter::MANIFEST_FORMAT + 1);
    // An older format stops the search: the release of this format after it is not read.
    let (old, ours) = (lab.root.join("old"), lab.root.join("ours"));
    with_format(&old, &library, starter::MANIFEST_FORMAT - 1);
    testing::write_mobile_starter_bundle(&ours, &library);
    write_releases(
        &lab.root.join("releases.json"),
        &[
            ("v0.3.0", &lab.release(), false, false),
            ("v0.2.0", &old, false, false),
            ("v0.1.0", &ours, false, false),
        ],
    );
    let mut s = lab.mobile_launch();
    s.check_starter_release().unwrap();
    settle(&mut s);
    assert!(s.starter.needs_app_update());
    let error = s.starter.error().unwrap();
    assert!(error.contains("Update the app"), "{error}");
    assert_eq!(row_ids(&s), ["bsb", "strongs-hebrew"]);
    assert_eq!(s.starter.files_base(), s.starter.download_base());
    // The desktop keeps today's refusal.
    let (mut d, _) = lab.launch(None);
    d.starter.set_download_base(&file_url(&lab.release()));
    d.check_starter_release().unwrap();
    settle(&mut d);
    assert!(!d.starter.needs_app_update());
    let error = d.starter.error().unwrap();
    assert!(error.contains("format 2 (this app reads 1)"), "{error}");
}

/// Library → Storage: each Resource's `.obr`, Search index and alias bytes; whether a
/// download fits.
#[test]
fn storage_counts_each_resources_files_and_a_download_that_wont_fit_is_told() {
    let lab = Lab::new("storage");
    testing::write_starter_bundle(&lab.bundle(), &starter_set());
    let (s, _) = lab.launch(Some(&lab.bundle()));
    let storage = s.library_storage();
    assert_eq!(storage.resources.len(), 5);
    for row in &storage.resources {
        let obr = std::fs::metadata(lab.lib().join(format!("{}.obr", row.resource_id)))
            .unwrap()
            .len();
        assert_eq!(row.obr, obr, "{}", row.resource_id);
        assert!(row.search_index > 0, "{}: prebuilt index", row.resource_id);
        assert_eq!(row.aliases, 0);
    }
    let sum: u64 = storage.resources.iter().map(|r| r.total()).sum();
    assert_eq!(storage.total, sum);
    assert_eq!(library_storage::space_needed(100), 250);
    assert!(library_storage::fits(100, 250));
    assert!(!library_storage::fits(100, 249));
}

/// S23's Done when over the real Starter library (after `cargo xtask starter`): its Bundled
/// set installed, then a Study set Resource added through the release path from a `file://`
/// base (`dist/starter` as the release); Library → Starter library lists 22 rows once the
/// released manifest is in, before and after. Run by hand:
/// `cargo test -p obs-engine --release -- --ignored --nocapture built_starter_release`.
#[test]
#[ignore = "needs dist/starter from `cargo xtask starter`"]
fn the_built_starter_release_adds_a_study_set_item_after_the_bundled_set() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dist/starter");
    if !dist.join(starter::MANIFEST_FILE).is_file() {
        eprintln!("no dist/starter; skipped");
        return;
    }
    let lab = Lab::new("built-release");
    testing::copy_bundled_set(&dist, &lab.bundle());
    let mut s = lab.mobile_launch();
    s.starter.set_download_base(&file_url(&dist));
    assert_eq!(s.library.entries().len(), 4);
    assert_eq!(s.starter_rows().len(), 4, "offline: the Bundled set's rows");
    s.check_starter_release().unwrap();
    settle(&mut s);
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    assert_eq!(s.starter_rows().len(), 22);
    assert_eq!(s.starter_wanted_set(MobileSet::Study).len(), 15);
    let started = std::time::Instant::now();
    let out = s.run("starter.add", &json!({ "id": "tbesg" })).unwrap();
    assert!(out["job"].is_u64(), "{out}");
    assert_eq!(s.starter_rows().len(), 22);
    settle(&mut s);
    eprintln!("tbesg added in {:?}", started.elapsed());
    assert!(s.starter.error().is_none(), "{:?}", s.starter.error());
    assert_eq!(s.starter_rows().len(), 22);
    assert_eq!(s.library.entries().len(), 5);
    assert_eq!(status(&s, "tbesg"), StarterStatus::InLibrary);
    let entries = lexicon_entries(&mut s, "G26");
    assert!(
        entries.iter().any(|e| e.resource_id == "tbesg"),
        "{entries:?}"
    );
}
