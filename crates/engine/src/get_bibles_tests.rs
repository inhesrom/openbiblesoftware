#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;

use obs_model::ResourceType;

use crate::get_bibles::*;
use crate::testing::MockRepos;
use crate::*;

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("obs-getbibles-{name}-{}", std::process::id()))
}

/// An inline Session pointed at mock repositories, with its catalog cache in `root`.
fn session(name: &str) -> (Session, Arc<MockRepos>, PathBuf) {
    let root = temp(name);
    let repos = Arc::new(MockRepos::new(&root.join("repos")));
    let mut s = Session::new(JobMode::Inline);
    s.set_fetch(repos.clone());
    s.get_bibles
        .set_cache_path(root.join("cache/sword-catalog.json"));
    (s, repos, root)
}

fn refresh(s: &mut Session) {
    s.run("getbibles.refresh", &json!({})).unwrap();
    s.jobs().pump(1);
    assert!(s.poll_imports().is_empty(), "no stray events");
    assert!(!s.get_bibles.is_refreshing());
}

fn listing(module: &str, repo: &str) -> ResourceListing {
    ResourceListing {
        module: module.into(),
        kind: obs_model::ResourceType::Bible,
        repo: repo.into(),
        title: module.into(),
        abbreviation: module.into(),
        language: "en".into(),
        size: None,
        licence: None,
        version: None,
        strongs: false,
        refused: None,
    }
}

#[test]
fn featured_data_file_parses_with_nasb_1995_first() {
    let featured = featured(ResourceType::Bible);
    assert_eq!(featured.len(), 13);
    assert_eq!(
        featured[0],
        FeaturedModule {
            module: "NASB1995".into(),
            repo: "Lockman Foundation".into()
        }
    );
    let modules: Vec<&str> = featured.iter().map(|f| f.module.as_str()).collect();
    for m in ["NASB", "NETfree", "LEB", "BSB", "KJV", "ASV", "YLT"] {
        assert!(modules.contains(&m), "{m} missing from {modules:?}");
    }
}

