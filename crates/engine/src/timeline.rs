//! The Timeline (#185): biblical history on a zoomable horizontal axis.
//!
//! - **Data** comes from the library's People & places indexes that carry an Events dataset
//!   (ADR 0011): Theographic's in the Starter library. Each record is an Era (a band such as
//!   the Patriarchs or the Exile), a Lifespan (one person's life) or an Event (a day, a year or
//!   years, such as a reign). Dates are as the index gives them; the Timeline shows how sure
//!   they are: "c. 2242 BC" for a date the source marks approximate, ranges for what lasts
//!   ("1055–1015 BC"), and days only where the source fixes them.
//! - **The Link set.** The Tab follows its Link set's Passage like the Atlas. Events whose
//!   Passages overlap it are emphasised ([`Timeline::emphasis`]), the one the Passage tells most
//!   of is the focus the view centres on, and the Lifespans of the people they name are
//!   emphasised with them. A Verse no Event tells widens to its chapter.
//! - **Clicks.** An Event opens its Passages; a Lifespan opens its person's Factbook.
//!
//! Everything here is plain data; the UI only draws it ([`TimelineView`] maps years to points).

use std::collections::BTreeSet;

use obs_model::{Event, EventEntity, EventKind, HistoricalDate};
use obs_scripture::{Passage, Standard};

use crate::Library;

/// People & places indexes in the Starter library that carry an Events dataset, by resource id:
/// the Timeline's empty state offers them.
pub const EVENT_INDEXES: &[&str] = &["theographic"];

/// One Era, Lifespan or Event, ready to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEntry {
    /// The index's id for it (`event:162`), unique within `index_id`.
    pub id: String,
    /// The People & places index it comes from.
    pub index_id: String,
    pub title: String,
    pub kind: EventKind,
    /// Where it starts and ends on the axis, in years ([`position`]); equal for an event on one
    /// day or in one year.
    pub from: f64,
    pub to: f64,
    /// `1055–1015 BC`, `c. 2242 BC`, `4 April AD 30`.
    pub dates: String,
    pub approximate: bool,
    /// The id of the Event or Lifespan it is part of.
    pub part_of: Option<String>,
    pub people: Vec<EventEntity>,
    pub places: Vec<EventEntity>,
    pub passages: Vec<Passage>,
}

impl TimelineEntry {
    pub fn from_event(index_id: &str, e: &Event) -> Self {
        let from = position(e.start);
        // An Era ends where the next begins, so the two share one point and a date on it
        // belongs to the later Era alone.
        let end_at = |end| match e.kind {
            EventKind::Era => position(end),
            _ => position_end(end),
        };
        let to = e.end.map_or(from, |end| end_at(end).max(from));
        TimelineEntry {
            id: e.id.clone(),
            index_id: index_id.to_string(),
            title: e.title.clone(),
            kind: e.kind,
            from,
            to,
            dates: date_label(e.start, e.end, e.approximate),
            approximate: e.approximate,
            part_of: e.part_of.clone(),
            people: e.people.clone(),
            places: e.places.clone(),
            passages: e.passages.clone(),
        }
    }

    /// Whether it lasts (is drawn as a bar), rather than happening on one day or in one year.
    pub fn is_span(&self) -> bool {
        self.to > self.from
    }

    /// How many Verses of `passage` its Passages tell.
    pub fn verses_in(&self, passage: Passage) -> usize {
        self.passages
            .iter()
            .filter(|p| p.overlaps(passage))
            .map(|p| {
                let start = p.start().max(passage.start());
                let end = p.end().min(passage.end());
                Passage::new(start, end).map_or(0, |q| q.verses(&Standard).count())
            })
            .sum()
    }

    /// How many Verses its Passages hold.
    pub fn verse_count(&self) -> usize {
        self.passages
            .iter()
            .map(|p| p.verses(&Standard).count())
            .sum()
    }

    /// The person a Lifespan is the life of.
    pub fn person(&self) -> Option<&EventEntity> {
        (self.kind == EventKind::Lifespan)
            .then(|| self.people.first())
            .flatten()
    }
}

/// The Timeline's data: every Era, Lifespan and Event of the library's indexes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Timeline {
    /// Eras first, then Lifespans, then Events, each by start, then end, then title.
    pub entries: Vec<TimelineEntry>,
    /// The abbreviations of the indexes the data comes from, in Preferred order.
    pub sources: Vec<String>,
}

/// What a Passage brings out on the Timeline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Emphasis {
    /// Indexes into [`Timeline::entries`] of the Events whose Passages overlap the Passage,
    /// the Lifespans of the people they name, and the Era the focus is in.
    pub entries: BTreeSet<usize>,
    /// The Event the Passage tells most of (the largest share of its Verses), which the view
    /// centres on.
    pub focus: Option<usize>,
    /// The chapter, when the Passage was one Verse that no Event tells.
    pub widened: Option<Passage>,
}

