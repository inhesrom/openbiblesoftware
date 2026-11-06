//! Reading plans (#147): the templates as open data, laying a plan out on days, ticking
//! Readings off, and Catch up and Reschedule after missed days. A real `.obr` library and a
//! real User database in a temp folder for the Session tests.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use obs_scripture::{Book, Passage, PassageList, Standard, Testament, Verse, Versification};
use serde_json::json;

use crate::reading_plans::{CUSTOM, PlanDate, PlanTemplate, Weekdays, template, templates};
use crate::testing::bibles::{bible, v, write_library};
use crate::*;

fn day(s: &str) -> PlanDate {
    PlanDate::parse(s).unwrap()
}

/// How many times each Standard Verse (title slots left out) is read.
fn coverage<'a>(lists: impl IntoIterator<Item = &'a PassageList>) -> BTreeMap<Verse, u32> {
    let mut out = BTreeMap::new();
    for list in lists {
        for p in list.passages() {
            for verse in p.verses(&Standard).filter(|v| !v.is_title()) {
                *out.entry(verse).or_insert(0) += 1;
            }
        }
    }
    out
}

/// Every non-title Verse of `books`, `times` times each.
fn every_verse(books: impl Iterator<Item = Book>, times: u32) -> BTreeMap<Verse, u32> {
    let mut out = BTreeMap::new();
    for b in books {
        for verse in Passage::book(&Standard, b)
            .unwrap()
            .verses(&Standard)
            .filter(|v| !v.is_title())
        {
            out.insert(verse, times);
        }
    }
    out
}

fn schedule(id: &str, start: &str, weekdays: Weekdays) -> Vec<(PassageList, PlanDate)> {
    template(id)
        .unwrap()
        .schedule(day(start), weekdays)
        .unwrap()
}

#[test]
fn plan_dates_read_write_and_know_their_weekday() {
    let d = day("2026-11-05");
    assert_eq!(d.to_string(), "2026-11-05");
    assert_eq!(d.long_label(), "Thursday 5 November 2026");
    assert_eq!(d.short_label(), "Thu 5 Nov");
    assert_eq!(d.label_with_year(), "Thu 5 Nov 2026");
    assert_eq!(d.plus(365).to_string(), "2027-11-05");
    assert_eq!(day("2028-02-28").plus(1).to_string(), "2028-02-29");
    assert_eq!(day("1970-01-01"), PlanDate(0));
    for bad in ["2026-02-30", "2026-13-01", "5 Nov", "2026-11-05-1", ""] {
        assert_eq!(PlanDate::parse(bad), None, "{bad}");
    }
    assert_eq!(Weekdays::parse("mon,wed fri"), Some(Weekdays(0b001_0101)));
    assert_eq!(
        Weekdays::parse("weekdays").unwrap().label(),
        "Monday to Friday"
    );
    assert_eq!(Weekdays::parse("").unwrap().label(), "Every day");
    assert_eq!(Weekdays(0b001_0101).label(), "Mon, Wed, Fri");
    assert_eq!(Weekdays::parse("someday"), None);
}

#[test]
fn every_template_file_loads_in_the_chooser_order() {
    let ids: Vec<&str> = templates().iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "bible-canonical",
            "bible-chronological",
            "mcheyne",
            "nt-90-days",
            "psalms-proverbs"
        ]
    );
    let names: Vec<&str> = templates().iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Bible in a year (canonical)",
            "Bible in a year (chronological)",
            "Bible in a year (M'Cheyne)",
            "New Testament in 90 days",
            "Psalms and Proverbs monthly",
        ]
    );
}

