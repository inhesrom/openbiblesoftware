//! The Atlas (#150): the places a Passage names with their Place locations and confidence,
//! the search box, the Factbook's Map section, the Tool's Tab, and an ignored check against
//! the real Starter library.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use obs_model::{
    Entity, EntityKind, EntityRelation, Key, Manifest, PlaceLocation, Relation, Resource,
    ResourceBuilder, ResourceType,
};
use obs_scripture::{Book, Passage, Standard, Verse};
use serde_json::json;

use crate::atlas::{self, Confidence, MapPlace};
use crate::testing::{openbible_geo, tipnr_acts_13};
use crate::*;

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-atlas-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("library")).unwrap();
        Self(root)
    }
    fn lib(&self) -> PathBuf {
        self.0.join("library")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn session(t: &Temp, resources: &[Resource]) -> Session {
    for r in resources {
        let path = t.lib().join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
    }
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&t.lib()).is_empty());
    s.set_alias_cache_dir(t.0.join("cache"));
    s
}

/// TIPNR's Acts 13 places, then the geocoding matched to them: the Starter library's pair.
fn starter_pair() -> Vec<Resource> {
    let tipnr = tipnr_acts_13();
    let geo = openbible_geo(Some(&tipnr));
    vec![tipnr, geo]
}

fn chapter(book: Book, c: u16) -> Passage {
    Passage::chapter(&Standard, book, c).unwrap()
}

fn places(s: &Session, passage: Passage) -> Vec<MapPlace> {
    atlas::passage_places(&s.library, &s.guides.directory(), passage).unwrap()
}

fn named(places: &[MapPlace]) -> Vec<(&str, Option<&str>)> {
    places
        .iter()
        .map(|p| (p.name.as_str(), p.best().map(|b| b.location.as_str())))
        .collect()
}

/// The acceptance check: Acts 13 shows Paul's first journey, every stop located with high
/// confidence, in the order the chapter names them.
#[test]
fn acts_13_shows_the_places_of_pauls_first_journey() {
    let t = Temp::new("acts-13");
    let s = session(&t, &starter_pair());
    let acts_13 = atlas::atlas_passage(Passage::verse(Verse::new(Book::Acts, 13, 14)));
    assert_eq!(acts_13, chapter(Book::Acts, 13));
    let found = places(&s, acts_13);
    assert_eq!(
        named(&found),
        [
            ("Antioch", Some("Antioch on the Orontes")),
            ("Cyrene", Some("Cyrene")),
            ("Cyprus", Some("Cyprus")),
            ("Seleucia", Some("Seleucia Pieria")),
            ("Salamis", Some("Salamis")),
            ("Paphos", Some("New Paphos")),
            ("Jerusalem", Some("Jerusalem")),
            ("Pamphylia", Some("Pamphylia")),
            ("Perga", Some("Perga")),
            ("Antioch", Some("Antioch in Pisidia")),
            ("Pisidia", Some("Antioch in Pisidia")),
            ("Iconium", Some("Konya")),
        ]
    );
    // Each is TIPNR's place (its Factbook), with the Verses of Acts 13 that name it.
    let pisidian = found
        .iter()
        .find(|p| p.entity == "Antioch@Act.13.14-2Ti")
        .unwrap();
    assert_eq!(pisidian.verses, [Verse::new(Book::Acts, 13, 14)]);
    let best = pisidian.best().unwrap();
    assert_eq!(best.confidence, Confidence::High);
    assert!((best.lon() - 31.189167).abs() < 1e-9 && (best.lat() - 38.306111).abs() < 1e-9);
    assert_eq!(
        pisidian.describe(),
        "Antioch · Antioch in Pisidia · High confidence"
    );
    // Every stop of the journey (Acts 13:1-51) is on the map, with high confidence.
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
        let p = found.iter().find(|p| p.entity == id).unwrap();
        assert_eq!(p.best().unwrap().confidence, Confidence::High, "{id}");
    }
    // Pisidia is drawn at Pisidian Antioch: one marker there, the city's.
    let drawn = atlas::first_at_each_point(&found);
    assert_eq!(drawn.len(), found.len() - 1);
    assert!(!drawn.iter().any(|p| p.name == "Pisidia"));
    // The map opens around every place: Cyrene (Libya) to Iconium.
    let b = atlas::bounds_of(&found, 3.0).unwrap();
    for p in &found {
        let best = p.best().unwrap();
        assert!(b.contains(best.lon(), best.lat()), "{}", p.name);
    }
    assert!(b.west < 22.0 && b.north > 37.8, "{b:?}");
    assert!(crate::base_map::builtin().bounds.contains(b.west, b.south));
}

