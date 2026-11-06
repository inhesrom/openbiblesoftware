//! Reading plans (#147): reading the Bible, or chosen books of it, day by day.
//!
//! A [`ReadingPlan`] is made from a [`PlanTemplate`] with a start day, a Bible and the days of
//! the week it is read on. The templates are open data files compiled in from
//! `assets/reading-plans/` (canonical, chronological and M'Cheyne Bible in a year, the New
//! Testament in 90 days, Psalms and Proverbs monthly); a custom plan reads chosen books over a
//! chosen number of days. Making a plan lays out every [`Reading`] on a day; the plan and its
//! progress live in the User database and need no account or network.
//!
//! When unread Readings fall behind today, the plan is "behind by N days" (N reading days
//! with something unread). **Catch up** moves the unread Readings on so the first of them is
//! read today, keeping their spacing (the plan ends later); **Reschedule** keeps the plan's
//! last day and spreads the unread Readings over the reading days from today to it.

use std::fmt;
use std::sync::OnceLock;

use obs_model::ResourceType;
use obs_scripture::{
    Book, Passage, PassageList, RefStyle, Standard, Versification, format_passage_list,
    parse_reference,
};
use obs_store::{PlanReadingRow, ReadingPlanRow};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::command::{CommandError, CommandSpec, ParamKind, ParamSpec, Params};
use crate::session::Session;
use crate::workspace::Effect;

// -------------------------------------------------------------------------------- days

/// A calendar day, as whole days since 1970-01-01.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlanDate(pub i64);

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

/// The days of the week, Monday first.
pub const WEEKDAY_NAMES: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

impl PlanDate {
    /// The day `YYYY-MM-DD` names, if it is a real date.
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.trim().split('-');
        let y: i64 = parts.next()?.parse().ok()?;
        let m: i64 = parts.next()?.parse().ok()?;
        let d: i64 = parts.next()?.parse().ok()?;
        if parts.next().is_some() || !(1..=12).contains(&m) || d < 1 {
            return None;
        }
        let date = Self::from_civil(y, m, d);
        (date.civil() == (y, m, d)).then_some(date)
    }

    /// Howard Hinnant's days-from-civil.
    fn from_civil(y: i64, m: i64, d: i64) -> Self {
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Self(era * 146_097 + doe - 719_468)
    }

    /// `(year, month, day)`, by Howard Hinnant's civil-from-days.
    pub fn civil(self) -> (i64, i64, i64) {
        let z = self.0 + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        (y, m, d)
    }

    /// The day `n` days later (earlier when negative).
    pub fn plus(self, n: i64) -> Self {
        Self(self.0 + n)
    }

    /// 0 for Monday … 6 for Sunday (1970-01-01 was a Thursday).
    pub fn weekday(self) -> usize {
        usize::try_from((self.0 + 3).rem_euclid(7)).unwrap_or(0)
    }

    /// "Thursday 5 November 2026".
    pub fn long_label(self) -> String {
        let (y, m, d) = self.civil();
        format!(
            "{} {d} {} {y}",
            WEEKDAY_NAMES[self.weekday()],
            month_name(m)
        )
    }

    /// "Thu 5 Nov 2026".
    pub fn label_with_year(self) -> String {
        format!("{} {}", self.short_label(), self.civil().0)
    }

    /// "Thu 5 Nov".
    pub fn short_label(self) -> String {
        let (_, m, d) = self.civil();
        format!(
            "{} {d} {}",
            &WEEKDAY_NAMES[self.weekday()][..3],
            &month_name(m)[..3]
        )
    }
}

fn month_name(m: i64) -> &'static str {
    usize::try_from(m - 1)
        .ok()
        .and_then(|i| MONTHS.get(i))
        .copied()
        .unwrap_or("?")
}

impl fmt::Display for PlanDate {
    /// `YYYY-MM-DD`, as the User database keeps days.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.civil();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

/// The days of the week a plan is read on: bit 0 Monday … bit 6 Sunday.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Weekdays(pub u8);

impl Weekdays {
    pub const ALL: Weekdays = Weekdays(0b111_1111);

    pub fn contains(self, day: PlanDate) -> bool {
        self.has(day.weekday())
    }

    /// Whether weekday `i` (0 Monday) is on.
    pub fn has(self, i: usize) -> bool {
        i < 7 && self.0 & (1 << i) != 0
    }