impl Timeline {
    pub fn new(mut entries: Vec<TimelineEntry>, sources: Vec<String>) -> Self {
        let rank = |k: EventKind| match k {
            EventKind::Era => 0,
            EventKind::Lifespan => 1,
            EventKind::Event => 2,
        };
        entries.sort_by(|a, b| {
            rank(a.kind)
                .cmp(&rank(b.kind))
                .then(a.from.total_cmp(&b.from))
                .then(a.to.total_cmp(&b.to))
                .then_with(|| a.title.cmp(&b.title))
                .then_with(|| a.index_id.cmp(&b.index_id))
                .then_with(|| a.id.cmp(&b.id))
        });
        Timeline { entries, sources }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn of_kind(&self, kind: EventKind) -> impl Iterator<Item = (usize, &TimelineEntry)> {
        self.entries
            .iter()
            .enumerate()
            .filter(move |(_, e)| e.kind == kind)
    }

    /// The entry titled `title` (tests and the UI's lookups by name).
    pub fn titled(&self, title: &str) -> Option<&TimelineEntry> {
        self.entries.iter().find(|e| e.title == title)
    }

    /// The first and last year anything on the Timeline touches.
    pub fn bounds(&self) -> Option<(f64, f64)> {
        let from = self.entries.iter().map(|e| e.from).reduce(f64::min)?;
        let to = self.entries.iter().map(|e| e.to).reduce(f64::max)?;
        Some((from, to))
    }

    /// What `passage` brings out: see [`Emphasis`].
    pub fn emphasis(&self, passage: Passage) -> Emphasis {
        let mut out = self.emphasis_exact(passage);
        if out.focus.is_none() && passage.start() == passage.end() {
            let v = passage.start();
            if let Some(chapter) = Passage::chapter(&Standard, v.book, v.chapter) {
                out = self.emphasis_exact(chapter);
                if out.focus.is_some() {
                    out.widened = Some(chapter);
                }
            }
        }
        out
    }

    fn emphasis_exact(&self, passage: Passage) -> Emphasis {
        let mut out = Emphasis::default();
        let mut best: Option<(f64, f64, usize)> = None;
        for (i, e) in self.of_kind(EventKind::Event) {
            let told = e.verses_in(passage);
            if told == 0 {
                continue;
            }
            out.entries.insert(i);
            // The largest share of its own Verses; at equal shares, the shorter.
            let share = told as f64 / e.verse_count().max(1) as f64;
            let length = e.to - e.from;
            let better = best.is_none_or(|(s, l, _)| share > s || (share == s && length < l));
            if better {
                best = Some((share, length, i));
            }
        }
        out.focus = best.map(|(_, _, i)| i);
        let named: Vec<&EventEntity> = out
            .entries
            .iter()
            .flat_map(|&i| &self.entries[i].people)
            .collect();
        for (i, life) in self.of_kind(EventKind::Lifespan) {
            let told = life.passages.iter().any(|p| p.overlaps(passage));
            let in_event = life
                .person()
                .is_some_and(|p| named.iter().any(|n| same_person(n, p)));
            if told || in_event {
                out.entries.insert(i);
            }
        }
        if let Some(f) = out.focus {
            let at = self.entries[f].from;
            if let Some((i, _)) = self
                .of_kind(EventKind::Era)
                .find(|(_, era)| era.from <= at && at < era.to)
            {
                out.entries.insert(i);
            }
        }
        out
    }

    /// The emphasised Events, by date: the list beside the axis.
    pub fn events_in(&self, emphasis: &Emphasis) -> Vec<&TimelineEntry> {
        emphasis
            .entries
            .iter()
            .map(|&i| &self.entries[i])
            .filter(|e| e.kind == EventKind::Event)
            .collect()
    }
}

/// The same person: the same index id, or with no id on either side the same name.
fn same_person(a: &EventEntity, b: &EventEntity) -> bool {
    match (&a.entity, &b.entity) {
        (Some(x), Some(y)) => x == y,
        (None, None) => a.name == b.name,
        _ => false,
    }
}

/// A date's place on the axis in years: its astronomical year, plus how far into the year its
/// month and day are.
pub fn position(d: HistoricalDate) -> f64 {
    let month = f64::from(d.month.unwrap_or(1).clamp(1, 12)) - 1.0;
    let day = f64::from(d.day.unwrap_or(1).max(1)) - 1.0;
    f64::from(d.year) + month / 12.0 + day / 365.25
}

/// Where something that lasts until `d` ends: the end of that day (the last day it runs) when
/// the source gives one, else the start of that year.
fn position_end(d: HistoricalDate) -> f64 {
    position(d) + if d.day.is_some() { 1.0 / 365.25 } else { 0.0 }
}

/// An astronomical year the usual way: `-1011` → `1012 BC`, `30` → `AD 30`.
pub fn year_label(year: i32) -> String {
    if year <= 0 {
        format!("{} BC", 1 - i64::from(year))
    } else {
        format!("AD {year}")
    }
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn month_name(m: u8) -> Option<&'static str> {
    MONTHS.get(usize::from(m).checked_sub(1)?).copied()
}

/// `4 April`, `April` or nothing, for a date's day and month.
fn day_month(d: HistoricalDate) -> Option<String> {
    let month = month_name(d.month?)?;
    Some(match d.day {
        Some(day) => format!("{day} {month}"),
        None => month.to_string(),
    })
}

/// A date the usual way: `1012 BC`, `4 April AD 30`.
pub fn single_date_label(d: HistoricalDate) -> String {
    match day_month(d) {
        Some(dm) => format!("{dm} {}", year_label(d.year)),
        None => year_label(d.year),
    }
}

/// How the Timeline writes when something happened: `1012 BC`, `1055–1015 BC`, `4 BC–AD 30`,
/// `31 March–7 April AD 30`, and `c. 2242 BC` when the source marks the date approximate.
pub fn date_label(start: HistoricalDate, end: Option<HistoricalDate>, approximate: bool) -> String {
    let body = match end {
        None => single_date_label(start),
        Some(end) if end == start => single_date_label(start),
        Some(end) => {
            let same_era = (start.year <= 0) == (end.year <= 0);
            match (day_month(start), day_month(end)) {
                (None, None) if same_era && start.year <= 0 => {
                    format!(
                        "{}–{} BC",
                        1 - i64::from(start.year),
                        1 - i64::from(end.year)
                    )
                }
                (None, None) if same_era => {
                    format!("AD {}–{}", start.year, end.year)
                }
                (Some(a), Some(b)) if start.year == end.year => {
                    format!("{a}–{b} {}", year_label(end.year))
                }
                _ => format!("{}–{}", single_date_label(start), single_date_label(end)),
            }
        }
    };
    if approximate {
        format!("c. {body}")
    } else {
        body
    }
}

/// Where the Timeline looks: the year at its centre and how many points a year takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineView {
    pub center: f64,
    /// Points per year.
    pub scale: f64,
}