#[test]
fn catalog_lists_bibles_of_every_repository_with_conf_licence_and_size() {
    let (mut s, repos, root) = session("fetch");
    assert!(s.get_bibles.catalog().is_none());
    refresh(&mut s);
    assert_eq!(s.get_bibles.offline(), None);
    let catalog = s.get_bibles.catalog().unwrap();

    // Bibles, locked modules left out, one row per module.
    let mut modules: Vec<&str> = catalog
        .all("", None, Some(ResourceType::Bible))
        .iter()
        .map(|b| b.module.as_str())
        .collect();
    modules.sort();
    assert_eq!(
        modules,
        [
            "NASB1995",
            "ObsGreekBible",
            "ObsHebrew",
            "ObsKjvGbf",
            "ObsKjvOsis"
        ]
    );
    let nasb = catalog.get("nasb1995").unwrap();
    assert_eq!(nasb.repo, "Lockman Foundation");
    assert_eq!(nasb.title, "New American Standard Bible - NASB 1995");
    assert_eq!(nasb.abbreviation, "NASB95");
    assert_eq!(
        nasb.licence.as_deref(),
        Some("Copyrighted; Permission to distribute granted to CrossWire")
    );
    // Lockman's conf has no InstallSize: the size is the package zip's, asked by HEAD.
    assert_eq!(nasb.size, Some(crate::testing::NASB_PACKAGE_SIZE));
    assert_eq!(nasb.source_label(), "from Lockman Foundation");

    // NASB 1995 heads Featured; Featured modules no repository offers are left out.
    let featured: Vec<&str> = catalog
        .featured(ResourceType::Bible)
        .iter()
        .map(|b| b.module.as_str())
        .collect();
    assert_eq!(featured, ["NASB1995"]);

    // A repository that cannot be read is reported, and does not fail the rest.
    assert_eq!(catalog.failed.len(), 1);
    assert_eq!(catalog.failed[0].0, "Broken");
    assert_eq!(
        catalog
            .repo("CrossWire")
            .unwrap()
            .repository()
            .sources
            .len(),
        1
    );
    // Every byte came from the repositories (the master list, then each file:// repo).
    let fetched = repos.fetched.lock().unwrap().clone();
    assert_eq!(fetched[0], obs_formats::sword_repo::MASTER_REPO_LIST_URL);
    assert!(
        fetched[1..]
            .iter()
            .all(|u| u.starts_with("file://") || u.starts_with("HEAD file://")),
        "{fetched:?}"
    );
    // Only Featured Bibles without a conf size are asked (here NASB1995 alone).
    let heads: Vec<&String> = fetched.iter().filter(|u| u.starts_with("HEAD ")).collect();
    assert_eq!(heads.len(), 1, "{heads:?}");
    assert!(heads[0].ends_with("/lockman/packages/NASB1995.zip"));
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_same_module_in_several_repositories_shows_once_from_the_preferred_source() {
    let (mut s, _repos, root) = session("dedupe");
    refresh(&mut s);
    let catalog = s.get_bibles.catalog().unwrap();
    // ObsKjvOsis is in CrossWire and eBible.org: CrossWire wins over listing order.
    let kjv = catalog.get("ObsKjvOsis").unwrap();
    assert_eq!(kjv.repo, "CrossWire");
    assert_ne!(kjv.title, "eBible copy");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();

    // The Featured list's repository beats CrossWire; otherwise the first listed wins.
    let featured = [FeaturedModule {
        module: "NET".into(),
        repo: "Bible.org".into(),
    }];
    let kept = dedupe(
        vec![
            listing("NET", "CrossWire"),
            listing("net", "Bible.org"),
            listing("X", "eBible.org"),
            listing("X", "IBT"),
            listing("Y", "IBT"),
            listing("Y", "CrossWire"),
        ],
        &featured,
    );
    let got: Vec<(&str, &str)> = kept
        .iter()
        .map(|b| (b.module.as_str(), b.repo.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("net", "Bible.org"),
            ("X", "eBible.org"),
            ("Y", "CrossWire")
        ]
    );
}

fn modules(listings: &[&ResourceListing]) -> Vec<String> {
    listings.iter().map(|b| b.module.clone()).collect()
}

#[test]
fn type_filter_search_and_language_filter_combine() {
    let (mut s, _repos, root) = session("filter");
    refresh(&mut s);
    let catalog = s.get_bibles.catalog().unwrap();
    let bible = Some(ResourceType::Bible);
    let lexicon = Some(ResourceType::Lexicon);
    assert_eq!(catalog.all("", None, bible).len(), 5);
    assert_eq!(catalog.all("", None, None).len(), 10, "every listed type");
    assert_eq!(
        modules(&catalog.all("", Some("grc"), bible)),
        ["ObsGreekBible"]
    );
    assert_eq!(
        modules(&catalog.all("nasb", None, bible)),
        ["NASB1995"],
        "abbreviation/title search"
    );
    assert!(catalog.all("nasb", Some("grc"), bible).is_empty());

    // "greek" matches a Bible and a Lexicon; the type and language filters narrow it.
    assert_eq!(
        modules(&catalog.all("greek", None, None)),
        ["ObsGreekBible", "StrongsGreek"]
    );
    assert_eq!(
        modules(&catalog.all("greek", None, lexicon)),
        ["StrongsGreek"]
    );
    assert_eq!(
        modules(&catalog.all("greek", Some("grc"), None)),
        ["ObsGreekBible"]
    );
    assert!(catalog.all("greek", Some("grc"), lexicon).is_empty());
    assert!(catalog.all("strongs", None, bible).is_empty());

    // The language list counts the chosen type.
    let langs = catalog.languages(bible);
    assert_eq!(langs[0], ("en".to_string(), 4));
    assert!(langs.contains(&("grc".to_string(), 1)));
    assert_eq!(catalog.languages(lexicon), [("en".to_string(), 1)]);
    assert_eq!(catalog.languages(None)[0], ("en".to_string(), 9));

    // The same through the command.
    let out = s
        .run(
            "getbibles.list",
            &json!({"query": "greek", "language": "grc"}),
        )
        .unwrap();
    assert_eq!(out["all"].as_array().unwrap().len(), 1);
    assert_eq!(out["featured"][0]["module"], "NASB1995");
    let out = s
        .run(
            "getbibles.list",
            &json!({"query": "greek", "type": "lexicons"}),
        )
        .unwrap();
    assert_eq!(out["all"].as_array().unwrap().len(), 1);
    assert_eq!(out["all"][0]["module"], "StrongsGreek");
    assert_eq!(out["all"][0]["type"], "lexicon");
    assert_eq!(out["featured"][0]["module"], "StrongsGreek");
    assert!(
        s.run("getbibles.list", &json!({"type": "maps"}))
            .unwrap_err()
            .to_string()
            .contains("Unknown type")
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn offline_the_cached_catalog_is_shown_with_the_reason() {
    let (mut s, repos, root) = session("offline");
    refresh(&mut s);
    let fetched = s.get_bibles.catalog().unwrap().clone();
    assert!(root.join("cache/sword-catalog.json").is_file());

    // A new Session (next app start) opens the cache with no network at all.
    repos.set_offline(true);
    let mut s = Session::new(JobMode::Inline);
    s.set_fetch(repos.clone());
    s.get_bibles
        .set_cache_path(root.join("cache/sword-catalog.json"));
    assert_eq!(s.get_bibles.catalog(), Some(&fetched));
    refresh(&mut s);
    let why = s.get_bibles.offline().unwrap();
    assert!(why.contains("repository list"), "{why}");
    assert_eq!(s.get_bibles.catalog(), Some(&fetched), "cache kept");
    let out = s.run("getbibles.list", &json!({})).unwrap();
    assert!(out["offline"].as_str().is_some());
    assert_eq!(out["featured"][0]["module"], "NASB1995");

    // Back online, a refresh clears the message.
    repos.set_offline(false);
    refresh(&mut s);
    assert_eq!(s.get_bibles.offline(), None);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn without_an_app_transport_nothing_reaches_the_network() {
    let root = temp("no-transport");
    let mut s = Session::new(JobMode::Inline);
    s.get_bibles.set_cache_path(root.join("sword-catalog.json"));
    refresh(&mut s);
    let why = s.get_bibles.offline().unwrap();
    assert!(why.contains("no network transport"), "{why}");
    assert!(s.get_bibles.catalog().is_none());
    let _ = std::fs::remove_dir_all(root);
}

/// A session with the catalog fetched, plus library and SWORD folders under `root`.
fn ready(name: &str, mode: JobMode) -> (Session, Arc<MockRepos>, PathBuf) {
    let (mut s, repos, root) = session(name);
    refresh(&mut s);
    let mut s2 = Session::new(mode);
    s2.set_fetch(repos.clone());
    s2.get_bibles
        .set_cache_path(root.join("cache/sword-catalog.json"));
    s2.get_bibles.set_sword_dir(root.join("sword"));
    std::fs::create_dir_all(root.join("library")).unwrap();
    s2.library.scan(&root.join("library"));
    drop(s);
    (s2, repos, root)
}

fn nasb(s: &Session) -> ResourceListing {
    s.get_bibles
        .catalog()
        .unwrap()
        .get("NASB1995")
        .unwrap()
        .clone()
}

fn obr_files(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.ends_with(".obr"))
                .collect()
        })
        .unwrap_or_default()
}

fn sword_modules(dir: &std::path::Path) -> Vec<String> {
    obs_formats::SwordSource::open(dir)
        .map(|s| s.modules().into_iter().map(|m| m.name).collect())
        .unwrap_or_default()
}

#[test]
fn one_download_fetches_imports_and_lands_in_the_library_with_no_prompts() {
    let (mut s, repos, root) = ready("download", JobMode::Inline);
    let listing = nasb(&s);
    assert_eq!(s.listing_status(&listing), ListingStatus::Available);
    let out = s
        .run("getbibles.download", &json!({"module": "NASB1995"}))
        .unwrap();
    let id = out["id"].as_str().unwrap().to_string();
    assert_eq!(id, "nasb1995-1");
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Download);
    assert_eq!(record.source, "NASB1995 from Lockman Foundation");
    assert!(matches!(
        s.listing_status(&listing),
        ListingStatus::InProgress { .. }
    ));
    // The live Import report Tab opened; nothing asks the user anything.
    assert!(
        s.workspace
            .tabs()
            .any(|t| t.kind == TabKind::ImportReport(id.clone()))
    );
    assert!(
        !s.workspace
            .take_effects()
            .iter()
            .any(|e| matches!(e, Effect::PickImport | Effect::ConfirmRemove { .. }))
    );

    s.jobs().pump(1);
    assert!(s.poll_imports().is_empty());
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    assert!(!record.report.as_ref().unwrap().license_missing);
    let entry = s.library.get(&id).unwrap();
    assert_eq!(entry.manifest.abbreviation, "NASB95");
    assert_eq!(
        s.library
            .of_type(obs_model::ResourceType::Bible)
            .last()
            .unwrap()
            .manifest
            .resource_id,
        id,
        "lands at the bottom of its type"
    );
    assert_eq!(
        s.listing_status(&listing),
        ListingStatus::Installed {
            resource_id: id.clone()
        }
    );
    assert_eq!(sword_modules(&root.join("sword")), ["NASB1995"]);
    // Bytes came from the repository's own URLs.
    let fetched = repos.fetched.lock().unwrap().clone();
    let lockman = obs_formats::sword_repo::file_url(&root.join("repos/lockman"));
    assert!(fetched.iter().any(|u| u.starts_with(&lockman)));

    // Already installed: a second Download is refused, nothing duplicated.
    let err = s
        .run("getbibles.download", &json!({"module": "NASB1995"}))
        .unwrap_err();
    assert!(err.to_string().contains("already in your Library"), "{err}");
    assert_eq!(obr_files(&root.join("library")), ["nasb1995-1.obr"]);

    // The origin survives a restart with the report.
    s.run("resource.open", &json!({"id": id})).unwrap();
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn offline_download_fails_cleanly_then_retry_succeeds_and_replaces_the_failure() {
    let (mut s, repos, root) = ready("retry", JobMode::Inline);
    let listing = nasb(&s);
    repos.set_offline(true);
    let id = s
        .run("getbibles.download", &json!({"module": "NASB1995"}))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    s.jobs().pump(1);
    s.poll_imports();
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Failed);
    assert!(
        record
            .error
            .as_deref()
            .unwrap()
            .contains("Lockman Foundation"),
        "{:?}",
        record.error
    );
    assert!(matches!(
        s.listing_status(&listing),
        ListingStatus::Failed { .. }
    ));
    assert!(s.library.get(&id).is_none());
    assert!(obr_files(&root.join("library")).is_empty());
    assert!(sword_modules(&root.join("sword")).is_empty());

    repos.set_offline(false);
    let retry = s
        .run("getbibles.download", &json!({"module": "NASB1995"}))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        s.imports.report(&id).is_none() || retry == id,
        "failure replaced"
    );
    assert!(
        !s.workspace
            .tabs()
            .any(|t| t.kind == TabKind::ImportReport(id.clone()) && id != retry)
    );
    s.jobs().pump(1);
    s.poll_imports();
    assert_eq!(
        s.listing_status(&listing),
        ListingStatus::Installed {
            resource_id: retry.clone()
        }
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// Holds the first module-data fetch until released, so a test can cancel mid-download.
#[derive(Debug)]
struct Held {
    inner: Arc<MockRepos>,
    entered: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}
impl obs_formats::sword_repo::Fetch for Held {
    fn get(
        &self,
        url: &str,
        progress: obs_formats::sword_repo::Progress<'_>,
    ) -> Result<Vec<u8>, obs_formats::sword_repo::FetchError> {
        if url.contains("/modules/")
            && let Some(tx) = self.entered.lock().unwrap().take()
        {
            progress(1, Some(10));
            tx.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(20))
                .unwrap();
        }
        self.inner.get(url, progress)
    }
}

