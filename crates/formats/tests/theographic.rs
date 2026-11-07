//! Theographic Bible Metadata → a People & places index carrying a Timeline's Events dataset
//! (#185), against an excerpt of the real `Events.csv`, `events.json` and `People.csv`
//! (CC BY-SA 4.0; `People.csv`'s Easton text blanked) and of TIPNR (CC BY 4.0).
//!
//! The full sources are checked too when `OBS_TEST_THEOGRAPHIC` points at a folder holding
//! `Events.csv`, `events.json` and `People.csv`, and `OBS_TEST_TIPNR` at the TIPNR `.txt`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_formats::{
    DatasetOptions, ERAS, THEOGRAPHIC_FORMAT, import_theographic_events, import_tipnr,
};
use obs_model::{Event, EventKind, HistoricalDate, Resource, ResourceType};
use obs_scripture::{Book, Passage, Standard, Verse};

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
        source_file: "Events.csv".into(),
        tool_version: "test".into(),
        ..DatasetOptions::default()
    }
}

fn tipnr() -> Resource {
    import_tipnr(
        &fixture("tipnr/tipnr-timeline-excerpt.txt"),
        &options("tipnr"),
    )
    .unwrap()
    .0
}

fn theographic(tipnr: Option<&Resource>) -> Resource {
    let (r, report) = import_theographic_events(
        &fixture("theographic/Events-excerpt.csv"),
        &fixture("theographic/events-excerpt.json"),
        &fixture("theographic/People-excerpt.csv"),
        tipnr,
        &options("theographic"),
    )
    .unwrap();
    assert_eq!(report.events, r.events.len());
    r
}

fn event<'a>(r: &'a Resource, id: &str) -> &'a Event {
    r.events
        .iter()
        .find(|e| e.id == id)
        .unwrap_or_else(|| panic!("no event {id}"))
}

fn titled<'a>(r: &'a Resource, title: &str) -> &'a Event {
    r.events
        .iter()
        .find(|e| e.title == title)
        .unwrap_or_else(|| panic!("no event titled {title}"))
}

fn chapter(book: Book, chapter: u16) -> Passage {
    Passage::chapter(&Standard, book, chapter).unwrap()
}

/// The Eras the Timeline's bands show, dated by the events they are anchored to: the
/// Patriarchs from Abraham's birth to Joseph's death, the United and Divided Kingdom, and the
/// Exile from Jerusalem's fall (586 BC) to Cyrus's decree seventy years after Daniel was taken
/// (536 BC), in Jones's chronology.
#[test]
fn eras_are_dated_from_the_events_they_are_anchored_to() {
    let r = theographic(None);
    assert_eq!(r.manifest.resource_type, ResourceType::PeopleAndPlaces);
    assert_eq!(r.manifest.source_format, THEOGRAPHIC_FORMAT);
    let eras: Vec<&Event> = ERAS.iter().map(|rule| event(&r, rule.id)).collect();
    assert!(eras.iter().all(|e| e.kind == EventKind::Era));
    // Each Era ends where the next begins.
    for pair in eras.windows(2) {
        assert_eq!(pair[0].end, Some(pair[1].start), "{}", pair[0].title);
    }
    let span = |id: &str| {
        let e = event(&r, id);
        (e.title.as_str(), e.start, e.end.unwrap())
    };
    assert_eq!(
        span("era:patriarchs"),
        (
            "Patriarchs",
            HistoricalDate::bc(1996),
            HistoricalDate::bc(1635)
        )
    );
    assert_eq!(
        span("era:united-kingdom"),
        (
            "United Kingdom",
            HistoricalDate::bc(1095),
            HistoricalDate::bc(975)
        )
    );
    assert_eq!(
        span("era:divided-kingdom"),
        (
            "Divided Kingdom",
            HistoricalDate::bc(975),
            HistoricalDate::bc(586)
        )
    );
    assert_eq!(
        span("era:exile"),
        ("Exile", HistoricalDate::bc(586), HistoricalDate::bc(536))
    );
    // A bound keeps its anchor's day: the Resurrection, 7 April AD 30, ends the Life of Jesus
    // and begins the Early Church, so the Crucifixion (4 April) falls before it.
    let resurrection = HistoricalDate {
        year: 30,
        month: Some(4),
        day: Some(7),
    };
    assert_eq!(
        span("era:life-of-jesus"),
        ("Life of Jesus", HistoricalDate::bc(4), resurrection)
    );
    assert_eq!(span("era:early-church").1, resurrection);
    assert!(event(&r, "event:459").start < resurrection);
}