    pub fn with(self, i: usize, on: bool) -> Self {
        if i >= 7 {
            return self;
        }
        if on {
            Self(self.0 | (1 << i))
        } else {
            Self(self.0 & !(1 << i))
        }
    }

    pub fn is_empty(self) -> bool {
        self.0 & 0b111_1111 == 0
    }

    /// Days named by their first three letters (any case), separated by commas or spaces
    /// ("mon,wed,fri"); also "all" (every day) and "weekdays" (Monday to Friday).
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "all" | "every day" => return Some(Self::ALL),
            "weekdays" => return Some(Self(0b001_1111)),
            _ => {}
        }
        let mut out = Self(0);
        for word in s.split([',', ' ']).filter(|w| !w.is_empty()) {
            let w = word.to_ascii_lowercase();
            let i = WEEKDAY_NAMES
                .iter()
                .position(|d| w.len() >= 3 && d.to_ascii_lowercase().starts_with(&w))?;
            out = out.with(i, true);
        }
        (!out.is_empty()).then_some(out)
    }

    /// "Every day", "Monday to Friday", or the days ("Mon, Wed, Fri").
    pub fn label(self) -> String {
        match self.0 & 0b111_1111 {
            0b111_1111 => "Every day".into(),
            0b001_1111 => "Monday to Friday".into(),
            _ => (0..7)
                .filter(|&i| self.has(i))
                .map(|i| &WEEKDAY_NAMES[i][..3])
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

/// The days in `span` days from `start` that a plan with `weekdays` is read on.
fn reading_dates(start: PlanDate, span: u32, weekdays: Weekdays) -> Vec<PlanDate> {
    (0..i64::from(span))
        .map(|i| start.plus(i))
        .filter(|d| weekdays.contains(*d))
        .collect()
}

/// For each of `n` groups in order, the day it goes on: spread evenly over `dates` (several
/// to a day when there are more groups than days, days left free when fewer).
fn spread(n: usize, dates: &[PlanDate]) -> Vec<PlanDate> {
    if dates.is_empty() {
        return Vec::new();
    }
    (0..n).map(|j| dates[j * dates.len() / n]).collect()
}

// -------------------------------------------------------------------------------- templates

/// The id of a custom plan's template: chosen books over a chosen number of days.
pub const CUSTOM: &str = "custom";

const TEMPLATE_FILES: &[&str] = &[
    include_str!("../assets/reading-plans/bible-canonical.toml"),
    include_str!("../assets/reading-plans/bible-chronological.toml"),
    include_str!("../assets/reading-plans/mcheyne.toml"),
    include_str!("../assets/reading-plans/nt-90-days.toml"),
    include_str!("../assets/reading-plans/psalms-proverbs.toml"),
];

#[derive(Debug, Deserialize)]
struct TemplateFile {
    id: String,
    name: String,
    about: String,
    days: u32,
    #[serde(default)]
    tracks: Vec<Vec<String>>,
    #[serde(default)]
    readings: Vec<Vec<String>>,
}

/// What a plan reads, before it is laid out on days.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TemplateBody {
    /// Each track's Passages, in order, divided into one Reading per reading day.
    Tracks(Vec<Vec<Passage>>),
    /// Each day's Readings, as the template gives them.
    Days(Vec<Vec<PassageList>>),
}

/// A Reading plan template: what a plan reads and over how many days.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTemplate {
    pub id: String,
    pub name: String,
    /// One sentence for the template chooser.
    pub about: String,
    /// How many calendar days the plan takes from its start.
    pub days: u32,
    body: TemplateBody,
}

fn parse_standard(text: &str) -> Result<PassageList, String> {
    let list = parse_reference(text, &Standard, None).map_err(|e| format!("“{text}”: {e}"))?;
    if list.is_empty() {
        return Err(format!("“{text}” names no Passage"));
    }
    Ok(list)
}

impl PlanTemplate {
    fn from_toml(text: &str) -> Result<Self, String> {
        let f: TemplateFile = toml::from_str(text).map_err(|e| e.to_string())?;
        let body = if f.readings.is_empty() {
            let tracks = f
                .tracks
                .iter()
                .map(|t| {
                    t.iter()
                        .map(|r| parse_standard(r).map(|l| l.passages().to_vec()))
                        .collect::<Result<Vec<_>, _>>()
                        .map(|v| v.concat())
                })
                .collect::<Result<Vec<_>, _>>()?;
            TemplateBody::Tracks(tracks)
        } else {
            let days = f
                .readings
                .iter()
                .map(|day| day.iter().map(|r| parse_standard(r)).collect())
                .collect::<Result<Vec<_>, _>>()?;
            TemplateBody::Days(days)
        };
        Ok(Self {
            id: f.id,
            name: f.name,
            about: f.about,
            days: f.days,
            body,
        })
    }

