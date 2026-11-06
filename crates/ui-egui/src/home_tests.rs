//! Home (#178) through the headless harness: a fresh install opens on Home with Quick start
//! and the Starter library summary; with a Reading plan, History and Favorites, Home lists
//! today's reading and the places, each one click from opening; cards move and hide from
//! their headers, and "Open Home at launch" shows Home on top of the Last session. Real
//! `.obr` files and a real User database in a temp folder.

use std::path::{Path, PathBuf};

use obs_engine::testing::bibles::{bible, v, write_library};
use obs_engine::testing::{starter_set, write_starter_bundle};
use obs_engine::{Guide, HomeCard, PlanDate, TabId, verse_of_the_day_on};
use obs_scripture::{Book, Verse};

use super::*;

struct Lab {
    root: PathBuf,
}

impl Lab {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("obs-shell-home-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("library")).unwrap();
        Self { root }
    }

    fn db(&self) -> PathBuf {
        self.root.join("user.db")
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// Run queued jobs (inline mode) and draw until the Bible Tabs are placed.
fn settle(h: &mut Harness) {
    for _ in 0..10 {
        h.shell.st.session.jobs().pump(64);
        h.frames(2);
    }
    settle_all(h);
}

fn focused_kind(h: &Harness) -> Option<TabKind> {
    let ws = &h.shell.st.session.workspace;
    ws.focused().and_then(|t| ws.tab(t)).map(|t| t.kind.clone())
}

fn home_tab(h: &Harness) -> Option<TabId> {
    (h.shell.st.session.workspace.tabs())
        .find(|t| t.kind == TabKind::Home)
        .map(|t| t.id)
}

fn tab_of(h: &Harness, kind: &TabKind) -> Option<TabId> {
    (h.shell.st.session.workspace.tabs())
        .find(|t| t.kind == *kind)
        .map(|t| t.id)
}

/// The menu bar's Home button (the only "Home" painted above the Workspace).
fn click_home_button(h: &mut Harness) {
    let r = h
        .shapes
        .iter()
        .find(|(r, t)| t == "Home" && r.top() < 40.0)
        .map(|(r, _)| *r)
        .unwrap_or_else(|| panic!("no Home button: {:?}", h.texts()));
    h.click(r.center());
    settle(h);
}

/// The control `symbol` (⏶ ⏷ ×) in the header of the card titled `title`.
fn card_control(h: &Harness, title: &str, symbol: &str) -> Pos2 {
    let head = h
        .find(title)
        .unwrap_or_else(|| panic!("{title:?} not on screen: {:?}", h.texts()));
    h.shapes
        .iter()
        .filter(|(r, t)| {
            t == symbol && r.left() > head.right() && (r.center().y - head.center().y).abs() < 10.0
        })
        .min_by(|a, b| a.0.left().total_cmp(&b.0.left()))
        .map(|(r, _)| r.center())
        .unwrap_or_else(|| panic!("no {symbol} by {title:?}"))
}

fn order(h: &Harness) -> Vec<HomeCard> {
    (h.shell.st.session.home_settings().visible()).collect()
}

/// A fresh install: the Starter library bundled, nothing in the User database yet.
fn fresh_install(lab: &Lab) -> Harness {
    let bundle = lab.root.join("starter");
    write_starter_bundle(&bundle, &starter_set());
    let mut session = Session::new(JobMode::Inline);
    session.library.set_directory(lab.root.join("library"));
    session.set_search_cache_dir(lab.root.join("cache"));
    let (store, _) = obs_store::UserStore::open(&lab.db()).unwrap();
    session.attach_user(Box::new(store));
    let sync = session.sync_starter_library(Some(&bundle));
    assert!(sync.problems.is_empty(), "{:?}", sync.problems);
    // A day whose Verse of the day (Gen 1:1) the Starter fixture's KJV has.
    session.pretend_today(PlanDate::parse("2026-12-21").unwrap());
    let mut shell = Shell::new(session);
    shell.restore_at_launch();
    let mut h = Harness::from_shell(shell);
    settle(&mut h);
    h
}

#[test]
fn a_fresh_install_opens_on_home_with_quick_starts_and_the_starter_library_summary() {
    let lab = Lab::new("fresh");
    let mut h = fresh_install(&lab);
    // Home is focused, over the ★ Bible in the reading Panel.
    assert_eq!(focused_kind(&h), Some(TabKind::Home));
    let panels = h.panels();
    let home_panel = panels
        .iter()
        .find(|(_, t)| t.iter().any(|t| t == "Home"))
        .expect("Home in a Panel");
    assert!(home_panel.1.contains(&"KJV".to_string()), "{panels:?}");
    for t in [
        "Quick start",
        "Passage Guide",
        "Word Study",
        "Get Resources",
        "Import",
        "Your library",
        "Bibles",
        "Lexicons",
        "Get Commentaries…",
        "Get Dictionaries…",
        "Starter library: 5 Resources",
        "Genesis 1:1",
        "Today's reading",
        "No Reading plan yet.",
        "Verse of the day",
        "Continue where you left off",
        "History and Favorites",
        "Open Home at launch",
    ] {
        assert!(h.texts().contains(&t), "{t:?} missing: {:?}", h.texts());
    }
    preview(&h, "home-fresh-install");

    // One click on the Passage Guide Quick start: a Passage Guide at the Verse of the day.
    h.click_text("Passage Guide");
    settle(&mut h);
    let pg = tab_of(&h, &TabKind::Guide(Guide::PassageGuide)).expect("a Passage Guide");
    let today = h.shell.st.session.plan_today();
    assert_eq!(
        h.shell
            .st
            .session
            .workspace
            .tab(pg)
            .unwrap()
            .history
            .current(),
        Some(verse_of_the_day_on(today).start())
    );
    // An empty Resource type links to Get Resources at that type.
    click_home_button(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::Home));
    h.click_text("Get Commentaries…");
    settle(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::GetBibles));
    assert_eq!(
        h.shell.st.session.get_bibles.filter(),
        obs_engine::get_bibles::ResourceFilter::Type(obs_model::ResourceType::Commentary)
    );

