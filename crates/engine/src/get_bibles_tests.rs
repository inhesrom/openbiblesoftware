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
        morphology: false,
        refused: None,
        source: ListingSource::Sword,
        evidence_url: None,
        declared_licence_class: None,
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
            "MorphGNT",
            "NASB1995",
            "OSHB",
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
    assert_eq!(catalog.all("", None, bible).len(), 7);
    assert_eq!(catalog.all("", None, None).len(), 12, "every listed type");
    assert_eq!(
        modules(&catalog.all("", Some("grc"), bible)),
        ["ObsGreekBible", "MorphGNT"]
    );
    assert_eq!(
        modules(&catalog.all("nasb", None, bible)),
        ["NASB1995"],
        "abbreviation/title search"
    );
    assert!(catalog.all("nasb", Some("grc"), bible).is_empty());

    // "greek" matches Bibles and a Lexicon; the type and language filters narrow it.
    assert_eq!(
        modules(&catalog.all("greek", None, None)),
        ["ObsGreekBible", "MorphGNT", "StrongsGreek"]
    );
    assert_eq!(
        modules(&catalog.all("greek", None, lexicon)),
        ["StrongsGreek"]
    );
    assert_eq!(
        modules(&catalog.all("greek", Some("grc"), None)),
        ["ObsGreekBible", "MorphGNT"]
    );
    assert!(catalog.all("greek", Some("grc"), lexicon).is_empty());
    assert!(catalog.all("strongs", None, bible).is_empty());

    // The language list counts the chosen type.
    let langs = catalog.languages(bible);
    assert_eq!(langs[0], ("en".to_string(), 4));
    assert!(langs.contains(&("grc".to_string(), 2)));
    assert_eq!(catalog.languages(lexicon), [("en".to_string(), 1)]);
    assert_eq!(catalog.languages(None)[0], ("en".to_string(), 9));
    assert!(catalog.languages(None).contains(&("hbo".to_string(), 1)));

    // The same through the command.
    let out = s
        .run(
            "getbibles.list",
            &json!({"query": "greek", "language": "grc"}),
        )
        .unwrap();
    assert_eq!(out["all"].as_array().unwrap().len(), 2);
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
    let inline = matches!(mode, JobMode::Inline);
    let mut s2 = Session::new(mode);
    s2.set_fetch(repos.clone());
    s2.get_bibles
        .set_cache_path(root.join("cache/sword-catalog.json"));
    s2.get_bibles.set_sword_dir(root.join("sword"));
    std::fs::create_dir_all(root.join("library")).unwrap();
    s2.library.scan(&root.join("library"));
    drop(s);
    // Checked in this Session too, as after the Tab's first refresh.
    if inline {
        refresh(&mut s2);
    }
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
        ["Easton", "Smith", "ISBE", "Nave", "Hitchcock", "Torrey"]
    );
    assert!(
        featured(ResourceType::Lexicon)
            .iter()
            .all(|f| f.repo == "CrossWire")
    );
    assert_eq!(featured_all().len(), 13 + 8 + 3 + 5 + 6);
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
    assert_eq!(
        featured,
        ["NASB1995", "OSHB", "MorphGNT", "StrongsGreek"],
        "Bibles first, then Hebrew & Greek"
    );
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

// ---- Updates and removing SWORD copies (#136) -------------------------------------------

