//! OpenBible.info Bible Geocoding Data → a People & places index with Place locations (#150),
//! against an excerpt of the real `ancient.jsonl` / `modern.jsonl` and of TIPNR (all CC BY 4.0).
//!
//! The full sources are checked too when `OBS_TEST_OPENBIBLE_GEO` points at a folder holding
//! `ancient.jsonl` and `modern.jsonl`, and `OBS_TEST_TIPNR` at the TIPNR `.txt`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_formats::{DatasetOptions, OPENBIBLE_GEO_FORMAT, import_openbible_geocoding, import_tipnr};
use obs_model::{Entity, EntityKind, Resource, ResourceType};
use obs_scripture::{Book, Passage, Verse};

fn fixture(path: &str) -> String {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/assets")
        .join(path);
    std::fs::read_to_string(p).unwrap()
}

fn options(id: &str) -> DatasetOptions {
    DatasetOptions {
        resource_id: id.into(),
        title: id.into(),
        source_file: "ancient.jsonl".into(),
        tool_version: "test".into(),
        ..DatasetOptions::default()
    }
}

fn tipnr() -> Resource {
    import_tipnr(
        &fixture("tipnr/tipnr-acts-13-excerpt.txt"),
        &options("tipnr"),
    )
    .unwrap()
    .0
}

fn geo(tipnr: Option<&Resource>) -> Resource {
    let (r, report) = import_openbible_geocoding(
        &fixture("openbible-geo/ancient-excerpt.jsonl"),
        &fixture("openbible-geo/modern-excerpt.jsonl"),
        tipnr,
        &options("openbible-geo"),
    )
    .unwrap();
    assert_eq!(report.entities, r.entities.len());
    r
}

fn entity<'a>(r: &'a Resource, id: &str) -> &'a Entity {
    r.entities
        .iter()
        .find(|e| e.id == id)
        .unwrap_or_else(|| panic!("no entity {id}"))
}

