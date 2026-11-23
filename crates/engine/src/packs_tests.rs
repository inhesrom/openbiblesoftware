#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;

use super::*;
use crate::open_catalogue::open_sources;

fn ids(pack: &Pack) -> BTreeSet<String> {
    pack.books(open_sources())
        .iter()
        .map(|s| s.id.clone())
        .collect()
}

/// #415: the compiled-in packs parse, are sound and are not empty; every author a pack names
/// writes at least one of its Books (a misspelt name would match none); ids are unique and
/// "All Books" is there.
#[test]
fn every_pack_is_sound_and_has_books() {
    let packs = parse_packs(PACKS_TOML).unwrap();
    assert_eq!(super::packs(), packs.as_slice());
    let problems: Vec<String> = packs.iter().flat_map(Pack::problems).collect();
    assert!(problems.is_empty(), "{problems:#?}");
    let mut seen = BTreeSet::new();
    for p in &packs {
        assert!(seen.insert(p.id.clone()), "{} twice", p.id);
        let books = p.books(open_sources());
        assert!(!books.is_empty(), "{} has no Books", p.id);
        for author in &p.authors {
            assert!(
                open_sources()
                    .iter()
                    .any(|s| s.author.eq_ignore_ascii_case(author) && p.contains(s)),
                "{}: no Book by {author:?}",
                p.id
            );
        }
        for id in &p.ids {
            assert!(
                open_sources().iter().any(|s| &s.id == id),
                "{}: no entry {id}",
                p.id
            );
        }
    }
    assert!(pack(ALL_BOOKS).is_some());
}

/// "All Books" is every Book of the Open catalogue, and only Books.
#[test]
fn all_books_is_every_book_and_no_bible() {
    let all = pack(ALL_BOOKS).unwrap().books(open_sources());
    let books = open_sources()
        .iter()
        .filter(|s| s.resource_type() == Some(obs_model::ResourceType::Book))
        .count();
    assert_eq!(all.len(), books);
    assert!(all.len() > 2500, "{}", all.len());
    assert!(!all.iter().any(|s| s.id == "ylt"));
}

/// The packs hold what their names promise.
#[test]
fn packs_hold_what_their_names_promise() {
    let confessions = ids(pack("confessions-catechisms").unwrap());
    for id in [
        "tcp-westminster-confession-1646",
        "tcp-a72693",
        "tcp-a34255",
        "tcp-n07592",
    ] {
        assert!(
            confessions.contains(id),
            "{id} not in Confessions & Catechisms"
        );
    }
    let puritans = ids(pack("puritans").unwrap());
    for id in [
        "tcp-owen-mortification-1668",
        "tcp-baxter-reformed-pastor-1656",
        "tcp-sibbes-bruised-reed-1630",
    ] {
        assert!(puritans.contains(id), "{id} not in The Puritans");
    }
    // Printed after 1720: not a Puritan pack Book, whoever wrote it.
    assert!(
        pack("puritans")
            .unwrap()
            .books(open_sources())
            .iter()
            .all(|s| s.printed_year().is_some_and(|y| (1560..=1720).contains(&y)))
    );
    let early_america = pack("early-america").unwrap().books(open_sources());
    assert!(
        early_america
            .iter()
            .all(|s| s.collection_name() == "Evans-TCP")
    );
    assert!(
        early_america
            .iter()
            .any(|s| s.id == "tcp-edwards-sinners-1741")
    );
    let nineteenth = pack("nineteenth-century").unwrap().books(open_sources());
    assert!(
        nineteenth
            .iter()
            .all(|s| s.collection_name() == "Gutenberg")
    );
    assert!(nineteenth.iter().any(|s| s.author == "C. H. Spurgeon"));
    assert!(nineteenth.iter().all(|s| s.died.is_some_and(|d| d >= 1800)));
    let reformation = pack("reformation").unwrap().books(open_sources());
    assert!(
        reformation
            .iter()
            .all(|s| s.printed_year().is_some_and(|y| (1517..=1603).contains(&y)))
    );
}

#[test]
fn counts_and_sizes_read_as_words() {
    assert_eq!(count_words(7), "7");
    assert_eq!(count_words(2818), "2,818");
    assert_eq!(count_words(1_234_567), "1,234,567");
    assert_eq!(size_words(24_000_000), "24 MB");
    assert_eq!(size_words(1_933_000_000), "1.9 GB");
    assert_eq!(size_words(400_000), "0.4 MB");
}