/// [`ready`] with a User database, so Annotations can be made and re-anchored.
fn ready_with_user(name: &str) -> (Session, Arc<MockRepos>, PathBuf) {
    let (mut s, repos, root) = ready(name, JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    (s, repos, root)
}

fn listing_of(s: &Session, module: &str) -> ResourceListing {
    s.get_bibles.catalog().unwrap().get(module).unwrap().clone()
}

/// The unit of Resource `id` (as in the library now) holding `quote`, and where it starts.
fn find_text(s: &Session, id: &str, quote: &str) -> (String, usize) {
    use obs_store::ResourceStore;
    let reader = obs_store::ObrReader::open(&s.library.get(id).unwrap().path).unwrap();
    for unit in reader.unit_ids().unwrap() {
        let text = reader.unit(&unit).unwrap().unwrap().text;
        if let Some(at) = text.find(quote) {
            return (unit.as_str().to_string(), at);
        }
    }
    panic!("{quote:?} not in {id}");
}

/// Run queued jobs (downloads, imports, re-anchoring) until nothing is left.
fn settle(s: &mut Session) {
    for _ in 0..20 {
        s.jobs().pump(16);
        let _ = s.poll_imports();
        if !s.imports.is_running() && !s.reanchoring.is_busy() {
            return;
        }
    }
    panic!("background work did not finish");
}

fn sword_version(dir: &std::path::Path, module: &str) -> Option<String> {
    obs_formats::SwordSource::open(dir)
        .unwrap()
        .modules()
        .into_iter()
        .find(|m| m.name == module)
        .and_then(|m| m.version)
}

/// The GBF test Bible's John 1:1 opens with this; the update below inserts a word into it,
/// so text after it in the unit moves.
const JOHN_1_1: &str = "In<WG1722> the beginning";
const JOHN_1_1_FIXED: &str = "In<WG1722> the very beginning";

#[test]
fn bumping_a_module_version_shows_update_available_and_updating_keeps_its_notes_reanchored() {
    let (mut s, repos, root) = ready_with_user("update");
    let id = download(&mut s, "ObsKjvGbf");
    let other = download(&mut s, "ObsKjvOsis");
    let gbf = listing_of(&s, "ObsKjvGbf");
    let origin = s.imports.report(&id).unwrap().origin.clone().unwrap();
    assert_eq!(
        origin.version.as_deref(),
        Some("1.0"),
        "imported version kept"
    );
    assert_eq!(
        s.listing_status(&gbf),
        ListingStatus::Installed {
            resource_id: id.clone()
        }
    );
    assert!(s.resource_updates().up_to_date());
    // The user ranks it first among Bibles and writes a Note on John 1:4.
    s.run("library.move", &json!({"id": id, "to": 0})).unwrap();
    let (unit, at) = find_text(&s, &id, "the light of men");
    let note = s
        .run(
            "annotation.note",
            &json!({"resource": id, "start_unit": unit, "start": at,
                    "end": at + "the light of men".len(), "body": "Life is light"}),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let open_tabs = s.workspace.tabs().count();

    // CrossWire publishes 1.1 with a word added before the Note's text. Nothing changes
    // until the catalog is fetched again; then the row and the summary offer the update.
    repos.publish(
        "crosswire",
        "obskjvgbf.conf",
        "1.1",
        Some((JOHN_1_1, JOHN_1_1_FIXED)),
    );
    assert!(s.resource_updates().updates.is_empty());
    refresh(&mut s);
    let gbf = listing_of(&s, "ObsKjvGbf");
    assert_eq!(
        s.listing_status(&gbf),
        ListingStatus::UpdateAvailable {
            resource_id: id.clone(),
            installed: "1.0".into(),
            available: "1.1".into(),
        }
    );
    let summary = s.resource_updates();
    assert_eq!(
        summary.updates,
        [ResourceUpdate {
            resource_id: id.clone(),
            module: "ObsKjvGbf".into(),
            repo: "CrossWire".into(),
            installed: "1.0".into(),
            available: "1.1".into(),
        }]
    );
    assert!(!summary.up_to_date());
    let list = s.run("getbibles.list", &json!({"query": "gbf"})).unwrap();
    assert_eq!(list["all"][0]["update"]["available"], "1.1");
    assert_eq!(list["updates"]["available"][0]["id"], id.as_str());
    // A second Download is still refused: the update is the way in.
    assert!(
        s.run("getbibles.download", &json!({"module": "ObsKjvGbf"}))
            .is_err()
    );

    // One click updates it in place: no new Resource, no report Tab.
    let out = s
        .run("getbibles.update", &json!({"module": "ObsKjvGbf"}))
        .unwrap();
    assert_eq!(out["id"], id.as_str());
    assert!(s.imports.is_update(&id));
    // While it is downloading, the other download being current says nothing about this
    // one: it is updating, not up to date and not offered again.
    let summary = s.resource_updates();
    assert_eq!(summary.updating, std::slice::from_ref(&id));
    assert!(summary.updates.is_empty());
    assert_eq!(summary.downloaded, 2);
    assert!(!summary.up_to_date());
    let list = s.run("getbibles.list", &json!({"query": "gbf"})).unwrap();
    assert_eq!(list["updates"]["updating"][0], id.as_str());
    assert_eq!(list["updates"]["up_to_date"], false);
    assert!(matches!(
        s.listing_status(&gbf),
        ListingStatus::InProgress { .. }
    ));
    assert_eq!(s.workspace.tabs().count(), open_tabs);
    s.workspace.take_effects();
    settle(&mut s);
    let effects = s.workspace.take_effects();
    assert!(
        effects.contains(&Effect::Replaced(id.clone())),
        "{effects:?}"
    );

    // The same Resource, now its next Resource revision, in the same place.
    let entry = s.library.get(&id).unwrap();
    assert_eq!(entry.manifest.revision, 2);
    assert_eq!(
        s.library.position_in_type(&id),
        Some(0),
        "Preferred order kept"
    );
    assert_eq!(s.library.position_in_type(&other), Some(1));
    let mut files = obr_files(&root.join("library"));
    files.sort();
    assert_eq!(files, [format!("{id}.obr"), format!("{other}.obr")]);
    let (_, fixed_at) = find_text(&s, &id, "the very beginning");
    assert!(fixed_at > 0, "the new text is in the library");
    assert_eq!(
        sword_version(&root.join("sword"), "ObsKjvGbf").as_deref(),
        Some("1.1")
    );
    assert_eq!(
        s.imports
            .report(&id)
            .unwrap()
            .origin
            .as_ref()
            .unwrap()
            .version
            .as_deref(),
        Some("1.1")
    );
    assert_eq!(
        s.listing_status(&gbf),
        ListingStatus::Installed {
            resource_id: id.clone()
        }
    );
    assert!(s.resource_updates().up_to_date());

    // The Note is kept and re-anchored onto its words in the new text.
    let report = s.reanchoring.report(&id).unwrap();
    assert_eq!(report.moved, 1, "{report:?}");
    let a = s.annotation(&note).unwrap();
    assert!(!a.is_orphaned());
    let range = a.anchor.unwrap().text_range().cloned().unwrap();
    let (unit, at) = find_text(&s, &id, "the light of men");
    assert_eq!(range.unit.as_str(), unit);
    assert_eq!(range.start as usize, at);
    assert_eq!(range.end as usize, at + "the light of men".len());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_all_updates_every_downloaded_resource_with_a_newer_version() {
    let (mut s, repos, root) = ready("update-all", JobMode::Inline);
    let bible = download(&mut s, "ObsKjvGbf");
    let notes = download(&mut s, "ObsNotes");
    let topics = download(&mut s, "ObsTopics");
    repos.publish(
        "crosswire",
        "obskjvgbf.conf",
        "1.1",
        Some((JOHN_1_1, JOHN_1_1_FIXED)),
    );
    repos.publish("crosswire", "obsnotes.conf", "2.0", None);
    refresh(&mut s);
    let updates: Vec<String> = s
        .resource_updates()
        .updates
        .into_iter()
        .map(|u| u.resource_id)
        .collect();
    assert_eq!(updates, [bible.clone(), notes.clone()]);
    let out = s.run("getbibles.update_all", &json!({})).unwrap();
    assert_eq!(out["started"], json!([bible, notes]));
    settle(&mut s);
    assert!(s.resource_updates().up_to_date());
    assert_eq!(s.library.get(&bible).unwrap().manifest.revision, 2);
    assert_eq!(s.library.get(&notes).unwrap().manifest.revision, 2);
    assert_eq!(s.library.get(&topics).unwrap().manifest.revision, 1);
    // Nothing left to update.
    let out = s.run("getbibles.update_all", &json!({})).unwrap();
    assert_eq!(out["started"], json!([]));
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_failed_update_keeps_the_revision_in_the_library() {
    let (mut s, repos, root) = ready("update-fails", JobMode::Inline);
    let id = download(&mut s, "ObsKjvGbf");
    let before = s.library.get(&id).unwrap().manifest.clone();
    repos.publish(
        "crosswire",
        "obskjvgbf.conf",
        "1.1",
        Some((JOHN_1_1, JOHN_1_1_FIXED)),
    );
    refresh(&mut s);
    repos.set_offline(true);
    s.run("getbibles.update", &json!({"module": "ObsKjvGbf"}))
        .unwrap();
    s.workspace.take_effects();
    settle(&mut s);
    let effects = s.workspace.take_effects();
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::Message(m) if m.starts_with("Could not update "))),
        "{effects:?}"
    );
    assert_eq!(s.library.get(&id).unwrap().manifest, before);
    assert_eq!(obr_files(&root.join("library")), [format!("{id}.obr")]);
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete);
    assert!(matches!(
        s.listing_status(&listing_of(&s, "ObsKjvGbf")),
        ListingStatus::UpdateAvailable { .. }
    ));
    // Back online, the update goes through.
    repos.set_offline(false);
    s.run("getbibles.update", &json!({"module": "ObsKjvGbf"}))
        .unwrap();
    settle(&mut s);
    assert_eq!(s.library.get(&id).unwrap().manifest.revision, 2);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// The Import indexer, then the staged revision goes missing: the commit after the Index
/// phase fails.
#[derive(Debug)]
struct StagedFileLost(CacheIndexer);

impl ImportIndexer for StagedFileLost {
    fn build(
        &self,
        resource: &obs_model::Resource,
        path: &std::path::Path,
        ctx: &JobContext,
    ) -> Result<ImportIndexStatus, String> {
        let built = self.0.build(resource, path, ctx);
        let mut staged = path.as_os_str().to_owned();
        staged.push(".update");
        std::fs::remove_file(staged).unwrap();
        built
    }
}

#[test]
fn an_update_failing_after_its_index_phase_keeps_the_old_revision_searchable() {
    let (mut s, repos, root) = ready("update-commit-fails", JobMode::Inline);
    let cache_dir = root.join("search");
    s.set_search_cache_dir(cache_dir.clone());
    let id = download(&mut s, "ObsKjvGbf");
    settle(&mut s);
    let caches = || -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&cache_dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&id))
            .collect();
        names.sort();
        names
    };
    let before = caches();
    assert_eq!(before.len(), 1, "{before:?}");
    assert_eq!(s.search.status(&id), Some(&IndexStatus::Ready));

    repos.publish(
        "crosswire",
        "obskjvgbf.conf",
        "1.1",
        Some((JOHN_1_1, JOHN_1_1_FIXED)),
    );
    refresh(&mut s);
    let indexer = CacheIndexer {
        dir: cache_dir.clone(),
        version: INDEX_VERSION,
    };
    s.imports
        .set_indexer(Arc::new(StagedFileLost(indexer.clone())));
    s.run("getbibles.update", &json!({"module": "ObsKjvGbf"}))
        .unwrap();
    s.workspace.take_effects();
    settle(&mut s);
    assert!(
        s.workspace
            .take_effects()
            .iter()
            .any(|e| matches!(e, Effect::Message(m) if m.starts_with("Could not update "))),
    );
    assert_eq!(s.library.get(&id).unwrap().manifest.revision, 1);
    // The old revision's cache was not removed with the update: it still searches, with no
    // rebuild.
    assert!(caches().contains(&before[0]), "{:?}", caches());
    let hits = s
        .search(&SearchRequest {
            query: "beginning".into(),
            resources: vec![id.clone()],
            ..Default::default()
        })
        .unwrap();
    assert!(
        hits.indexing.is_empty() && hits.stale.is_empty(),
        "{hits:?}"
    );
    assert!(hits.total() > 0);
    assert_eq!(s.search.status(&id), Some(&IndexStatus::Ready));

    // Once an update is in, only the new revision's cache is left.
    s.imports.set_indexer(Arc::new(indexer));
    s.run("getbibles.update", &json!({"module": "ObsKjvGbf"}))
        .unwrap();
    settle(&mut s);
    assert_eq!(s.library.get(&id).unwrap().manifest.revision, 2);
    let after = caches();
    assert_eq!(after.len(), 1, "{after:?}");
    assert_ne!(after, before);
    assert_eq!(s.search.status(&id), Some(&IndexStatus::Ready));
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_update_replaces_the_resource_file_the_library_read_whatever_it_is_named() {
    let (mut s, repos, root) = ready("update-renamed", JobMode::Inline);
    let library = root.join("library");
    let id = download(&mut s, "ObsKjvGbf");
    // The user renames the file; another Resource now sits under the downloaded one's
    // id-named file. The library ties neither file name to an id.
    std::fs::rename(library.join(format!("{id}.obr")), library.join("zzz.obr")).unwrap();
    let mut unrelated = obs_store::sample_resource();
    unrelated.manifest.resource_id = "unrelated".into();
    obs_store::write_obr(&unrelated, &library.join(format!("{id}.obr"))).unwrap();
    assert!(s.library.scan(&library).is_empty());
    assert_eq!(s.library.get(&id).unwrap().path, library.join("zzz.obr"));

    repos.publish(
        "crosswire",
        "obskjvgbf.conf",
        "1.1",
        Some((JOHN_1_1, JOHN_1_1_FIXED)),
    );
    refresh(&mut s);
    s.run("getbibles.update", &json!({"module": "ObsKjvGbf"}))
        .unwrap();
    settle(&mut s);
    let entry = s.library.get(&id).unwrap();
    assert_eq!(entry.manifest.revision, 2);
    assert_eq!(
        entry.path,
        library.join("zzz.obr"),
        "the same file, updated"
    );
    let mut files = obr_files(&library);
    files.sort();
    assert_eq!(files, [format!("{id}.obr"), "zzz.obr".to_string()]);
    assert_eq!(
        s.library.get("unrelated").unwrap().manifest,
        unrelated.manifest,
        "the other Resource is untouched"
    );

    // After a restart the library still has the new revision, not the one before it.
    let mut restarted = Library::default();
    assert!(restarted.scan(&library).is_empty());
    assert_eq!(restarted.get(&id).unwrap().manifest.revision, 2);
    assert_eq!(restarted.get("unrelated").unwrap().manifest.revision, 1);
    assert_eq!(
        s.listing_status(&listing_of(&s, "ObsKjvGbf")),
        ListingStatus::Installed {
            resource_id: id.clone()
        }
    );

    // Remove deletes that file and leaves the other Resource's.
    s.run("library.remove", &json!({"id": id, "confirm": true}))
        .unwrap();
    settle(&mut s);
    assert_eq!(obr_files(&library), [format!("{id}.obr")]);
    let mut restarted = Library::default();
    assert!(restarted.scan(&library).is_empty());
    assert!(restarted.get(&id).is_none());
    assert!(restarted.get("unrelated").is_some());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn after_a_restart_offline_the_saved_catalog_makes_no_up_to_date_claim() {
    let (mut s, repos, root) = ready_with_user("update-restart");
    let id = download(&mut s, "ObsKjvGbf");
    refresh(&mut s);
    assert!(s.resource_updates().up_to_date());
    drop(s);

    // Next app start, soon after and with no network. The saved catalog is fresh, so no
    // refresh runs, and it still shows no update (CrossWire has one it cannot see): the Tab
    // must say when updates were last checked, not that the downloads are up to date.
    repos.publish("crosswire", "obskjvgbf.conf", "1.1", None);
    repos.set_offline(true);
    let mut s = Session::new(JobMode::Inline);
    s.set_fetch(repos.clone());
    s.get_bibles
        .set_cache_path(root.join("cache/sword-catalog.json"));
    s.get_bibles.set_sword_dir(root.join("sword"));
    s.library.scan(&root.join("library"));
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    let at = s.get_bibles.catalog().unwrap().fetched_at;
    assert!(
        !s.get_bibles.needs_refresh(at + 60),
        "fresh cache: nothing fetched"
    );
    let summary = s.resource_updates();
    assert_eq!(summary.downloaded, 1, "the download is restored");
    assert_eq!(summary.check, UpdateCheck::Saved { at });
    assert!(summary.updates.is_empty() && summary.unchecked.is_empty());
    assert!(!summary.up_to_date());
    let list = s.run("getbibles.list", &json!({})).unwrap();
    assert_eq!(list["updates"]["up_to_date"], false);
    assert!(list["updates"]["checked_at"].is_null());

    // Back online, a refresh in this Session checks them and finds the update.
    repos.set_offline(false);
    refresh(&mut s);
    let summary = s.resource_updates();
    assert!(matches!(summary.check, UpdateCheck::Checked { .. }));
    assert_eq!(summary.updates.len(), 1);
    assert_eq!(summary.updates[0].resource_id, id);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn offline_the_tab_makes_no_up_to_date_claim() {
    let (mut s, repos, root) = ready("update-offline", JobMode::Inline);
    // Nothing downloaded: nothing to claim.
    assert!(!s.resource_updates().up_to_date());
    let id = download(&mut s, "ObsKjvGbf");
    let checked = s.resource_updates();
    assert!(matches!(checked.check, UpdateCheck::Checked { .. }));
    assert!(checked.up_to_date());

    // Offline: a refresh fails, and the Tab can no longer say "up to date", even though
    // the saved catalog shows no update (CrossWire has one it cannot see).
    repos.publish("crosswire", "obskjvgbf.conf", "1.1", None);
    repos.set_offline(true);
    refresh(&mut s);
    let summary = s.resource_updates();
    assert!(matches!(summary.check, UpdateCheck::Offline(_)));
    assert!(summary.updates.is_empty());
    assert!(!summary.up_to_date());
    let list = s.run("getbibles.list", &json!({})).unwrap();
    assert_eq!(list["updates"]["up_to_date"], false);
    assert!(list["updates"]["checked_at"].is_null());

    // While a check runs, nothing is claimed either.
    repos.set_offline(false);
    s.refresh_resource_catalog();
    assert_eq!(s.resource_updates().check, UpdateCheck::Checking);
    assert!(!s.resource_updates().up_to_date());
    s.jobs().pump(1);
    s.poll_imports();
    assert_eq!(s.resource_updates().updates.len(), 1);

    // A repository that does not answer leaves its Resources unchecked: no claim for them.
    std::fs::rename(root.join("repos/crosswire"), root.join("repos/gone")).unwrap();
    refresh(&mut s);
    let summary = s.resource_updates();
    assert!(matches!(summary.check, UpdateCheck::Checked { .. }));
    assert_eq!(summary.unchecked, [id]);
    assert!(!summary.up_to_date());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_download_from_before_updates_gets_its_version_from_its_sword_copy() {
    let (mut s, repos, root) = ready("update-legacy", JobMode::Inline);
    let id = download(&mut s, "ObsKjvGbf");
    // As saved by v0.1: no version on the origin.
    s.imports
        .reports
        .get_mut(&id)
        .unwrap()
        .origin
        .as_mut()
        .unwrap()
        .version = None;
    assert_eq!(s.resource_updates().unchecked, std::slice::from_ref(&id));
    repos.publish("crosswire", "obskjvgbf.conf", "1.1", None);
    refresh(&mut s);
    assert_eq!(
        s.imports
            .report(&id)
            .unwrap()
            .origin
            .as_ref()
            .unwrap()
            .version
            .as_deref(),
        Some("1.0")
    );
    assert_eq!(s.resource_updates().updates.len(), 1);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn removing_a_resource_while_it_is_updating_is_refused_and_deletes_nothing() {
    let (mut s, repos, root) = ready("remove-updating", JobMode::Inline);
    let id = download(&mut s, "ObsKjvGbf");
    repos.publish(
        "crosswire",
        "obskjvgbf.conf",
        "1.1",
        Some((JOHN_1_1, JOHN_1_1_FIXED)),
    );
    refresh(&mut s);
    s.run("getbibles.update", &json!({"module": "ObsKjvGbf"}))
        .unwrap();
    assert!(s.imports.job_of(&id).is_some(), "the update is queued");
    // Neither the question nor the confirmed Remove go through while it runs.
    for confirm in [false, true] {
        let err = s
            .run("library.remove", &json!({"id": id, "confirm": confirm}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("is being updated"), "{err}");
    }
    assert!(s.library.get(&id).is_some());
    assert!(
        !s.workspace
            .take_effects()
            .iter()
            .any(|e| matches!(e, Effect::ConfirmRemove { .. } | Effect::Removed(_)))
    );
    settle(&mut s);
    assert!(!s.imports.is_running(), "no Import left tracked");
    assert_eq!(s.library.get(&id).unwrap().manifest.revision, 2);
    // Once the update is in, Remove deletes the new revision and its SWORD copy for good.
    s.run("library.remove", &json!({"id": id, "confirm": true}))
        .unwrap();
    settle(&mut s);
    assert!(obr_files(&root.join("library")).is_empty());
    assert!(sword_modules(&root.join("sword")).is_empty());
    s.library.scan(&root.join("library"));
    assert!(s.library.get(&id).is_none());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn removing_a_downloaded_resource_frees_its_sword_folder_and_search_index() {
    let (mut s, _repos, root) = ready("remove", JobMode::Inline);
    s.set_search_cache_dir(root.join("search"));
    let fetch = |s: &mut Session, module: &str| {
        let out = s
            .run("getbibles.download", &json!({"module": module}))
            .unwrap();
        settle(s);
        out["id"].as_str().unwrap().to_string()
    };
    let id = fetch(&mut s, "ObsKjvGbf");
    let kept = fetch(&mut s, "ObsKjvOsis");
    let data = root.join("sword/modules/texts/rawtext/obskjvgbf");
    assert!(data.is_dir());
    let caches = |id: &str| -> usize {
        std::fs::read_dir(root.join("search"))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(id))
            .count()
    };
    assert!(caches(&id) > 0, "the Import built its Search index");

    // Remove asks first, and says the SWORD copy goes too.
    let ask = s.run("library.remove", &json!({"id": id})).unwrap();
    let message = ask["confirm"].as_str().unwrap();
    assert!(
        message.contains("So is the SWORD module ObsKjvGbf"),
        "{message}"
    );
    assert!(data.is_dir(), "nothing deleted before the user confirms");
    let out = s
        .run("library.remove", &json!({"id": id, "confirm": true}))
        .unwrap();
    assert_eq!(out["deleted_sword_module"], "ObsKjvGbf");
    assert!(!data.exists());
    assert_eq!(sword_modules(&root.join("sword")), ["ObsKjvOsis"]);
    assert_eq!(obr_files(&root.join("library")), [format!("{kept}.obr")]);
    assert_eq!(caches(&id), 0);
    assert!(caches(&kept) > 0);
    // The row offers Download again, and a new download works.
    assert_eq!(
        s.listing_status(&listing_of(&s, "ObsKjvGbf")),
        ListingStatus::Available
    );
    assert_eq!(fetch(&mut s, "ObsKjvGbf"), id);
    assert!(data.is_dir());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

// ---- Get Resources (#133): Hebrew & Greek --------------------------------------------------

#[test]
fn featured_hebrew_and_greek_data_file_lists_the_checked_original_language_bibles() {
    // Checked against the live catalogs (docs/research/original-language-catalog.md): OSHB
    // replaced OSMHB (now only in CrossWire Attic); the Septuagint is left out (licence).
    let names: Vec<String> = featured_hebrew_greek()
        .into_iter()
        .map(|f| f.module)
        .collect();
    assert_eq!(
        names,
        [
            "OSHB", "WLC", "MorphGNT", "SBLGNT", "Tisch", "TR", "Byz", "WHNU"
        ]
    );
    assert!(
        featured_hebrew_greek()
            .iter()
            .all(|f| f.repo == "CrossWire")
    );
    assert!(!names.iter().any(|n| n == "LXX" || n == "OSMHB"));
    // Not a Featured Bible: a group of its own.
    assert!(
        !featured(ResourceType::Bible)
            .iter()
            .any(|f| names.contains(&f.module))
    );
}

#[test]
fn catalog_tags_bibles_with_strongs_and_morphology_from_the_conf() {
    let (mut s, _repos, root) = session("tags");
    refresh(&mut s);
    let catalog = s.get_bibles.catalog().unwrap();
    let tags = |m: &str| catalog.get(m).unwrap().tags();
    // GlobalOptionFilter=OSISStrongs and OSISMorph.
    assert_eq!(tags("OSHB"), [TAG_STRONGS, TAG_MORPHOLOGY]);
    assert_eq!(tags("MorphGNT"), ["Strong's", "Morphology"]);
    // Feature=StrongsNumbers (with or without OSISStrongs): Strong's, no Morphology.
    assert_eq!(tags("ObsKjvOsis"), [TAG_STRONGS]);
    assert_eq!(tags("ObsGreekBible"), [TAG_STRONGS]);
    assert_eq!(
        catalog.get("ObsGreekBible").unwrap().used_by(),
        Some("Used by Word Info, Word Study › Occurrences, and the Exegetical Guide")
    );
    assert!(tags("ObsHebrew").is_empty());
    // Only Bibles carry tags.
    assert!(tags("StrongsGreek").is_empty());

    let oshb = catalog.get("OSHB").unwrap();
    assert_eq!(oshb.language, "hbo");
    assert_eq!(oshb.licence.as_deref(), Some("Creative Commons: BY 4.0"));
    assert_eq!(
        oshb.used_by(),
        Some("Used by Word Info, Word Study › Occurrences and Forms, and the Exegetical Guide")
    );
    assert_eq!(
        catalog.get("ObsKjvOsis").unwrap().used_by(),
        Some("Used by Word Study › Translated as and Occurrences"),
        "an English Bible with Strong's numbers is unchanged"
    );

    // Through the command, each row names its tags.
    let out = s
        .run("getbibles.list", &json!({"type": "hebrew-greek"}))
        .unwrap();
    let row = out["all"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["module"] == "MorphGNT")
        .unwrap();
    assert_eq!(row["tags"], json!(["Strong's", "Morphology"]));
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_hebrew_and_greek_filter_shows_its_featured_group_and_original_language_bibles() {
    let (mut s, _repos, root) = ready("hebrew-greek", JobMode::Inline);
    let catalog = s.get_bibles.catalog().unwrap();
    let titles = |filter| -> Vec<(String, Vec<String>)> {
        catalog
            .featured_sections(filter)
            .into_iter()
            .map(|(t, rows)| (t, modules(&rows)))
            .collect()
    };
    let group = (
        "Featured Hebrew & Greek".to_string(),
        vec!["OSHB".to_string(), "MorphGNT".to_string()],
    );
    assert_eq!(
        titles(ResourceFilter::HebrewAndGreek),
        std::slice::from_ref(&group)
    );
    assert_eq!(
        titles(ResourceFilter::Type(ResourceType::Bible)),
        [
            ("Featured Bibles".to_string(), vec!["NASB1995".to_string()]),
            group.clone()
        ]
    );
    assert_eq!(
        titles(ResourceFilter::All)[..2],
        [
            ("Featured Bibles".to_string(), vec!["NASB1995".to_string()]),
            group
        ]
    );
    assert!(
        titles(ResourceFilter::Type(ResourceType::Lexicon))
            .iter()
            .all(|(t, _)| t == "Featured Lexicons")
    );
    // "All Hebrew & Greek": Bibles in Hebrew, Aramaic or Greek only (no Lexicons).
    assert_eq!(
        modules(&catalog.all("", None, ResourceFilter::HebrewAndGreek)),
        ["ObsGreekBible", "MorphGNT", "OSHB"]
    );
    let langs: Vec<String> = catalog
        .languages(ResourceFilter::HebrewAndGreek)
        .into_iter()
        .map(|(l, _)| l)
        .collect();
    assert_eq!(langs, ["grc", "hbo"]);
    assert_eq!(ResourceFilter::HebrewAndGreek.label(), "Hebrew & Greek");
    assert_eq!(
        ResourceFilter::HebrewAndGreek.resource_type(),
        Some(ResourceType::Bible)
    );

    // `library.get_resources` opens the Tab at Hebrew & Greek (the empty states' action).
    s.run("library.get_resources", &json!({"type": "hebrew-greek"}))
        .unwrap();
    assert_eq!(s.get_bibles.filter(), ResourceFilter::HebrewAndGreek);
    assert_eq!(s.get_bibles.type_filter(), Some(ResourceType::Bible));
    let out = s
        .run("getbibles.list", &json!({"type": "hebrew-greek"}))
        .unwrap();
    assert_eq!(
        out["sections"],
        json!([{"title": "Featured Hebrew & Greek", "modules": ["OSHB", "MorphGNT"]}])
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

fn forms_of(s: &mut Session, number: &str) -> Vec<(String, u32)> {
    let key = obs_model::Key::StrongsNumber(number.parse().unwrap());
    let data = section(s, &key, GuideSection::Forms);
    let SectionData::Forms(f) = &*data else {
        panic!("{data:?}")
    };
    f.forms.iter().map(|f| (f.label.clone(), f.count)).collect()
}

/// Acceptance (#133): from a fresh Library, a Morphology-tagged Greek NT and a Hebrew OT are
/// one Download each from the Hebrew & Greek group, and Word Study › Forms fills in.
#[test]
fn from_a_fresh_library_a_tagged_greek_nt_and_hebrew_ot_download_and_forms_fill_in() {
    let (mut s, _repos, root) = ready("originals", JobMode::Inline);
    assert_eq!(s.library.entries().len(), 0, "a fresh Library");
    assert!(forms_of(&mut s, "G26").is_empty());
    assert!(
        GuideSection::Forms
            .empty_hint(&s.library, &s.guides.indexes())
            .contains("Get Resources › Hebrew & Greek")
    );

    let group: Vec<String> = modules(
        &s.get_bibles
            .catalog()
            .unwrap()
            .featured_sections(ResourceFilter::HebrewAndGreek)[0]
            .1,
    );
    for module in &group {
        download(&mut s, module);
    }
    let gnt = s.library.get("morphgnt-1").unwrap();
    assert_eq!(gnt.manifest.language, "grc");
    let hot = s.library.get("oshb-1").unwrap();
    assert_eq!(hot.manifest.language, "hbo");

    // Greek: John 3:16 ἠγάπησεν (a tense-voice-mood number after it, skipped), Rom 5:8
    // ἀγάπην, 1 John 4:9 ἀγάπη.
    assert_eq!(
        forms_of(&mut s, "G25"),
        [("aorist active indicative, 3rd sing.".to_string(), 1)]
    );
    assert_eq!(
        forms_of(&mut s, "G26"),
        [
            ("noun, acc. fem. sing.".to_string(), 1),
            ("noun, nom. fem. sing.".to_string(), 1)
        ]
    );
    assert!(forms_of(&mut s, "G5656").is_empty(), "not a word");
    // Hebrew: Gen 1:1 בָּרָא and Gen 2:4 בְּהִבָּרְאָם.
    let bara = forms_of(&mut s, "H1254");
    assert_eq!(bara.len(), 2, "{bara:?}");
    assert!(
        bara.iter()
            .any(|(l, n)| l == "Qal perfect, 3rd masc. sing." && *n == 1),
        "{bara:?}"
    );
    assert!(
        forms_of(&mut s, "H428")
            .iter()
            .any(|(l, _)| l == "demonstrative pronoun, com. pl."),
        "OSHB's `x` (no person) reads"
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// A Get Resources state showing `listings` (module, language) as its SWORD catalog.
fn get_bibles_with(listings: &[(&str, &str)]) -> GetBibles {
    let mut gb = GetBibles::default();
    gb.set_open_catalogue(Vec::new());
    gb.set_catalog(ResourceCatalog::new(
        0,
        Vec::new(),
        listings
            .iter()
            .map(|(module, lang)| ResourceListing {
                language: (*lang).into(),
                ..listing(module, "CrossWire")
            })
            .collect(),
    ));
    gb
}

fn choice_labels(gb: &GetBibles) -> Vec<String> {
    gb.languages(ResourceFilter::All, LicenceFilter::All)
        .iter()
        .map(|c| format!("{} · {}", c.name.label(), c.count))
        .collect()
}

#[test]
fn the_language_filter_names_every_language_sorted_by_name_with_counts() {
    let gb = get_bibles_with(&[
        ("benirv2019eb", "bn"),
        ("ben2006eb", "bn"),
        ("benobcv2019eb", "bn"),
        ("kmoeb", "kmo"),
        ("KJV", "en"),
        ("ChiUns", "zh-Hans"),
        ("Odd", "qqq"),
    ]);
    assert_eq!(
        choice_labels(&gb),
        [
            "Bangla — বাংলা · 3",
            "English · 1",
            "Kwoma · 1",
            "Simplified Chinese — 简体中文 · 1",
            "Unknown language (qqq) · 1",
        ]
    );
    // No choice is a bare code.
    let choices = gb.languages(ResourceFilter::All, LicenceFilter::All);
    assert!(choices.iter().all(|c| c.name.label() != c.code));
}

#[test]
fn the_os_language_heads_the_language_filter() {
    let mut gb = get_bibles_with(&[("KJV", "en"), ("ben2006eb", "bn"), ("Luther", "de")]);
    gb.set_system_language(Some("de-CH".into()));
    assert_eq!(
        choice_labels(&gb),
        ["German — Deutsch · 1", "Bangla — বাংলা · 1", "English · 1"]
    );
    // A 639-3 OS tag is the same language.
    gb.set_system_language(Some("ben".into()));
    assert_eq!(choice_labels(&gb)[0], "Bangla — বাংলা · 1");
    gb.set_system_language(None);
    assert_eq!(choice_labels(&gb)[0], "Bangla — বাংলা · 1");
}

#[test]
fn searching_bengali_bangla_or_ben_finds_the_bengali_bibles() {
    let gb = get_bibles_with(&[
        ("benirv2019eb", "bn"),
        ("ben2006eb", "bn"),
        ("KJV", "en"),
        ("Byz", "grc"),
    ]);
    for query in ["Bengali", "Bangla", "ben", "বাংলা"] {
        let found: Vec<String> = gb
            .languages(ResourceFilter::All, LicenceFilter::All)
            .iter()
            .filter(|c| c.name.matches(query))
            .map(|c| c.code.clone())
            .collect();
        assert_eq!(found, ["bn"], "{query:?}");
        // Choosing it lists the Bengali Bibles.
        assert_eq!(
            modules(&gb.all_listings("", Some(&found[0]), ResourceFilter::All, LicenceFilter::All)),
            ["ben2006eb", "benirv2019eb"]
        );
    }
}

#[test]
fn getbibles_list_names_each_rows_language_and_lists_the_language_filter() {
    let (mut s, _repos, root) = session("language-names");
    refresh(&mut s);
    let out = s
        .run("getbibles.list", &json!({"language": "grc"}))
        .unwrap();
    let all = out["all"].as_array().unwrap();
    assert!(!all.is_empty());
    assert!(
        all.iter().all(|r| r["language_name"] == "Ancient Greek"),
        "{all:#?}"
    );
    let languages = out["languages"].as_array().unwrap();
    assert!(
        languages
            .iter()
            .any(|l| l["code"] == "grc" && l["name"] == "Ancient Greek"),
        "{languages:#?}"
    );
    assert!(languages.iter().all(|l| l["name"] != l["code"]));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn tags_of_one_language_are_one_language_filter_choice() {
    // Live catalogs tag English `en` and `en-Latn`, and Ossetic `os` and `oss-Cyrl`.
    let gb = get_bibles_with(&[
        ("KJV", "en"),
        ("WEB", "en-Latn"),
        ("Oss", "os"),
        ("Oss2", "oss-Cyrl"),
        ("ben2006eb", "bn"),
        ("ben_bss", "ben-Beng-IN"),
    ]);
    assert_eq!(
        choice_labels(&gb),
        [
            "Bangla — বাংলা · 1",
            "Bangla (India) — বাংলা · 1",
            "English · 2",
            "Ossetic — ирон · 2",
        ]
    );
    let all =
        |l: &str| modules(&gb.all_listings("", Some(l), ResourceFilter::All, LicenceFilter::All));
    assert_eq!(all("en"), ["KJV", "WEB"]);
    assert_eq!(all("oss"), ["Oss", "Oss2"]);
    assert_eq!(all("bn-IN"), ["ben_bss"]);
    // Searching Bengali finds both.
    let found = gb
        .languages(ResourceFilter::All, LicenceFilter::All)
        .iter()
        .filter(|c| c.name.matches("Bengali"))
        .count();
    assert_eq!(found, 2);
}

#[test]
fn searching_a_catalogs_own_tag_finds_its_grouped_language_filter_choice() {
    // `ben-Beng-IN` groups as `bn-IN` and `en-Latn` as `en`; the catalog's tags still find them.
    let gb = get_bibles_with(&[
        ("KJV", "en"),
        ("WEB", "en-Latn"),
        ("ben2006eb", "bn"),
        ("ben_bss", "ben-Beng-IN"),
        ("Oss2", "oss-Cyrl"),
    ]);
    let choices = gb.languages(ResourceFilter::All, LicenceFilter::All);
    for (query, code, listed) in [
        ("ben-Beng-IN", "bn-IN", &["ben_bss"][..]),
        ("BEN-beng-in", "bn-IN", &["ben_bss"]),
        ("en-Latn", "en", &["KJV", "WEB"]),
        ("oss-Cyrl", "os", &["Oss2"]),
    ] {
        let found: Vec<&str> = choices
            .iter()
            .filter(|c| c.name.match_rank(query) == Some(0))
            .map(|c| c.code.as_str())
            .collect();
        assert_eq!(found, [code], "{query:?}");
        // Choosing it lists the tag's Bibles.
        assert_eq!(
            modules(&gb.all_listings("", Some(code), ResourceFilter::All, LicenceFilter::All)),
            listed,
            "{query:?}"
        );
    }
}
