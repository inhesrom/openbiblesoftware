//! Get Resources' licence filter and Open catalogue (#134), against the mock SWORD
//! repositories and a mock Open catalogue at a `file://` URL. No network.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::get_bibles::{ListingSource, ListingStatus, ResourceFilter, ResourceListing};
use crate::licence::{LicenceClass, LicenceFilter};
use crate::testing::{MOCK_OPEN_NOTICE, MockRepos, mock_open_catalogue};
use crate::*;

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("obs-open-catalogue-{name}-{}", std::process::id()))
}

/// An inline Session with the mock repositories' catalog fetched, a Library folder, and the
/// mock Open catalogue whose zip carries `notice`.
fn session(name: &str, notice: &str) -> (Session, Arc<MockRepos>, PathBuf) {
    let root = temp(name);
    let repos = Arc::new(MockRepos::new(&root.join("repos")));
    let mut s = Session::new(JobMode::Inline);
    s.set_fetch(repos.clone());
    s.get_bibles
        .set_cache_path(root.join("cache/sword-catalog.json"));
    s.get_bibles.set_sword_dir(root.join("sword"));
    s.get_bibles
        .set_open_catalogue(mock_open_catalogue(&root.join("open"), notice));
    std::fs::create_dir_all(root.join("library")).unwrap();
    s.library.scan(&root.join("library"));
    s.run("getbibles.refresh", &json!({})).unwrap();
    s.jobs().pump(1);
    assert!(s.poll_imports().is_empty());
    (s, repos, root)
}

fn modules(rows: &Value) -> Vec<String> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|r| r["module"].as_str().unwrap().to_string())
        .collect()
}

fn download_open(s: &mut Session, id: &str) -> String {
    let out = s
        .run("getbibles.download_open", &json!({"id": id}))
        .unwrap();
    let rid = out["id"].as_str().unwrap().to_string();
    s.jobs().pump(1);
    assert!(s.poll_imports().is_empty());
    rid
}

