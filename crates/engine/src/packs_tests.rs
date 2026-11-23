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