/// David's Lifespan runs from his birth in 1085 BC to his death in 1015 BC (People.csv's
/// years), takes TIPNR's id, and holds his forty-year reign, which is part of the United
/// Kingdom.
#[test]
fn davids_lifespan_spans_his_reign_and_takes_tipnrs_id() {
    let tipnr = tipnr();
    let r = theographic(Some(&tipnr));
    let david = event(&r, "lifespan:david_994");
    assert_eq!(david.kind, EventKind::Lifespan);
    assert_eq!(david.title, "David");
    assert_eq!(david.start, HistoricalDate::bc(1085));
    assert_eq!(david.end, Some(HistoricalDate::bc(1015)));
    assert_eq!(
        david.people[0].entity.as_deref(),
        Some("David@Rut.4.17-Rev")
    );
    let reign = titled(&r, "Reign of David");
    assert_eq!(reign.start, HistoricalDate::bc(1055));
    assert_eq!(reign.end, Some(HistoricalDate::bc(1015)));
    assert!(david.start < reign.start && reign.end == david.end);
    assert_eq!(reign.part_of.as_deref(), Some("event:156"));
    assert_eq!(event(&r, "event:156").title, "United Kingdom");
    assert_eq!(
        reign.people[0].entity.as_deref(),
        Some("David@Rut.4.17-Rev")
    );
    // Goliath falls within his life, and the date is marked approximate in the source.
    let goliath = titled(&r, "David Kills Goliath");
    assert!(goliath.approximate);
    assert!(goliath.start > david.start && goliath.start < reign.start);
}

/// The temple's construction is told in 1 Kings 5-7, starts in 1012 BC and lasts seven years.
#[test]
fn the_temples_construction_is_told_in_1_kings_6() {
    let r = theographic(None);
    let temple = titled(&r, "Construction of Solomon's Temple");
    assert_eq!(temple.id, "event:162");
    assert_eq!(temple.start, HistoricalDate::bc(1012));
    assert_eq!(temple.end, Some(HistoricalDate::bc(1005)));
    assert!(
        temple
            .passages
            .iter()
            .any(|p| p.overlaps(chapter(Book::FirstKings, 6)))
    );
    // Adjacent Verses are joined, across chapters: 1 Kings 5:1-7:51 is one Passage.
    assert_eq!(
        temple.passages,
        [Passage::new(
            Verse::new(Book::FirstKings, 5, 1),
            Verse::new(Book::FirstKings, 7, 51)
        )
        .unwrap()]
    );
}

/// A `Lifetime of …` event becomes the person's Lifespan; a person whose years People.csv
/// gives keeps those, and the events inside are part of it.
#[test]
fn lifetimes_become_lifespans() {
    let r = theographic(None);
    // Abraham: People.csv's years win over the Lifetime event's.
    let abraham = event(&r, "lifespan:abraham_58");
    assert_eq!(abraham.start, HistoricalDate::bc(1996));
    assert_eq!(abraham.end, Some(HistoricalDate::bc(1821)));
    assert!(r.events.iter().all(|e| e.title != "Lifetime of Abraham"));
    assert!(!abraham.passages.is_empty());
    assert_eq!(
        titled(&r, "Death of Abraham").part_of.as_deref(),
        Some("lifespan:abraham_58")
    );
    assert!(
        r.dropped_markup
            .contains_key("event: a Lifetime of a person whose years are known")
    );
    // Joshua has no years in People.csv: his Lifetime event is his Lifespan.
    let joshua = event(&r, "lifespan:joshua_1727");
    assert_eq!(joshua.kind, EventKind::Lifespan);
    assert_eq!(joshua.title, "Joshua");
    assert_eq!(joshua.start, HistoricalDate::bc(1521));
    assert_eq!(joshua.end, Some(HistoricalDate::bc(1411)));
}

/// Calendar dates keep their day; dates within days stay one day.
#[test]
fn calendar_dates_keep_their_day() {
    let r = theographic(None);
    let week = titled(&r, "Holy Week");
    assert_eq!(
        (week.start, week.end),
        (
            HistoricalDate {
                year: 30,
                month: Some(3),
                day: Some(31)
            },
            Some(HistoricalDate {
                year: 30,
                month: Some(4),
                day: Some(7)
            })
        )
    );
    let cross = titled(&r, "Crucifixion and Burial");
    assert_eq!(cross.end, None);
    assert_eq!(cross.part_of.as_deref(), Some(week.id.as_str()));
}