#[test]
fn rules_combine_inclusions_with_any_and_restrictions_with_all() {
    let mut s = open_sources()
        .iter()
        .find(|s| s.id == "tcp-owen-mortification-1668")
        .unwrap()
        .clone();
    let pack = |toml: &str| {
        parse_packs(&format!(
            "[[pack]]\nid = \"t\"\ntitle = \"T\"\ndescription = \"D\"\n{toml}"
        ))
        .unwrap()
        .remove(0)
    };
    assert!(pack("authors = [\"john owen\"]").contains(&s));
    assert!(pack("subjects = [\"christian LIFE\"]").contains(&s));
    assert!(pack("titles = [\"mortification\"]").contains(&s));
    assert!(pack("authors = [\"Nobody\"]\ntitles = [\"mortification\"]").contains(&s));
    assert!(!pack("all = true\ncollections = [\"Gutenberg\"]").contains(&s));
    assert!(pack("all = true\ncollections = [\"EEBO-TCP\"]").contains(&s));
    assert!(pack("all = true\nprinted = [1660, 1670]").contains(&s));
    assert!(!pack("all = true\nprinted = [1670, 1700]").contains(&s));
    assert!(!pack("all = true\nlanguages = [\"cy\"]").contains(&s));
    // A Bible is never in a pack.
    s.kind = "bible".into();
    assert!(!pack("all = true").contains(&s));
    // Unknown fields, collections and backward ranges are refused.
    assert!(
        parse_packs("[[pack]]\nid = \"t\"\ntitle = \"T\"\ndescription = \"D\"\nauthor = \"x\"")
            .is_err()
    );
    assert_eq!(
        pack("all = true\ncollections = [\"CCEL\"]")
            .problems()
            .len(),
        1
    );
    assert_eq!(
        pack("all = true\nprinted = [1700, 1600]").problems().len(),
        1
    );
    assert_eq!(pack("").problems().len(), 1, "no inclusion");
}

/// Print every pack's size, for the ticket and the docs.
#[test]
#[ignore = "prints the packs' sizes"]
fn print_pack_sizes() {
    for p in packs() {
        let books = p.books(open_sources());
        let bytes: u64 = books.iter().filter_map(|s| s.size).sum();
        println!(
            "{:24} {:5} Books  {:6.0} MB",
            p.id,
            books.len(),
            bytes as f64 / 1e6
        );
    }
}

// ------------------------------------------------------------------------- download (#415)

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::JobMode;
use crate::get_bibles::ListingStatus;
use crate::workspace::TabKind;

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("obs-packs-{name}-{}", std::process::id()))
}

/// The two TCP Books the formats fixtures hold, served from `file://` copies of the published
/// files (their real pins), and a third whose file was changed: its pin refuses it.
fn session(name: &str) -> (Session, PathBuf) {
    let root = temp(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("open")).unwrap();
    std::fs::create_dir_all(root.join("library")).unwrap();
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../formats/tests/assets/tcp");
    let entry = |id: &str| open_sources().iter().find(|s| s.id == id).unwrap().clone();
    let mut sources = Vec::new();
    for (id, file) in [
        ("tcp-edwards-sinners-1741", "N03833.xml"),
        ("tcp-westminster-confession-1646", "A96226.xml"),
    ] {
        let copy = root.join("open").join(file);
        std::fs::copy(assets.join(file), &copy).unwrap();
        let mut s = entry(id);
        s.url = crate::credits::file_url(&copy);
        sources.push(s);
    }
    let changed = root.join("open/changed.xml");
    let xml = std::fs::read_to_string(assets.join("N03833.xml")).unwrap();
    std::fs::write(&changed, xml.replace("Angry GOD", "Angry King")).unwrap();
    let mut tampered = entry("tcp-edwards-sinners-1741");
    tampered.id = "tcp-tampered".into();
    tampered.title = "A changed file".into();
    tampered.url = crate::credits::file_url(&changed);
    sources.push(tampered);
    let mut s = Session::new(JobMode::Inline);
    s.pack_pause = std::time::Duration::ZERO;
    s.get_bibles.set_open_catalogue(sources);
    s.library.scan(&root.join("library"));
    (s, root)
}

fn settle(s: &mut Session) {
    for _ in 0..50 {
        s.jobs().pump(16);
        let _ = s.poll_imports();
        if !s.imports.is_running() {
            break;
        }
    }
}

fn summary(s: &Session, id: &str) -> PackSummary {
    s.pack_summaries().into_iter().find(|p| p.id == id).unwrap()
}