#[test]
fn bible_in_a_year_canonical_reads_the_bible_once_a_day_at_a_time() {
    let plan = schedule("bible-canonical", "2026-11-05", Weekdays::ALL);
    assert_eq!(plan.len(), 365);
    let days: Vec<PlanDate> = plan.iter().map(|(_, d)| *d).collect();
    let expected: Vec<PlanDate> = (0..365).map(|i| day("2026-11-05").plus(i)).collect();
    assert_eq!(days, expected);
    assert_eq!(
        coverage(plan.iter().map(|(l, _)| l)),
        every_verse(Book::ALL.into_iter(), 1)
    );
    // In order, whole chapters, about the same amount each day.
    let first = plan[0].0.passages()[0];
    assert_eq!(first.start(), v(Book::Genesis, 1, 1));
    assert_eq!(
        first.end().verse,
        Standard
            .last_verse(first.end().book, first.end().chapter)
            .unwrap()
    );
    let sizes: Vec<usize> = plan
        .iter()
        .map(|(l, _)| {
            l.passages()
                .iter()
                .map(|p| p.verses(&Standard).count())
                .sum()
        })
        .collect();
    let mean = 31_102 / 365;
    assert!(
        sizes.iter().filter(|&&n| n > mean * 2).count() <= 3,
        "{sizes:?}"
    );
}

#[test]
fn bible_in_a_year_chronological_reads_every_chapter_exactly_once() {
    let t = template("bible-chronological").unwrap();
    let plan = t.schedule(day("2027-01-01"), Weekdays::ALL).unwrap();
    assert_eq!(plan.len(), 365);
    assert_eq!(
        coverage(plan.iter().map(|(l, _)| l)),
        every_verse(Book::ALL.into_iter(), 1)
    );
    // Job comes among the patriarchs, before Abraham's call.
    let at = |verse: Verse| {
        plan.iter()
            .position(|(l, _)| l.contains_verse(verse))
            .unwrap()
    };
    assert!(at(v(Book::Job, 1, 1)) < at(v(Book::Genesis, 12, 1)));
    assert!(at(v(Book::Genesis, 11, 1)) <= at(v(Book::Job, 1, 1)));
    assert!(at(v(Book::Galatians, 1, 1)) < at(v(Book::Romans, 1, 1)));
}

#[test]
fn mcheyne_reads_the_old_testament_once_and_the_new_testament_and_psalms_twice() {
    let plan = schedule("mcheyne", "2027-01-01", Weekdays::ALL);
    assert_eq!(plan.len(), 365 * 4);
    let day_one: Vec<String> = plan[..4]
        .iter()
        .map(|(l, d)| {
            assert_eq!(*d, day("2027-01-01"));
            obs_scripture::format_passage_list(l, &Standard, obs_scripture::RefStyle::Short)
        })
        .collect();
    assert_eq!(day_one, ["Gen 1", "Matt 1", "Ezra 1", "Acts 1"]);
    let twice = |b: Book| b == Book::Psalms || b.testament() == Testament::New;
    let mut expected = every_verse(Book::ALL.into_iter().filter(|b| !twice(*b)), 1);
    expected.extend(every_verse(Book::ALL.into_iter().filter(|b| twice(*b)), 2));
    let got = coverage(plan.iter().map(|(l, _)| l));
    let wrong: Vec<String> = expected
        .iter()
        .filter(|(verse, n)| got.get(verse) != Some(n))
        .map(|(verse, n)| format!("{verse}: {:?} not {n}", got.get(verse)))
        .chain(
            got.keys()
                .filter(|verse| !expected.contains_key(verse))
                .map(|verse| format!("{verse}: extra")),
        )
        .collect();
    assert!(wrong.is_empty(), "{wrong:?}");
    // The last day of the calendar.
    assert_eq!(plan.last().unwrap().1, day("2027-12-31"));
}

#[test]
fn new_testament_in_90_days_and_psalms_and_proverbs_monthly() {
    let nt = schedule("nt-90-days", "2026-11-05", Weekdays::ALL);
    assert_eq!(nt.len(), 90);
    assert_eq!(
        coverage(nt.iter().map(|(l, _)| l)),
        every_verse(
            Book::ALL
                .into_iter()
                .filter(|b| b.testament() == Testament::New),
            1
        )
    );

    let pp = schedule("psalms-proverbs", "2026-11-01", Weekdays::ALL);
    // A share of the Psalms and one chapter of Proverbs on each of 31 days.
    assert_eq!(pp.len(), 62);
    for (i, pair) in pp.chunks(2).enumerate() {
        let d = day("2026-11-01").plus(i64::try_from(i).unwrap());
        assert!(pair.iter().all(|(_, day)| *day == d));
        assert_eq!(pair[0].0.passages()[0].start().book, Book::Psalms);
        let proverbs = pair[1].0.passages()[0];
        assert_eq!(
            proverbs,
            Passage::chapter(&Standard, Book::Proverbs, u16::try_from(i + 1).unwrap()).unwrap()
        );
    }
    assert_eq!(
        coverage(pp.iter().map(|(l, _)| l)),
        every_verse([Book::Psalms, Book::Proverbs].into_iter(), 1)
    );
}