impl TimelineView {
    /// Closest zoom: a day is about five points, so a week's events stand apart.
    pub const MAX_SCALE: f64 = 2000.0;

    /// The view that shows `from`..`to` across `width` points, with a margin each side.
    pub fn fit(from: f64, to: f64, width: f64, margin: f64) -> Self {
        let span = (to - from).max(1.0);
        let usable = (width - 2.0 * margin).max(1.0);
        TimelineView {
            center: (from + to) / 2.0,
            scale: (usable / span).min(Self::MAX_SCALE),
        }
    }

    /// The view on `entry`, showing at least `years` around it, with the entry a third of the
    /// way across so its label, which follows it, has room.
    pub fn around(entry: &TimelineEntry, years: f64, width: f64) -> Self {
        let span = (entry.to - entry.from).max(years);
        let mid = (entry.from + entry.to) / 2.0;
        let mut v = Self::fit(mid - span / 2.0, mid + span / 2.0, width, 24.0);
        v.center = mid + (0.5 - Self::FOCUS_AT) * width / v.scale;
        v
    }

    /// Where across the axis [`TimelineView::around`] puts its entry.
    pub const FOCUS_AT: f64 = 0.35;

    /// The x of `year` on an axis `width` points wide.
    pub fn x(&self, year: f64, width: f64) -> f64 {
        width / 2.0 + (year - self.center) * self.scale
    }

    /// The year at `x`.
    pub fn year_at(&self, x: f64, width: f64) -> f64 {
        self.center + (x - width / 2.0) / self.scale
    }

    /// The years `width` points show.
    pub fn visible(&self, width: f64) -> (f64, f64) {
        (self.year_at(0.0, width), self.year_at(width, width))
    }

    /// Move by `dx` points (a drag right shows earlier years), keeping the centre within
    /// `bounds`.
    pub fn pan(&mut self, dx: f64, bounds: (f64, f64)) {
        self.center -= dx / self.scale;
        self.clamp(bounds);
    }

    /// Zoom by `factor` keeping the year under `x` where it is; never further out than shows
    /// all of `bounds` in `width`.
    pub fn zoom_about(&mut self, factor: f64, x: f64, width: f64, bounds: (f64, f64)) {
        let year = self.year_at(x, width);
        let min = Self::fit(bounds.0, bounds.1, width, 24.0).scale;
        self.scale = (self.scale * factor).clamp(min, Self::MAX_SCALE);
        self.center = year - (x - width / 2.0) / self.scale;
        self.clamp(bounds);
    }