/// #415 acceptance: Download pack brings every Book of the pack not in the Library yet in one
/// job, under one Import report; each Book is checked by its pin (a changed file is refused
/// and listed with why, and the rest go on), joins the Library as a single download would,
/// and goes in a Library Collection named for the pack. Downloading the pack again fetches
/// only what is missing.
#[test]
fn a_pack_downloads_in_one_job_each_book_checked_and_collected() {
    let (mut s, root) = session("download");
    let all = summary(&s, ALL_BOOKS);
    assert_eq!((all.books, all.installed), (3, 0));
    assert!(all.running.is_none());
    let early = summary(&s, "early-america");
    assert_eq!(early.books, 2, "Edwards and the changed copy are Evans-TCP");

    let out = s
        .run(
            "getbibles.download_pack",
            &json!({"id": ALL_BOOKS, "confirm": true}),
        )
        .unwrap();
    let report = out["report"].as_str().unwrap().to_string();
    assert!(report.starts_with(crate::books::BATCH_PREFIX), "{report}");
    settle(&mut s);

    // One report Tab for the whole pack, listing two Books in and one refused.
    let report_tabs = s
        .workspace
        .tabs()
        .filter(|t| matches!(t.kind, TabKind::ImportReport(_)))
        .count();
    assert_eq!(report_tabs, 1);
    let record = s.imports.report(&report).unwrap();
    assert_eq!(
        record.phase,
        crate::ImportPhase::Complete,
        "{:?}",
        record.error
    );
    let books = record.books.as_ref().unwrap();
    assert_eq!(books.imported.len(), 2, "{books:?}");
    assert_eq!(books.failed.len(), 1);
    assert_eq!(books.failed[0].name, "A changed file");
    assert!(
        books.failed[0].error.contains("has changed"),
        "{:?}",
        books.failed
    );

    // Each Book is in the Library as a single download leaves it.
    for id in [
        "tcp-edwards-sinners-1741",
        "tcp-westminster-confession-1646",
    ] {
        let listing = s.get_bibles.open_source(id).unwrap().listing();
        let ListingStatus::Installed { resource_id } = s.listing_status(&listing) else {
            panic!("{id} is not installed: {:?}", s.listing_status(&listing));
        };
        let entry = s.library.get(&resource_id).unwrap();
        assert_eq!(entry.manifest.resource_type, obs_model::ResourceType::Book);
        assert_eq!(entry.manifest.license, "CC0-1.0");
        let r = s.imports.report(&resource_id).unwrap();
        assert_eq!(r.open_origin.as_ref().unwrap().id, id);
        assert!(r.open_origin.as_ref().unwrap().sha256.is_some());
        // In the pack's Collection.
        assert!(s.library.in_collection("All Books", &resource_id));
    }
    let all = summary(&s, ALL_BOOKS);
    assert_eq!((all.books, all.installed), (3, 2));

    // Again: only the Book still missing is fetched (and refused again).
    let out = s
        .run(
            "getbibles.download_pack",
            &json!({"id": ALL_BOOKS, "confirm": true}),
        )
        .unwrap();
    settle(&mut s);
    let again = s.imports.report(out["report"].as_str().unwrap()).unwrap();
    let books = again.books.as_ref().unwrap();
    assert_eq!((books.imported.len(), books.failed.len()), (0, 1));

    // All Books asks first, with its size; a small pack does not.
    let (mut s2, root2) = session("confirm");
    let out = s2
        .run("getbibles.download_pack", &json!({"id": ALL_BOOKS}))
        .unwrap();
    assert_eq!(out["asked"], true);
    let asked = s2.workspace.take_effects();
    assert!(
        asked.iter().any(|e| matches!(e, crate::Effect::ConfirmPack { pack, title, message }
            if pack == ALL_BOOKS && title == "Download All Books?" && message.starts_with("3 Books"))),
        "{asked:?}"
    );
    assert!(
        s2.imports.packs.is_empty(),
        "nothing started before the answer"
    );
    let out = s2
        .run("getbibles.download_pack", &json!({"id": "early-america"}))
        .unwrap();
    assert!(out["report"].is_string(), "{out}");
    drop(s2);
    std::fs::remove_dir_all(root2).unwrap();

    // A pack with nothing left to download says so.
    let err = s
        .run(
            "getbibles.download_pack",
            &json!({"id": "confessions-catechisms"}),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("in your Library already"), "{err}");
    let err = s
        .run("getbibles.download_pack", &json!({"id": "nope"}))
        .unwrap_err()
        .to_string();
    assert!(err.contains("no Book pack"), "{err}");
    drop(s);
    std::fs::remove_dir_all(root).unwrap();
}