#[test]
fn cancelling_mid_download_leaves_no_resource_no_obr_and_no_sword_copy() {
    let (mut s, repos, root) = ready("cancel", JobMode::Threads(1));
    let listing = nasb(&s);
    let (entered, held) = std::sync::mpsc::channel();
    let (release, rx) = std::sync::mpsc::channel();
    s.set_fetch(Arc::new(Held {
        inner: repos,
        entered: std::sync::Mutex::new(Some(entered)),
        release: std::sync::Mutex::new(rx),
    }));
    let id = s
        .run("getbibles.download", &json!({"module": "NASB1995"}))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    held.recv_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    s.poll_imports();
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Download);
    assert!(record.done > 0, "download progress shown");

    s.run("import.cancel", &json!({"id": id})).unwrap();
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while s.imports.is_running() && std::time::Instant::now() < deadline {
        s.poll_imports();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Failed);
    assert!(s.library.get(&id).is_none());
    assert!(obr_files(&root.join("library")).is_empty());
    assert!(sword_modules(&root.join("sword")).is_empty());
    assert!(matches!(
        s.listing_status(&listing),
        ListingStatus::Failed { .. }
    ));
    assert!(s.run("import.cancel", &json!({"id": id})).is_err());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

// ---- Get Resources (#132): Lexicons, Commentaries and Dictionaries ----------------------

