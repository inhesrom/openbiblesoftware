//! Live checks of Get Resources (#91, #132, #133) against the real SWORD repositories, and of
//! the Open catalogue (#134) against its publishers. Ignored by default:
//! they need the network and download copyrighted modules. Run them by hand:
//!
//! ```sh
//! cargo test -p openbible --release -- --ignored --nocapture live_
//! ```
//!
//! Everything goes to a temp folder that is deleted afterwards. Nothing is kept or committed,
//! and CI never runs these (docs/dev/local-copyrighted-modules.md).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use obs_engine::get_bibles::{ListingStatus, featured, featured_hebrew_greek};
use obs_engine::{GuideSection, ImportPhase, JobMode, SectionData, SectionStatus, Session};
use obs_model::ResourceType;
use serde_json::json;

use crate::fetch::HttpFetch;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-live-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A Session as the app sets it up, with every folder under `root`.
fn session(root: &std::path::Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    s.set_fetch(Arc::new(HttpFetch::new()));
    s.get_bibles.set_cache_path(root.join("sword-catalog.json"));
    s.get_bibles.set_sword_dir(root.join("sword"));
    std::fs::create_dir_all(root.join("library")).unwrap();
    s.library.set_directory(root.join("library"));
    s
}

fn refresh(s: &mut Session) {
    s.run("library.get_bibles", &json!({})).unwrap();
    s.jobs().pump(1);
    s.poll_imports();
    assert_eq!(s.get_bibles.offline(), None, "catalog fetch failed");
}

/// Download one module and return its finished Import record's Resource id.
fn download(s: &mut Session, module: &str) -> Result<String, String> {
    let out = s
        .run("getbibles.download", &json!({"module": module}))
        .map_err(|e| e.to_string())?;
    let id = out["id"].as_str().unwrap().to_string();
    s.jobs().pump(1);
    s.poll_imports();
    let r = s.imports.report(&id).unwrap();
    match r.phase {
        ImportPhase::Complete => Ok(id),
        _ => Err(r.error.clone().unwrap_or_default()),
    }
}

