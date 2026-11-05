//! Passage Guide (#53) through the shell's real-event headless harness: sections fill in as
//! their jobs finish, the Guide arrangement is shared and persisted, empty-state hints and
//! navigation rules.

use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use obs_engine::{
    Guide, GuideSection, JobContext, Library, LibrarySections, SectionData, SectionSource, TabId,
};
use obs_model::Key;
use obs_scripture::{Book, Passage, Verse};

use super::*;

fn john(c: u16, v: u16) -> Verse {
    Verse::new(Book::John, c, v)
}

struct Files(std::path::PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn add_file(h: &mut Harness, files: &Files, resource: &obs_model::Resource) {
    let path = files
        .0
        .join(format!("{}.obr", resource.manifest.resource_id));
    obs_store::write_obr(resource, &path).unwrap();
    h.shell.st.session.library.add_file(&path).unwrap();
}

/// A library of real `.obr` files: the KJV sample Bible, the OpenBible.info excerpt and
/// (when `with_commentaries`) two synthetic Commentaries on John 3. The Bible is at Jn 3:16
/// in Link set A; returns it.
fn setup(h: &mut Harness, name: &str, with_commentaries: bool) -> (Files, TabId) {
    let dir = std::env::temp_dir().join(format!("obs-shell-guide-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    h.shell.st.session.library = Library::new();
    let options = obs_formats::DatasetOptions {
        resource_id: "openbible".into(),
        title: "OpenBible.info".into(),
        ..Default::default()
    };
    let (mut references, _) = obs_formats::import_openbible_cross_references(
        include_str!("../../formats/tests/assets/openbible/cross-references-excerpt.txt"),
        &options,
    )
    .unwrap();
    // Low-voted targets inside the sample Bible, so clicks land on text it has.
    for n in 17..21 {
        references.cross_references.push(obs_model::CrossReference {
            from: Passage::verse(john(3, 16)),
            to: Passage::verse(john(3, n)),
            votes: 21 - i32::from(n),
        });
    }
    references.cross_references.sort();
    references.manifest.content_hash = references.content_hash();
    add_file(h, &files, &obs_store::sample_resource());
    add_file(h, &files, &references);
    if with_commentaries {
        let chapter = Passage::new(john(3, 1), john(3, 21)).unwrap();
        add_file(
            h,
            &files,
            &obs_engine::testing::commentary(
                "alpha",
                "Alpha Commentary",
                "ALPHA",
                &[(
                    chapter,
                    Some("The Night Visit"),
                    "A teacher comes by night with questions about new birth and the kingdom.",
                )],
            ),
        );
        add_file(
            h,
            &files,
            &obs_engine::testing::commentary(
                "beta",
                "Beta Notes",
                "BETA",
                &[(
                    Passage::verse(john(3, 16)),
                    None,
                    "16. Love, gift, belief and life together in one verse.",
                )],
            ),
        );
    }
    h.shell.reset_layout();
    let bible = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("kjv-sample".into()))
        .unwrap()
        .id;
    h.shell
        .run("tab.go", &json!({ "tab": bible.0, "text": "Jn 3:16" }));
    h.frames(2);
    (files, bible)
}

fn open_guide(h: &mut Harness) -> TabId {
    let tab = h
        .shell
        .run("guide.passage_guide", &json!({ "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    h.frames(2);
    TabId(tab)
}

/// Paced frames until `done` holds (worker results arrive between frames).
fn until(h: &mut Harness, what: &str, done: impl Fn(&Harness) -> bool) {
    for _ in 0..600 {
        if done(h) {
            return;
        }
        h.paced_frame();
    }
    panic!("never saw {what}: {:?}", h.texts());
}

fn pane(h: &Harness, tab: TabId) -> Rect {
    let tiles = &h.shell.tree.tiles;
    let id = tiles.find_pane(&tab).unwrap();
    tiles.rect(id).unwrap()
}

/// Where `text` is painted inside a Tab's pane.
fn find_in(h: &Harness, tab: TabId, text: &str) -> Option<Rect> {
    let r = pane(h, tab);
    h.shapes
        .iter()
        .find(|(s, t)| t == text && r.contains(s.center()))
        .map(|(s, _)| *s)
}

fn has_prefix_in(h: &Harness, tab: TabId, prefix: &str) -> bool {
    let r = pane(h, tab);
    h.shapes
        .iter()
        .any(|(s, t)| t.starts_with(prefix) && r.contains(s.center()))
}

/// Section header titles in a Guide, top to bottom.
fn headers(h: &Harness, tab: TabId) -> Vec<String> {
    let r = pane(h, tab);
    let mut found: Vec<(f32, String)> = h
        .shapes
        .iter()
        .filter(|(s, t)| GuideSection::ALL.iter().any(|g| g.title() == t) && r.contains(s.center()))
        .map(|(s, t)| (s.top(), t.clone()))
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, t)| t).collect()
}

/// Holds Commentaries until released; Cross-references come from the library.
#[derive(Debug)]
struct HeldCommentaries {
    entered: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
    inner: LibrarySections,
}
impl SectionSource for HeldCommentaries {
    fn compute(
        &self,
        library: &Library,
        key: &Key,
        section: GuideSection,
        ctx: &JobContext,
    ) -> Result<SectionData, String> {
        if section == GuideSection::Commentaries {
            self.entered.lock().unwrap().send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(20))
                .unwrap();
        }
        self.inner.compute(library, key, section, ctx)
    }
}

#[test]
fn sections_appear_as_they_finish_without_blocking_frames() {
    let mut h = Harness::launch_with(None, JobMode::Threads(2));
    let (_files, _bible) = setup(&mut h, "held", true);
    let (entered, held) = mpsc::channel();
    let (release, rx) = mpsc::channel();
    h.shell
        .st
        .session
        .guides
        .set_source(Arc::new(HeldCommentaries {
            entered: Mutex::new(entered),
            release: Mutex::new(rx),
            inner: LibrarySections::default(),
        }));
    let guide = open_guide(&mut h);
    assert_eq!(
        h.shell.session().workspace.tab(guide).unwrap().link,
        Some('A')
    );
    assert!(find_in(&h, guide, "John 3:16").is_some(), "{:?}", h.texts());
    held.recv_timeout(Duration::from_secs(20)).unwrap();

    // The Commentaries job is held on a worker. Frames keep coming at interactive speed and
    // Cross-references fill in on their own while Commentaries still say Loading.
    let mut saw_references = false;
    for _ in 0..600 {
        let t = Instant::now();
        h.paced_frame();
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "a frame blocked on a Guide section"
        );
        if find_in(&h, guide, "John 11:25-26").is_some() {
            saw_references = true;
            break;
        }
    }
    assert!(saw_references, "{:?}", h.texts());
    assert!(find_in(&h, guide, "Loading…").is_some(), "{:?}", h.texts());
    assert!(find_in(&h, guide, "Alpha Commentary").is_none());
    assert_eq!(headers(&h, guide), ["Commentaries", "Cross-references"]);
    // Menus still answer while the section is held.
    h.open_menu("Guides");
    assert!(h.find("Word Study").is_some());
    h.events.push(Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(2);

    release.send(()).unwrap();
    until(&mut h, "the Commentaries rows", |h| {
        find_in(h, guide, "Alpha Commentary").is_some() && find_in(h, guide, "Beta Notes").is_some()
    });
    assert!(find_in(&h, guide, "Loading…").is_none());
    // One row per Commentary, in Preferred order, with its section reference.
    let alpha = find_in(&h, guide, "Alpha Commentary").unwrap();
    let beta = find_in(&h, guide, "Beta Notes").unwrap();
    assert!(alpha.top() < beta.top());
    assert!(find_in(&h, guide, "John 3:1-21").is_some());
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/guide");
        h.frames(30);
        h.screenshot(&dir.join("passage-guide.ppm"));
    }
    // The Session holds the .obr files open; close it before `Files` deletes them.
    drop(h);
}

#[test]
fn with_no_commentaries_in_the_library_the_section_shows_its_hint() {
    let mut h = Harness::new();
    let (_files, _bible) = setup(&mut h, "hint", false);
    let guide = open_guide(&mut h);
    until(&mut h, "the Commentaries hint", |h| {
        has_prefix_in(h, guide, "None in your library. Add a Commentary")
    });
    // Cross-references, which the library has, still fill in beside it.
    until(&mut h, "Cross-references", |h| {
        find_in(h, guide, "John 11:25-26").is_some()
    });
    drop(h);
}

#[test]
fn arrangement_changes_apply_to_every_passage_guide_and_persist_across_restarts() {
    let db = temp_db("guide-arrangement");
    let mut h = Harness::launch(Some(&db));
    let first = open_guide(&mut h);
    let second = open_guide(&mut h);
    h.frames(4);
    assert_eq!(headers(&h, first), ["Commentaries", "Cross-references"]);
    assert_eq!(headers(&h, second), ["Commentaries", "Cross-references"]);

    // Drag the Cross-references header above Commentaries in the first Guide.
    let from = find_in(&h, first, "Cross-references").unwrap().center();
    let to = find_in(&h, first, "Commentaries").unwrap().center() - vec2(0.0, 20.0);
    h.drag(from, to);
    h.frames(2);
    assert_eq!(headers(&h, first), ["Cross-references", "Commentaries"]);
    assert_eq!(
        headers(&h, second),
        ["Cross-references", "Commentaries"],
        "the arrangement is per kind of Guide, not per Tab"
    );

    // Collapse Commentaries from the second Guide: its hint disappears from both.
    let hint = "None in your library. Add a Commentary";
    until(&mut h, "the hint in both", |h| {
        has_prefix_in(h, first, hint) && has_prefix_in(h, second, hint)
    });
    let header = find_in(&h, second, "Commentaries").unwrap().center();
    h.click(header);
    h.frames(2);
    assert!(!has_prefix_in(&h, first, hint));
    assert!(!has_prefix_in(&h, second, hint));
    assert_eq!(headers(&h, first), ["Cross-references", "Commentaries"]);

    // Quit and relaunch: the Guides come back with the same arrangement.
    h.shell.save_session(true);
    drop(h);
    let mut h = Harness::launch(Some(&db));
    h.frames(4);
    let guides: Vec<TabId> = h
        .shell
        .session()
        .workspace
        .tabs()
        .filter(|t| t.kind == TabKind::Guide(Guide::PassageGuide))
        .map(|t| t.id)
        .collect();
    assert_eq!(guides.len(), 2);
    for g in &guides {
        assert_eq!(headers(&h, *g), ["Cross-references", "Commentaries"]);
        assert!(!has_prefix_in(&h, *g, hint));
    }

    // Hide from a header's ×, show again from the footer, then Reset arrangement from ⋯.
    let title = find_in(&h, guides[0], "Cross-references").unwrap();
    let hide = h
        .shapes
        .iter()
        .find(|(r, t)| {
            t == "×"
                && pane(&h, guides[0]).contains(r.center())
                && (r.center().y - title.center().y).abs() < 6.0
        })
        .unwrap()
        .0
        .center();
    h.click(hide);
    h.frames(2);
    assert_eq!(headers(&h, guides[1]), ["Commentaries"]);
    let show = find_in(&h, guides[1], "Show Cross-references")
        .unwrap()
        .center();
    h.click(show);
    h.frames(2);
    assert_eq!(headers(&h, guides[1]), ["Cross-references", "Commentaries"]);
    let menu = find_in(&h, guides[0], "…").unwrap().center();
    h.click(menu);
    h.click_text("Reset arrangement");
    h.frames(2);
    for g in &guides {
        assert_eq!(headers(&h, *g), ["Commentaries", "Cross-references"]);
        assert!(has_prefix_in(&h, *g, hint) || find_in(&h, *g, "Loading…").is_some());
    }
    drop(h);
    let _ = std::fs::remove_dir_all(db.parent().unwrap());
}

#[test]
fn clicks_follow_the_navigation_rules() {
    let mut h = Harness::new();
    let (_files, bible) = setup(&mut h, "navigate", true);
    let guide = open_guide(&mut h);
    // Cross-references first, so the low-voted 3:17 row is on screen.
    h.shell.run(
        "guide.move_section",
        &json!({ "guide": "passage_guide", "section": "cross_references", "to": 0 }),
    );
    until(&mut h, "rows", |h| {
        find_in(h, guide, "Alpha Commentary").is_some() && find_in(h, guide, "John 3:17").is_some()
    });

    // A Cross-reference click navigates the Guide's Link set: the Bible follows.
    let target = find_in(&h, guide, "John 3:17").unwrap().center();
    h.click(target);
    h.frames(2);
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tab(bible).unwrap().history.current(), Some(john(3, 17)));
    assert_eq!(ws.tab(guide).unwrap().history.current(), Some(john(3, 17)));
    h.shell
        .run("tab.go", &json!({ "tab": bible.0, "text": "Jn 3:16" }));
    h.frames(3);
    until(&mut h, "rows again", |h| {
        find_in(h, guide, "Alpha Commentary").is_some()
    });

    // Hovering a Commentary row shows its popup; clicking opens that Commentary at the
    // section in the Guide's Link set, without moving the Guide or the Bible.
    let row = find_in(&h, guide, "Alpha Commentary").unwrap().center();
    h.events.push(Event::PointerMoved(row));
    h.frames(3);
    assert!(
        h.find("Click to open this section · Ctrl/Cmd-click opens a new Tab")
            .is_some(),
        "{:?}",
        h.texts()
    );
    let tabs = h.shell.session().workspace.tabs().count();
    h.click(row);
    h.frames(3);
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tabs().count(), tabs + 1);
    let alpha = ws.focused().unwrap();
    let t = ws.tab(alpha).unwrap();
    assert_eq!(t.kind, TabKind::Resource("alpha".into()));
    assert_eq!(t.link, Some('A'));
    assert_eq!(t.history.current(), Some(john(3, 1)));
    assert_eq!(ws.tab(guide).unwrap().history.current(), Some(john(3, 16)));
    assert_eq!(ws.tab(bible).unwrap().history.current(), Some(john(3, 16)));

    // A plain click again reuses that Tab; Ctrl/Cmd-click opens another.
    h.shell.session_mut().workspace.focus(guide);
    h.shell.apply_effects();
    h.frames(2);
    let row = find_in(&h, guide, "Alpha Commentary").unwrap().center();
    h.click(row);
    h.frames(2);
    assert_eq!(h.shell.session().workspace.tabs().count(), tabs + 1);
    assert_eq!(h.shell.session().workspace.focused(), Some(alpha));
    h.shell.session_mut().workspace.focus(guide);
    h.shell.apply_effects();
    h.frames(2);
    let row = find_in(&h, guide, "Alpha Commentary").unwrap().center();
    h.modifiers = Modifiers::COMMAND;
    h.click(row);
    h.modifiers = Modifiers::NONE;
    h.frames(2);
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tabs().count(), tabs + 2);
    assert_ne!(ws.focused(), Some(alpha));
    drop(h);
}