#[test]
fn one_verse_widens_to_its_chapter_and_a_range_stays_as_typed() {
    let v = |c, n| Verse::new(Book::Acts, c, n);
    assert_eq!(
        atlas::atlas_passage(Passage::verse(v(13, 4))),
        chapter(Book::Acts, 13)
    );
    let range = Passage::new(v(13, 4), v(13, 12)).unwrap();
    assert_eq!(atlas::atlas_passage(range), range);
    let t = Temp::new("range");
    let s = session(&t, &starter_pair());
    let found = places(&s, range);
    let cyprus: Vec<&str> = found.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(cyprus, ["Cyprus", "Seleucia", "Salamis", "Paphos"]);
}

#[test]
fn a_place_no_index_locates_is_listed_without_a_point() {
    let t = Temp::new("unlocated");
    let s = session(&t, &starter_pair());
    // Zarephath's coordinates come from OpenStreetMap, which is not shipped.
    let found = places(&s, chapter(Book::FirstKings, 17));
    assert_eq!(named(&found), [("Zarephath", None)]);
    assert_eq!(found[0].describe(), "Zarephath · location unknown");
    assert!(atlas::bounds_of(&found, 3.0).is_none());
}

#[test]
fn with_tipnr_alone_places_are_listed_but_not_located() {
    let t = Temp::new("tipnr-only");
    let s = session(&t, &[tipnr_acts_13()]);
    let found = places(&s, chapter(Book::Acts, 13));
    assert_eq!(found.len(), 11);
    assert!(found.iter().all(|p| p.points.is_empty()));
}

#[test]
fn confidence_levels_follow_the_scores() {
    assert_eq!(Confidence::of(1000), Confidence::High);
    assert_eq!(Confidence::of(500), Confidence::High);
    assert_eq!(Confidence::of(499), Confidence::Medium);
    assert_eq!(Confidence::of(100), Confidence::Medium);
    assert_eq!(Confidence::of(99), Confidence::Low);
    assert_eq!(Confidence::Low.label(), "Low confidence");
    // Eden's identifications are all tentative: most confident first, none high.
    let t = Temp::new("eden");
    let s = session(&t, &starter_pair());
    let eden = places(&s, chapter(Book::Genesis, 2))
        .into_iter()
        .find(|p| p.name == "Eden")
        .unwrap();
    assert!(eden.points.len() > 3);
    assert!(eden.points.windows(2).all(|w| w[0].score >= w[1].score));
    assert!(eden.points.iter().all(|p| p.confidence != Confidence::High));
}

#[test]
fn the_search_box_finds_located_places_by_name() {
    let t = Temp::new("search");
    let mut s = session(&t, &starter_pair());
    let search = |s: &mut Session, q: &str| -> Arc<Vec<MapPlace>> {
        for _ in 0..20 {
            match s.atlas_search(q) {
                WordStatus::Ready(r) => return r,
                WordStatus::Loading => {
                    s.jobs().pump(8);
                    s.poll_imports();
                }
                WordStatus::Failed(e) => panic!("{e}"),
            }
        }
        panic!("the directory never loaded");
    };
    let antioch = search(&mut s, "antioch");
    let mut at: Vec<&str> = antioch
        .iter()
        .map(|p| p.best().unwrap().location.as_str())
        .collect();
    at.sort_unstable();
    assert_eq!(at, ["Antioch in Pisidia", "Antioch on the Orontes"]);
    assert_eq!(
        search(&mut s, "Ico")
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["Iconium"]
    );
    // A name only the geocoding knows finds TIPNR's record.
    assert_eq!(
        search(&mut s, "Antioch of Pisidia")
            .iter()
            .map(|p| p.entity.as_str())
            .collect::<Vec<_>>(),
        ["Antioch@Act.13.14-2Ti"]
    );
    // Known, but not located: not offered on the map.
    assert!(search(&mut s, "Zarephath").is_empty());
    assert!(search(&mut s, "").is_empty());
}