    // The next launch restores the Last session; Home shows again only because it was open.
    let home = home_tab(&h).unwrap();
    h.shell.run("tab.close", &json!({ "tab": home.0 }));
    h.shell.save_session(true);
    drop(h);
    let mut session = Session::new(JobMode::Inline);
    session.library.set_directory(lab.root.join("library"));
    assert!(session.library.scan(&lab.root.join("library")).is_empty());
    let (store, _) = obs_store::UserStore::open(&lab.db()).unwrap();
    session.attach_user(Box::new(store));
    let mut shell = Shell::new(session);
    shell.restore_at_launch();
    assert!(
        !shell
            .st
            .session
            .workspace
            .tabs()
            .any(|t| t.kind == TabKind::Home)
    );
}

/// BSB with Genesis 1-3, Psalm 46:1 (the Verse of the day on 5 November 2026), John 3:16
/// and Romans 8:28.
fn write_bsb(dir: &Path) {
    let mut text: Vec<(Verse, String)> = Vec::new();
    for (chapter, count) in [(1, 31), (2, 25), (3, 24)] {
        for n in 1..=count {
            text.push((
                v(Book::Genesis, chapter, n),
                format!("Genesis {chapter}:{n}. In the beginning God created the heavens."),
            ));
        }
    }
    text.push((
        v(Book::Psalms, 46, 1),
        "God is our refuge and strength, an ever-present help in trouble.".into(),
    ));
    text.push((
        v(Book::John, 3, 16),
        "For God so loved the world that He gave His one and only Son.".into(),
    ));
    text.push((
        v(Book::Romans, 8, 28),
        "And we know that God works all things together for the good.".into(),
    ));
    let verses: Vec<(Verse, &str)> = text.iter().map(|(v, t)| (*v, t.as_str())).collect();
    write_library(dir, &[bible("bsb", "BSB", &verses, None, &[])]);
}

fn launch_bsb(lab: &Lab, today: &str) -> Harness {
    write_bsb(&lab.root);
    let mut h = Harness::launch_full(Some(&lab.db()), JobMode::Inline, Some(&lab.root));
    let session = &mut h.shell.st.session;
    session.library.add_file(&lab.root.join("bsb.obr")).unwrap();
    session.library.set_preferred_order(vec!["bsb".into()]);
    session.pretend_today(PlanDate::parse(today).unwrap());
    settle(&mut h);
    h
}

fn bsb_tab(h: &Harness) -> TabId {
    tab_of(h, &TabKind::Resource("bsb".into())).expect("BSB is open")
}

