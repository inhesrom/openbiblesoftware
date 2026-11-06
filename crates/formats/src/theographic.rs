//! Theographic Bible Metadata (`CSV/Events.csv`, `json/events.json` and `CSV/People.csv`,
//! CC BY-SA 4.0) → a People & places index that carries a Timeline's Events dataset (#185,
//! ADR 0011).
//!
//! Theographic's events follow one chronology (Floyd Nolen Jones's for the Old Testament and
//! the Gospels, Todd Atwood's for Acts) and store years in ISO 8601 astronomical numbering
//! (`-1011` is 1012 BC). `Events.csv` names each event's Verses (OSIS, KJV numbering, which is
//! the Standard versification) and its people and places by lookup id (`david_994`,
//! `jerusalem_636`). `events.json` is the same table with record ids for links; it is read only
//! for `rangeFlag`, the dataset's mark of an approximate date, which the CSV leaves out.
//! `People.csv` gives birth and death years (from Jones's Appendix F) and the Verses that name
//! each person.
//!
//! Kept:
//!
//! | source | kept as |
//! |---|---|
//! | an event's `title`, `startDate`, `duration`, `partOf`, `verses` | an [`Event`]: title, start, end (start plus duration), the event it is part of, its Passages |
//! | `rangeFlag` (`events.json`) | `approximate` |
//! | `participants`, `locations` | the event's people and places, under TIPNR's ids when matched |
//! | a person's `birthYear` and `deathYear` (both known) | a Lifespan |
//! | a `Lifetime of …` event of a person with no such years | a Lifespan |
//! | — | the Eras of [`ERAS`], dated from the events they are anchored to |
//!
//! **Matched to TIPNR.** Given TIPNR's People & places index, each person takes the id and name
//! of the TIPNR person with one of their names (`name`, `alsoCalled`) who shares the most Verses
//! with them; each place, the TIPNR place with its name that shares the most Verses with the
//! events at it (or the only TIPNR place of that name). So a click on a Lifespan or a person opens
//! the same Factbook the Passage Guide does. People and places TIPNR lacks keep their name only.
//!
//! **Never read:** `notes`, `groups`, and every column of `People.csv` other than the lookup
//! id, name, other names, gender, birth and death years and Verses. In particular `dictText`
//! (Easton's Bible Dictionary text, which Theographic took from a CCEL edition) is never read
//! or kept.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use obs_model::{
    Entity, EntityKind, Event, EventEntity, EventKind, HistoricalDate, Resource, ResourceType,
};
use obs_scripture::{Book, Passage, PassageList, Standard, Verse, Versification};

use crate::dataset::{DatasetError, DatasetOptions, start};
use crate::report::ImportReport;

/// `source_format` of a People & places index converted from Theographic's events.
pub const THEOGRAPHIC_FORMAT: &str = "theographic-events";

/// Dropped-record reasons, counted in the report.
pub const DROPPED_DATE: &str = "event: unreadable start date";
pub const DROPPED_VERSE: &str = "event Verse: unreadable or outside the Standard versification";
pub const DROPPED_LIFETIME: &str = "event: a Lifetime of a person whose years are known";