/// #134 acceptance: "Free to share" hides every Resource whose licence is not Public Domain,
/// CC0, CC BY, CC BY-SA or GPL — in the Featured sections, the Open catalogue and All.
#[test]
fn free_to_share_hides_every_resource_whose_licence_is_not_free() {
    let (mut s, _repos, root) = session("free-to-share", MOCK_OPEN_NOTICE);
    // Every mock SWORD module states a licence; one more Open row gives an Unclear one.
    let mut sources = s.get_bibles.open_sources().to_vec();
    let mut unclear = sources[0].clone();
    unclear.id = "unclear-excerpt".into();
    unclear.licence = "Copyrighted".into();
    sources.push(unclear);
    // A spelled-out no-derivatives licence beside "Attribution" is not free (#134 review).
    let mut no_derivs = sources[0].clone();
    no_derivs.id = "noderivs-excerpt".into();
    no_derivs.licence = "Creative Commons Attribution-NoDerivs 3.0 Unported".into();
    sources.push(no_derivs);
    // A free licence that denies redistribution in another clause is Unclear (#134 review).
    let mut no_redistribution = sources[0].clone();
    no_redistribution.id = "no-redistribution-excerpt".into();
    no_redistribution.licence = "Public Domain; no redistribution".into();
    sources.push(no_redistribution);
    s.get_bibles.set_open_catalogue(sources);
    let gb = &s.get_bibles;
    let every: Vec<&ResourceListing> = gb
        .catalog()
        .unwrap()
        .listings
        .iter()
        .chain(gb.open_listings())
        .collect();
    // The mock has all three classes, so the filter has something to hide.
    let class_of = |m: &str| {
        every
            .iter()
            .find(|b| b.module == m)
            .unwrap()
            .licence_class()
    };
    assert_eq!(class_of("NASB1995"), LicenceClass::PersonalUse);
    assert_eq!(class_of("ObsKjvOsis"), LicenceClass::FreeToShare);
    assert_eq!(class_of("web-excerpt"), LicenceClass::FreeToShare);
    assert_eq!(class_of("noderivs-excerpt"), LicenceClass::PersonalUse);
    let unclear: Vec<&str> = every
        .iter()
        .filter(|b| b.licence_class() == LicenceClass::Unclear)
        .map(|b| b.module.as_str())
        .collect();
    assert_eq!(unclear, ["unclear-excerpt", "no-redistribution-excerpt"]);

    for filter in ResourceFilter::CHOICES {
        let shown: Vec<&ResourceListing> = gb
            .sections(filter, LicenceFilter::FreeToShare)
            .into_iter()
            .flat_map(|(_, rows)| rows)
            .chain(gb.all_listings("", None, filter, LicenceFilter::FreeToShare))
            .collect();
        for b in &shown {
            assert_eq!(b.licence_class(), LicenceClass::FreeToShare, "{}", b.module);
        }
        // Nothing free is lost: every Free to share listing of the type is in All.
        let all = gb.all_listings("", None, filter, LicenceFilter::FreeToShare);
        let free = every
            .iter()
            .filter(|b| filter.accepts(b) && b.licence_class() == LicenceClass::FreeToShare)
            .count();
        assert_eq!(all.len(), free, "{filter:?}");
    }

    // Free for personal use adds the permission and non-commercial ones; Unclear only under All.
    let personal = gb.all_listings("", None, ResourceFilter::All, LicenceFilter::PersonalUse);
    assert!(personal.iter().any(|b| b.module == "NASB1995"));
    assert!(personal.iter().any(|b| b.module == "noderivs-excerpt"));
    assert!(
        !personal
            .iter()
            .any(|b| b.module == "no-redistribution-excerpt")
    );
    assert!(
        personal
            .iter()
            .all(|b| b.licence_class() != LicenceClass::Unclear)
    );
    let all = gb.all_listings("", None, ResourceFilter::All, LicenceFilter::All);
    assert!(all.iter().any(|b| unclear.contains(&b.module.as_str())));

    // Through the command: NASB 1995 (permission to distribute) leaves Featured and All.
    let out = s.run("getbibles.list", &json!({})).unwrap();
    assert!(modules(&out["featured"]).contains(&"NASB1995".to_string()));
    let out = s
        .run("getbibles.list", &json!({"licence": "free-to-share"}))
        .unwrap();
    assert!(!modules(&out["featured"]).contains(&"NASB1995".to_string()));
    assert!(!modules(&out["all"]).contains(&"NASB1995".to_string()));
    for row in out["all"].as_array().unwrap() {
        assert_eq!(row["licence_class"], "free-to-share", "{row}");
    }
    // `library.get_resources` can open the Tab already filtered, and keeps the filter otherwise.
    s.run(
        "library.get_resources",
        &json!({"licence": "free-to-share"}),
    )
    .unwrap();
    assert_eq!(s.get_bibles.licence_filter(), LicenceFilter::FreeToShare);
    s.run("library.get_resources", &json!({"type": "bibles"}))
        .unwrap();
    assert_eq!(s.get_bibles.licence_filter(), LicenceFilter::FreeToShare);
    s.run("library.get_resources", &json!({"licence": "all"}))
        .unwrap();
    assert_eq!(s.get_bibles.licence_filter(), LicenceFilter::All);
    assert!(
        s.run("getbibles.list", &json!({"licence": "cheap"}))
            .unwrap_err()
            .to_string()
            .contains("Unknown licence filter")
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_open_catalogue_is_its_own_section_and_lists_offline() {
    let (s, _repos, root) = session("section", MOCK_OPEN_NOTICE);
    let sections = s
        .get_bibles
        .sections(ResourceFilter::All, LicenceFilter::All);
    let (title, rows) = sections.last().unwrap();
    assert_eq!(title, "Open catalogue");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].source, ListingSource::Open);
    assert_eq!(rows[0].source_label(), "from eBible.org");
    assert!(
        rows[0]
            .evidence_url
            .as_deref()
            .unwrap()
            .starts_with("https://web.archive.org/web/")
    );
    // Lexicons: no Open catalogue section (the mock has only a Bible).
    assert!(
        s.get_bibles
            .sections(
                ResourceFilter::Type(obs_model::ResourceType::Lexicon),
                LicenceFilter::All
            )
            .iter()
            .all(|(t, _)| t != "Open catalogue")
    );
    drop(s);

    // A fresh Session with no catalog fetched (offline at first start) still lists it.
    let mut s = Session::new(JobMode::Inline);
    assert!(s.get_bibles.catalog().is_none());
    let out = s.run("getbibles.list", &json!({})).unwrap();
    let open = modules(&out["open"]);
    assert_eq!(open.len(), crate::open_catalogue::open_sources().len());
    assert!(open.contains(&"ylt".to_string()));
    assert_eq!(out["open"][0]["source"], "open");
    assert_eq!(out["open"][0]["licence_class"], "free-to-share");
    assert!(out["open"][0]["evidence_url"].as_str().is_some());
    assert_eq!(modules(&out["all"]).len(), open.len());
    std::fs::remove_dir_all(root).unwrap();
}