    /// A custom plan: `books` (a Bible reference, such as "Rom-Gal; Jas") over `days` days.
    pub fn custom(books: &str, days: u32) -> Result<Self, String> {
        if days == 0 {
            return Err("A plan needs at least one day".into());
        }
        let list = parse_standard(books)?;
        let reference = format_passage_list(&list, &Standard, RefStyle::Short);
        Ok(Self {
            id: CUSTOM.into(),
            name: format!("{reference} in {days} days"),
            about: "Chosen books over a chosen number of days.".into(),
            days,
            body: TemplateBody::Tracks(vec![list.passages().to_vec()]),
        })
    }

    /// Lay the plan out from `start` on the days of `weekdays`: every Reading with its day,
    /// in order.
    pub fn schedule(
        &self,
        start: PlanDate,
        weekdays: Weekdays,
    ) -> Result<Vec<(PassageList, PlanDate)>, String> {
        let dates = reading_dates(start, self.days, weekdays);
        if dates.is_empty() {
            return Err("Choose at least one day of the week to read on".into());
        }
        let mut out = Vec::new();
        match &self.body {
            TemplateBody::Tracks(tracks) => {
                let parts: Vec<Vec<Vec<Passage>>> = tracks
                    .iter()
                    .map(|t| partition(&units(t, dates.len()), dates.len()))
                    .collect();
                for (i, day) in dates.iter().enumerate() {
                    for part in &parts {
                        if let Some(group) = part.get(i).filter(|g| !g.is_empty()) {
                            out.push((PassageList::from(group.clone()), *day));
                        }
                    }
                }
            }
            TemplateBody::Days(days) => {
                for (readings, day) in days.iter().zip(spread(days.len(), &dates)) {
                    out.extend(readings.iter().map(|r| (r.clone(), day)));
                }
            }
        }
        Ok(out)
    }
}

/// The built-in Reading plan templates, in the order the chooser lists them.
pub fn templates() -> &'static [PlanTemplate] {
    static TEMPLATES: OnceLock<Vec<PlanTemplate>> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        TEMPLATE_FILES
            .iter()
            .filter_map(|t| PlanTemplate::from_toml(t).ok())
            .collect()
    })
}

/// A built-in template by id.
pub fn template(id: &str) -> Option<&'static PlanTemplate> {
    templates().iter().find(|t| t.id == id)
}

/// Standard Verses from `start` to `end`, both included (title slots count).
fn verse_count(p: Passage) -> u32 {
    match (Standard.ordinal(p.start()), Standard.ordinal(p.end())) {
        (Some(a), Some(b)) if b >= a => b - a + 1,
        _ => 1,
    }
}

/// `passages` cut into weighted units to divide among `groups` days: whole chapters (a
/// Passage's partial first or last chapter stays partial), or single Verses when there are
/// fewer chapters than days.
fn units(passages: &[Passage], groups: usize) -> Vec<(Passage, u32)> {
    let mut chapters = Vec::new();
    for p in passages {
        let (s, e) = (p.start(), p.end());
        for book in Book::ALL {
            if book < s.book || book > e.book {
                continue;
            }
            for c in 1..=Standard.chapter_count(book) {
                let Some(chapter) = Passage::chapter(&Standard, book, c) else {
                    continue;
                };
                if let Some(part) = p.intersection(chapter) {
                    chapters.push((part, verse_count(part)));
                }
            }
        }
    }
    if chapters.len() >= groups {
        return chapters;
    }
    chapters
        .iter()
        .flat_map(|(p, _)| p.verses(&Standard).filter(|v| !v.is_title()))
        .map(|v| (Passage::verse(v), 1))
        .collect()
}