    fn clamp(&mut self, bounds: (f64, f64)) {
        self.center = self
            .center
            .clamp(bounds.0.min(bounds.1), bounds.1.max(bounds.0));
    }

    /// Year ticks for an axis `width` points wide, about one every `spacing` points: each tick's
    /// year (astronomical) and label (`1000 BC`, `AD 50`). Steps are 1, 2, 5, 10, 20, 25, 50,
    /// 100, 200, 250, 500 or 1000 years, counted as BC and AD years are, so BC ticks fall on
    /// round BC years.
    pub fn ticks(&self, width: f64, spacing: f64) -> Vec<(f64, String)> {
        const STEPS: [i64; 12] = [1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000];
        let wanted = spacing / self.scale.max(1e-9);
        let step = STEPS
            .iter()
            .copied()
            .find(|s| *s as f64 >= wanted)
            .unwrap_or(1000);
        let (from, to) = self.visible(width);
        // The view spans at most a few thousand years.
        #[allow(clippy::cast_possible_truncation)]
        let (lo, hi) = (from.floor() as i64 - step, to.ceil() as i64 + step);
        let mut out = Vec::new();
        // BC years: astronomical t <= 0 is the year 1 - t BC.
        let mut bc = ((1 - hi.min(0)) / step).max(1) * step;
        while 1 - bc >= lo && 1 - bc <= 0 {
            let t = 1 - bc;
            if t as f64 >= from && t as f64 <= to {
                out.push((t as f64, format!("{bc} BC")));
            }
            bc += step;
        }
        let mut ad = (lo.max(1) / step).max(1) * step;
        while ad <= hi {
            if ad as f64 >= from && ad as f64 <= to {
                out.push((ad as f64, format!("AD {ad}")));
            }
            ad += step;
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }
}

/// The Timeline of `library`: every Event of every People & places index that has some, in
/// Preferred order.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_timeline(library: &Library) -> Result<Timeline, String> {
    let mut entries = Vec::new();
    let mut sources = Vec::new();
    let mut errors = Vec::new();
    for index in library.of_type(obs_model::ResourceType::PeopleAndPlaces) {
        let events = obs_store::ObrReader::open(&index.path).and_then(|r| r.events());
        match events {
            Ok(events) if events.is_empty() => {}
            Ok(events) => {
                sources.push(index.manifest.abbreviation.clone());
                entries.extend(
                    events
                        .iter()
                        .map(|e| TimelineEntry::from_event(&index.manifest.resource_id, e)),
                );
            }
            Err(e) => errors.push(format!("{}: {e}", index.manifest.abbreviation)),
        }
    }
    if entries.is_empty() && !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(Timeline::new(entries, sources))
}

#[cfg(target_arch = "wasm32")]
pub fn read_timeline(_: &Library) -> Result<Timeline, String> {
    Err("Opening Resources is not supported here".into())
}

/// What the Timeline says when the library has no Events, and what would fill it: the Starter
/// library's events index ([`EVENT_INDEXES`]) when it is not in the library. The online
/// repositories offer no People & places indexes, so there is no Get Resources filter.
pub fn empty_state(
    library: &Library,
    starter: Option<&crate::starter::StarterManifest>,
) -> crate::missing_resource::EmptyState {
    use crate::missing_resource::{EmptyState, MissingResource, StarterOffer, Tagging};
    let missing: Vec<String> = match starter {
        Some(m) => m
            .resources
            .iter()
            .filter(|i| EVENT_INDEXES.contains(&i.resource_id.as_str()))
            .filter(|i| library.get(&i.resource_id).is_none())
            .map(|i| i.resource_id.clone())
            .collect(),
        None => Vec::new(),
    };
    let has_index = EVENT_INDEXES.iter().any(|id| library.get(id).is_some());
    let starter = match starter {
        _ if has_index => StarterOffer::None,
        Some(_) if missing.is_empty() => StarterOffer::None,
        Some(_) => StarterOffer::Resources(missing),
        None => StarterOffer::Unlisted,
    };
    EmptyState {
        message: "None in your library. Add a People & places index with events (the Theographic \
                  timeline in the Starter library) to see the Timeline.",
        missing: MissingResource {
            resource_type: obs_model::ResourceType::PeopleAndPlaces,
            tagging: Tagging::None,
            get_resources: None,
            starter,
            import: true,
        },
    }
}

impl crate::Session {
    /// The Timeline's empty state for this library ([`empty_state`]).
    pub fn timeline_empty_state(&self) -> crate::missing_resource::EmptyState {
        empty_state(&self.library, self.starter.manifest())
    }
}