#[test]
fn chosen_days_of_the_week_keep_the_plan_to_its_year() {
    let weekdays = Weekdays::parse("mon,tue,wed,thu,fri").unwrap();
    let plan = schedule("bible-canonical", "2026-11-05", weekdays);
    assert!(plan.iter().all(|(_, d)| d.weekday() < 5));
    assert!(plan.last().unwrap().1 < day("2027-11-05"));
    assert_eq!(plan.len(), 261);
    assert_eq!(
        coverage(plan.iter().map(|(l, _)| l)),
        every_verse(Book::ALL.into_iter(), 1)
    );
    // M'Cheyne's 365 days fold onto the reading days, keeping each day's four together.
    let m = schedule(
        "mcheyne",
        "2027-01-04",
        Weekdays::parse("mon,wed,fri").unwrap(),
    );
    assert_eq!(m.len(), 365 * 4);
    assert!(m.iter().all(|(_, d)| [0, 2, 4].contains(&d.weekday())));
    assert!(m.last().unwrap().1 <= day("2028-01-03"));
    assert_eq!(m[0].1, m[3].1);
    assert!(
        template("bible-canonical")
            .unwrap()
            .schedule(day("2026-11-05"), Weekdays(0))
            .is_err()
    );
}

#[test]
fn a_custom_plan_reads_chosen_books_over_n_days() {
    let t = PlanTemplate::custom("Rom-Gal; Jas", 30).unwrap();
    assert_eq!(t.id, CUSTOM);
    assert_eq!(t.name, "Rom-Gal; Jas in 30 days");
    let plan = t.schedule(day("2026-11-05"), Weekdays::ALL).unwrap();
    assert_eq!(plan.len(), 30);
    assert_eq!(
        coverage(plan.iter().map(|(l, _)| l)),
        every_verse(
            [
                Book::Romans,
                Book::FirstCorinthians,
                Book::SecondCorinthians,
                Book::Galatians,
                Book::James
            ]
            .into_iter(),
            1
        )
    );
    // Fewer chapters than days: the Verses are shared out instead.
    let jude = PlanTemplate::custom("Jude", 5)
        .unwrap()
        .schedule(day("2026-11-05"), Weekdays::ALL)
        .unwrap();
    assert_eq!(jude.len(), 5);
    assert_eq!(
        coverage(jude.iter().map(|(l, _)| l)),
        every_verse([Book::Jude].into_iter(), 1)
    );
    assert!(PlanTemplate::custom("Hezekiah 1", 5).is_err());
    assert!(PlanTemplate::custom("Rom", 0).is_err());
}

// -------------------------------------------------------------------------------- Session

fn folder(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("obs-plans-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let verses = [
        (
            v(Book::Genesis, 1, 1),
            "In the beginning God created the heavens and the earth.",
        ),
        (
            v(Book::Genesis, 4, 1),
            "And Adam knew Eve his wife; and she conceived.",
        ),
    ];
    write_library(
        &root.join("library"),
        &[
            bible("bsb", "BSB", &verses, None, &[]),
            bible("web", "WEB", &verses, None, &[]),
        ],
    );
    root
}

fn launch(root: &Path, today: &str) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    s.pretend_today(day(today));
    s
}

fn labels(s: &Session, plan: i64, on: &str) -> Vec<String> {
    s.reading_plan(plan)
        .unwrap()
        .due_on(day(on))
        .iter()
        .map(|r| r.label.clone())
        .collect()
}