/// Divide weighted units, in order, into `groups` runs of about equal weight; adjacent units
/// in a run join into one Passage. Every run gets at least one unit while units last; with
/// fewer units than runs, the units spread out and some runs stay empty.
fn partition(units: &[(Passage, u32)], groups: usize) -> Vec<Vec<Passage>> {
    let mut runs: Vec<Vec<Passage>> = vec![Vec::new(); groups];
    if groups == 0 || units.is_empty() {
        return runs;
    }
    if units.len() <= groups {
        for (j, (p, _)) in units.iter().enumerate() {
            runs[j * groups / units.len()].push(*p);
        }
        return runs;
    }
    let mut remaining: u64 = units.iter().map(|(_, w)| u64::from(*w)).sum();
    let mut next = 0usize;
    for (g, run) in runs.iter_mut().enumerate() {
        let left = groups - g;
        // Leave at least one unit for each later run.
        let max_take = units.len() - next - (left - 1);
        let target = remaining as f64 / left as f64;
        let mut weight = 0u64;
        let mut taken = 0usize;
        while taken < max_take {
            let w = u64::from(units[next + taken].1);
            let over = (weight + w) as f64 - target;
            if taken > 0 && left > 1 && over > target - weight as f64 {
                break;
            }
            weight += w;
            taken += 1;
        }
        for (p, _) in &units[next..next + taken] {
            match run.last_mut() {
                Some(last) if Standard.next_verse(last.end()) == Some(p.start()) => {
                    *last = last.span(*p);
                }
                _ => run.push(*p),
            }
        }
        next += taken;
        remaining -= weight;
    }
    runs
}

// -------------------------------------------------------------------------------- plans

/// One Reading of a plan: Passages to read on a day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// Its place in the plan, from 0.
    pub position: u32,
    /// In the Standard versification.
    pub passages: PassageList,
    /// "Gen 1-3".
    pub label: String,
    pub day: PlanDate,
    pub read: bool,
}

/// A Reading plan: Readings laid out on days from a start day, read in one Bible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadingPlan {
    pub id: i64,
    pub name: String,
    /// The template's id ([`CUSTOM`] for a custom plan).
    pub template: String,
    /// The Bible its Readings open in.
    pub bible_id: String,
    pub start: PlanDate,
    pub weekdays: Weekdays,
    /// In order.
    pub readings: Vec<Reading>,
}

impl ReadingPlan {
    fn from_rows(row: ReadingPlanRow, readings: Vec<PlanReadingRow>) -> Option<Self> {
        let readings = readings
            .into_iter()
            .filter_map(|r| {
                let passages = parse_standard(&r.passages).ok()?;
                Some(Reading {
                    position: r.position,
                    label: format_passage_list(&passages, &Standard, RefStyle::Short),
                    passages,
                    day: PlanDate::parse(&r.day)?,
                    read: r.read_at.is_some(),
                })
            })
            .collect();
        Some(Self {
            id: row.id,
            name: row.name,
            template: row.template,
            bible_id: row.bible_id,
            start: PlanDate::parse(&row.start_day)?,
            weekdays: Weekdays(row.weekdays),
            readings,
        })
    }

    /// Its last day: the day of its last Reading.
    pub fn end(&self) -> PlanDate {
        self.readings
            .iter()
            .map(|r| r.day)
            .max()
            .unwrap_or(self.start)
    }

    pub fn read_count(&self) -> usize {
        self.readings.iter().filter(|r| r.read).count()
    }

    pub fn is_finished(&self) -> bool {
        self.readings.iter().all(|r| r.read)
    }

    /// The Readings due on `day`, in order.
    pub fn due_on(&self, day: PlanDate) -> Vec<&Reading> {
        self.readings.iter().filter(|r| r.day == day).collect()
    }

    /// Unread Readings due before `today`, in order.
    pub fn overdue(&self, today: PlanDate) -> Vec<&Reading> {
        let mut out: Vec<&Reading> = self
            .readings
            .iter()
            .filter(|r| !r.read && r.day < today)
            .collect();
        out.sort_by_key(|r| (r.day, r.position));
        out
    }

    /// How many reading days before `today` still have an unread Reading.
    pub fn behind_days(&self, today: PlanDate) -> usize {
        let mut days: Vec<PlanDate> = self.overdue(today).iter().map(|r| r.day).collect();
        days.dedup();
        days.len()
    }

    /// The first day from `today` on with a Reading, read or not.
    pub fn next_day(&self, today: PlanDate) -> Option<PlanDate> {
        self.readings
            .iter()
            .map(|r| r.day)
            .filter(|d| *d >= today)
            .min()
    }

    /// The Reading "Today's reading" opens: the first unread one due today, else the first
    /// overdue one, else the first due today.
    pub fn todays_reading(&self, today: PlanDate) -> Option<&Reading> {
        let due = self.due_on(today);
        let overdue = self.overdue(today);
        due.iter()
            .find(|r| !r.read)
            .or_else(|| overdue.first())
            .or_else(|| due.first())
            .copied()
    }