/// People's and places' names come from TIPNR when matched; without TIPNR, from Theographic.
#[test]
fn people_and_places_take_tipnrs_ids_when_matched() {
    let tipnr = tipnr();
    let r = theographic(Some(&tipnr));
    let fall = titled(&r, "The Fall");
    assert!(fall.approximate);
    let goliath = titled(&r, "David Kills Goliath");
    let names: Vec<(&str, Option<&str>)> = goliath
        .people
        .iter()
        .map(|p| (p.name.as_str(), p.entity.as_deref()))
        .collect();
    assert_eq!(
        names,
        [
            ("David", Some("David@Rut.4.17-Rev")),
            ("Goliath", Some("Goliath@1Sa.17.4-1Ch"))
        ]
    );
    let death = titled(&r, "Death of Abraham");
    assert_eq!(death.places[0].name, "Machpelah");
    assert_eq!(
        death.places[0].entity.as_deref(),
        Some("Machpelah@Gen.23.9-")
    );
    // Without TIPNR, Theographic's names and no ids.
    let bare = theographic(None);
    let goliath = titled(&bare, "David Kills Goliath");
    assert!(goliath.people.iter().all(|p| p.entity.is_none()));
    assert_eq!(goliath.people[0].name, "David");
    // A person People.csv lacks is named from the lookup id.
    let creation = titled(&bare, "Creation of all things");
    assert_eq!(creation.people[2].name, "Holy Spirit");
    assert_eq!(
        titled(&bare, "Death of Abraham").places[0].name,
        "Machpelah"
    );
}

#[test]
fn the_conversion_is_deterministic_and_reads_crlf() {
    let tipnr = tipnr();
    let a = theographic(Some(&tipnr));
    let crlf = import_theographic_events(
        &fixture("theographic/Events-excerpt.csv").replace('\n', "\r\n"),
        &fixture("theographic/events-excerpt.json"),
        &fixture("theographic/People-excerpt.csv").replace('\n', "\r\n"),
        Some(&tipnr),
        &options("theographic"),
    )
    .unwrap()
    .0;
    assert_eq!(a, crlf);
    assert_eq!(a.content_hash(), theographic(Some(&tipnr)).content_hash());
}

/// The full sources, when available locally.
#[test]
fn full_sources_convert_when_available() {
    let (Ok(dir), Ok(tipnr_path)) = (
        std::env::var("OBS_TEST_THEOGRAPHIC"),
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
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    let (r, report) = import_theographic_events(
        &read("Events.csv"),
        &read("events.json"),
        &read("People.csv"),
        Some(&tipnr),
        &options("theographic"),
    )
    .unwrap();
    eprintln!("{report}");
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    let count = |kind: EventKind| r.events.iter().filter(|e| e.kind == kind).count();
    let people: Vec<_> = r.events.iter().flat_map(|e| &e.people).collect();
    let places: Vec<_> = r.events.iter().flat_map(|e| &e.places).collect();
    let matched =
        |list: &[&obs_model::EventEntity]| list.iter().filter(|p| p.entity.is_some()).count();
    eprintln!(
        "{} eras, {} lifespans, {} events, {} approximate; people {}/{} matched, places {}/{} matched",
        count(EventKind::Era),
        count(EventKind::Lifespan),
        count(EventKind::Event),
        r.events.iter().filter(|e| e.approximate).count(),
        matched(&people),
        people.len(),
        matched(&places),
        places.len(),
    );
    let mut unmatched: Vec<&str> = people
        .iter()
        .chain(&places)
        .filter(|p| p.entity.is_none())
        .map(|p| p.name.as_str())
        .collect();
    unmatched.sort_unstable();
    unmatched.dedup();
    eprintln!("unmatched: {unmatched:?}");
    for e in r.events.iter().filter(|e| e.kind != EventKind::Event) {
        eprintln!(
            "{:?} {} {:?}–{:?} {:?}",
            e.kind,
            e.title,
            e.start,
            e.end,
            e.people.first().and_then(|p| p.entity.as_deref())
        );
    }
    let david = event(&r, "lifespan:david_994");
    assert_eq!(
        (david.start, david.end),
        (HistoricalDate::bc(1085), Some(HistoricalDate::bc(1015)))
    );
    assert_eq!(
        david.people[0].entity.as_deref(),
        Some("David@Rut.4.17-Rev")
    );
    let temple = titled(&r, "Construction of Solomon's Temple");
    assert!(
        temple
            .passages
            .iter()
            .any(|p| p.overlaps(chapter(Book::FirstKings, 6)))
    );
}