#[test]
fn with_a_reading_plan_and_favorites_home_shows_todays_reading_and_places_each_one_click() {
    let lab = Lab::new("places");
    let mut h = launch_bsb(&lab, "2026-11-05");
    h.shell.run(
        "reading_plan.create",
        &json!({ "template": "bible-canonical", "bible": "bsb" }),
    );
    let out = h
        .shell
        .run("resource.open", &json!({ "id": "bsb" }))
        .unwrap();
    let bsb = TabId(out["tab"].as_u64().unwrap());
    settle(&mut h);
    for (place, star) in [("Rom 8:28", true), ("Gen 2:4", false), ("Jn 3:16", false)] {
        h.shell
            .run("tab.go", &json!({ "tab": bsb.0, "text": place }));
        settle(&mut h);
        if star {
            h.shell.run("favorite.add", &json!({ "tab": bsb.0 }));
        }
        h.shell.save_session(true);
    }
    let reading = h.shell.st.session.home_plans()[0].readings[0].label.clone();
    assert!(reading.starts_with("Gen 1-"), "{reading}");

    // Home, from the toolbar.
    click_home_button(&mut h);
    assert_eq!(focused_kind(&h), Some(TabKind::Home));
    for t in [
        reading.as_str(),
        "Bible in a year (canonical)",
        "John 3:16 · BSB",
        "Genesis 2:4 · BSB",
        "★ Romans 8:28 · BSB",
    ] {
        assert!(h.texts().contains(&t), "{t:?} missing: {:?}", h.texts());
    }
    preview(&h, "home-reading-plan-and-favorites");

    // Today's reading: one click opens it in the plan's Bible.
    h.click_text(&reading);
    settle(&mut h);
    let bsb = bsb_tab(&h);
    assert_eq!(h.shell.st.session.workspace.focused(), Some(bsb));
    assert_eq!(location(&mut h, bsb), Some(v(Book::Genesis, 1, 1)));

    // A Favorite: one click.
    click_home_button(&mut h);
    h.click_text("★ Romans 8:28 · BSB");
    settle(&mut h);
    assert_eq!(h.shell.st.session.workspace.focused(), Some(bsb));
    assert_eq!(location(&mut h, bsb), Some(v(Book::Romans, 8, 28)));

    // Continue where you left off, and a History row: one click each.
    click_home_button(&mut h);
    h.click_text("Genesis 2:4 · BSB");
    settle(&mut h);
    assert_eq!(location(&mut h, bsb), Some(v(Book::Genesis, 2, 4)));
    click_home_button(&mut h);
    let last = h.shell.st.session.home_continue().unwrap().label.clone();
    h.click_text(&last);
    settle(&mut h);
    assert_eq!(h.shell.st.session.workspace.focused(), Some(bsb));
    assert_eq!(
        location(&mut h, bsb),
        Some(match last.as_str() {
            "Genesis 2:4 · BSB" => v(Book::Genesis, 2, 4),
            "Romans 8:28 · BSB" => v(Book::Romans, 8, 28),
            "Genesis 1:1 · BSB" => v(Book::Genesis, 1, 1),
            _ => v(Book::John, 3, 16),
        })
    );
}

/// BSB (the ★ Bible) and WEB in the library, Home open, and only WEB open at Genesis 1:1.
fn launch_bsb_with_only_web_open(lab: &Lab, today: &str) -> (Harness, TabId) {
    let mut h = launch_bsb(lab, today);
    let votd = verse_of_the_day_on(PlanDate::parse(today).unwrap()).start();
    let verses = [
        (
            v(Book::Genesis, 1, 1),
            "In the beginning God created the heavens.",
        ),
        (votd, "God is our refuge and strength, a very present help."),
    ];
    write_library(&lab.root, &[bible("web", "WEB", &verses, None, &[])]);
    let session = &mut h.shell.st.session;
    session.library.add_file(&lab.root.join("web.obr")).unwrap();
    session
        .library
        .set_preferred_order(vec!["bsb".into(), "web".into()]);
    if let Some(bsb) = tab_of(&h, &TabKind::Resource("bsb".into())) {
        h.shell.run("tab.close", &json!({ "tab": bsb.0 }));
    }
    let out = h
        .shell
        .run("resource.open", &json!({ "id": "web" }))
        .unwrap();
    let web = TabId(out["tab"].as_u64().unwrap());
    settle(&mut h);
    h.shell
        .run("tab.go", &json!({ "tab": web.0, "text": "Gen 1:1" }));
    settle(&mut h);
    assert!(tab_of(&h, &TabKind::Resource("bsb".into())).is_none());
    click_home_button(&mut h);
    (h, web)
}

#[test]
fn the_verse_of_the_day_opens_in_the_star_bible_not_in_another_open_bible() {
    let lab = Lab::new("votd-bible");
    let (mut h, web) = launch_bsb_with_only_web_open(&lab, "2026-11-05");
    let votd = h.shell.st.session.verse_of_the_day();
    assert_eq!(votd.bible.as_deref(), Some("bsb"));
    // Home previews it in the ★ Bible, and one click reads it there.
    assert!(h.texts().contains(&"★ BSB"), "{:?}", h.texts());
    h.click_text(&votd.label);
    settle(&mut h);
    let bsb = bsb_tab(&h);
    assert_eq!(h.shell.st.session.workspace.focused(), Some(bsb));
    assert_eq!(location(&mut h, bsb), Some(votd.passage.start()));
    assert_eq!(location(&mut h, web), Some(v(Book::Genesis, 1, 1)));

    // Again from Home, with WEB the most recent Bible Tab: BSB's Tab moves there.
    h.shell
        .run("tab.go", &json!({ "tab": bsb.0, "text": "Jn 3:16" }));
    h.shell.st.session.workspace.focus(web);
    settle(&mut h);
    click_home_button(&mut h);
    h.click_text(&votd.label);
    settle(&mut h);
    assert_eq!(bsb_tab(&h), bsb);
    assert_eq!(h.shell.st.session.workspace.focused(), Some(bsb));
    assert_eq!(location(&mut h, bsb), Some(votd.passage.start()));
    assert_eq!(location(&mut h, web), Some(v(Book::Genesis, 1, 1)));
}

