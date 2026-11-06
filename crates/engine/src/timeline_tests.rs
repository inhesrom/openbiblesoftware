//! The Timeline (#185): Eras, Lifespans and Events read from the library, what a Passage
//! emphasises, how dates are written, the view's fit, pan, zoom and ticks, the Tool's Tab,
//! and an ignored check against the real Starter library.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use obs_model::{EventKind, HistoricalDate, Resource};
use obs_scripture::{Book, Passage, Standard, Verse};
use serde_json::json;

use crate::testing::{theographic, tipnr_timeline};
use crate::timeline::{self, Timeline, TimelineView, date_label, position, year_label};
use crate::*;

struct Temp(PathBuf);
impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-timeline-{name}-{}", std::process::id()));
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
    s
}

/// TIPNR, then Theographic's events matched to it: the Starter library's pair.
fn starter_pair() -> Vec<Resource> {
    let tipnr = tipnr_timeline();
    let events = theographic(Some(&tipnr));
    vec![tipnr, events]
}

fn read(t: &Temp) -> Timeline {
    let s = session(t, &starter_pair());
    timeline::read_timeline(&s.library).unwrap()
}

fn chapter(book: Book, c: u16) -> Passage {
    Passage::chapter(&Standard, book, c).unwrap()
}

/// The first acceptance check: the Timeline shows the patriarchs, kingdom and exile eras, in
/// order and each where the next begins, and David's Lifespan runs from 1085 to 1015 BC, after
/// Saul's reign began and holding his own forty-year reign.
#[test]
fn the_timeline_shows_the_patriarchs_kingdom_and_exile_eras_with_davids_life() {
    let t = Temp::new("eras");
    let tl = read(&t);
    assert_eq!(tl.sources, ["Theographic"]);
    let eras: Vec<(&str, &str)> = tl
        .of_kind(EventKind::Era)
        .map(|(_, e)| (e.title.as_str(), e.dates.as_str()))
        .collect();
    assert_eq!(
        eras,
        [
            ("Primeval history", "4004–1996 BC"),
            ("Patriarchs", "1996–1635 BC"),
            ("Egypt and the Exodus", "1635–1451 BC"),
            ("Conquest and Judges", "1451–1095 BC"),
            ("United Kingdom", "1095–975 BC"),
            ("Divided Kingdom", "975–586 BC"),
            ("Exile", "586–536 BC"),
            ("Return and Restoration", "536–441 BC"),
            ("Between the Testaments", "441–4 BC"),
            ("Life of Jesus", "4 BC–AD 30"),
            ("Early Church", "AD 30–60"),
        ]
    );
    let era = |title: &str| tl.titled(title).unwrap();
    let david = tl.titled("David").unwrap();
    assert_eq!(david.kind, EventKind::Lifespan);
    assert_eq!(david.dates, "1085–1015 BC");
    assert_eq!(
        (david.from, david.to),
        (
            position(HistoricalDate::bc(1085)),
            position(HistoricalDate::bc(1015))
        )
    );
    assert_eq!(
        david.person().unwrap().entity.as_deref(),
        Some("David@Rut.4.17-Rev")
    );
    // His life is in the United Kingdom, after the Patriarchs and before the Exile.
    let kingdom = era("United Kingdom");
    assert!(kingdom.from < david.from && david.to < kingdom.to);
    assert!(era("Patriarchs").to < david.from && david.to < era("Exile").from);
    // It holds his reign and his fight with Goliath ("c.": the source marks it approximate).
    let reign = tl.titled("Reign of David").unwrap();
    assert_eq!(reign.dates, "1055–1015 BC");
    assert!(david.from < reign.from && reign.to == david.to);
    let goliath = tl.titled("David Kills Goliath").unwrap();
    assert_eq!(goliath.dates, "c. 1067 BC");
    assert!(!goliath.is_span() && david.from < goliath.from && goliath.from < reign.from);
    // Abraham, one of the patriarchs, lives inside their Era.
    let abraham = tl.titled("Abraham").unwrap();
    assert_eq!(abraham.dates, "1996–1821 BC");
    assert!(era("Patriarchs").from <= abraham.from && abraham.to < era("Patriarchs").to);
}

