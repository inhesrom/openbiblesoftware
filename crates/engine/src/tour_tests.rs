//! The First-run tour (#218): it starts at the first launch with a User database, never again
//! unless replayed from Help, and its Coach marks step, go back and skip.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::tour::{TOUR_PREF, TOUR_STEPS, TourTarget};
use crate::*;

struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-tour-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn db(&self) -> PathBuf {
        self.0.join("user.db")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn launch(db: &Path) -> Session {
    let mut s = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(db).unwrap();
    s.attach_user(Box::new(store));
    s
}

fn step(s: &Session) -> Option<usize> {
    s.tour().step().map(|(i, _)| i)
}

/// The Coach marks the ticket asks for: 5–7 of them, pointing at the toolbar, the Library,
/// Link sets, the Passage Guide, Word Study on a right-click and Home.
#[test]
fn the_tour_points_at_the_toolbar_library_link_sets_passage_guide_word_study_and_home() {
    assert!((5..=7).contains(&TOUR_STEPS.len()));
    let targets: Vec<TourTarget> = TOUR_STEPS.iter().map(|s| s.target).collect();
    assert_eq!(
        targets,
        [
            TourTarget::Toolbar,
            TourTarget::Library,
            TourTarget::LinkSets,
            TourTarget::PassageGuide,
            TourTarget::WordStudy,
            TourTarget::Home,
        ]
    );
    for s in TOUR_STEPS {
        assert!(!s.title.is_empty() && s.text.len() < 200, "{s:?}");
    }
    assert!(TOUR_STEPS[4].text.contains("Right-click"));
}

/// Acceptance (#218): the tour runs on first launch and never again unless replayed.
#[test]
fn the_tour_runs_on_the_first_launch_and_never_again_unless_replayed() {
    let t = Temp::new("first-launch");
    let mut s = launch(&t.db());
    assert!(!s.tour().shown());
    assert!(s.start_tour_at_first_launch());
    assert_eq!(step(&s), Some(0));
    // Closed mid-tour: it was shown, so it does not come back.
    drop(s);

    for _ in 0..2 {
        let mut s = launch(&t.db());
        assert!(s.tour().shown());
        assert!(!s.start_tour_at_first_launch());
        assert!(!s.tour().is_running());
    }

    // Replayed from Help, as often as asked.
    let mut s = launch(&t.db());
    assert_eq!(crate::menu_of("help.tour"), Some("Help"));
    let out = s.run("help.tour", &json!({})).unwrap();
    assert_eq!(out["step"], 1);
    assert_eq!(out["of"], TOUR_STEPS.len());
    s.run("tour.skip", &json!({})).unwrap();
    assert!(!s.tour().is_running());
    s.run("help.tour", &json!({})).unwrap();
    assert_eq!(step(&s), Some(0));
    drop(s);
    assert!(!launch(&t.db()).start_tour_at_first_launch());
}

/// Skipping at the first Coach mark still counts as shown.
#[test]
fn skipping_the_tour_at_once_keeps_it_from_coming_back() {
    let t = Temp::new("skip");
    let mut s = launch(&t.db());
    assert!(s.start_tour_at_first_launch());
    s.run("tour.skip", &json!({})).unwrap();
    assert!(!s.tour().is_running());
    let raw = s.user.as_ref().unwrap().preference(TOUR_PREF).unwrap();
    assert_eq!(raw.as_deref(), Some(r#"{"shown":true}"#));
    drop(s);
    assert!(!launch(&t.db()).start_tour_at_first_launch());
}

/// Without a User database nothing could remember the tour, so it does not start by itself
/// (it would run at every launch); Help → Take the tour still runs it.
#[test]
fn without_a_user_database_the_tour_starts_only_when_asked() {
    let mut s = Session::new(JobMode::Inline);
    assert!(!s.start_tour_at_first_launch());
    assert!(!s.tour().is_running());
    s.run("help.tour", &json!({})).unwrap();
    assert_eq!(step(&s), Some(0));
}

#[test]
fn next_and_back_step_through_the_coach_marks_and_the_last_next_ends_the_tour() {
    let mut s = Session::new(JobMode::Inline);
    s.run("help.tour", &json!({})).unwrap();
    s.run("tour.back", &json!({})).unwrap();
    assert_eq!(step(&s), Some(0), "Back on the first stays there");
    for (i, step) in TOUR_STEPS.iter().enumerate().skip(1) {
        let out = s.run("tour.next", &json!({})).unwrap();
        assert_eq!(out["step"], i + 1);
        assert_eq!(out["title"], step.title);
    }
    s.run("tour.back", &json!({})).unwrap();
    assert_eq!(step(&s), Some(TOUR_STEPS.len() - 2));
    s.run("tour.next", &json!({})).unwrap();
    let out = s.run("tour.next", &json!({})).unwrap();
    assert!(out["step"].is_null());
    assert!(!s.tour().is_running());
    // Stepping with no tour running does nothing.
    s.run("tour.next", &json!({})).unwrap();
    s.run("tour.back", &json!({})).unwrap();
    assert!(!s.tour().is_running());
}

/// The Coach mark buttons are in-place commands, not offered by the Command box; Take the tour
/// is.
#[test]
fn only_take_the_tour_is_listed_in_the_command_box() {
    let s = Session::new(JobMode::Inline);
    let listed = |id: &str| crate::command_box::is_listed(s.commands().get(id).unwrap());
    assert!(listed("help.tour"));
    assert!(listed("help.user_guide"));
    for id in ["tour.next", "tour.back", "tour.skip"] {
        assert!(!listed(id), "{id}");
    }
}

/// At the first launch Home covers the Bible; the Coach marks about a Bible show the most recent
/// Bible Tab, so they point at its Link set and its text.
#[test]
fn the_coach_marks_about_a_bible_show_one_over_home() {
    use crate::testing::bibles::{bible, v, write_library};
    use obs_scripture::Book;

    let t = Temp::new("bible-over-home");
    let verses = [(v(Book::John, 3, 16), "For God so loved the world.")];
    write_library(
        &t.0.join("library"),
        &[bible("bsb", "BSB", &verses, None, &[])],
    );
    let mut s = Session::new(JobMode::Inline);
    assert!(s.library.scan(&t.0.join("library")).is_empty());
    let bsb = TabId(
        s.run("resource.open", &json!({ "id": "bsb" })).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    let home = TabId(
        s.run("home.open", &json!({})).unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    s.run("help.tour", &json!({})).unwrap();
    assert_eq!(s.workspace.focused(), Some(home));
    s.run("tour.next", &json!({})).unwrap();
    assert_eq!(
        s.workspace.focused(),
        Some(home),
        "the Library step leaves Home"
    );
    s.run("tour.next", &json!({})).unwrap();
    assert_eq!(TOUR_STEPS[step(&s).unwrap()].target, TourTarget::LinkSets);
    assert_eq!(s.workspace.focused(), Some(bsb));
    // The user goes back to Home; Word Study shows the Bible again.
    s.workspace.focus(home);
    s.run("tour.next", &json!({})).unwrap();
    s.run("tour.next", &json!({})).unwrap();
    assert_eq!(TOUR_STEPS[step(&s).unwrap()].target, TourTarget::WordStudy);
    assert_eq!(s.workspace.focused(), Some(bsb));
}