#[test]
fn todays_reading_lists_every_plan_even_after_three_that_start_later() {
    let lab = Lab::new("four-plans");
    let mut h = launch_bsb(&lab, "2026-11-05");
    for (template, start) in [
        ("bible-chronological", "2026-12-01"),
        ("nt-90-days", "2026-12-02"),
        ("psalms-proverbs", "2027-01-01"),
        ("bible-canonical", "2026-11-05"),
    ] {
        h.shell.run(
            "reading_plan.create",
            &json!({ "template": template, "bible": "bsb", "start": start }),
        );
    }
    settle(&mut h);
    click_home_button(&mut h);
    let plans = h.shell.st.session.home_plans();
    assert_eq!(plans.len(), 4);
    let reading = plans[3].readings[0].label.clone();
    for t in [
        "Bible in a year (canonical)",
        "Next reading: Tue 1 Dec",
        reading.as_str(),
    ] {
        assert!(h.texts().contains(&t), "{t:?} missing: {:?}", h.texts());
    }
    // The fourth plan's Reading, due today, opens with one click.
    h.click_text(&reading);
    settle(&mut h);
    let bsb = bsb_tab(&h);
    assert_eq!(h.shell.st.session.workspace.focused(), Some(bsb));
    assert_eq!(location(&mut h, bsb), Some(v(Book::Genesis, 1, 1)));
}

#[test]
fn cards_move_and_hide_from_their_headers_and_home_can_open_at_every_launch() {
    let lab = Lab::new("arrange");
    let mut h = launch_bsb(&lab, "2026-11-05");
    click_home_button(&mut h);
    let home = home_tab(&h).unwrap();
    assert_eq!(order(&h), HomeCard::ALL);

    // ⏶ on Quick start: it goes before the Verse of the day.
    let up = card_control(&h, "Quick start", "⏶");
    h.click(up);
    settle(&mut h);
    assert_eq!(
        order(&h),
        [
            HomeCard::TodaysReading,
            HomeCard::ContinueReading,
            HomeCard::HistoryAndFavorites,
            HomeCard::QuickStart,
            HomeCard::VerseOfTheDay,
            HomeCard::LibrarySummary,
        ]
    );
    // × hides the Verse of the day; Show brings it back.
    let hide = card_control(&h, "Verse of the day", "×");
    h.click(hide);
    settle(&mut h);
    assert!(!h.texts().contains(&"Verse of the day"));
    assert!(h.texts().contains(&"Show Verse of the day"));
    h.click_text("Show Verse of the day");
    settle(&mut h);
    assert!(h.texts().contains(&"Verse of the day"));
    h.click(card_control(&h, "Verse of the day", "×"));
    settle(&mut h);

    // Open Home at launch, then quit with Home closed.
    h.click_text("Open Home at launch");
    settle(&mut h);
    assert!(h.shell.st.session.home_opens_at_launch());
    h.shell.run("tab.close", &json!({ "tab": home.0 }));
    let bsb = h
        .shell
        .run("resource.open", &json!({ "id": "bsb" }))
        .unwrap();
    settle(&mut h);
    h.shell.save_session(true);
    drop(h);

    // Relaunch: the Last session (BSB) with Home on top, arranged as it was left.
    let mut session = Session::new(JobMode::Inline);
    session.library.add_file(&lab.root.join("bsb.obr")).unwrap();
    let (store, _) = obs_store::UserStore::open(&lab.db()).unwrap();
    session.attach_user(Box::new(store));
    let mut shell = Shell::new(session);
    shell.restore_at_launch();
    let h = Harness::from_shell(shell);
    assert_eq!(focused_kind(&h), Some(TabKind::Home));
    assert!(
        tab_of(&h, &TabKind::Resource("bsb".into())).is_some(),
        "{bsb}"
    );
    assert!(!order(&h).contains(&HomeCard::VerseOfTheDay));
    assert_eq!(order(&h)[3], HomeCard::QuickStart);
    assert!(h.texts().contains(&"Show Verse of the day"));
}

#[test]
fn card_controls_are_in_the_chrome_fonts() {
    let h = Harness::new();
    h.ctx.fonts_mut(|f| {
        for s in ["⏶", "⏷", "×", "★", "☆"] {
            assert!(f.has_glyphs(&egui::FontId::proportional(14.0), s), "{s}");
        }
    });
}