    /// The unread Readings, grouped by the day they are on, in day order.
    fn unread_by_day(&self) -> Vec<Vec<u32>> {
        let mut unread: Vec<&Reading> = self.readings.iter().filter(|r| !r.read).collect();
        unread.sort_by_key(|r| (r.day, r.position));
        let mut groups: Vec<(PlanDate, Vec<u32>)> = Vec::new();
        for r in unread {
            match groups.last_mut() {
                Some((day, g)) if *day == r.day => g.push(r.position),
                _ => groups.push((r.day, vec![r.position])),
            }
        }
        groups.into_iter().map(|(_, g)| g).collect()
    }

    fn moves(&self, groups: &[Vec<u32>], days: &[PlanDate]) -> Vec<(u32, PlanDate)> {
        let mut out = Vec::new();
        for (group, day) in groups.iter().zip(days) {
            for &position in group {
                if self
                    .readings
                    .iter()
                    .any(|r| r.position == position && r.day != *day)
                {
                    out.push((position, *day));
                }
            }
        }
        out
    }

    /// The reading days from `from` on, as many as `n`.
    fn next_reading_days(&self, from: PlanDate, n: usize) -> Vec<PlanDate> {
        let weekdays = if self.weekdays.is_empty() {
            Weekdays::ALL
        } else {
            self.weekdays
        };
        (0..)
            .map(|i| from.plus(i))
            .filter(|d| weekdays.contains(*d))
            .take(n)
            .collect()
    }

    /// Catch up: the unread Readings move on, keeping their spacing, so the first of them is
    /// on the first reading day from `today`. The plan ends as many reading days later as it
    /// was behind. Nothing moves when it is not behind.
    pub fn catch_up(&self, today: PlanDate) -> Vec<(u32, PlanDate)> {
        if self.behind_days(today) == 0 {
            return Vec::new();
        }
        let groups = self.unread_by_day();
        let days = self.next_reading_days(today, groups.len());
        self.moves(&groups, &days)
    }

    /// Reschedule: the plan keeps its last day, and the unread Readings, in order, are spread
    /// evenly over the reading days from `today` to it. When no reading day is left, it
    /// catches up instead. Nothing moves when it is not behind.
    pub fn reschedule(&self, today: PlanDate) -> Vec<(u32, PlanDate)> {
        if self.behind_days(today) == 0 {
            return Vec::new();
        }
        let end = self.end();
        let span = u32::try_from(end.0 - today.0 + 1).unwrap_or(0);
        let days = reading_dates(today, span, self.weekdays);
        if days.is_empty() {
            return self.catch_up(today);
        }
        let groups = self.unread_by_day();
        self.moves(&groups, &spread(groups.len(), &days))
    }
}

/// What [`Session`] keeps of Reading plans: the User database's, cached so that drawing a
/// frame never reads it, and today's date.
#[derive(Debug, Default)]
pub struct ReadingPlans {
    plans: Vec<ReadingPlan>,
    /// Today, the local day, as of the last check.
    today: Option<PlanDate>,
    /// A day tests (and the app's automation) set as today.
    pretend: Option<PlanDate>,
}

impl Session {
    /// Load the Reading plans from the User database.
    pub(crate) fn load_reading_plans(&mut self) {
        self.refresh_plan_day();
        let Some(user) = &self.user else {
            self.plans.plans.clear();
            return;
        };
        let rows = user.reading_plans().unwrap_or_default();
        self.plans.plans = rows
            .into_iter()
            .filter_map(|row| {
                let readings = user.plan_readings(row.id).ok()?;
                ReadingPlan::from_rows(row, readings)
            })
            .collect();
    }

    /// Every Reading plan, oldest first.
    pub fn reading_plans(&self) -> &[ReadingPlan] {
        &self.plans.plans
    }

    pub fn reading_plan(&self, id: i64) -> Option<&ReadingPlan> {
        self.plans.plans.iter().find(|p| p.id == id)
    }

    /// Today, the local day (as the User database's clock has it).
    pub fn plan_today(&self) -> PlanDate {
        self.plans
            .pretend
            .or(self.plans.today)
            .unwrap_or_else(utc_today)
    }

    /// Treat `day` as today from now on (tests).
    #[doc(hidden)]
    pub fn pretend_today(&mut self, day: PlanDate) {
        self.plans.pretend = Some(day);
    }

