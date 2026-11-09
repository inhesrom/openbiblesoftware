//! The First-run tour's Coach marks (#218) through the headless harness: at the first launch
//! over Home and never again, each Coach mark ringing what it explains, Next, Back, Done, Skip
//! tour and Esc, and a target that is not on screen.

use std::path::{Path, PathBuf};

use obs_engine::{TOUR_STEPS, TourTarget};

use super::*;

struct Lab(PathBuf);

impl Lab {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("obs-tour-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn db(&self) -> PathBuf {
        self.0.join("user.db")
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn preview(h: &Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews");
        std::fs::create_dir_all(&dir).unwrap();
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

/// Launch as the app does: the User database at `db`, the Last session (or the default
/// layout), Home on the first run, and the tour.
fn launch(db: &Path) -> Harness {
    let mut session = Session::new(JobMode::Inline);
    let (store, _) = obs_store::UserStore::open(db).unwrap();
    session.attach_user(Box::new(store));
    let mut shell = Shell::new(session);
    for doc in [bible(), hebrew_doc()] {
        add_doc(&mut shell, doc);
    }
    shell.restore_at_launch();
    let mut h = Harness::from_shell(shell);
    h.frames(5);
    h
}

fn step(h: &Harness) -> Option<usize> {
    h.shell.st.session.tour().step().map(|(i, _)| i)
}

/// The Coach mark card drawn last frame, and what it pointed at.
fn card(h: &Harness) -> (Option<TourTarget>, Rect) {
    h.shell.st.tour_ui.drawn.expect("a Coach mark is drawn")
}

/// Click the Coach mark's button `label`.
fn press(h: &mut Harness, label: &str) {
    let (_, rect) = card(h);
    let at = h
        .shapes
        .iter()
        .find(|(r, t)| t == label && rect.contains(r.center()))
        .map(|(r, _)| r.center())
        .unwrap_or_else(|| panic!("no {label:?} on the Coach mark: {:?}", h.texts()));
    h.click(at);
}

/// Acceptance (#218): the tour runs on first launch and never again unless replayed.
#[test]
fn the_tour_runs_over_home_at_the_first_launch_and_never_again_unless_replayed() {
    let lab = Lab::new("first");
    let mut h = launch(&lab.db());
    assert!(
        (h.shell.st.session.workspace.tabs()).any(|t| t.kind == TabKind::Home),
        "the first launch opens Home"
    );
    assert_eq!(step(&h), Some(0));
    let (target, _) = card(&h);
    assert_eq!(target, Some(TourTarget::Toolbar));
    assert!(
        h.find(&format!("Tour · 1 of {}", TOUR_STEPS.len()))
            .is_some()
    );
    assert!(h.find(TOUR_STEPS[0].title).is_some());
    preview(&h, "tour-first-launch-kittest");
    // Quit in the middle of it.
    press(&mut h, "Next");
    assert_eq!(step(&h), Some(1));
    h.shell.save_session(true);
    drop(h);

    for _ in 0..2 {
        let h = launch(&lab.db());
        assert_eq!(step(&h), None, "the tour came back");
        assert!(h.shell.st.tour_ui.drawn.is_none());
    }

    // Help → Take the tour replays it.
    let mut h = launch(&lab.db());
    h.open_menu("Help");
    h.click_text("Take the tour");
    h.frames(2);
    assert_eq!(step(&h), Some(0));
}

/// Each Coach mark rings what it explains and sits beside it, not over it; Next steps on, Back
/// steps back, and Done on the last ends the tour.
#[test]
fn each_coach_mark_points_at_what_it_explains() {
    let mut h = Harness::new();
    h.shell.run("help.tour", &json!({}));
    h.frames(3);
    for (i, s) in TOUR_STEPS.iter().enumerate() {
        assert_eq!(step(&h), Some(i));
        let (target, rect) = card(&h);
        assert_eq!(target, Some(s.target), "{} is not on screen", s.title);
        let at = h.shell.st.target_rect(s.target).unwrap();
        assert!(
            !rect.intersects(at.shrink(2.0)) || s.target == TourTarget::WordStudy,
            "{}: the card {rect:?} covers {at:?}",
            s.title
        );
        assert!(h.find(s.title).is_some(), "{:?}", h.texts());
        if s.target == TourTarget::LinkSets {
            preview(&h, "tour-link-sets-kittest");
        }
        if i == 1 {
            press(&mut h, "Back");
            assert_eq!(step(&h), Some(0));
            press(&mut h, "Next");
        }
        let last = i + 1 == TOUR_STEPS.len();
        press(&mut h, if last { "Done" } else { "Next" });
    }
    assert_eq!(step(&h), None);
    assert!(h.shell.st.tour_ui.drawn.is_none());
}

/// The Library Coach mark points at the Library Launcher; Word Study at the Bible's text.
#[test]
fn the_launcher_coach_marks_ring_their_launchers() {
    let mut h = Harness::new();
    h.shell.run("help.tour", &json!({}));
    h.shell.run("tour.next", &json!({}));
    h.frames(2);
    let library = h.shell.st.target_rect(TourTarget::Library).unwrap();
    assert!(
        (h.shapes.iter()).any(|(r, t)| t == "Library" && library.contains(r.center())),
        "{library:?}"
    );
    let (_, rect) = card(&h);
    assert!(
        rect.top() > library.bottom(),
        "the card is under the Launcher"
    );
    h.shell.run("tour.next", &json!({}));
    h.shell.run("tour.next", &json!({}));
    h.frames(2);
    let guides = h.shell.st.target_rect(TourTarget::PassageGuide).unwrap();
    assert!(
        h.shapes
            .iter()
            .any(|(r, t)| t == "Guides" && guides.contains(r.center()))
    );
    h.shell.run("tour.next", &json!({}));
    h.frames(2);
    let text = h.shell.st.target_rect(TourTarget::WordStudy).unwrap();
    let tb = h.tab_id("TB");
    let tiles = &h.shell.tree.tiles;
    let pane = tiles.rect(tiles.find_pane(&tb).unwrap()).unwrap();
    assert!(pane.contains_rect(text));
}

#[test]
fn skip_tour_and_esc_end_the_tour() {
    let mut h = Harness::new();
    h.shell.run("help.tour", &json!({}));
    h.frames(2);
    press(&mut h, "Skip tour");
    assert_eq!(step(&h), None);
    h.shell.run("help.tour", &json!({}));
    h.shell.run("tour.next", &json!({}));
    h.frames(2);
    h.events.push(Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(2);
    assert_eq!(step(&h), None);
}

fn escape(h: &mut Harness) {
    for pressed in [true, false] {
        h.events.push(Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.frame();
    }
    h.frames(2);
}

/// Esc closes what is open over the window first (a Launcher's pane the Library Coach mark
/// invites a click on, a window, the Command box with the keyboard), and the tour stays at
/// its Coach mark; with nothing open, the next Esc skips the tour.
#[test]
fn esc_closes_a_pane_window_or_field_before_it_skips_the_tour() {
    let mut h = Harness::new();
    h.shell.run("help.tour", &json!({}));
    h.shell.run("tour.next", &json!({}));
    h.frames(2);
    assert_eq!(
        h.shell.st.session.tour().step().unwrap().1.target,
        TourTarget::Library
    );
    let library = h.shell.st.target_rect(TourTarget::Library).unwrap();
    h.click(library.center());
    h.frames(2);
    assert_eq!(h.shell.st.toolbar.open, Some(obs_engine::Launcher::Library));
    escape(&mut h);
    assert_eq!(h.shell.st.toolbar.open, None, "Esc closes the Library pane");
    assert_eq!(step(&h), Some(1), "and the tour stays at its Coach mark");

    h.shell.run("view.fonts", &json!({}));
    h.frames(2);
    assert!(h.shell.st.fonts_ui.open);
    escape(&mut h);
    assert!(!h.shell.st.fonts_ui.open, "Esc closes the Fonts window");
    assert_eq!(step(&h), Some(1));

    h.shell.run("command_box.focus", &json!({}));
    h.frames(2);
    assert!(
        h.ctx.text_edit_focused(),
        "the Command box has the keyboard"
    );
    escape(&mut h);
    assert!(!h.ctx.text_edit_focused(), "Esc leaves the Command box");
    assert_eq!(step(&h), Some(1));

    escape(&mut h);
    assert_eq!(step(&h), None, "with nothing open, Esc skips the tour");
}

/// A target not on screen (the toolbar Auto-hidden, so no Launchers drawn) puts its Coach mark
/// in the middle of the window.
#[test]
fn a_coach_mark_whose_target_is_hidden_shows_in_the_middle_of_the_window() {
    let mut h = Harness::new();
    h.shell.run("view.toolbar_auto_hide", &json!({}));
    h.events.push(Event::PointerMoved(pos2(700.0, 500.0)));
    h.frames(10);
    h.shell.run("help.tour", &json!({}));
    h.shell.run("tour.next", &json!({}));
    h.frames(3);
    assert_eq!(
        h.shell.st.session.tour().step().unwrap().1.target,
        TourTarget::Library
    );
    let (target, rect) = card(&h);
    assert_eq!(target, None);
    let middle = pos2(700.0, 450.0);
    assert!(
        (rect.center() - middle).length() < 2.0,
        "{rect:?} is not in the middle"
    );
}