/// The second acceptance check: 1 Kings 6 emphasises the temple's construction, which is the
/// focus; Solomon's reign (which 1 Kings 6 is also part of) and his Lifespan are emphasised
/// with it, and so is the United Kingdom Era.
#[test]
fn opening_1_kings_6_emphasises_the_temples_construction() {
    let t = Temp::new("temple");
    let tl = read(&t);
    let e = tl.emphasis(chapter(Book::FirstKings, 6));
    let focus = &tl.entries[e.focus.unwrap()];
    assert_eq!(focus.title, "Construction of Solomon's Temple");
    assert_eq!(focus.dates, "1012–1005 BC");
    let titles: Vec<&str> = e
        .entries
        .iter()
        .map(|&i| tl.entries[i].title.as_str())
        .collect();
    for expected in [
        "Construction of Solomon's Temple",
        "Reign of Solomon",
        "Solomon",
        "United Kingdom",
    ] {
        assert!(titles.contains(&expected), "{expected}: {titles:?}");
    }
    assert!(!titles.contains(&"David"), "{titles:?}");
    assert!(!titles.contains(&"Reign of David"), "{titles:?}");
    // The list beside the axis: the emphasised Events by date.
    let listed: Vec<&str> = tl.events_in(&e).iter().map(|e| e.title.as_str()).collect();
    assert_eq!(
        listed,
        ["Reign of Solomon", "Construction of Solomon's Temple"]
    );
    // One Verse of it does too.
    let v = tl.emphasis(Passage::verse(Verse::new(Book::FirstKings, 6, 14)));
    assert_eq!(tl.entries[v.focus.unwrap()].title, focus.title);
    assert_eq!(v.widened, None);
}

/// A Verse no Event tells widens to its chapter; a Passage none tells emphasises nothing.
#[test]
fn a_verse_no_event_tells_widens_to_its_chapter() {
    let t = Temp::new("widen");
    let tl = read(&t);
    // 1 Kings 2:10-11 tell David's death and 2:12 on Solomon's reign; no Event tells 2:1.
    let v = tl.emphasis(Passage::verse(Verse::new(Book::FirstKings, 2, 1)));
    assert_eq!(v.widened, Some(chapter(Book::FirstKings, 2)));
    assert_eq!(tl.entries[v.focus.unwrap()].title, "Death of David");
    let none = tl.emphasis(chapter(Book::Obadiah, 1));
    assert_eq!(none, timeline::Emphasis::default());
}

#[test]
fn dates_say_how_sure_they_are() {
    assert_eq!(year_label(-1011), "1012 BC");
    assert_eq!(year_label(0), "1 BC");
    assert_eq!(year_label(30), "AD 30");
    let y = HistoricalDate::year;
    assert_eq!(date_label(y(-1011), None, false), "1012 BC");
    assert_eq!(date_label(y(-2245), None, true), "c. 2246 BC");
    assert_eq!(date_label(y(-1054), Some(y(-1014)), false), "1055–1015 BC");
    assert_eq!(date_label(y(-3), Some(y(30)), false), "4 BC–AD 30");
    assert_eq!(date_label(y(46), Some(y(49)), false), "AD 46–49");
    let day = |year, month, day| HistoricalDate {
        year,
        month: Some(month),
        day: Some(day),
    };
    assert_eq!(date_label(day(30, 4, 4), None, false), "4 April AD 30");
    assert_eq!(
        date_label(day(30, 3, 31), Some(day(30, 4, 7)), false),
        "31 March–7 April AD 30"
    );
    assert_eq!(
        date_label(day(53, 9, 1), Some(day(56, 2, 29)), false),
        "1 September AD 53–29 February AD 56"
    );
    // Positions: BC before AD, days within their year, a day-long span ends that evening.
    assert!(position(y(-1)) < position(y(0)) && position(y(0)) < position(y(1)));
    assert!(position(day(30, 4, 4)) > 30.25 && position(day(30, 4, 4)) < 30.26);
}

