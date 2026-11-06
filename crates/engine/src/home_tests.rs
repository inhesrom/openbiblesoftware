//! Home (#178): the Tab and where it opens, its cards' arrangement and "Open Home at launch"
//! kept in the User database, the Verse of the day, and what each card lists on a fresh
//! install and with a Reading plan, History and Favorites. A real `.obr` library and a real
//! User database in a temp folder.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use obs_model::ResourceType;
use obs_scripture::{Book, Passage, Standard, Verse, Versification, parse_reference};
use serde_json::json;

use crate::get_bibles::ResourceFilter;
use crate::home::{HOME_PREF, HomeCard, HomeSettings};
use crate::testing::bibles::{bible, v, write_library};
use crate::*;

/// A library of BSB and WEB (Gen 1:1, Jn 3:16, Rom 8:28), and where the User database goes.
fn folder(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("obs-home-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let verses = [
        (
            v(Book::Genesis, 1, 1),
            "In the beginning God created the heavens and the earth.",
        ),
        (
            v(Book::John, 3, 16),
            "For God so loved the world that He gave His one and only Son.",
        ),
        (
            v(Book::Romans, 8, 28),
            "And we know that God works all things together for the good.",
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

/// Launch as the app does: scan the library, then attach the User database.
fn launch(root: &Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.library
        .set_preferred_order(vec!["bsb".into(), "web".into()]);
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    s
}

fn tab(out: &serde_json::Value) -> TabId {
    TabId(out["tab"].as_u64().unwrap())
}

fn open(s: &mut Session, id: &str) -> TabId {
    tab(&s.run("resource.open", &json!({ "id": id })).unwrap())
}

fn go(s: &mut Session, t: TabId, text: &str) {
    s.run("tab.go", &json!({ "tab": t.0, "text": text }))
        .unwrap();
}

fn at(s: &Session, t: TabId) -> Option<Verse> {
    s.workspace.tab(t).unwrap().history.current()
}

fn order(s: &Session) -> Vec<HomeCard> {
    s.home_settings().cards.iter().map(|p| p.card).collect()
}

#[test]
fn home_is_one_keyless_tab_that_opens_over_the_bible() {
    let root = folder("tab");
    let mut s = launch(&root);
    let bsb = open(&mut s, "bsb");
    s.run("library.open", &json!({})).unwrap();
    let home = tab(&s.run("home.open", &json!({})).unwrap());
    let t = s.workspace.tab(home).unwrap();
    assert_eq!((t.kind.clone(), t.title.as_str()), (TabKind::Home, "Home"));
    assert_eq!(t.kind.header_box(), HeaderBox::Keyless);
    assert!(!t.kind.has_link_chip());
    assert!(t.kind.default_key_schemes().is_empty());
    // It wants the room: the reading Panel, over the ★ Bible, not the Library's.
    assert_eq!(s.workspace.panel_of(home), s.workspace.panel_of(bsb));
    assert_eq!(s.workspace.focused(), Some(home));
    // Focusing Home leaves the non-reading Panel as it was: a Tool still opens beside it.
    let notes = tab(&s.run("tool.notes", &json!({})).unwrap());
    assert_ne!(s.workspace.panel_of(notes), s.workspace.panel_of(home));
    assert_eq!(
        s.workspace.active_in(s.workspace.panel_of(home).unwrap()),
        Some(home)
    );
    // One Home: opening it again focuses it.
    s.workspace.focus(bsb);
    assert_eq!(tab(&s.run("home.open", &json!({})).unwrap()), home);
    assert_eq!(s.workspace.focused(), Some(home));
    assert_eq!(
        s.commands().get("home.open").and_then(|c| c.shortcut),
        Some("Alt+H")
    );
    // Home is not a place: it makes no Visit and cannot be a Favorite.
    s.save_last_session().unwrap();
    assert!(s.location_of(home).is_none());
    assert!(s.history().iter().all(|v| v.location.kind != TabKind::Home));
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn without_a_bible_home_gets_its_own_panel_and_the_starter_offer_opens_beside_it() {
    let root = std::env::temp_dir().join(format!("obs-home-empty-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut s = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    // The default layout with no Bible, Home on top, then the first frame's Starter offer.
    let library = tab(&s.run("library.open", &json!({})).unwrap());
    s.run("tool.word_info", &json!({})).unwrap();
    s.workspace.focus(library);
    let home = tab(&s.run("home.open", &json!({})).unwrap());
    assert_ne!(s.workspace.panel_of(home), s.workspace.panel_of(library));
    let starter = tab(&s.run("library.starter", &json!({})).unwrap());
    assert_eq!(s.workspace.panel_of(starter), s.workspace.panel_of(library));
    let home_panel = s.workspace.panel_of(home).unwrap();
    assert_eq!(s.workspace.active_in(home_panel), Some(home));
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn home_cards_are_reordered_hidden_and_reset_and_kept_after_a_restart() {
    let root = folder("arrange");
    let mut s = launch(&root);
    assert_eq!(order(&s), HomeCard::ALL);
    assert!(!s.home_opens_at_launch());
    s.run("home.move_card", &json!({ "card": "quick_start", "to": 0 }))
        .unwrap();
    s.run("home.hide_card", &json!({ "card": "verse_of_the_day" }))
        .unwrap();
    s.run("home.open_at_launch", &json!({})).unwrap();
    assert!(
        s.run("home.hide_card", &json!({ "card": "weather" }))
            .is_err()
    );
    drop(s);

    let mut s = launch(&root);
    assert_eq!(
        order(&s),
        [
            HomeCard::QuickStart,
            HomeCard::TodaysReading,
            HomeCard::ContinueReading,
            HomeCard::HistoryAndFavorites,
            HomeCard::VerseOfTheDay,
            HomeCard::LibrarySummary,
        ]
    );
    let visible: Vec<HomeCard> = s.home_settings().visible().collect();
    assert!(!visible.contains(&HomeCard::VerseOfTheDay));
    assert_eq!(
        s.home_settings().hidden().collect::<Vec<_>>(),
        [HomeCard::VerseOfTheDay]
    );
    assert!(s.home_opens_at_launch());
    // Shown again, then the order reset; "Open Home at launch" is not part of the cards.
    s.run(
        "home.hide_card",
        &json!({ "card": "verse_of_the_day", "hidden": false }),
    )
    .unwrap();
    s.run("home.reset_cards", &json!({})).unwrap();
    drop(s);
    let s = launch(&root);
    assert_eq!(s.home_settings().cards, HomeSettings::default().cards);
    assert!(s.home_opens_at_launch());
    let stored = s.user.as_ref().unwrap().preference(HOME_PREF).unwrap();
    assert!(stored.unwrap().contains("\"open_at_launch\":true"));
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_saved_home_arrangement_drops_unknown_cards_and_places_missing_ones() {
    let h = HomeSettings::from_json(
        r#"{"cards":[{"id":"library"},{"id":"weather","hidden":true},
            {"id":"library","hidden":true},{"id":"quick_start","hidden":true}]}"#,
    );
    let cards: Vec<(HomeCard, bool)> = h.cards.iter().map(|p| (p.card, p.hidden)).collect();
    assert_eq!(
        cards,
        [
            (HomeCard::TodaysReading, false),
            (HomeCard::ContinueReading, false),
            (HomeCard::HistoryAndFavorites, false),
            (HomeCard::VerseOfTheDay, false),
            (HomeCard::LibrarySummary, false),
            (HomeCard::QuickStart, true),
        ]
    );
    assert!(!h.open_at_launch);
    assert_eq!(HomeSettings::from_json(&h.to_json()), h);
    assert_eq!(HomeSettings::from_json("not json"), HomeSettings::default());
}

#[test]
fn every_verse_of_the_day_is_one_short_standard_passage() {
    for text in VERSES_OF_THE_DAY {
        let list = parse_reference(text, &Standard, None).unwrap();
        assert_eq!(list.passages().len(), 1, "{text}");
        let p = list.passages()[0];
        let n = p.verses(&Standard).count();
        assert!((1..=4).contains(&n), "{text}: {n} Verses");
        assert!(Standard.contains(p.end()), "{text}");
    }
    let mut unique = VERSES_OF_THE_DAY.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), VERSES_OF_THE_DAY.len());
}

#[test]
fn the_verse_of_the_day_is_the_same_all_day_and_goes_through_the_list() {
    let day = PlanDate::parse("2026-11-05").unwrap();
    assert_eq!(verse_of_the_day_on(day), verse_of_the_day_on(day));
    assert_ne!(verse_of_the_day_on(day), verse_of_the_day_on(day.plus(1)));
    let n = i64::try_from(VERSES_OF_THE_DAY.len()).unwrap();
    let seen: std::collections::BTreeSet<Passage> =
        (0..n).map(|i| verse_of_the_day_on(day.plus(i))).collect();
    assert_eq!(seen.len(), VERSES_OF_THE_DAY.len());
    assert_eq!(verse_of_the_day_on(day), verse_of_the_day_on(day.plus(n)));

    // In a Session: today's, in the ★ Bible.
    let root = folder("votd");
    let mut s = launch(&root);
    s.pretend_today(day);
    let votd = s.verse_of_the_day();
    assert_eq!(votd.passage, verse_of_the_day_on(day));
    assert_eq!(votd.bible.as_deref(), Some("bsb"));
    assert!(!votd.label.is_empty());
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_fresh_install_lists_quick_starts_and_the_starter_library_by_resource_type() {
    let root = std::env::temp_dir().join(format!("obs-home-fresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bundle = root.join("starter");
    let set = crate::testing::starter_set();
    crate::testing::write_starter_bundle(&bundle, &set);
    let mut s = Session::new(JobMode::Inline);
    s.library.set_directory(root.join("library"));
    s.set_search_cache_dir(root.join("cache"));
    let (store, _) = obs_store::UserStore::open(&root.join("user.db")).unwrap();
    s.attach_user(Box::new(store));
    let sync = s.sync_starter_library(Some(&bundle));
    assert!(sync.problems.is_empty(), "{:?}", sync.problems);
    assert_eq!(sync.added.len(), set.len());

    // Quick start: each command is there and can run now.
    let labels: Vec<&str> = QUICK_STARTS.iter().map(|q| q.label).collect();
    assert_eq!(
        labels,
        ["Passage Guide", "Word Study", "Get Resources", "Import"]
    );
    for q in &QUICK_STARTS {
        assert!(s.is_enabled(q.command), "{}", q.command);
    }
    // With no Bible Tab yet, the Passage Guide opens at the Verse of the day.
    let pg = s.quick_start_params(&QUICK_STARTS[0]);
    let votd = verse_of_the_day_on(s.plan_today());
    assert_eq!(
        parse_reference(pg["passage"].as_str().unwrap(), &Standard, None)
            .unwrap()
            .passages(),
        [votd]
    );
    assert_eq!(s.quick_start_params(&QUICK_STARTS[2]), json!({}));

    // The library by Resource type: the Starter's one Bible and four Lexicons; an empty type
    // links to Get Resources at that type when the repositories offer it.
    let summary: Vec<(ResourceType, usize, Option<ResourceFilter>)> = s
        .library_summary()
        .into_iter()
        .map(|c| (c.resource_type, c.count, c.get))
        .collect();
    assert_eq!(
        summary,
        [
            (
                ResourceType::Bible,
                1,
                Some(ResourceFilter::Type(ResourceType::Bible))
            ),
            (
                ResourceType::Commentary,
                0,
                Some(ResourceFilter::Type(ResourceType::Commentary))
            ),
            (
                ResourceType::Lexicon,
                4,
                Some(ResourceFilter::Type(ResourceType::Lexicon))
            ),
            (
                ResourceType::Dictionary,
                0,
                Some(ResourceFilter::Type(ResourceType::Dictionary))
            ),
            (ResourceType::CrossReferenceSet, 0, None),
            (ResourceType::PeopleAndPlaces, 0, None),
        ]
    );
    let starter = s.starter_summary();
    assert_eq!(starter.in_library, set.len());
    assert_eq!(starter.offered, Some(set.len()));
    // Nothing read yet: no plans, no History, no Favorites.
    assert!(s.home_plans().is_empty());
    assert!(s.home_continue().is_none());
    assert!(s.home_history().is_empty());
    assert!(s.home_favorites().is_empty());
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn with_a_reading_plan_history_and_favorites_each_opens_with_one_command() {
    let root = folder("places");
    let mut s = launch(&root);
    let today = PlanDate::parse("2026-11-05").unwrap();
    s.pretend_today(today);
    s.run(
        "reading_plan.create",
        &json!({ "template": "bible-canonical", "bible": "web" }),
    )
    .unwrap();
    let bsb = open(&mut s, "bsb");
    go(&mut s, bsb, "Rom 8:28");
    s.run("favorite.add", &json!({ "tab": bsb.0 })).unwrap();
    go(&mut s, bsb, "Gen 1:1");
    go(&mut s, bsb, "Jn 3:16");
    s.save_last_session().unwrap();
    drop(s);

    // Next launch, nothing open but Home.
    let mut s = launch(&root);
    s.pretend_today(today);
    let home = tab(&s.run("home.open", &json!({})).unwrap());

    // Today's reading: the plan's Readings due today, in its Bible.
    let plans = s.home_plans();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].behind, 0);
    assert_eq!(plans[0].next, None);
    let reading = plans[0].readings[0].clone();
    assert!(reading.label.starts_with("Gen 1-"), "{}", reading.label);
    assert!(!reading.read);
    let out = s
        .run(
            "reading_plan.open",
            &json!({ "plan": plans[0].id, "position": reading.position }),
        )
        .unwrap();
    let web = tab(&out);
    assert_eq!(
        s.workspace.tab(web).unwrap().kind,
        TabKind::Resource("web".into())
    );

    // Continue where you left off: the newest Visit; History lists the others, once each.
    let last = s.home_continue().cloned().unwrap();
    assert_eq!(last.label, "John 3:16 · BSB");
    let history: Vec<&str> = s.home_history().iter().map(|v| v.label.as_str()).collect();
    assert_eq!(history, ["Genesis 1:1 · BSB", "Romans 8:28 · BSB"]);
    s.workspace.focus(home);
    let t = tab(&s.run("history.open", &json!({ "id": last.id })).unwrap());
    assert_eq!(
        s.workspace.tab(t).unwrap().kind,
        TabKind::Resource("bsb".into())
    );
    assert_eq!(at(&s, t), Some(v(Book::John, 3, 16)));

    // Favorites: one click opens the starred place.
    let favorites = s.home_favorites().to_vec();
    assert_eq!(favorites.len(), 1);
    assert_eq!(favorites[0].label, "Romans 8:28 · BSB");
    s.workspace.focus(home);
    let t = tab(&s
        .run("favorite.open", &json!({ "id": favorites[0].id }))
        .unwrap());
    assert_eq!(at(&s, t), Some(v(Book::Romans, 8, 28)));

    // The Passage Guide Quick start opens where the most recent Bible Tab is.
    s.workspace.focus(home);
    assert_eq!(
        s.quick_start_params(&QUICK_STARTS[0]),
        json!({ "passage": "Rom 8:28" })
    );

    // Read: ticked on Home too; and the next day, behind, the missed Reading is offered.
    s.run(
        "reading_plan.mark",
        &json!({ "plan": plans[0].id, "position": reading.position }),
    )
    .unwrap();
    assert!(s.home_plans()[0].readings[0].read);
    s.pretend_today(today.plus(2));
    let plans = s.home_plans();
    assert_eq!(plans[0].behind, 1);
    assert_eq!(plans[0].readings.len(), 1);
    assert!(!plans[0].readings[0].read);
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_visit_to_a_removed_resource_is_not_offered_to_continue() {
    let root = folder("removed");
    let mut s = launch(&root);
    let web = open(&mut s, "web");
    go(&mut s, web, "Gen 1:1");
    let bsb = open(&mut s, "bsb");
    go(&mut s, bsb, "Jn 3:16");
    s.save_last_session().unwrap();
    drop(s);
    std::fs::remove_file(root.join("library").join("bsb.obr")).unwrap();
    let s = launch(&root);
    assert_eq!(s.home_continue().unwrap().label, "Genesis 1:1 · WEB");
    assert!(s.home_history().is_empty());
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_verse_of_the_day_opens_in_the_star_bible_even_when_only_another_bible_is_open() {
    let root = folder("votd-bible");
    let mut s = launch(&root);
    let day = PlanDate::parse("2026-11-05").unwrap();
    s.pretend_today(day);
    let votd = verse_of_the_day_on(day).start();
    // Only WEB is open; BSB is the ★ Bible.
    let web = open(&mut s, "web");
    go(&mut s, web, "Gen 1:1");
    s.run("home.open", &json!({})).unwrap();
    s.workspace.take_effects();

    let out = s.run("home.read_verse_of_the_day", &json!({})).unwrap();
    assert_eq!(
        (out["how"].as_str(), out["bible"].as_str()),
        (Some("new"), Some("bsb"))
    );
    let bsb = tab(&out);
    assert_eq!(
        s.workspace.tab(bsb).unwrap().kind,
        TabKind::Resource("bsb".into())
    );
    assert!(
        s.workspace
            .take_effects()
            .contains(&Effect::NavigateStandard {
                tab: bsb,
                verse: votd
            })
    );
    // WEB stays where it was.
    assert_eq!(at(&s, web), Some(v(Book::Genesis, 1, 1)));

    // Again, with WEB the most recent Tab: BSB's Tab moves, no new Tab.
    s.workspace.focus(web);
    let out = s.run("home.read_verse_of_the_day", &json!({})).unwrap();
    assert_eq!((out["how"].as_str(), tab(&out)), (Some("tab"), bsb));
    assert_eq!(s.workspace.focused(), Some(bsb));
    assert_eq!(at(&s, web), Some(v(Book::Genesis, 1, 1)));
    // Ctrl/Cmd: a new ★ Bible Tab.
    let out = s
        .run("home.read_verse_of_the_day", &json!({ "split": true }))
        .unwrap();
    assert_eq!(out["how"].as_str(), Some("new"));
    assert_ne!(tab(&out), bsb);
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn every_unfinished_reading_plan_is_on_home_even_after_three_that_start_later() {
    let root = folder("plans");
    let mut s = launch(&root);
    let today = PlanDate::parse("2026-11-05").unwrap();
    s.pretend_today(today);
    for (template, start) in [
        ("bible-canonical", "2026-12-01"),
        ("nt-90-days", "2026-12-02"),
        ("psalms-proverbs", "2027-01-01"),
    ] {
        s.run(
            "reading_plan.create",
            &json!({ "template": template, "bible": "bsb", "start": start }),
        )
        .unwrap();
    }
    s.run(
        "reading_plan.create",
        &json!({ "template": "mcheyne", "bible": "web" }),
    )
    .unwrap();

    let plans = s.home_plans();
    assert_eq!(plans.len(), 4);
    for p in &plans[..3] {
        assert!(p.readings.is_empty(), "{}", p.name);
        assert!(p.next.is_some(), "{}", p.name);
    }
    // The fourth, created last, is due today and is listed with its Readings.
    assert!(!plans[3].readings.is_empty());
    assert_eq!(plans[3].next, None);
    drop(s);
    std::fs::remove_dir_all(&root).unwrap();
}