#[test]
fn the_directory_keeps_tipnrs_record_and_takes_the_geocodings_locations() {
    let t = Temp::new("directory");
    let mut s = session(&t, &starter_pair());
    let order: Vec<&str> = s
        .library
        .of_type(ResourceType::PeopleAndPlaces)
        .map(|e| e.manifest.resource_id.as_str())
        .collect();
    assert_eq!(order, ["tipnr", "openbible-geo"]);
    let dir = loop {
        match s.people_directory() {
            WordStatus::Ready(d) => break d,
            WordStatus::Loading => {
                s.jobs().pump(8);
                s.poll_imports();
            }
            WordStatus::Failed(e) => panic!("{e}"),
        }
    };
    let iconium = dir.get("Iconium@Act.13.51-2Ti").unwrap();
    assert_eq!(iconium.index_id, "tipnr");
    assert_eq!(iconium.entity.locations[0].name, "Konya");
    // One Iconium, not two.
    assert_eq!(dir.choose("Iconium").choices.len(), 1);
}

/// A tiny index: Haran, where Terah lived, located; Terah himself.
fn haran_and_terah() -> Resource {
    let mut m = Manifest::new(
        "places",
        "Places",
        "Places",
        ResourceType::PeopleAndPlaces,
        "en",
    );
    m.key_schemes = vec![obs_model::KeyScheme::Passage];
    let mut b = ResourceBuilder::new(m);
    let entity = |id: &str, kind| Entity {
        id: id.into(),
        name: id.split('@').next().unwrap().into(),
        kind,
        strongs: None,
        other_strongs: Vec::new(),
        other_names: Vec::new(),
        region: None,
        relations: Vec::new(),
        locations: Vec::new(),
    };
    let terah = entity("Terah@Gen.11.24-Luk", EntityKind::Male);
    let mut haran = entity("Haran@Gen.11.31-Act", EntityKind::Place);
    haran.relations.push(EntityRelation {
        relation: Relation::Inhabitant,
        entity: terah.id.clone(),
        uncertain: false,
    });
    haran.locations = vec![
        PlaceLocation {
            name: "Harran".into(),
            lon_e6: 39_031_000,
            lat_e6: 36_865_000,
            score: 900,
        },
        PlaceLocation {
            name: "Somewhere else".into(),
            lon_e6: 40_000_000,
            lat_e6: 36_000_000,
            score: 40,
        },
    ];
    let gen_11_31 = Passage::verse(Verse::new(Book::Genesis, 11, 31));
    b.add_mention(&terah.id, gen_11_31);
    b.add_mention(&haran.id, gen_11_31);
    b.add_entity(terah);
    b.add_entity(haran);
    b.build().unwrap()
}

fn map_section(s: &mut Session, id: &str) -> Arc<SectionData> {
    let key = Key::Headword(id.into());
    for _ in 0..50 {
        match s.guide_section(&key, GuideSection::Map) {
            SectionStatus::Loading => {
                s.jobs().pump(8);
                s.poll_imports();
            }
            SectionStatus::Ready(d) => return d,
            SectionStatus::Failed(e) => panic!("Map failed: {e}"),
        }
    }
    panic!("the Map never finished");
}

#[test]
fn the_factbook_map_shows_a_places_locations_and_where_a_person_lived() {
    assert!(Guide::Factbook.sections().contains(&GuideSection::Map));
    assert_eq!(GuideSection::Map.id(), "map");
    let t = Temp::new("factbook-map");
    let mut s = session(&t, &[haran_and_terah()]);
    let SectionData::Map(haran) = &*map_section(&mut s, "Haran@Gen.11.31-Act") else {
        panic!("not a Map");
    };
    assert_eq!(haran.len(), 1);
    let points: Vec<(&str, Confidence)> = haran[0]
        .points
        .iter()
        .map(|p| (p.location.as_str(), p.confidence))
        .collect();
    assert_eq!(
        points,
        [
            ("Harran", Confidence::High),
            ("Somewhere else", Confidence::Low)
        ]
    );
    let SectionData::Map(terah) = &*map_section(&mut s, "Terah@Gen.11.24-Luk") else {
        panic!("not a Map");
    };
    assert_eq!(
        terah.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["Haran"]
    );
}