#[test]
fn the_view_fits_pans_zooms_and_ticks() {
    let bounds = (-4003.0, 60.0);
    let mut v = TimelineView::fit(bounds.0, bounds.1, 1000.0, 20.0);
    assert!((v.x(-4003.0, 1000.0) - 20.0).abs() < 1e-6);
    assert!((v.x(60.0, 1000.0) - 980.0).abs() < 1e-6);
    // Zooming keeps the year under the pointer where it is.
    let at = v.year_at(300.0, 1000.0);
    v.zoom_about(8.0, 300.0, 1000.0, bounds);
    assert!((v.year_at(300.0, 1000.0) - at).abs() < 1e-6);
    // Never further out than the whole Timeline, never past its ends.
    let mut out = v;
    out.zoom_about(1e-6, 500.0, 1000.0, bounds);
    assert!((out.scale - TimelineView::fit(bounds.0, bounds.1, 1000.0, 24.0).scale).abs() < 1e-9);
    v.pan(1e9, bounds);
    assert_eq!(v.center, bounds.0);
    // Around an entry: it sits a third of the way across, with years either side.
    let t = Temp::new("around");
    let tl = read(&t);
    let temple = tl.titled("Construction of Solomon's Temple").unwrap();
    let around = TimelineView::around(temple, 160.0, 600.0);
    let mid = around.x((temple.from + temple.to) / 2.0, 600.0);
    assert!((mid - 600.0 * TimelineView::FOCUS_AT).abs() < 1e-6, "{mid}");
    let (from, to) = around.visible(600.0);
    assert!(to - from >= 160.0);
    // Ticks fall on round BC and AD years.
    let whole = TimelineView::fit(bounds.0, bounds.1, 1000.0, 20.0);
    let ticks = whole.ticks(1000.0, 80.0);
    let labels: Vec<&str> = ticks.iter().map(|(_, l)| l.as_str()).collect();
    assert!(
        labels.contains(&"1000 BC") && labels.contains(&"3000 BC"),
        "{labels:?}"
    );
    let thousand = ticks.iter().find(|(_, l)| l == "1000 BC").unwrap().0;
    assert_eq!(thousand, -999.0);
    let close = TimelineView {
        center: 0.0,
        scale: 40.0,
    };
    let labels: Vec<String> = close.ticks(400.0, 80.0).into_iter().map(|t| t.1).collect();
    assert_eq!(labels, ["6 BC", "4 BC", "2 BC", "AD 2", "AD 4"]);
}

#[test]
fn the_timeline_tool_opens_at_a_passage_and_follows_its_link_set() {
    let t = Temp::new("tool");
    let mut s = session(&t, &starter_pair());
    let kind = TabKind::Tool(Tool::Timeline);
    assert_eq!(kind.default_key_schemes(), [obs_model::KeyScheme::Passage]);
    assert!(kind.has_link_chip());
    let out = s
        .run("tool.timeline", &json!({ "passage": "1 Kings 6" }))
        .unwrap();
    let tab = TabId(out["tab"].as_u64().unwrap());
    let shown = s.workspace.tab(tab).unwrap().shown_passage().unwrap();
    assert_eq!(shown, chapter(Book::FirstKings, 6));
    assert_eq!(s.workspace.tab(tab).unwrap().title, "Timeline");
    // Without a Passage it opens on the whole Timeline.
    let out = s.run("tool.timeline", &json!({ "new_tab": true })).unwrap();
    let all = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(s.workspace.tab(all).unwrap().shown_passage(), None);
    let tools = MENUS.iter().find(|m| m.title == "Tools").unwrap();
    assert!(tools.items.contains(&MenuItem::Command("tool.timeline")));
}

/// An index without Events (TIPNR alone) gives an empty Timeline, not an error.
#[test]
fn a_library_without_events_has_an_empty_timeline() {
    let t = Temp::new("empty");
    let s = session(&t, &[tipnr_timeline()]);
    let tl = timeline::read_timeline(&s.library).unwrap();
    assert!(tl.is_empty() && tl.sources.is_empty());
    assert_eq!(tl.bounds(), None);
}

/// Real Starter library (skips when absent): the eras, David and 1 Kings 6 in the shipped data.
#[test]
#[ignore = "reads the Starter library from OBS_LIBRARY or the default library folder"]
fn starter_library_timeline() {
    let dir = std::env::var_os("OBS_LIBRARY")
        .map(PathBuf::from)
        .or_else(default_library_dir)
        .filter(|d| d.join("theographic.obr").is_file());
    let Some(dir) = dir else {
        eprintln!("no Starter library with the Timeline; skipped");
        return;
    };
    let mut s = Session::new(JobMode::Inline);
    s.library.scan(&dir);
    let started = std::time::Instant::now();
    let tl = timeline::read_timeline(&s.library).unwrap();
    eprintln!(
        "{} entries from {:?} in {:?}",
        tl.entries.len(),
        tl.sources,
        started.elapsed()
    );
    for (_, era) in tl.of_kind(EventKind::Era) {
        eprintln!("  era {} · {}", era.title, era.dates);
    }
    let david = tl.titled("David").unwrap();
    eprintln!("  lifespan David · {}", david.dates);
    assert_eq!(david.dates, "1085–1015 BC");
    let e = tl.emphasis(chapter(Book::FirstKings, 6));
    for &i in &e.entries {
        let x = &tl.entries[i];
        eprintln!(
            "  1 Kings 6 · {}{} · {}",
            x.title,
            if Some(i) == e.focus { " (focus)" } else { "" },
            x.dates
        );
    }
    assert_eq!(
        tl.entries[e.focus.unwrap()].title,
        "Construction of Solomon's Temple"
    );
}