#[test]
fn bible_in_a_year_from_today_shows_todays_passages_and_marking_them_read_advances_it() {
    let root = folder("today");
    let mut s = launch(&root, "2026-11-05");
    let out = s
        .run(
            "reading_plan.create",
            &json!({ "template": "bible-canonical", "bible": "web" }),
        )
        .unwrap();
    let id = out["id"].as_i64().unwrap();
    assert_eq!(out["readings"], 365);
    // Starting today: today's Passages are the first Reading.
    let today = labels(&s, id, "2026-11-05");
    assert_eq!(out["today"], json!(today));
    assert_eq!(today.len(), 1);
    assert!(today[0].starts_with("Gen 1-"), "{today:?}");
    let plan = s.reading_plan(id).unwrap();
    assert_eq!(plan.start, day("2026-11-05"));
    assert_eq!(plan.end(), day("2027-11-04"));
    assert_eq!(plan.bible_id, "web");
    assert_eq!(s.toolbar_plan().map(|p| p.id), Some(id));

    // One click opens today's reading in the plan's Bible.
    let opened = s.run("reading_plan.open", &json!({ "plan": id })).unwrap();
    assert_eq!(opened["label"], today[0]);
    let tab = TabId(opened["tab"].as_u64().unwrap());
    assert_eq!(
        s.workspace.tab(tab).unwrap().kind,
        TabKind::Resource("web".into())
    );
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::NavigateStandard {
                tab,
                verse: v(Book::Genesis, 1, 1),
            })
    );

    // Marking it read advances the plan: today is done, tomorrow's Reading is next.
    let position = s
        .reading_plan(id)
        .unwrap()
        .todays_reading(day("2026-11-05"))
        .unwrap()
        .position;
    s.run(
        "reading_plan.mark",
        &json!({ "plan": id, "position": position }),
    )
    .unwrap();
    let plan = s.reading_plan(id).unwrap();
    assert_eq!(plan.read_count(), 1);
    assert!(plan.due_on(day("2026-11-05")).iter().all(|r| r.read));
    assert_eq!(plan.behind_days(day("2026-11-06")), 0);
    s.pretend_today(day("2026-11-06"));
    let next = s
        .reading_plan(id)
        .unwrap()
        .todays_reading(day("2026-11-06"))
        .unwrap();
    assert_eq!(next.position, 1);
    assert!(!next.read);
    assert!(next.label.starts_with("Gen "), "{}", next.label);
    drop(s);

    // Progress is kept in the User database.
    let s = launch(&root, "2026-11-06");
    let plan = s.reading_plan(id).unwrap();
    assert_eq!(plan.read_count(), 1);
    assert!(plan.readings[0].read);
    assert_eq!(plan.name, "Bible in a year (canonical)");
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn missing_three_days_then_reschedule_spreads_the_rest_over_the_remaining_days() {
    let root = folder("reschedule");
    let mut s = launch(&root, "2026-11-05");
    let id = s
        .run(
            "reading_plan.create",
            &json!({ "template": "bible-canonical" }),
        )
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let before = s.reading_plan(id).unwrap().clone();
    // Three days go by without reading: 5, 6 and 7 November.
    s.pretend_today(day("2026-11-08"));
    let today = s.plan_today();
    assert_eq!(s.reading_plan(id).unwrap().behind_days(today), 3);
    assert_eq!(s.toolbar_plan().map(|p| p.id), Some(id));

    let out = s
        .run("reading_plan.reschedule", &json!({ "plan": id }))
        .unwrap();
    assert_eq!(out["end"], "2027-11-04");
    let plan = s.reading_plan(id).unwrap();
    assert_eq!(plan.behind_days(today), 0);
    assert_eq!(plan.end(), before.end());
    assert_eq!(plan.readings.len(), 365);
    // Every Reading still to read, in order, on the 362 days from today to the end: none
    // left in the past, every day read on, never more than two on a day.
    let mut per_day: BTreeMap<PlanDate, usize> = BTreeMap::new();
    for pair in plan.readings.windows(2) {
        assert!(pair[0].day <= pair[1].day);
    }
    for r in &plan.readings {
        assert!(r.day >= today && r.day <= before.end());
        assert_eq!(r.passages, before.readings[r.position as usize].passages);
        *per_day.entry(r.day).or_default() += 1;
    }
    assert_eq!(per_day.len(), 362);
    assert_eq!(per_day.values().filter(|&&n| n == 2).count(), 3);
    assert!(per_day.values().all(|&n| n <= 2));
    assert!(!labels(&s, id, "2026-11-08").is_empty());

    // Rescheduling again when not behind moves nothing.
    let again = s
        .run("reading_plan.reschedule", &json!({ "plan": id }))
        .unwrap();
    assert_eq!(again["moved"], 0);
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn catch_up_moves_the_unread_readings_on_and_the_plan_ends_later() {
    let root = folder("catch-up");
    let mut s = launch(&root, "2026-11-02");
    let id = s
        .run(
            "reading_plan.create",
            &json!({ "template": "nt-90-days", "weekdays": "mon,tue,wed,thu,fri" }),
        )
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let before = s.reading_plan(id).unwrap().clone();
    // Monday 2 November is read; Tuesday to Thursday are missed.
    s.run("reading_plan.mark", &json!({ "plan": id, "position": 0 }))
        .unwrap();
    s.pretend_today(day("2026-11-06"));
    assert_eq!(
        s.reading_plan(id).unwrap().behind_days(day("2026-11-06")),
        3
    );
    s.run("reading_plan.catch_up", &json!({ "plan": id }))
        .unwrap();
    let plan = s.reading_plan(id).unwrap();
    assert_eq!(plan.behind_days(day("2026-11-06")), 0);
    // Tuesday's Reading is today's; the rest keep one a weekday, three weekdays later.
    assert_eq!(plan.readings[0].day, day("2026-11-02"));
    assert_eq!(plan.readings[1].day, day("2026-11-06"));
    assert_eq!(plan.readings[2].day, day("2026-11-09"));
    let weekdays_after = |d: PlanDate, n: usize| {
        (1..)
            .map(|i| d.plus(i))
            .filter(|x| x.weekday() < 5)
            .nth(n - 1)
            .unwrap()
    };
    assert_eq!(plan.end(), weekdays_after(before.end(), 3));
    assert!(plan.readings.iter().all(|r| r.day.weekday() < 5));
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn custom_plans_and_bad_requests() {
    let root = folder("custom");
    let mut s = launch(&root, "2026-11-05");
    let out = s
        .run(
            "reading_plan.create",
            &json!({
                "template": "custom", "books": "Rom-Gal", "days": 30,
                "start": "2026-12-01", "name": "Advent letters"
            }),
        )
        .unwrap();
    let id = out["id"].as_i64().unwrap();
    let plan = s.reading_plan(id).unwrap();
    assert_eq!(plan.name, "Advent letters");
    assert_eq!(plan.template, "custom");
    assert_eq!(plan.start, day("2026-12-01"));
    // It starts later: nothing today, nothing behind.
    assert_eq!(out["today"], json!([]));
    assert_eq!(plan.behind_days(day("2026-11-05")), 0);
    assert_eq!(plan.next_day(day("2026-11-05")), Some(day("2026-12-01")));
    for bad in [
        json!({ "template": "nope" }),
        json!({ "template": "custom", "books": "Rom" }),
        json!({ "template": "bible-canonical", "start": "tomorrow" }),
        json!({ "template": "bible-canonical", "weekdays": "someday" }),
        json!({ "template": "bible-canonical", "bible": "kjv" }),
    ] {
        assert!(s.run("reading_plan.create", &bad).is_err(), "{bad}");
    }
    s.run("reading_plan.remove", &json!({ "plan": id }))
        .unwrap();
    assert!(s.reading_plans().is_empty());
    assert!(!s.is_enabled("reading_plan.mark"));
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();

    // Without a User database there is nowhere to keep a plan.
    let s = Session::new(JobMode::Inline);
    assert!(!s.is_enabled("reading_plan.create"));
}

#[test]
fn tools_menu_opens_the_reading_plans_tool() {
    let mut s = Session::new(JobMode::Inline);
    let out = s.run("tool.reading_plans", &json!({})).unwrap();
    let t = TabId(out["tab"].as_u64().unwrap());
    assert_eq!(
        s.workspace.tab(t).unwrap().kind,
        TabKind::Tool(Tool::ReadingPlans)
    );
    assert!(
        MENUS
            .iter()
            .find(|m| m.title == "Tools")
            .unwrap()
            .items
            .contains(&MenuItem::Command("tool.reading_plans"))
    );
    // One Reading plans Tab.
    let again = s.run("tool.reading_plans", &json!({})).unwrap();
    assert_eq!(again["tab"], out["tab"]);
}