    /// Check the local day again (saving the Last session calls this, at most once a second).
    pub(crate) fn refresh_plan_day(&mut self) {
        let now = i64::try_from(crate::get_bibles::now_secs()).unwrap_or(i64::MAX);
        self.plans.today = self
            .user
            .as_ref()
            .and_then(|u| u.local_day(now).ok())
            .and_then(|d| PlanDate::parse(&d));
    }

    /// The plan the toolbar's Today's reading shows: the first with a Reading due today or
    /// overdue, else the first unfinished one.
    pub fn toolbar_plan(&self) -> Option<&ReadingPlan> {
        let today = self.plan_today();
        let plans = self.reading_plans();
        plans
            .iter()
            .find(|p| p.due_on(today).iter().any(|r| !r.read) || p.behind_days(today) > 0)
            .or_else(|| plans.iter().find(|p| !p.is_finished()))
    }

    /// Show a Reading's Passage in the plan's Bible: its most recent Tab moves there (unless
    /// `split`), else a new Tab opens. A Bible no longer in the library gives way to any Bible
    /// Tab, else the ★ Bible. Returns how (`tab` or `new`) and the Tab.
    fn open_reading(
        &mut self,
        passage: Passage,
        bible: &str,
        split: bool,
    ) -> Result<(&'static str, crate::workspace::TabId), String> {
        if self.library.get(bible).is_none() {
            return self.show_passage(passage, None, split);
        }
        let kind = crate::workspace::TabKind::Resource(bible.to_string());
        if !split && let Some(t) = self.workspace.most_recent(|t| t.kind == kind) {
            self.workspace.focus(t);
            self.workspace.navigate_standard(t, passage.start());
            return Ok(("tab", t));
        }
        let out = self
            .run("resource.open", &json!({ "id": bible, "split": split }))
            .map_err(|e| e.to_string())?;
        let t = out["tab"]
            .as_u64()
            .map(crate::workspace::TabId)
            .ok_or("no Tab opened")?;
        // The new Tab's Verse map is known once the UI has loaded it.
        self.workspace.push_effect(Effect::NavigateStandard {
            tab: t,
            verse: passage.start(),
        });
        Ok(("new", t))
    }

    fn create_plan(&mut self, p: &Params<'_>) -> Result<Value, String> {
        let template = p.str("template").unwrap_or_default();
        let custom;
        let t = if template == CUSTOM {
            let days = p
                .i64("days")
                .and_then(|d| u32::try_from(d).ok())
                .ok_or("Say over how many days")?;
            custom = PlanTemplate::custom(p.str("books").unwrap_or_default(), days)?;
            &custom
        } else {
            self::template(template).ok_or_else(|| format!("No Reading plan “{template}”"))?
        };
        let start = match p.str("start").map(str::trim).filter(|s| !s.is_empty()) {
            Some(s) => PlanDate::parse(s).ok_or_else(|| format!("“{s}” is not a date"))?,
            None => self.plan_today(),
        };
        let weekdays = match p.str("weekdays") {
            Some(w) => Weekdays::parse(w).ok_or("Choose at least one day of the week")?,
            None => Weekdays::ALL,
        };
        let bible_id = match p.str("bible").filter(|b| !b.is_empty()) {
            Some(b) => {
                let ok = self
                    .library
                    .get(b)
                    .is_some_and(|e| e.manifest.resource_type == ResourceType::Bible);
                if !ok {
                    return Err(format!("{b} is not a Bible in your library"));
                }
                b.to_string()
            }
            None => self
                .library
                .preferred(ResourceType::Bible)
                .map(|e| e.manifest.resource_id.clone())
                .unwrap_or_default(),
        };
        let name = p
            .str("name")
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(&t.name)
            .to_string();
        let schedule = t.schedule(start, weekdays)?;
        let row = ReadingPlanRow {
            id: 0,
            name: name.clone(),
            template: t.id.clone(),
            bible_id,
            start_day: start.to_string(),
            weekdays: weekdays.0,
            created_at: 0,
        };
        let readings: Vec<PlanReadingRow> = schedule
            .iter()
            .map(|(passages, day)| PlanReadingRow {
                position: 0,
                passages: format_passage_list(passages, &Standard, RefStyle::Short),
                day: day.to_string(),
                read_at: None,
            })
            .collect();
        let user = self
            .user
            .as_ref()
            .ok_or("Reading plans are kept in the User database, which is not open")?;
        let id = user.add_reading_plan(&row, &readings)?;
        self.load_reading_plans();
        self.workspace
            .push_effect(Effect::Message(format!("Started {name}")));
        let today = self.plan_today();
        let due: Vec<String> = self
            .reading_plan(id)
            .map(|p| p.due_on(today).iter().map(|r| r.label.clone()).collect())
            .unwrap_or_default();
        Ok(json!({ "id": id, "name": name, "readings": readings.len(), "today": due }))
    }
}