#[test]
fn featured_resources_data_file_lists_lexicons_commentaries_and_dictionaries() {
    let names = |ty| -> Vec<String> { featured(ty).into_iter().map(|f| f.module).collect() };
    // Checked against the live catalogs (docs/research/get-resources-catalog.md): Thayer is
    // in no repository, Gill's only copy has no distribution licence, and AbbottSmith is
    // keyed by Greek lemma (Import finds no Strong's entries), so none of them is here.
    assert_eq!(
        names(ResourceType::Lexicon),
        ["StrongsGreek", "StrongsHebrew", "BDBGlosses_Strongs"]
    );
    assert_eq!(
        names(ResourceType::Commentary),
        ["MHC", "JFB", "Barnes", "Clarke", "TSK"]
    );
    assert_eq!(
        names(ResourceType::Dictionary),
        ["Easton", "Smith", "ISBE", "Nave", "Hitchcock"]
    );
    assert!(
        featured(ResourceType::Lexicon)
            .iter()
            .all(|f| f.repo == "CrossWire")
    );
    assert_eq!(featured_all().len(), 13 + 3 + 5 + 5);
    assert!(featured(ResourceType::CrossReferenceSet).is_empty());
}

#[test]
fn catalog_lists_lexicons_commentaries_and_dictionaries_with_what_they_are_used_by() {
    let (mut s, _repos, root) = session("types");
    refresh(&mut s);
    let catalog = s.get_bibles.catalog().unwrap();
    assert_eq!(catalog.version, CATALOG_VERSION);
    let of = |ty| modules(&catalog.all("", None, Some(ty)));
    // Feature=GreekDef makes a Dictionary module a Lexicon.
    assert_eq!(of(ResourceType::Lexicon), ["StrongsGreek"]);
    assert_eq!(
        of(ResourceType::Commentary),
        ["ObsNotes", "ObsLinks", "ObsHebrewNotes"]
    );
    assert_eq!(of(ResourceType::Dictionary), ["ObsTopics"]);

    let strongs = catalog.get("StrongsGreek").unwrap();
    assert_eq!(strongs.kind, ResourceType::Lexicon);
    assert_eq!(strongs.licence.as_deref(), Some("Public Domain"));
    assert_eq!(
        strongs.used_by(),
        Some("Used by Word Study › Lexicon entries and Word Info")
    );
    assert_eq!(strongs.refused, None);
    assert_eq!(
        catalog.get("ObsNotes").unwrap().used_by(),
        Some("Used by Passage Guide › Commentaries")
    );
    assert_eq!(
        catalog.get("ObsTopics").unwrap().used_by(),
        Some("Used by Passage Guide › People & places")
    );
    assert_eq!(
        catalog.get("ObsKjvOsis").unwrap().used_by(),
        Some("Used by Word Study › Translated as and Occurrences"),
        "a Bible with Strong's numbers"
    );
    assert_eq!(catalog.get("ObsHebrew").unwrap().used_by(), None);

    // Modules Import refuses today are listed, with the reason.
    let mt = catalog.get("ObsHebrewNotes").unwrap();
    assert_eq!(
        mt.refused.as_deref(),
        Some("Commentary numbering MT cannot yet be normalized to Standard; import refused")
    );
    let href = catalog.get("ObsLinks").unwrap();
    assert!(
        href.refused.as_deref().unwrap().contains("HREFCom"),
        "{href:?}"
    );

    // Featured, per type; ObsNotes is not on any Featured list.
    assert_eq!(
        modules(&catalog.featured(ResourceType::Lexicon)),
        ["StrongsGreek"]
    );
    assert!(catalog.featured(ResourceType::Commentary).is_empty());
    let out = s.run("getbibles.list", &json!({})).unwrap();
    let featured: Vec<&str> = out["featured"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["module"].as_str().unwrap())
        .collect();
    assert_eq!(featured, ["NASB1995", "StrongsGreek"], "Bibles first");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_refused_module_cannot_be_downloaded_and_says_why() {
    let (mut s, repos, root) = ready("refused", JobMode::Inline);
    let before = repos.fetched.lock().unwrap().len();
    let err = s
        .run("getbibles.download", &json!({"module": "ObsHebrewNotes"}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot be imported yet"), "{err}");
    assert!(err.contains("Commentary numbering MT"), "{err}");
    assert_eq!(
        repos.fetched.lock().unwrap().len(),
        before,
        "nothing fetched"
    );
    assert!(s.imports.records().next().is_none());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// Download `module` with one command and run the import to the end.
fn download(s: &mut Session, module: &str) -> String {
    let out = s
        .run("getbibles.download", &json!({"module": module}))
        .unwrap();
    let id = out["id"].as_str().unwrap().to_string();
    s.jobs().pump(1);
    assert!(s.poll_imports().is_empty());
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    id
}

/// Ask for a Guide section until it is ready.
fn section(s: &mut Session, key: &obs_model::Key, section: GuideSection) -> Arc<SectionData> {
    for _ in 0..50 {
        match s.guide_section(key, section) {
            SectionStatus::Ready(d) => return d,
            SectionStatus::Failed(e) => panic!("{e}"),
            _ => {
                s.jobs().pump(4);
                assert!(s.poll_imports().is_empty());
            }
        }
    }
    panic!("{section:?} never finished");
}

#[test]
fn strongs_greek_downloads_in_one_click_and_word_study_on_g26_shows_its_entry() {
    let (mut s, _repos, root) = ready("lexicon", JobMode::Inline);
    let listing = s
        .get_bibles
        .catalog()
        .unwrap()
        .get("StrongsGreek")
        .unwrap()
        .clone();
    let id = download(&mut s, "StrongsGreek");
    assert_eq!(id, "strongsgreek-1");
    assert_eq!(
        s.listing_status(&listing),
        ListingStatus::Installed {
            resource_id: id.clone()
        }
    );
    let entry = s.library.get(&id).unwrap();
    assert_eq!(entry.manifest.resource_type, ResourceType::Lexicon);
    assert_eq!(sword_modules(&root.join("sword")), ["StrongsGreek"]);

    let g26 = obs_model::Key::StrongsNumber("G26".parse().unwrap());
    let data = section(&mut s, &g26, GuideSection::LexiconEntries);
    let SectionData::LexiconEntries(entries) = &*data else {
        panic!("{data:?}")
    };
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].resource_id, id);
    assert!(entries[0].text.contains("love"), "{:?}", entries[0].text);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_sword_commentary_downloads_and_shows_in_passage_guide_commentaries() {
    let (mut s, _repos, root) = ready("commentary", JobMode::Inline);
    let id = download(&mut s, "ObsNotes");
    assert_eq!(
        s.library.get(&id).unwrap().manifest.resource_type,
        ResourceType::Commentary
    );
    let john_3_16 = obs_model::Key::Passage(obs_scripture::Passage::verse(
        obs_scripture::Verse::new(obs_scripture::Book::John, 3, 16),
    ));
    let data = section(&mut s, &john_3_16, GuideSection::Commentaries);
    let SectionData::Commentaries(rows) = &*data else {
        panic!("{data:?}")
    };
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].resource_id, id);
    assert!(
        rows[0].excerpt.contains("gospel in a nutshell"),
        "{:?}",
        rows[0].excerpt
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_sword_dictionary_downloads_into_the_library_as_a_dictionary() {
    let (mut s, _repos, root) = ready("dictionary", JobMode::Inline);
    let id = download(&mut s, "ObsTopics");
    let entry = s.library.get(&id).unwrap();
    assert_eq!(entry.manifest.resource_type, ResourceType::Dictionary);
    assert_eq!(entry.manifest.key_schemes, [obs_model::KeyScheme::Headword]);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn offline_the_cached_catalog_still_lists_every_type() {
    let (mut s, repos, root) = session("offline-types");
    refresh(&mut s);
    drop(s);
    repos.set_offline(true);
    let mut s = Session::new(JobMode::Inline);
    s.set_fetch(repos.clone());
    s.get_bibles
        .set_cache_path(root.join("cache/sword-catalog.json"));
    refresh(&mut s);
    assert!(s.get_bibles.offline().is_some());
    let catalog = s.get_bibles.catalog().unwrap();
    for (ty, module) in [
        (ResourceType::Lexicon, "StrongsGreek"),
        (ResourceType::Commentary, "ObsNotes"),
        (ResourceType::Dictionary, "ObsTopics"),
    ] {
        assert_eq!(
            modules(&catalog.featured(ty)).len(),
            usize::from(ty == ResourceType::Lexicon)
        );
        assert!(
            modules(&catalog.all("", None, Some(ty))).contains(&module.to_string()),
            "{module}"
        );
    }
    assert!(catalog.get("ObsHebrewNotes").unwrap().refused.is_some());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_bibles_only_cache_from_before_get_resources_opens_and_is_refreshed() {
    let root = temp("old-cache");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("sword-catalog.json");
    let old = json!({
        "fetched_at": crate::get_bibles::now_secs(),
        "repos": [],
        "bibles": [{"module": "KJV", "repo": "CrossWire", "title": "King James Version",
                    "abbreviation": "KJV", "language": "en", "size": null,
                    "licence": "Public Domain", "version": "3.1"}],
        "failed": [],
    });
    std::fs::write(&path, old.to_string()).unwrap();
    let mut s = Session::new(JobMode::Inline);
    s.get_bibles.set_cache_path(path);
    let catalog = s.get_bibles.catalog().unwrap();
    assert_eq!(catalog.version, 0);
    assert_eq!(catalog.get("KJV").unwrap().kind, ResourceType::Bible);
    assert!(
        s.get_bibles.needs_refresh(crate::get_bibles::now_secs()),
        "fresh, but from before Lexicons, Commentaries and Dictionaries were listed"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn get_bibles_opens_the_tab_filtered_to_bibles_and_get_resources_to_all_or_a_type() {
    let (mut s, _repos, root) = ready("menu", JobMode::Inline);
    let out = s.run("library.get_bibles", &json!({})).unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(s.get_bibles.type_filter(), Some(ResourceType::Bible));
    assert_eq!(s.workspace.tab(tab).unwrap().title, "Get Resources");
    let again = s.run("library.get_resources", &json!({})).unwrap();
    assert_eq!(again["tab"], out["tab"], "the same single Tab");
    assert_eq!(s.get_bibles.type_filter(), None);
    s.run("library.get_resources", &json!({"type": "lexicons"}))
        .unwrap();
    assert_eq!(s.get_bibles.type_filter(), Some(ResourceType::Lexicon));
    assert!(
        s.run("library.get_resources", &json!({"type": "maps"}))
            .is_err()
    );
    let library = crate::MENUS.iter().find(|m| m.title == "Library").unwrap();
    let ids: Vec<&str> = library
        .items
        .iter()
        .filter_map(|i| match i {
            crate::MenuItem::Command(id) => Some(*id),
            _ => None,
        })
        .collect();
    assert!(ids.contains(&"library.get_resources") && ids.contains(&"library.get_bibles"));
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}