/// Where an Era starts or ends: at the start or end of a Theographic event (by title), plus
/// whole years.
#[derive(Debug, Clone, Copy)]
pub enum EraBound {
    Start(&'static str),
    End(&'static str),
    /// The event's start plus years (the seventy years of 2 Chr 36:21 from Daniel's
    /// deportation, Dan 1:1, to Cyrus's decree).
    StartPlus(&'static str, i32),
}

/// One Era: a period of the biblical story, as the Timeline's bands show it.
#[derive(Debug, Clone, Copy)]
pub struct EraRule {
    /// `era:<slug>`.
    pub id: &'static str,
    pub title: &'static str,
    pub start: EraBound,
    pub end: EraBound,
}

/// The Eras, in order, each dated from the events of the same chronology so the bands agree
/// with the events inside them. A pinned dataset that loses an anchor fails the conversion.
pub const ERAS: &[EraRule] = &[
    EraRule {
        id: "era:primeval",
        title: "Primeval history",
        start: EraBound::Start("Creation of all things"),
        end: EraBound::Start("Birth of Abraham"),
    },
    EraRule {
        id: "era:patriarchs",
        title: "Patriarchs",
        start: EraBound::Start("Birth of Abraham"),
        end: EraBound::Start("Death of Joseph"),
    },
    EraRule {
        id: "era:egypt-exodus",
        title: "Egypt and the Exodus",
        start: EraBound::Start("Death of Joseph"),
        end: EraBound::End("Wilderness Wanderings"),
    },
    EraRule {
        id: "era:conquest-judges",
        title: "Conquest and Judges",
        start: EraBound::End("Wilderness Wanderings"),
        end: EraBound::Start("Reign of Saul"),
    },
    EraRule {
        id: "era:united-kingdom",
        title: "United Kingdom",
        start: EraBound::Start("United Kingdom"),
        end: EraBound::End("United Kingdom"),
    },
    EraRule {
        id: "era:divided-kingdom",
        title: "Divided Kingdom",
        start: EraBound::Start("Northern Kingdom (Israel)"),
        end: EraBound::End("Reign of Zedekiah"),
    },
    EraRule {
        id: "era:exile",
        title: "Exile",
        start: EraBound::End("Reign of Zedekiah"),
        end: EraBound::StartPlus("Prophecies of Daniel", 70),
    },
    EraRule {
        id: "era:return",
        title: "Return and Restoration",
        start: EraBound::StartPlus("Prophecies of Daniel", 70),
        end: EraBound::End("Prophecies of Malachi"),
    },
    EraRule {
        id: "era:between-testaments",
        title: "Between the Testaments",
        start: EraBound::End("Prophecies of Malachi"),
        end: EraBound::Start("Birth of Jesus"),
    },
    EraRule {
        id: "era:life-of-jesus",
        title: "Life of Jesus",
        start: EraBound::Start("Birth of Jesus"),
        end: EraBound::Start("Resurrection and Ascension"),
    },
    EraRule {
        id: "era:early-church",
        title: "Early Church",
        start: EraBound::Start("Resurrection and Ascension"),
        end: EraBound::End("Paul's First Roman imprisonment"),
    },
];

/// One `Events.csv` row, read.
struct Row {
    event_id: u32,
    title: String,
    start: HistoricalDate,
    end: Option<HistoricalDate>,
    part_of: Vec<String>,
    verses: BTreeSet<Verse>,
    participants: Vec<String>,
    locations: Vec<String>,
}

/// One `People.csv` row, read.
struct Person {
    lookup: String,
    name: String,
    names: BTreeSet<String>,
    kind: Option<EntityKind>,
    birth: Option<i32>,
    death: Option<i32>,
    verses: BTreeSet<Verse>,
}

/// Convert Theographic's `Events.csv`, `events.json` and `People.csv`, matching people and
/// places to `tipnr` (a People & places index converted from TIPNR) when given.
pub fn import_theographic_events(
    events_csv: &str,
    events_json: &str,
    people_csv: &str,
    tipnr: Option<&Resource>,
    options: &DatasetOptions,
) -> Result<(Resource, ImportReport), DatasetError> {
    let (mut builder, mut report) =
        start(options, ResourceType::PeopleAndPlaces, THEOGRAPHIC_FORMAT)?;
    let approximate = read_range_flags(events_json)?;
    let people = read_people(people_csv)?;
    let by_lookup: HashMap<&str, &Person> = people.iter().map(|p| (p.lookup.as_str(), p)).collect();
    let matcher = tipnr.map(Tipnr::new);
    let mut rows = Vec::new();
    for (line, record) in read_table(events_csv, "Events.csv")? {
        let field = |name: &str| record.get(name).map_or("", String::as_str);
        let syntax = |message: String| DatasetError::Syntax { line, message };
        let event_id: u32 = field("eventID")
            .trim()
            .parse()
            .map_err(|_| syntax(format!("bad eventID `{}`", field("eventID"))))?;
        let title = field("title").trim().to_string();
        let Some(start) = parse_date(field("startDate")) else {
            builder.count_dropped(DROPPED_DATE);
            report.warnings.push(format!(
                "event {event_id} ({title}): unreadable startDate `{}`",
                field("startDate")
            ));
            continue;
        };
        let duration = parse_duration(field("duration"))
            .ok_or_else(|| syntax(format!("bad duration `{}`", field("duration"))))?;
        let mut verses = BTreeSet::new();
        for osis in list(field("verses")) {
            match parse_verse(&osis).filter(|v| Standard.contains(*v)) {
                Some(v) => {
                    verses.insert(v);
                }
                None => {
                    builder.count_dropped(DROPPED_VERSE);
                    report
                        .warnings
                        .push(format!("event {event_id} ({title}): dropped `{osis}`"));
                }
            }
        }
        rows.push(Row {
            event_id,
            title,
            start,
            end: end_of(start, duration),
            part_of: list(field("partOf")),
            verses,
            participants: list(field("participants")),
            locations: list(field("locations")),
        });
    }
    if rows.is_empty() {
        return Err(DatasetError::NoRecords);
    }
    let by_title: HashMap<&str, &Row> = rows.iter().map(|r| (r.title.as_str(), r)).collect();

    // People: TIPNR's id and name where matched.
    let person = |lookup: &str| -> EventEntity {
        match by_lookup.get(lookup) {
            Some(p) => match matcher.as_ref().and_then(|m| m.person(p)) {
                Some(e) => EventEntity {
                    entity: Some(e.id.clone()),
                    name: usual(e),
                },
                None => EventEntity {
                    entity: None,
                    name: p.name.clone(),
                },
            },
            None => EventEntity {
                entity: None,
                name: name_of_lookup(lookup),
            },
        }
    };

    // Lifespans: every person with both years, then each `Lifetime of …` of anyone else.
    // `alias` maps a Lifetime event's id to the Lifespan that replaces it.
    let mut events: Vec<Event> = Vec::new();
    let mut alias: HashMap<String, String> = HashMap::new();
    let mut lifespan_of: HashMap<&str, usize> = HashMap::new();
    for p in &people {
        let (Some(birth), Some(death)) = (p.birth, p.death) else {
            continue;
        };
        if death < birth {
            report.warnings.push(format!(
                "{}: death year {death} before birth year {birth}; no Lifespan",
                p.lookup
            ));
            continue;
        }
        let who = person(&p.lookup);
        lifespan_of.insert(p.lookup.as_str(), events.len());
        events.push(Event {
            id: format!("lifespan:{}", p.lookup),
            title: who.name.clone(),
            kind: EventKind::Lifespan,
            start: HistoricalDate::year(birth),
            end: Some(HistoricalDate::year(death)),
            approximate: false,
            part_of: None,
            people: vec![who],
            places: Vec::new(),
            passages: Vec::new(),
        });
    }
    for row in &rows {
        let Some(rest) = row.title.strip_prefix("Lifetime of ") else {
            continue;
        };
        let lookup = row.participants.first().map(String::as_str);
        if let Some(&i) = lookup.and_then(|l| lifespan_of.get(l)) {
            builder.count_dropped(DROPPED_LIFETIME);
            alias.insert(event_id(row.event_id), events[i].id.clone());
            events[i].passages = passages(&row.verses);
            continue;
        }
        let who: Vec<EventEntity> = lookup.map(person).into_iter().collect();
        let id = match lookup {
            Some(l) => format!("lifespan:{l}"),
            None => event_id(row.event_id),
        };
        alias.insert(event_id(row.event_id), id.clone());
        events.push(Event {
            id,
            title: who
                .first()
                .map_or_else(|| rest.trim().to_string(), |w| w.name.clone()),
            kind: EventKind::Lifespan,
            start: row.start,
            end: row.end,
            approximate: approximate.contains(&row.event_id),
            part_of: None,
            people: who,
            places: Vec::new(),
            passages: passages(&row.verses),
        });
    }

    // Places: each lookup id once, by the Verses of every event at it, so one place is the same
    // TIPNR place wherever it is named (the Wise Men's Bethlehem, whose Verses do not name it,
    // is the Bethlehem of Luke 2:4).
    let mut place_verses: BTreeMap<&str, BTreeSet<Verse>> = BTreeMap::new();
    for row in &rows {
        for lookup in &row.locations {
            place_verses
                .entry(lookup.as_str())
                .or_default()
                .extend(row.verses.iter().copied());
        }
    }
    let places_by_lookup: HashMap<&str, EventEntity> = place_verses
        .into_iter()
        .map(|(lookup, verses)| {
            let name = name_of_lookup(lookup);
            let place = match matcher.as_ref().and_then(|m| m.place(&name, &verses)) {
                Some(e) => EventEntity {
                    entity: Some(e.id.clone()),
                    name: usual(e),
                },
                None => EventEntity { entity: None, name },
            };
            (lookup, place)
        })
        .collect();

    // Events.
    let resolve = |id: String| alias.get(&id).cloned().unwrap_or(id);
    for row in rows.iter().filter(|r| !r.title.starts_with("Lifetime of ")) {
        let part_of = row
            .part_of
            .iter()
            .find_map(|t| by_title.get(t.as_str()))
            .map(|parent| resolve(event_id(parent.event_id)));
        let places = row
            .locations
            .iter()
            .filter_map(|lookup| places_by_lookup.get(lookup.as_str()).cloned())
            .collect();
        events.push(Event {
            id: event_id(row.event_id),
            title: row.title.clone(),
            kind: EventKind::Event,
            start: row.start,
            end: row.end,
            approximate: approximate.contains(&row.event_id),
            part_of,
            people: row.participants.iter().map(|l| person(l)).collect(),
            places,
            passages: passages(&row.verses),
        });
    }

    // Eras.
    for era in ERAS {
        let bound = |b: EraBound| -> Result<HistoricalDate, DatasetError> {
            let (title, years, at_end) = match b {
                EraBound::Start(t) => (t, 0, false),
                EraBound::End(t) => (t, 0, true),
                EraBound::StartPlus(t, years) => (t, years, false),
            };
            let row = by_title.get(title).ok_or_else(|| DatasetError::Syntax {
                line: 0,
                message: format!("{}: no event titled `{title}` to date it by", era.id),
            })?;
            let date = if at_end {
                row.end.unwrap_or(row.start)
            } else {
                row.start
            };
            Ok(HistoricalDate::year(date.year + years))
        };
        events.push(Event {
            id: era.id.to_string(),
            title: era.title.to_string(),
            kind: EventKind::Era,
            start: bound(era.start)?,
            end: Some(bound(era.end)?),
            approximate: false,
            part_of: None,
            people: Vec::new(),
            places: Vec::new(),
            passages: Vec::new(),
        });
    }

    for e in events {
        builder.add_event(e);
    }
    let resource = builder.build().map_err(DatasetError::Invalid)?;
    report.events = resource.events.len();
    report.dropped_markup = resource.dropped_markup.clone();
    Ok((resource, report))
}

fn event_id(n: u32) -> String {
    format!("event:{n}")
}

/// TIPNR's usual name for an entity: `Mary_Magdalene` → `Mary Magdalene`.
fn usual(e: &Entity) -> String {
    e.name.replace('_', " ")
}

/// `mount_sinai_1045` → `Mount Sinai`.
fn name_of_lookup(lookup: &str) -> String {
    let base = match lookup.rsplit_once('_') {
        Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => lookup,
    };
    base.split('_')
        .filter(|w| !w.is_empty())
        .enumerate()
        .map(|(i, w)| {
            if i > 0 && matches!(w, "of" | "the" | "and") {
                return w.to_string();
            }
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| {
                f.to_uppercase().chain(c).collect::<String>()
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A comma-separated list, trimmed, empties dropped.
fn list(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Verses as Passages, adjacent Verses joined, in Bible order.
fn passages(verses: &BTreeSet<Verse>) -> Vec<Passage> {
    let list: PassageList = verses.iter().map(|v| Passage::verse(*v)).collect();
    list.merged(&Standard).passages().to_vec()
}

/// `Gen.1.1`.
fn parse_verse(s: &str) -> Option<Verse> {
    let mut parts = s.trim().split('.');
    let (Some(book), Some(chapter), Some(verse), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    Some(Verse::new(
        Book::from_osis(book)?,
        chapter.parse().ok()?,
        verse.parse().ok()?,
    ))
}

/// `-1011` (an astronomical year), `0030` or `30`, or `0030-04-04`.
pub(crate) fn parse_date(s: &str) -> Option<HistoricalDate> {
    let s = s.trim();
    let (negative, rest) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let mut parts = rest.split('-');
    let year_digits = parts.next()?;
    if year_digits.is_empty() || !year_digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year: i32 = year_digits.parse().ok()?;
    let year = if negative { -year } else { year };
    let month = parts.next().map(str::parse::<u8>).transpose().ok()?;
    let day = parts.next().map(str::parse::<u8>).transpose().ok()?;
    if parts.next().is_some() || (month.is_some() && day.is_none()) {
        return None;
    }
    if let (Some(m), Some(d)) = (month, day)
        && (!(1..=12).contains(&m) || d == 0 || d > days_in_month(year, m))
    {
        return None;
    }
    Some(HistoricalDate { year, month, day })
}

/// How long an event lasts: `7D`, `40Y`, `3M10D`, `2.5Y`, `1W`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Duration {
    pub years: u32,
    pub months: u32,
    pub days: u32,
}

pub(crate) fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let mut out = Duration::default();
    if s.is_empty() {
        return Some(out);
    }
    let mut number = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() || c == '.' {
            number.push(c);
            continue;
        }
        let n: f64 = number.parse().ok()?;
        number.clear();
        let (whole, fraction) = (n.trunc(), n.fract());
        // Durations are small whole numbers of days to years.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (whole, part) = (whole as u32, |per: f64| (fraction * per).round() as u32);
        match c.to_ascii_uppercase() {
            'Y' => {
                out.years += whole;
                out.months += part(12.0);
            }
            'M' => {
                out.months += whole;
                out.days += part(30.0);
            }
            'W' => out.days += whole * 7 + part(7.0),
            'D' => out.days += whole,
            _ => return None,
        }
    }
    number.is_empty().then_some(out)
}

/// When an event that starts at `start` and lasts `d` ends: the last day it runs (a start with
/// a day), or the year it ends (a start with a year only, durations under a year counting as
/// that year). `None` when it is over within its first day or year.
pub(crate) fn end_of(start: HistoricalDate, d: Duration) -> Option<HistoricalDate> {
    match (start.month, start.day) {
        (Some(month), Some(day)) => {
            let total_months = i64::from(month) - 1 + i64::from(d.months);
            let year = start.year
                + i32::try_from(d.years).ok()?
                + i32::try_from(total_months / 12).ok()?;
            let month = u8::try_from(total_months % 12 + 1).ok()?;
            let day = day.min(days_in_month(year, month));
            let days = days_from_civil(year, month, day) + i64::from(d.days) - 1;
            let start_days = days_from_civil(start.year, month_of(start), start.day.unwrap_or(1));
            if days <= start_days {
                return None;
            }
            let (y, m, dd) = civil_from_days(days);
            Some(HistoricalDate {
                year: y,
                month: Some(m),
                day: Some(dd),
            })
        }
        _ => {
            let years =
                f64::from(d.years) + f64::from(d.months) / 12.0 + f64::from(d.days) / 365.25;
            // A few thousand years at most.
            #[allow(clippy::cast_possible_truncation)]
            let whole = years.round() as i32;
            (whole >= 1).then(|| HistoricalDate::year(start.year + whole))
        }
    }
}

fn month_of(d: HistoricalDate) -> u8 {
    d.month.unwrap_or(1)
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 in the proleptic Gregorian calendar (Howard Hinnant's algorithm).
fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i32, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    // Within the dataset's few thousand years, and a calendar month and day.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (y as i32, m as u8, d as u8)
}

/// The eventIDs `events.json` flags as approximate (`rangeFlag`). Nothing else is read.
fn read_range_flags(json: &str) -> Result<BTreeSet<u32>, DatasetError> {
    let records: Vec<serde_json::Value> = serde_json::from_str(json.trim_start_matches('\u{feff}'))
        .map_err(|e| DatasetError::Syntax {
            line: e.line(),
            message: format!("events.json: {e}"),
        })?;
    let mut out = BTreeSet::new();
    for r in &records {
        let fields = r.get("fields");
        let flagged = fields
            .and_then(|f| f.get("rangeFlag"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let id = fields
            .and_then(|f| f.get("eventID"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| u32::try_from(n).ok());
        if let (true, Some(id)) = (flagged, id) {
            out.insert(id);
        }
    }
    Ok(out)
}

fn read_people(csv: &str) -> Result<Vec<Person>, DatasetError> {
    let mut out = Vec::new();
    for (line, record) in read_table(csv, "People.csv")? {
        let field = |name: &str| record.get(name).map_or("", |s| s.trim());
        let year = |name: &str| -> Result<Option<i32>, DatasetError> {
            let v = field(name);
            if v.is_empty() {
                return Ok(None);
            }
            v.parse().map(Some).map_err(|_| DatasetError::Syntax {
                line,
                message: format!("{}: bad {name} `{v}`", field("personLookup")),
            })
        };
        let lookup = field("personLookup").to_string();
        if lookup.is_empty() {
            continue;
        }
        let name = match field("name") {
            "" => name_of_lookup(&lookup),
            n => n.to_string(),
        };
        let mut names: BTreeSet<String> = list(field("alsoCalled"))
            .into_iter()
            .map(|n| n.to_lowercase())
            .collect();
        names.insert(name.to_lowercase());
        out.push(Person {
            kind: match field("gender") {
                "Male" => Some(EntityKind::Male),
                "Female" => Some(EntityKind::Female),
                _ => None,
            },
            birth: year("birthYear")?,
            death: year("deathYear")?,
            verses: list(field("verses"))
                .iter()
                .filter_map(|v| parse_verse(v))
                .collect(),
            lookup,
            name,
            names,
        });
    }
    Ok(out)
}

/// One CSV record by column name, with the line it starts on.
type Record = (usize, BTreeMap<String, String>);

/// One CSV record's fields, with the line it starts on.
type RawRecord = (usize, Vec<String>);

/// A CSV table's records as column name → value, with the line each starts on. Only the
/// columns named here are ever looked up.
fn read_table(text: &str, what: &str) -> Result<Vec<Record>, DatasetError> {
    let mut rows = parse_csv(text.trim_start_matches('\u{feff}')).map_err(|(line, message)| {
        DatasetError::Syntax {
            line,
            message: format!("{what}: {message}"),
        }
    })?;
    if rows.is_empty() {
        return Err(DatasetError::NoRecords);
    }
    let (_, header) = rows.remove(0);
    Ok(rows
        .into_iter()
        .filter(|(_, r)| r.iter().any(|f| !f.is_empty()))
        .map(|(line, r)| (line, header.iter().cloned().zip(r).collect()))
        .collect())
}

/// RFC 4180 CSV: comma-separated fields, double-quoted where they hold commas, quotes or
/// line breaks (`""` is a quote). Returns each record with the line it starts on.
fn parse_csv(text: &str) -> Result<Vec<RawRecord>, (usize, String)> {
    let mut records = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut line = 1;
    let mut record_line = 1;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                _ => {
                    if c == '\n' {
                        line += 1;
                    }
                    field.push(c);
                }
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                record.push(std::mem::take(&mut field));
                records.push((record_line, std::mem::take(&mut record)));
                line += 1;
                record_line = line;
            }
            _ => field.push(c),
        }
    }
    if quoted {
        return Err((record_line, "a quoted field is not closed".into()));
    }
    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push((record_line, record));
    }
    Ok(records)
}

/// TIPNR's people and places by lower-case name, with the Verses that mention each.
struct Tipnr<'a> {
    by_name: HashMap<String, Vec<&'a Entity>>,
    verses: HashMap<&'a str, BTreeSet<Verse>>,
}

impl<'a> Tipnr<'a> {
    fn new(index: &'a Resource) -> Self {
        let mut by_name: HashMap<String, Vec<&'a Entity>> = HashMap::new();
        for e in &index.entities {
            let mut seen = BTreeSet::new();
            for n in std::iter::once(&e.name).chain(&e.other_names) {
                let n = n.replace('_', " ").to_lowercase();
                if seen.insert(n.clone()) {
                    by_name.entry(n).or_default().push(e);
                }
            }
        }
        let mut verses: HashMap<&'a str, BTreeSet<Verse>> = HashMap::new();
        for m in &index.mentions {
            verses
                .entry(m.entity.as_str())
                .or_default()
                .extend(m.passage.verses(&Standard));
        }
        Self { by_name, verses }
    }

    fn shared(&self, e: &Entity, verses: &BTreeSet<Verse>) -> usize {
        self.verses
            .get(e.id.as_str())
            .map_or(0, |v| v.intersection(verses).count())
    }

    /// The candidate called one of `names` that shares the most of `verses` (ties to the lower
    /// id, so the build is deterministic); with none sharing a Verse, the only one of that
    /// name when `unique_ok`.
    fn best(
        &self,
        names: impl IntoIterator<Item = String>,
        verses: &BTreeSet<Verse>,
        keep: impl Fn(&Entity) -> bool,
        unique_ok: bool,
    ) -> Option<&'a Entity> {
        let mut candidates: BTreeMap<&'a str, &'a Entity> = BTreeMap::new();
        for n in names {
            for e in self.by_name.get(&n).into_iter().flatten() {
                if keep(e) {
                    candidates.insert(e.id.as_str(), e);
                }
            }
        }
        let mut best: Option<(usize, &'a Entity)> = None;
        for e in candidates.values() {
            let shared = self.shared(e, verses);
            if shared > 0 && best.is_none_or(|(n, _)| shared > n) {
                best = Some((shared, e));
            }
        }
        match best {
            Some((_, e)) => Some(e),
            None if unique_ok && candidates.len() == 1 => candidates.values().next().copied(),
            None => None,
        }
    }

    /// The TIPNR person `p` is.
    fn person(&self, p: &Person) -> Option<&'a Entity> {
        self.best(
            p.names.iter().cloned(),
            &p.verses,
            |e| match p.kind {
                Some(kind) => e.kind == kind,
                None => matches!(e.kind, EntityKind::Male | EntityKind::Female),
            },
            false,
        )
    }

    /// The TIPNR place called `name` that an event told in `verses` is at.
    fn place(&self, name: &str, verses: &BTreeSet<Verse>) -> Option<&'a Entity> {
        self.best(
            [name.to_lowercase()],
            verses,
            |e| e.kind == EntityKind::Place,
            true,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_durations_read_as_the_dataset_writes_them() {
        assert_eq!(parse_date("-1011"), Some(HistoricalDate::bc(1012)));
        assert_eq!(parse_date("0030"), Some(HistoricalDate::year(30)));
        assert_eq!(parse_date("9"), Some(HistoricalDate::year(9)));
        assert_eq!(
            parse_date("0030-04-04"),
            Some(HistoricalDate {
                year: 30,
                month: Some(4),
                day: Some(4)
            })
        );
        assert_eq!(parse_date("0030-02-30"), None);
        assert_eq!(parse_date("c. 30"), None);
        assert_eq!(
            parse_duration("3M10D"),
            Some(Duration {
                years: 0,
                months: 3,
                days: 10
            })
        );
        assert_eq!(
            parse_duration("2.5Y"),
            Some(Duration {
                years: 2,
                months: 6,
                days: 0
            })
        );
        assert_eq!(parse_duration("1W").map(|d| d.days), Some(7));
        assert_eq!(parse_duration("40X"), None);
        // Reign of David: 1055 BC for forty years, to 1015 BC.
        let reign = end_of(HistoricalDate::bc(1055), parse_duration("40Y").unwrap());
        assert_eq!(reign, Some(HistoricalDate::bc(1015)));
        // A day, or months within a year, stay a point.
        assert_eq!(
            end_of(HistoricalDate::bc(1066), parse_duration("1D").unwrap()),
            None
        );
        assert_eq!(
            end_of(HistoricalDate::bc(598), parse_duration("3M10D").unwrap()),
            None
        );
        // Holy Week: eight days from 31 March AD 30 runs to 7 April.
        let holy_week = parse_date("0030-03-31").unwrap();
        assert_eq!(
            end_of(holy_week, parse_duration("8D").unwrap()),
            parse_date("0030-04-07")
        );
        assert_eq!(end_of(holy_week, parse_duration("1D").unwrap()), None);
        assert_eq!(
            end_of(
                parse_date("0053-09-01").unwrap(),
                parse_duration("2.5Y").unwrap()
            ),
            parse_date("0056-02-29")
        );
    }

    #[test]
    fn lookups_name_people_and_places() {
        assert_eq!(name_of_lookup("mount_of_olives_2101"), "Mount of Olives");
        assert_eq!(name_of_lookup("jerusalem_636"), "Jerusalem");
        assert_eq!(name_of_lookup("eden"), "Eden");
    }

    #[test]
    fn csv_fields_may_be_quoted_across_lines() {
        let rows = parse_csv("a,b,c\r\n1,\"x, y\",\"say \"\"hi\"\"\nthere\"\n2,,\n").unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].1, ["1", "x, y", "say \"hi\"\nthere"]);
        assert_eq!(rows[2], (4, vec!["2".into(), String::new(), String::new()]));
        assert!(parse_csv("a\n\"open").is_err());
    }
}
