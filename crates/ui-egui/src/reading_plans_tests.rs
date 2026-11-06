//! Reading plans (#147) through the headless harness: Tools → Reading plans, the New plan
//! form, Today's reading on the toolbar, ticking a Reading off, and Reschedule after three
//! missed days. A real `.obr` BSB fixture and a real User database in a temp folder.

use std::path::{Path, PathBuf};

use obs_engine::testing::bibles::{bible, v};
use obs_engine::{PlanDate, TabId};
use obs_scripture::{Book, Verse};

use super::*;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("obs-plans-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A BSB fixture of Genesis 1-8, every Verse.
fn write_bsb(dir: &Path) {
    let mut text: Vec<(Verse, String)> = Vec::new();
    for (chapter, count) in [
        (1, 31),
        (2, 25),
        (3, 24),
        (4, 26),
        (5, 32),
        (6, 22),
        (7, 24),
        (8, 22),
    ] {
        for n in 1..=count {
            text.push((
                v(Book::Genesis, chapter, n),
                format!(
                    "Genesis {chapter}:{n}. In the beginning God created the heavens and the \
                     earth, and the Spirit of God was hovering over the face of the waters."
                ),
            ));
        }
    }
    let verses: Vec<(Verse, &str)> = text.iter().map(|(v, t)| (*v, t.as_str())).collect();
    obs_engine::testing::bibles::write_library(dir, &[bible("bsb", "BSB", &verses, None, &[])]);
}

/// Launch on `dir`'s User database with BSB as the ★ Bible, on `today`.
fn launch(dir: &Path, today: &str) -> Harness {
    let mut h = Harness::launch_full(Some(&dir.join("user.db")), JobMode::Inline, Some(dir));
    let session = &mut h.shell.st.session;
    session.library.add_file(&dir.join("bsb.obr")).unwrap();
    session.library.set_preferred_order(vec!["bsb".into()]);
    session.pretend_today(PlanDate::parse(today).unwrap());
    settle_all(&mut h);
    h
}

/// Visual evidence for `docs/previews/<name>.png` when `UPDATE_PREVIEWS` is set.
fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

fn plan_tool(h: &Harness) -> Option<TabId> {
    h.shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::ReadingPlans))
        .map(|t| t.id)
}

/// The tick box left of a Reading's first Passage in the Tool.
fn tick(h: &mut Harness, passage: &str) {
    let r = h
        .find(passage)
        .unwrap_or_else(|| panic!("{passage:?} not on screen: {:?}", h.texts()));
    h.click(pos2(r.left() - 16.0, r.center().y));
}

#[test]
fn bible_in_a_year_from_today_shows_todays_passages_ticks_them_off_and_reschedules() {
    let dir = folder("today");
    write_bsb(&dir);
    let mut h = launch(&dir, "2026-11-05");
    assert!(!h.texts().iter().any(|t| t.starts_with("Today: ")));

    // Tools → Reading plans: with no plan yet, the New plan form is open.
    h.open_menu("Tools");
    h.click_text("Reading plans");
    settle_all(&mut h);
    let tool = plan_tool(&h).expect("the Reading plans Tool opened");
    assert_eq!(
        h.shell.st.session.workspace.tab(tool).unwrap().header_box(),
        obs_engine::HeaderBox::Keyless
    );
    for t in [
        "New Reading plan",
        "Bible in a year (canonical)",
        "Bible in a year (chronological)",
        "Bible in a year (M'Cheyne)",
        "New Testament in 90 days",
        "Psalms and Proverbs monthly",
        "Custom: chosen books over N days",
        "Mon",
        "Sun",
    ] {
        assert!(h.texts().contains(&t), "{t:?} missing: {:?}", h.texts());
    }
    preview(&h, "reading-plans-new");
    h.click_text("Start plan");
    settle_all(&mut h);

    // Bible in a year from today: today's Passages are listed, and on the toolbar.
    let plans = h.shell.st.session.reading_plans().to_vec();
    assert_eq!(plans.len(), 1);
    let plan = &plans[0];
    assert_eq!(plan.template, "bible-canonical");
    assert_eq!(plan.bible_id, "bsb");
    let today = PlanDate::parse("2026-11-05").unwrap();
    let first = plan.due_on(today)[0].label.clone();
    assert!(first.starts_with("Gen 1-"), "{first}");
    assert!(h.texts().contains(&first.as_str()), "{:?}", h.texts());
    let toolbar = format!("Today: {first}");
    assert!(h.texts().contains(&toolbar.as_str()), "{:?}", h.texts());
    assert!(!h.texts().iter().any(|t| t.starts_with("Behind by")));
    preview(&h, "reading-plans-today");

    // One click on the toolbar opens today's reading in BSB at Genesis 1.
    h.click_text(&toolbar);
    settle_all(&mut h);
    let bsb = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("bsb".into()))
        .map(|t| t.id)
        .expect("BSB opened");
    assert_eq!(location(&mut h, bsb), Some(v(Book::Genesis, 1, 1)));

    // Ticking it off advances the plan: today is done, tomorrow's Reading is next.
    h.shell.st.session.workspace.focus(tool);
    h.frames(3);
    tick(&mut h, &first);
    let plan = h.shell.st.session.reading_plans()[0].clone();
    assert_eq!(plan.read_count(), 1);
    assert!(plan.readings[0].read);
    assert!(h.texts().contains(&"✓ Today's reading"), "{:?}", h.texts());
    let tomorrow = plan.readings[1].label.clone();
    assert!(h.texts().contains(&tomorrow.as_str()));
    preview(&h, "reading-plans-ticked");

    // Three days go by unread: the plan is behind by three days.
    h.shell
        .st
        .session
        .pretend_today(PlanDate::parse("2026-11-09").unwrap());
    h.frames(3);
    let behind = h
        .texts()
        .iter()
        .filter(|t| **t == "Behind by 3 days")
        .count();
    assert_eq!(behind, 2, "toolbar and plan: {:?}", h.texts());
    assert!(h.texts().contains(&"MISSED"));
    preview(&h, "reading-plans-behind");

    // Reschedule spreads the rest over the days left, keeping the last day.
    let end = plan.end();
    h.click_text("Reschedule");
    let plan = h.shell.st.session.reading_plans()[0].clone();
    let now = PlanDate::parse("2026-11-09").unwrap();
    assert_eq!(plan.behind_days(now), 0);
    assert_eq!(plan.end(), end);
    assert!(plan.readings.iter().all(|r| r.read || r.day >= now));
    assert!(!h.texts().iter().any(|t| t.starts_with("Behind by")));
    let due: Vec<&str> = plan.due_on(now).iter().map(|r| r.label.as_str()).collect();
    assert!(!due.is_empty());
    let toolbar = format!("Today: {}", due.join("; "));
    assert!(h.texts().contains(&toolbar.as_str()), "{:?}", h.texts());
    preview(&h, "reading-plans-rescheduled");
    h.shell.st.session.save_last_session().unwrap();
    drop(h);

    // The plan and its progress are still there after a restart.
    let h = launch(&dir, "2026-11-09");
    let plans = h.shell.st.session.reading_plans();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].read_count(), 1);
    assert_eq!(plans[0].behind_days(now), 0);
    drop(h);
    let _ = std::fs::remove_dir_all(&dir);
}