#[test]
#[ignore = "network; downloads a copyrighted module to a temp folder"]
fn live_nasb_1995_heads_featured_with_live_licence_and_size_and_one_download_installs_it() {
    let root = temp("nasb");
    let mut s = session(&root);
    refresh(&mut s);
    let catalog = s.get_bibles.catalog().unwrap();
    println!(
        "catalog: {} Resources from {} repositories ({} failed: {:?})",
        catalog.listings.len(),
        catalog.repos.len(),
        catalog.failed.len(),
        catalog.failed
    );
    let nasb = catalog.featured(ResourceType::Bible)[0].clone();
    println!("featured[0]: {nasb:?}");
    assert_eq!(nasb.module, "NASB1995");
    assert_eq!(nasb.repo, "Lockman Foundation");
    assert!(nasb.licence.as_deref().unwrap().contains("Copyrighted"));
    assert!(nasb.size.is_some_and(|n| n > 1_000_000));
    let lockman = catalog.repo("Lockman Foundation").unwrap().urls.clone();
    println!("Lockman sources: {lockman:?}");
    assert!(lockman.iter().all(|u| u.contains("crosswire.org")));

    let started = std::time::Instant::now();
    let id = download(&mut s, "NASB1995").unwrap();
    let entry = s.library.get(&id).unwrap();
    let report = s.imports.report(&id).unwrap().report.clone().unwrap();
    println!(
        "{id}: {} verses, {} tagged words in {:.1}s → {}",
        report.verses,
        report.tagged_words,
        started.elapsed().as_secs_f64(),
        entry.path.display()
    );
    assert!(report.verses > 31_000);
    assert_eq!(
        s.listing_status(&nasb),
        ListingStatus::Installed {
            resource_id: id.clone()
        }
    );
    s.run("resource.open", &json!({"id": id})).unwrap();
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "network; downloads every Featured Bible (copyrighted ones too) to a temp folder"]
fn live_every_featured_bible_downloads_and_imports() {
    let root = temp("featured");
    let mut s = session(&root);
    refresh(&mut s);
    let mut failures = Vec::new();
    for f in featured(ResourceType::Bible) {
        let Some(listing) = s.get_bibles.catalog().unwrap().get(&f.module).cloned() else {
            failures.push(format!("{}: not offered by any repository", f.module));
            continue;
        };
        let started = std::time::Instant::now();
        match download(&mut s, &f.module) {
            Ok(id) => {
                let verses = s
                    .imports
                    .report(&id)
                    .unwrap()
                    .report
                    .as_ref()
                    .unwrap()
                    .verses;
                println!(
                    "ok   {:<14} from {:<20} {:>6} verses {:>5.1}s  {}",
                    f.module,
                    listing.repo,
                    verses,
                    started.elapsed().as_secs_f64(),
                    listing.licence.as_deref().unwrap_or("-")
                );
            }
            Err(e) => {
                println!("FAIL {:<14} {e}", f.module);
                failures.push(format!("{}: {e}", f.module));
            }
        }
    }
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
#[ignore = "network"]
fn live_all_bibles_lists_every_repository_and_filters_by_language() {
    let root = temp("all");
    let mut s = session(&root);
    refresh(&mut s);
    let catalog = s.get_bibles.catalog().unwrap();
    let bibles = Some(ResourceType::Bible);
    let english = catalog.all("", Some("en"), bibles).len();
    let langs = catalog.languages(bibles);
    println!(
        "{} Bibles, {} languages, {english} English; repos: {:?}",
        catalog.all("", None, bibles).len(),
        langs.len(),
        catalog.repos.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
    assert!(catalog.all("", None, bibles).len() > 1000);
    assert!(english > 50);
    assert!(!catalog.all("king james", None, bibles).is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "network; downloads every Featured Lexicon, Commentary and Dictionary to a temp folder"]
fn live_every_featured_lexicon_commentary_and_dictionary_downloads_and_imports() {
    let root = temp("featured-resources");
    let mut s = session(&root);
    refresh(&mut s);
    let catalog = s.get_bibles.catalog().unwrap();
    for ty in [
        ResourceType::Lexicon,
        ResourceType::Commentary,
        ResourceType::Dictionary,
    ] {
        let all = catalog.all("", None, Some(ty));
        let refused = all.iter().filter(|l| l.refused.is_some()).count();
        println!("{ty:?}: {} listed, {refused} not importable yet", all.len());
    }
    let mut failures = Vec::new();
    for ty in [
        ResourceType::Lexicon,
        ResourceType::Commentary,
        ResourceType::Dictionary,
    ] {
        for f in featured(ty) {
            let Some(listing) = s.get_bibles.catalog().unwrap().get(&f.module).cloned() else {
                failures.push(format!("{}: not offered by any repository", f.module));
                continue;
            };
            if listing.kind != ty || listing.repo != f.repo || listing.refused.is_some() {
                failures.push(format!("{}: {listing:?}", f.module));
                continue;
            }
            let started = std::time::Instant::now();
            match download(&mut s, &f.module) {
                Ok(id) => {
                    let r = s.imports.report(&id).unwrap().report.clone().unwrap();
                    println!(
                        "ok   {:<20} {:<10} {:>6} entries {:>5.1}s  {}",
                        f.module,
                        format!("{ty:?}"),
                        r.entries,
                        started.elapsed().as_secs_f64(),
                        listing.licence.as_deref().unwrap_or("-")
                    );
                }
                Err(e) => {
                    println!("FAIL {:<20} {e}", f.module);
                    failures.push(format!("{}: {e}", f.module));
                }
            }
        }
    }
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
#[ignore = "network"]
fn live_strongs_greek_downloads_in_one_click_and_word_study_on_g26_shows_its_entry() {
    let root = temp("strongsgreek");
    let mut s = session(&root);
    refresh(&mut s);
    let id = download(&mut s, "StrongsGreek").unwrap();
    let g26 = obs_model::Key::StrongsNumber("G26".parse().unwrap());
    let entries = loop {
        match s.guide_section(&g26, GuideSection::LexiconEntries) {
            SectionStatus::Ready(d) => match &*d {
                SectionData::LexiconEntries(e) => break e.clone(),
                other => panic!("{other:?}"),
            },
            SectionStatus::Failed(e) => panic!("{e}"),
            _ => {
                s.jobs().pump(4);
                s.poll_imports();
            }
        }
    };
    println!("{entries:#?}");
    assert!(
        entries
            .iter()
            .any(|e| e.resource_id == id && e.text.contains("love"))
    );
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// #133: every Featured Hebrew & Greek Bible is offered by CrossWire, downloads and imports;
/// the tagged ones import Tagged words whose Morphology codes the expander reads, and Word
/// Study › Forms fills in from them.
#[test]
#[ignore = "network; downloads every Featured Hebrew & Greek Bible to a temp folder"]
fn live_every_featured_hebrew_and_greek_bible_downloads_and_imports_with_its_tags() {
    let root = temp("hebrew-greek");
    let mut s = session(&root);
    refresh(&mut s);
    let mut failures = Vec::new();
    for f in featured_hebrew_greek() {
        let Some(listing) = s.get_bibles.catalog().unwrap().get(&f.module).cloned() else {
            failures.push(format!("{}: not offered by any repository", f.module));
            continue;
        };
        if listing.repo != f.repo || listing.kind != ResourceType::Bible {
            failures.push(format!("{}: {listing:?}", f.module));
            continue;
        }
        let started = std::time::Instant::now();
        let id = match download(&mut s, &f.module) {
            Ok(id) => id,
            Err(e) => {
                println!("FAIL {:<9} {e}", f.module);
                failures.push(format!("{}: {e}", f.module));
                continue;
            }
        };
        let report = s.imports.report(&id).unwrap().report.clone().unwrap();
        // Read every Tagged word's Morphology code back from the imported Resource.
        let path = s.library.get(&id).unwrap().path.clone();
        let reader = obs_store::ObrReader::open(&path).unwrap();
        let mut codes = std::collections::BTreeSet::new();
        obs_store::ResourceStore::for_each_tagged_word(&reader, &mut |parts| {
            codes.extend(parts.iter().filter_map(|p| p.morph.clone()));
        })
        .unwrap();
        let unread: Vec<&String> = codes
            .iter()
            .filter(|c| obs_model::morph::parse(c).is_err())
            .collect();
        println!(
            "ok   {:<9} {:<4} tags {:<24} {:>6} verses {:>7} tagged words {:>5} codes \
             ({} unread) {:>2} warnings {:>5.1}s  {}",
            f.module,
            listing.language,
            format!("{:?}", listing.tags()),
            report.verses,
            report.tagged_words,
            codes.len(),
            unread.len(),
            report.warnings.len(),
            started.elapsed().as_secs_f64(),
            listing.licence.as_deref().unwrap_or("-")
        );
        if listing.morphology && (report.tagged_words == 0 || !unread.is_empty()) {
            failures.push(format!(
                "{}: {} tagged words, unread codes {:?}",
                f.module,
                report.tagged_words,
                unread.iter().take(10).collect::<Vec<_>>()
            ));
        }
    }
    for number in ["G26", "H1254"] {
        let key = obs_model::Key::StrongsNumber(number.parse().unwrap());
        let forms = loop {
            match s.guide_section(&key, GuideSection::Forms) {
                SectionStatus::Ready(d) => match &*d {
                    SectionData::Forms(f) => break f.clone(),
                    other => panic!("{other:?}"),
                },
                SectionStatus::Failed(e) => panic!("{e}"),
                _ => {
                    s.jobs().pump(4);
                    s.poll_imports();
                }
            }
        };
        println!(
            "Forms of {number} in {}: {:?}",
            forms.bible_abbreviation,
            forms
                .forms
                .iter()
                .map(|f| (&f.label, f.count))
                .collect::<Vec<_>>()
        );
        if forms.forms.is_empty() {
            failures.push(format!("Forms of {number} empty"));
        }
    }
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// #134: every Open catalogue entry downloads from its publisher through the app's HTTPS
/// transport, passes its checks (pinned sha256, or the licence notice inside the zip), and
/// imports with its licence and Attribution. Public-domain and CC BY-SA texts only.
#[test]
#[ignore = "network; downloads every Open catalogue entry to a temp folder"]
fn live_every_open_catalogue_entry_downloads_checks_and_imports() {
    let root = temp("open-catalogue");
    let mut s = session(&root);
    let sources = s.get_bibles.open_sources().to_vec();
    assert!(!sources.is_empty());
    let mut failed = Vec::new();
    for source in &sources {
        let out = s
            .run("getbibles.download_open", &json!({"id": source.id}))
            .unwrap();
        let id = out["id"].as_str().unwrap().to_string();
        s.jobs().pump(1);
        s.poll_imports();
        let r = s.imports.report(&id).unwrap().clone();
        if r.phase != ImportPhase::Complete {
            failed.push(format!("{}: {:?}", source.id, r.error));
            continue;
        }
        let entry = s.library.get(&id).unwrap();
        let report = r.report.as_ref().unwrap();
        let sha = r.open_origin.as_ref().unwrap().sha256.clone().unwrap();
        println!(
            "{}: {} ({}) {} books, {} bytes .obr, sha256 {sha}, licence {}",
            source.id,
            entry.manifest.title,
            entry.manifest.abbreviation,
            report.books.len(),
            std::fs::metadata(&entry.path).map(|m| m.len()).unwrap_or(0),
            entry.manifest.license
        );
        assert_eq!(entry.manifest.license, source.spdx);
        assert_eq!(entry.manifest.attribution, source.attribution);
        assert!(!report.books.is_empty());
    }
    assert!(failed.is_empty(), "{failed:#?}");
    assert_eq!(s.open_resource_credits().len(), sources.len());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}

/// #134: the licence classes of every live listing, and "Free to share" over the live catalog
/// keeps only Free to share rows. Prints the count of each class and of each unclassified
/// `DistributionLicense`, for the research note.
#[test]
#[ignore = "network"]
fn live_free_to_share_over_the_live_catalog_keeps_only_free_rows() {
    use obs_engine::get_bibles::ResourceFilter;
    use obs_engine::{LicenceClass, LicenceFilter};
    let root = temp("licence-classes");
    let mut s = session(&root);
    refresh(&mut s);
    let gb = &s.get_bibles;
    let catalog = gb.catalog().unwrap();
    let mut counts = std::collections::BTreeMap::new();
    let mut unclear = std::collections::BTreeMap::new();
    for b in &catalog.listings {
        *counts
            .entry(format!("{:?}", b.licence_class()))
            .or_insert(0) += 1;
        if b.licence_class() == LicenceClass::Unclear {
            *unclear
                .entry(b.licence.clone().unwrap_or_default())
                .or_insert(0) += 1;
        }
    }
    println!("{} listings: {counts:?}", catalog.listings.len());
    println!("Unclear values: {unclear:#?}");
    let free = gb.all_listings("", None, ResourceFilter::All, LicenceFilter::FreeToShare);
    assert!(!free.is_empty());
    assert!(
        free.iter()
            .all(|b| b.licence_class() == LicenceClass::FreeToShare)
    );
    println!("Free to share: {} rows", free.len());
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}