fn mentioned_in(r: &Resource, passage: Passage) -> Vec<&str> {
    let mut ids: Vec<&str> = r
        .mentions
        .iter()
        .filter(|m| m.passage.start() >= passage.start() && m.passage.end() <= passage.end())
        .map(|m| m.entity.as_str())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[test]
fn acts_13_places_take_tipnr_ids_and_carry_their_locations() {
    let tipnr = tipnr();
    let r = geo(Some(&tipnr));
    assert_eq!(r.manifest.resource_type, ResourceType::PeopleAndPlaces);
    assert_eq!(r.manifest.source_format, OPENBIBLE_GEO_FORMAT);
    let acts_13 = Passage::new(
        Verse::new(Book::Acts, 13, 1),
        Verse::new(Book::Acts, 13, 52),
    )
    .unwrap();
    // Paul's first journey (Acts 13:1-51), with TIPNR's ids, plus Jerusalem (13:13, 27, 31),
    // which the TIPNR excerpt lacks and so keeps OpenBible's id.
    assert_eq!(
        mentioned_in(&r, acts_13),
        [
            "Antioch@Act.13.14-2Ti",
            "Antioch@Act.6.5-Gal",
            "Cyprus@Num.24.24-Act",
            "Cyrene@Mat.27.32-Act",
            "Iconium@Act.13.51-2Ti",
            "Jerusalem@OpenBible.a15257a",
            "Pamphylia@Act.2.10-",
            "Paphos@Act.13.6-",
            "Perga@Act.13.13-",
            "Pisidia@Act.13.14-",
            "Salamis@Act.13.5",
            "Seleucia@Act.13.4",
        ]
    );
    let pisidian = entity(&r, "Antioch@Act.13.14-2Ti");
    assert_eq!(pisidian.name, "Antioch");
    assert_eq!(pisidian.kind, EntityKind::Place);
    assert_eq!(pisidian.locations.len(), 1);
    let at = &pisidian.locations[0];
    assert_eq!(at.name, "Antioch in Pisidia");
    assert_eq!(
        (at.lon_e6, at.lat_e6, at.score),
        (31_189_167, 38_306_111, 1000)
    );
    assert!(
        pisidian
            .other_names
            .contains(&"Antioch of Pisidia".to_string())
    );
    let syrian = entity(&r, "Antioch@Act.6.5-Gal");
    assert_eq!(syrian.locations[0].name, "Antioch on the Orontes");
    let iconium = entity(&r, "Iconium@Act.13.51-2Ti");
    assert_eq!(iconium.locations[0].name, "Konya");
    // OpenBible's place keeps its own name when TIPNR has no record for it.
    assert_eq!(entity(&r, "Jerusalem@OpenBible.a15257a").name, "Jerusalem");
}

#[test]
fn places_tipnr_counts_as_one_merge_their_verses_and_locations() {
    let tipnr = tipnr();
    let r = geo(Some(&tipnr));
    // Acco (Judg 1:31) and Ptolemais (Acts 21:7) are two OpenBible places but one TIPNR place.
    let acco = entity(&r, "Acco@Jdg.1.31-Act");
    let names: Vec<&str> = acco.locations.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Acre", "Tel Akko"]);
    assert!(acco.other_names.contains(&"Ptolemais".to_string()));
    let verses: Vec<Passage> = r
        .mentions
        .iter()
        .filter(|m| m.entity == acco.id)
        .map(|m| m.passage)
        .collect();
    assert_eq!(
        verses,
        [
            Passage::verse(Verse::new(Book::Judges, 1, 31)),
            Passage::verse(Verse::new(Book::Acts, 21, 7)),
        ]
    );
}

#[test]
fn locations_are_ordered_by_confidence_and_doubtful_or_odbl_ones_are_left_out() {
    let r = geo(Some(&tipnr()));
    // Eden: many tentative identifications, none confident; most confident first.
    let eden = r.entities.iter().find(|e| e.name == "Eden").unwrap();
    let scores: Vec<u16> = eden.locations.iter().map(|l| l.score).collect();
    assert!(scores.len() > 3, "{scores:?}");
    assert!(scores.windows(2).all(|w| w[0] >= w[1]), "{scores:?}");
    assert!(scores[0] < 500, "{scores:?}");
    // Zarephath's only coordinates come from OpenStreetMap (ODbL): TIPNR knows the place, so
    // it is kept, with no location.
    let zarephath = entity(&r, "Zarephath@1Ki.17.9-Luk");
    assert!(zarephath.locations.is_empty());
    // Abel-keramim: identifications scored below 0 are judged wrong and dropped.
    let abel = r
        .entities
        .iter()
        .find(|e| e.name == "Abel-keramim")
        .unwrap();
    assert!(abel.locations.iter().all(|l| l.score > 0));
    // Abila is named in no Verse: not a place in the index.
    assert!(r.entities.iter().all(|e| e.name != "Abila"));
    assert!(
        r.dropped_markup
            .contains_key("location: coordinates from OpenStreetMap (ODbL)")
    );
    assert!(
        r.dropped_markup
            .contains_key("location: identification scored 0 or below")
    );
    assert!(r.dropped_markup.contains_key("place: named in no Verse"));
}

#[test]
fn the_kjv_verse_is_used_where_the_kjv_numbers_differently() {
    let r = geo(None);
    let jerusalem = r.entities.iter().find(|e| e.name == "Jerusalem").unwrap();
    let at = |c, v| {
        r.mentions.iter().any(|m| {
            m.entity == jerusalem.id && m.passage == Passage::verse(Verse::new(Book::Acts, c, v))
        })
    };
    // The ESV names Jerusalem in Acts 4:5, the KJV in 4:6.
    assert!(at(4, 6));
    assert!(!at(4, 5));
}

#[test]
fn without_tipnr_every_located_place_keeps_its_own_id() {
    let r = geo(None);
    assert!(r.entities.iter().all(|e| e.id.contains("@OpenBible.a")));
    // Zarephath has no location left and no TIPNR place to stand for: dropped.
    assert!(r.entities.iter().all(|e| e.name != "Zarephath"));
    assert!(
        r.dropped_markup
            .contains_key("place: not in TIPNR and with no location")
    );
    // Two OpenBible places called Antioch stay two.
    assert_eq!(r.entities.iter().filter(|e| e.name == "Antioch").count(), 2);
}

#[test]
fn the_conversion_is_deterministic_and_reads_crlf() {
    let tipnr = tipnr();
    let a = geo(Some(&tipnr));
    let crlf = import_openbible_geocoding(
        &fixture("openbible-geo/ancient-excerpt.jsonl").replace('\n', "\r\n"),
        &fixture("openbible-geo/modern-excerpt.jsonl").replace('\n', "\r\n"),
        Some(&tipnr),
        &options("openbible-geo"),
    )
    .unwrap()
    .0;
    assert_eq!(a, crlf);
    assert_eq!(a.content_hash(), geo(Some(&tipnr)).content_hash());
}

/// The full sources, when available locally: every place of Paul's first journey is located.
#[test]
fn full_sources_convert_when_available() {
    let (Ok(dir), Ok(tipnr_path)) = (
        std::env::var("OBS_TEST_OPENBIBLE_GEO"),
        std::env::var("OBS_TEST_TIPNR"),
    ) else {
        return;
    };
    let dir = Path::new(&dir);
    let tipnr = import_tipnr(
        &std::fs::read_to_string(tipnr_path).unwrap(),
        &options("tipnr"),
    )
    .unwrap()
    .0;
    let (r, report) = import_openbible_geocoding(
        &std::fs::read_to_string(dir.join("ancient.jsonl")).unwrap(),
        &std::fs::read_to_string(dir.join("modern.jsonl")).unwrap(),
        Some(&tipnr),
        &options("openbible-geo"),
    )
    .unwrap();
    eprintln!("{report}");
    let matched = r
        .entities
        .iter()
        .filter(|e| !e.id.contains("@OpenBible."))
        .count();
    eprintln!(
        "{} places ({matched} matched to TIPNR), {} located, {} mentions",
        r.entities.len(),
        r.entities
            .iter()
            .filter(|e| !e.locations.is_empty())
            .count(),
        r.mentions.len()
    );
    for id in [
        "Antioch@Act.6.5-Gal",
        "Seleucia@Act.13.4",
        "Cyprus@Num.24.24-Act",
        "Salamis@Act.13.5",
        "Paphos@Act.13.6-",
        "Perga@Act.13.13-",
        "Antioch@Act.13.14-2Ti",
        "Iconium@Act.13.51-2Ti",
    ] {
        assert!(!entity(&r, id).locations.is_empty(), "{id} has no location");
    }
}