/// Today in UTC, when there is no User database to ask for the local day.
fn utc_today() -> PlanDate {
    let now = i64::try_from(crate::get_bibles::now_secs()).unwrap_or(0);
    PlanDate(now.div_euclid(86_400))
}

// -------------------------------------------------------------------------------- commands

fn failed(command: &'static str, message: impl ToString) -> CommandError {
    CommandError::Failed {
        command,
        message: message.to_string(),
    }
}

fn has_user(s: &Session) -> bool {
    s.user.is_some()
}

fn has_plans(s: &Session) -> bool {
    !s.plans.plans.is_empty()
}

const PLAN: ParamSpec = ParamSpec {
    name: "plan",
    kind: ParamKind::Integer,
    required: true,
    help: "The Reading plan's id",
};
const POSITION: ParamSpec = ParamSpec {
    name: "position",
    kind: ParamKind::Integer,
    required: true,
    help: "The Reading's place in the plan, from 0",
};

fn plan_of<'a>(
    s: &'a Session,
    p: &Params<'_>,
    cmd: &'static str,
) -> Result<&'a ReadingPlan, CommandError> {
    let id = p.i64("plan").unwrap_or_default();
    s.reading_plan(id)
        .ok_or_else(|| failed(cmd, format!("no Reading plan {id}")))
}

/// Move a plan's Readings and reload; how many moved.
fn move_readings(
    s: &mut Session,
    plan: i64,
    moves: &[(u32, PlanDate)],
    cmd: &'static str,
) -> Result<usize, CommandError> {
    if moves.is_empty() {
        return Ok(0);
    }
    let rows: Vec<(u32, String)> = moves.iter().map(|(p, d)| (*p, d.to_string())).collect();
    let user = s
        .user
        .as_ref()
        .ok_or_else(|| failed(cmd, "no User database"))?;
    user.move_readings(plan, &rows)
        .map_err(|e| failed(cmd, e))?;
    s.load_reading_plans();
    Ok(moves.len())
}