/// #134 acceptance: a non-SWORD open source (an eBible.org-style USFM Bible) downloads and
/// imports from the Open catalogue, with its licence and Attribution in the manifest, so
/// About and Help → Credits pick them up.
#[test]
fn an_ebible_usfm_bible_downloads_and_imports_from_the_open_catalogue() {
    let (mut s, repos, root) = session("download", MOCK_OPEN_NOTICE);
    let source = s.get_bibles.open_source("web-excerpt").unwrap().clone();
    let listing = source.listing();
    assert_eq!(s.listing_status(&listing), ListingStatus::Available);
    assert!(s.open_resource_credits().is_empty());

    let id = download_open(&mut s, "web-excerpt");
    assert!(id.starts_with("web-excerpt-"), "{id}");
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Complete, "{:?}", record.error);
    assert_eq!(record.origin, None, "not a SWORD download");
    let open = record.open_origin.as_ref().unwrap();
    assert_eq!(open.id, "web-excerpt");
    assert_eq!(open.url, source.url);
    let zip = std::fs::read(root.join("open/webx_usfm.zip")).unwrap();
    assert_eq!(
        open.sha256.as_deref(),
        Some(crate::starter::sha256_hex(&zip).as_str())
    );
    assert!(
        record.source.contains("from eBible.org"),
        "{}",
        record.source
    );
    assert!(repos.fetched.lock().unwrap().contains(&source.url));

    // In the Library, with the entry's licence, Attribution and changes.
    let entry = s.library.get(&id).unwrap();
    let m = &entry.manifest;
    assert_eq!(m.title, "World English Bible excerpt");
    assert_eq!(m.abbreviation, "WEBX");
    assert_eq!(m.resource_type, obs_model::ResourceType::Bible);
    assert_eq!(m.license, source.spdx);
    assert_eq!(m.attribution, source.attribution);
    assert_eq!(m.changes.as_deref(), Some(source.changes.as_str()));
    assert_eq!(m.source_url.as_deref(), Some(source.homepage.as_str()));
    assert_eq!(m.origin, obs_model::Origin::Imported);
    assert!(!record.report.as_ref().unwrap().license_missing);
    assert!(entry.path.is_file());

    // The row now offers Open; a second Download is refused.
    assert!(matches!(
        s.listing_status(&listing),
        ListingStatus::Installed { .. }
    ));
    let err = s
        .run("getbibles.download_open", &json!({"id": "web-excerpt"}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("already in your Library"), "{err}");

    // Help → Credits credits it from its own manifest.
    let credits = s.open_resource_credits();
    assert_eq!(credits.len(), 1);
    assert_eq!(credits[0].name, "World English Bible excerpt");
    assert_eq!(credits[0].credit, source.attribution);
    assert_eq!(credits[0].license, source.spdx);
    assert_eq!(credits[0].source, source.homepage);
    // A SWORD download is not an Open catalogue Resource.
    let out = s
        .run("getbibles.download", &json!({"module": "ObsKjvOsis"}))
        .unwrap();
    s.jobs().pump(1);
    assert!(s.poll_imports().is_empty());
    assert!(s.library.get(out["id"].as_str().unwrap()).is_some());
    assert_eq!(s.open_resource_credits().len(), 1);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// eBible regenerates its zips, so its entries are not pinned: the licence notice inside the
/// download is checked instead, and a changed notice stops the Import.
#[test]
fn a_download_whose_licence_notice_changed_is_refused() {
    let (mut s, _repos, root) = session(
        "notice",
        "<html><p>Copyright 2030. All rights reserved.</p></html>",
    );
    let id = download_open(&mut s, "web-excerpt");
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Failed);
    let error = record.error.clone().unwrap();
    assert!(
        error.contains("no longer says \"Public Domain\""),
        "{error}"
    );
    assert!(s.library.get(&id).is_none());
    assert!(!root.join(format!("library/{id}.obr")).exists());
    let listing = s.get_bibles.open_source("web-excerpt").unwrap().listing();
    assert!(matches!(
        s.listing_status(&listing),
        ListingStatus::Failed { .. }
    ));

    // The publisher puts the notice back: Retry replaces the failed attempt.
    mock_open_catalogue(&root.join("open"), MOCK_OPEN_NOTICE);
    let retry = download_open(&mut s, "web-excerpt");
    assert_eq!(
        s.imports.report(&retry).unwrap().phase,
        ImportPhase::Complete
    );
    assert!(s.imports.report(&id).is_none() || id == retry);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// #134 review: a notice that still contains the expected phrase but revokes the grant is
/// refused too, and nothing is imported under a Public Domain licence.
#[test]
fn a_download_whose_notice_negates_its_phrase_is_refused() {
    let (mut s, _repos, root) = session(
        "negated",
        "<html><p>Not Public Domain. All rights reserved.</p></html>",
    );
    let id = download_open(&mut s, "web-excerpt");
    let record = s.imports.report(&id).unwrap();
    assert_eq!(record.phase, ImportPhase::Failed);
    let error = record.error.clone().unwrap();
    assert!(error.contains("not imported"), "{error}");
    assert!(!error.contains("no longer says"), "{error}");
    assert!(s.library.get(&id).is_none());
    assert!(!root.join(format!("library/{id}.obr")).exists());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// #134 review: a notice that keeps the phrase but names a restrictive licence by its
/// abbreviation or spelled out, or denies redistribution, is refused before the Import
/// writes the catalogue's free licence into the manifest.
#[test]
fn a_download_whose_notice_names_a_restrictive_licence_is_refused() {
    for (name, notice) in [
        (
            "by-nd",
            "<p>Public Domain.</p><p>This edition is CC BY-ND 4.0.</p>",
        ),
        (
            "by-nc",
            "<p>Public Domain.</p><p>This edition is CC BY-NC 4.0.</p>",
        ),
        (
            "noderivs",
            "<p>Public Domain</p><p>Creative Commons Attribution-NoDerivs 3.0</p>",
        ),
        (
            "no-redistribution",
            "<p>Public Domain; no redistribution.</p>",
        ),
        // A licence excluded in the next sentence.
        (
            "excluded",
            "<p>Public Domain. This licence does not apply to this edition.</p>",
        ),
    ] {
        let (mut s, _repos, root) = session(&format!("restrictive-{name}"), notice);
        let id = download_open(&mut s, "web-excerpt");
        let record = s.imports.report(&id).unwrap();
        assert_eq!(record.phase, ImportPhase::Failed, "{notice}");
        let error = record.error.clone().unwrap();
        assert!(error.contains("not imported"), "{notice}: {error}");
        assert!(s.library.get(&id).is_none(), "{notice}");
        assert!(!root.join(format!("library/{id}.obr")).exists(), "{notice}");
        drop(s);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn a_pinned_download_with_another_sha256_is_refused() {
    let root = temp("pinned");
    let mut sources = mock_open_catalogue(&root.join("open"), MOCK_OPEN_NOTICE);
    sources[0].sha256 = Some("0".repeat(64));
    let mut s = Session::new(JobMode::Inline);
    s.get_bibles.set_open_catalogue(sources.clone());
    std::fs::create_dir_all(root.join("library")).unwrap();
    s.library.scan(&root.join("library"));
    let id = download_open(&mut s, "web-excerpt");
    let error = s.imports.report(&id).unwrap().error.clone().unwrap();
    assert!(error.contains("has changed"), "{error}");
    assert!(s.library.get(&id).is_none());

    // With the right pin it imports.
    let zip = std::fs::read(root.join("open/webx_usfm.zip")).unwrap();
    sources[0].sha256 = Some(crate::starter::sha256_hex(&zip));
    s.get_bibles.set_open_catalogue(sources);
    let id = download_open(&mut s, "web-excerpt");
    assert_eq!(s.imports.report(&id).unwrap().phase, ImportPhase::Complete);
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_unknown_open_entry_is_an_error() {
    let mut s = Session::new(JobMode::Inline);
    let err = s
        .run("getbibles.download_open", &json!({"id": "nope"}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("no entry \"nope\""), "{err}");
}