#[test]
fn the_guide_box_takes_a_passage_range() {
    let mut h = Harness::new();
    let (_files, _bible) = setup(&mut h, "range", true);
    let guide = open_guide(&mut h);
    h.shell.st.session.workspace.user_focused(guide);
    // Type a range in the Guide's own box (the topmost "John 3:16"), as a user would.
    let boxed = h
        .shapes
        .iter()
        .filter(|(r, t)| t == "John 3:16" && pane(&h, guide).contains(r.center()))
        .map(|(r, _)| *r)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap();
    h.click(boxed.center());
    h.events.push(Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.events.push(Event::Text("Jn 3:16-17".into()));
    h.frame();
    h.events.push(Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(3);
    let range = Passage::new(john(3, 16), john(3, 17)).unwrap();
    until(&mut h, "the range as the Guide's Passage", |h| {
        find_in(h, guide, "John 3:16-17").is_some()
    });
    assert_eq!(h.shell.st.tabs[&guide].reference_passage, Some(range));
    // The range's Commentaries: Beta's 3:16 note and Alpha's 3:1-21 section both overlap.
    until(&mut h, "Commentary rows for the range", |h| {
        find_in(h, guide, "Alpha Commentary").is_some() && find_in(h, guide, "Beta Notes").is_some()
    });
    drop(h);
}