pub(crate) const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        id: "reading_plan.create",
        label: "New Reading plan",
        shortcut: None,
        params: &[
            ParamSpec {
                name: "template",
                kind: ParamKind::String,
                required: true,
                help: "bible-canonical, bible-chronological, mcheyne, nt-90-days, \
                       psalms-proverbs or custom",
            },
            ParamSpec {
                name: "start",
                kind: ParamKind::String,
                required: false,
                help: "First day, YYYY-MM-DD (default: today)",
            },
            ParamSpec {
                name: "bible",
                kind: ParamKind::String,
                required: false,
                help: "Bible the Readings open in (default: the ★ Bible)",
            },
            ParamSpec {
                name: "weekdays",
                kind: ParamKind::String,
                required: false,
                help: "Days to read on, e.g. `mon,tue,wed,thu,fri` (default: every day)",
            },
            ParamSpec {
                name: "books",
                kind: ParamKind::String,
                required: false,
                help: "Custom plan: the books, as a Bible reference (e.g. `Rom-Gal; Jas`)",
            },
            ParamSpec {
                name: "days",
                kind: ParamKind::Integer,
                required: false,
                help: "Custom plan: over how many days",
            },
            ParamSpec {
                name: "name",
                kind: ParamKind::String,
                required: false,
                help: "The plan's name (default: the template's)",
            },
        ],
        enabled: has_user,
        run: |s, p| {
            s.create_plan(&p)
                .map_err(|e| failed("reading_plan.create", e))
        },
    },
    CommandSpec {
        id: "reading_plan.mark",
        label: "Mark Reading read",
        shortcut: None,
        params: &[
            PLAN,
            POSITION,
            ParamSpec {
                name: "read",
                kind: ParamKind::Boolean,
                required: false,
                help: "false to mark it unread again (default: true)",
            },
        ],
        enabled: has_plans,
        run: |s, p| {
            const CMD: &str = "reading_plan.mark";
            let plan = plan_of(s, &p, CMD)?.id;
            let position = p
                .i64("position")
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| failed(CMD, "no position"))?;
            let read = p.bool("read").unwrap_or(true);
            let user = s
                .user
                .as_ref()
                .ok_or_else(|| failed(CMD, "no User database"))?;
            if !user
                .set_reading_read(plan, position, read)
                .map_err(|e| failed(CMD, e))?
            {
                return Err(failed(CMD, format!("no Reading {position} in plan {plan}")));
            }
            s.load_reading_plans();
            let left = s
                .reading_plan(plan)
                .map_or(0, |p| p.readings.len() - p.read_count());
            Ok(json!({ "plan": plan, "position": position, "read": read, "left": left }))
        },
    },
    CommandSpec {
        id: "reading_plan.open",
        label: "Open today's reading",
        shortcut: None,
        params: &[
            PLAN,
            ParamSpec {
                name: "position",
                kind: ParamKind::Integer,
                required: false,
                help: "The Reading to open (default: today's)",
            },
            ParamSpec {
                name: "passage",
                kind: ParamKind::Integer,
                required: false,
                help: "Which of the Reading's Passages, from 0 (default: the first)",
            },
            ParamSpec {
                name: "split",
                kind: ParamKind::Boolean,
                required: false,
                help: "Open in a new Tab split off to the right (Ctrl/Cmd held)",
            },
        ],
        enabled: has_plans,
        run: |s, p| {
            const CMD: &str = "reading_plan.open";
            let plan = plan_of(s, &p, CMD)?;
            let reading = match p.i64("position") {
                Some(n) => plan
                    .readings
                    .iter()
                    .find(|r| i64::from(r.position) == n)
                    .ok_or_else(|| failed(CMD, format!("no Reading {n}")))?,
                None => plan
                    .todays_reading(s.plan_today())
                    .ok_or_else(|| failed(CMD, "This plan has no Readings"))?,
            };
            let index = p
                .i64("passage")
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(0);
            let passage = *reading
                .passages
                .passages()
                .get(index)
                .ok_or_else(|| failed(CMD, format!("no Passage {index}")))?;
            let (label, position) = (reading.label.clone(), reading.position);
            let bible = plan.bible_id.clone();
            let (how, tab) = s
                .open_reading(passage, &bible, p.bool("split").unwrap_or(false))
                .map_err(|e| failed(CMD, e))?;
            Ok(json!({ "tab": tab.0, "how": how, "label": label, "position": position }))
        },
    },
    CommandSpec {
        id: "reading_plan.catch_up",
        label: "Catch up",
        shortcut: None,
        params: &[PLAN],
        enabled: has_plans,
        run: |s, p| {
            const CMD: &str = "reading_plan.catch_up";
            let plan = plan_of(s, &p, CMD)?;
            let (id, moves) = (plan.id, plan.catch_up(s.plan_today()));
            let moved = move_readings(s, id, &moves, CMD)?;
            let end = s.reading_plan(id).map(|p| p.end().to_string());
            Ok(json!({ "plan": id, "moved": moved, "end": end }))
        },
    },
    CommandSpec {
        id: "reading_plan.reschedule",
        label: "Reschedule",
        shortcut: None,
        params: &[PLAN],
        enabled: has_plans,
        run: |s, p| {
            const CMD: &str = "reading_plan.reschedule";
            let plan = plan_of(s, &p, CMD)?;
            let (id, moves) = (plan.id, plan.reschedule(s.plan_today()));
            let moved = move_readings(s, id, &moves, CMD)?;
            let end = s.reading_plan(id).map(|p| p.end().to_string());
            Ok(json!({ "plan": id, "moved": moved, "end": end }))
        },
    },
    CommandSpec {
        id: "reading_plan.remove",
        label: "Delete Reading plan",
        shortcut: None,
        params: &[PLAN],
        enabled: has_plans,
        run: |s, p| {
            const CMD: &str = "reading_plan.remove";
            let plan = plan_of(s, &p, CMD)?;
            let (id, name) = (plan.id, plan.name.clone());
            let user = s
                .user
                .as_ref()
                .ok_or_else(|| failed(CMD, "no User database"))?;
            user.remove_reading_plan(id).map_err(|e| failed(CMD, e))?;
            s.load_reading_plans();
            s.workspace
                .push_effect(Effect::Message(format!("Deleted the plan {name}")));
            Ok(json!({ "removed": id }))
        },
    },
];