#[test]
fn an_unlocated_place_has_an_empty_map_that_says_why() {
    // TIPNR alone locates nothing: the Map asks for an index that does.
    let t = Temp::new("factbook-map-empty");
    let mut s = session(&t, &[tipnr_acts_13()]);
    let perga = Key::Headword("Perga@Act.13.13-".into());
    assert!(map_section(&mut s, "Perga@Act.13.13-").is_empty());
    let empty = s.guide_empty_state(&perga, GuideSection::Map);
    assert!(
        empty
            .message
            .contains("Add a People & places index with locations"),
        "{}",
        empty.message
    );
    drop(s);
    drop(t);
    // With the geocoding, a place it cannot locate says so.
    let t = Temp::new("factbook-map-unknown");
    let mut s = session(&t, &starter_pair());
    assert!(map_section(&mut s, "Zarephath@1Ki.17.9-Luk").is_empty());
    let zarephath = Key::Headword("Zarephath@1Ki.17.9-Luk".into());
    let empty = s.guide_empty_state(&zarephath, GuideSection::Map);
    assert!(
        empty.message.contains("no location is known"),
        "{}",
        empty.message
    );
    assert!(!map_section(&mut s, "Perga@Act.13.13-").is_empty());
}

/// #261 GUIDES-9: TIPNR records where almost no one lived, so a person's empty Map says so
/// and offers no Import that could not help; a place's says no location is known.
#[test]
fn a_persons_empty_map_says_no_home_is_recorded_and_offers_no_import() {
    let t = Temp::new("factbook-map-person");
    let mut resources = starter_pair();
    resources.push(crate::testing::johns());
    let mut s = session(&t, &resources);
    let apostle = crate::testing::APOSTLE;
    assert!(map_section(&mut s, apostle).is_empty());
    let empty = s.guide_empty_state(&Key::Headword(apostle.into()), GuideSection::Map);
    assert!(
        empty.message.starts_with("No home is recorded for them"),
        "{}",
        empty.message
    );
    assert!(!empty.missing.import);
    assert!(!empty.missing.offers_starter());
    assert_eq!(empty.missing.get_label(), None);
    assert!(
        !GuideSection::Map.counted(),
        "the Map's rows are not its places"
    );
}

#[test]
fn the_atlas_tool_opens_at_a_passage_and_follows_its_link_set() {
    let t = Temp::new("tool");
    let mut s = session(&t, &starter_pair());
    assert_eq!(
        TabKind::Tool(Tool::Atlas).default_key_schemes(),
        [obs_model::KeyScheme::Passage]
    );
    assert!(TabKind::Tool(Tool::Atlas).has_link_chip());
    let out = s
        .run("tool.atlas", &json!({ "passage": "Acts 13:14" }))
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    let shown = s.workspace.tab(tab).unwrap().shown_passage().unwrap();
    assert_eq!(atlas::atlas_passage(shown), chapter(Book::Acts, 13));
    assert_eq!(s.workspace.tab(tab).unwrap().title, "Atlas");
    // It is on the Tools menu (and so in the Tools launcher).
    let tools = MENUS.iter().find(|m| m.title == "Tools").unwrap();
    assert!(tools.items.contains(&MenuItem::Command("tool.atlas")));
}

/// Real Starter library (skips when absent): Acts 13 in the shipped TIPNR and geocoding.
#[test]
#[ignore = "reads the Starter library from the default library folder"]
fn starter_library_acts_13_atlas() {
    let Some(dir) = default_library_dir().filter(|d| d.join("openbible-geo.obr").is_file()) else {
        eprintln!("no Starter library with the geocoding; skipped");
        return;
    };
    let mut s = Session::new(JobMode::Inline);
    s.library.scan(&dir);
    let started = std::time::Instant::now();
    let found = places(&s, chapter(Book::Acts, 13));
    eprintln!("Acts 13: {} places in {:?}", found.len(), started.elapsed());
    for p in &found {
        eprintln!("  {} [{}] {:?}", p.describe(), p.entity, p.verses);
    }
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
        let p = found
            .iter()
            .find(|p| p.entity == id)
            .unwrap_or_else(|| panic!("{id}"));
        assert_eq!(
            p.best().map(|b| b.confidence),
            Some(Confidence::High),
            "{id}"
        );
    }
    // Jerusalem is drawn at Jerusalem, not at its City of David (both score 1000).
    let jerusalem = found.iter().find(|p| p.name == "Jerusalem").unwrap();
    assert_eq!(jerusalem.best().unwrap().location, "Jerusalem");
    let WordStatus::Ready(search) = s.atlas_search("Jerusalem") else {
        panic!("the directory is loaded by now");
    };
    assert!(!search.is_empty());
}
