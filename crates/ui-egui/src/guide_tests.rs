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
    assert!(
        find_in(&h, guide, "Loading Commentaries…").is_some(),
        "{:?}",
        h.texts()
    );
    assert!(find_in(&h, guide, "Alpha Commentary").is_none());
    assert_eq!(
        headers(&h, guide)[..2],
        ["Commentaries", "Cross-references"]
    );
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
    assert!(find_in(&h, guide, "Loading Commentaries…").is_none());
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
    assert_eq!(
        headers(&h, first),
        [
            "Commentaries",
            "Cross-references",
            "Parallel passages",
            "Key words",
            "People & places"
        ]
    );
    assert_eq!(
        headers(&h, second),
        [
            "Commentaries",
            "Cross-references",
            "Parallel passages",
            "Key words",
            "People & places"
        ]
    );

    // Drag the Cross-references header above Commentaries in the first Guide.
    let from = find_in(&h, first, "Cross-references").unwrap().center();
    let to = find_in(&h, first, "Commentaries").unwrap().center() - vec2(0.0, 20.0);
    h.drag(from, to);
    h.frames(2);
    assert_eq!(
        headers(&h, first),
        [
            "Cross-references",
            "Commentaries",
            "Parallel passages",
            "Key words",
            "People & places"
        ]
    );
    assert_eq!(
        headers(&h, second),
        [
            "Cross-references",
            "Commentaries",
            "Parallel passages",
            "Key words",
            "People & places"
        ],
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
    assert_eq!(
        headers(&h, first),
        [
            "Cross-references",
            "Commentaries",
            "Parallel passages",
            "Key words",
            "People & places"
        ]
    );

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
        assert_eq!(
            headers(&h, *g),
            [
                "Cross-references",
                "Commentaries",
                "Parallel passages",
                "Key words",
                "People & places"
            ]
        );
        assert!(!has_prefix_in(&h, *g, hint));
    }

    // Hide from a header's Close, show again from the footer, then Reset arrangement from
    // the More menu.
    let title = find_in(&h, guides[0], "Cross-references").unwrap();
    let hide = h
        .shapes
        .iter()
        .find(|(r, t)| {
            t == "[x]"
                && pane(&h, guides[0]).contains(r.center())
                && (r.center().y - title.center().y).abs() < 6.0
        })
        .unwrap()
        .0
        .center();
    h.click(hide);
    h.frames(2);
    assert_eq!(
        headers(&h, guides[1]),
        [
            "Commentaries",
            "Parallel passages",
            "Key words",
            "People & places"
        ]
    );
    let show = find_in(&h, guides[1], "Show Cross-references")
        .unwrap()
        .center();
    h.click(show);
    h.frames(2);
    assert_eq!(
        headers(&h, guides[1]),
        [
            "Cross-references",
            "Commentaries",
            "Parallel passages",
            "Key words",
            "People & places"
        ]
    );
    let menu = find_in(&h, guides[0], "[ellipsis]").unwrap().center();
    h.click(menu);
    h.click_text("Reset arrangement");
    h.frames(2);
    for g in &guides {
        assert_eq!(
            headers(&h, *g),
            [
                "Commentaries",
                "Cross-references",
                "Parallel passages",
                "Key words",
                "People & places"
            ]
        );
        assert!(has_prefix_in(&h, *g, hint) || has_prefix_in(&h, *g, "Loading "));
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
    assert_eq!(
        h.shell.st.session.workspace.tab(guide).unwrap().passage,
        Some(range)
    );
    // The range's Commentaries: Beta's 3:16 note and Alpha's 3:1-21 section both overlap.
    until(&mut h, "Commentary rows for the range", |h| {
        find_in(h, guide, "Alpha Commentary").is_some() && find_in(h, guide, "Beta Notes").is_some()
    });
    drop(h);
}

/// A library of `.obr` files from the #54 fixtures; the first Bible goes to `reference` in
/// Link set A and a Passage Guide opens beside it with `section` moved to the top. Returns
/// the Bible and the Guide.
fn setup_words(
    h: &mut Harness,
    name: &str,
    resources: &[obs_model::Resource],
    reference: &str,
    section: GuideSection,
) -> (Files, TabId, TabId) {
    let dir = std::env::temp_dir().join(format!("obs-shell-words-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    h.shell.st.session.library = Library::new();
    for r in resources {
        add_file(h, &files, r);
    }
    h.shell.reset_layout();
    let id = resources[0].manifest.resource_id.clone();
    let bible = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource(id.clone()))
        .unwrap()
        .id;
    h.shell
        .run("tab.go", &json!({ "tab": bible.0, "text": reference }));
    h.frames(2);
    h.shell.run(
        "guide.move_section",
        &json!({ "guide": "passage_guide", "section": section.id(), "to": 0 }),
    );
    let guide = open_guide(h);
    (files, bible, guide)
}

#[test]
fn key_words_lead_with_rare_words_and_a_click_opens_a_word_study() {
    use obs_engine::testing::{greek_lexicon, kjv_john, tagnt_john_3_16};
    let mut h = Harness::new();
    let (_files, _bible, guide) = setup_words(
        &mut h,
        "key-words",
        &[kjv_john(), tagnt_john_3_16(), greek_lexicon()],
        "Jn 3:16",
        GuideSection::KeyWords,
    );
    until(&mut h, "the Key words rows", |h| {
        find_in(h, guide, "agapao").is_some() && find_in(h, guide, "kosmos").is_some()
    });
    // Let the Lemma snippets finish laying out, so rows stay where they are.
    for _ in 0..20 {
        h.paced_frame();
    }
    // Rarest first (κόσμος, four times in this Bible, after the words found once); the
    // 13th and most common, θεός, waits behind Show all; function words are left out.
    let love = find_in(&h, guide, "agapao").unwrap();
    let world = find_in(&h, guide, "kosmos").unwrap();
    assert!(love.top() < world.top());
    assert!(
        find_in(&h, guide, "1× · 4 in KJV").is_some(),
        "{:?}",
        h.texts()
    );
    assert!(find_in(&h, guide, "theos").is_none());
    // Scroll the Guide so Show all is in view, show the rest, and scroll back.
    let wheel = |h: &mut Harness, dy: f32| {
        h.events.push(Event::PointerMoved(pane(h, guide).center()));
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, dy),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        });
        h.frames(4);
    };
    wheel(&mut h, -400.0);
    // Let the sections below finish loading, so the scroll offset stays put.
    for _ in 0..20 {
        h.paced_frame();
    }
    let all = find_in(&h, guide, "Show all 13").unwrap().center();
    h.click(all);
    h.frames(2);
    assert!(
        find_in(&h, guide, "Show fewer").is_some(),
        "{:?}",
        h.texts()
    );
    let world = find_in(&h, guide, "kosmos").unwrap();
    let god = find_in(&h, guide, "theos").unwrap();
    assert!(world.top() < god.top());
    assert!(find_in(&h, guide, "1× · 5 in KJV").is_some());
    wheel(&mut h, 2000.0);
    assert!(find_in(&h, guide, "to love").is_some());
    assert!(find_in(&h, guide, "gar").is_none(), "γάρ is a conjunction");
    assert!(has_prefix_in(&h, guide, "7 function words left out"));
    assert!(
        headers(&h, guide).first().map(String::as_str) == Some("Key words"),
        "{:?}",
        headers(&h, guide)
    );
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/guide");
        h.frames(30);
        h.screenshot(&dir.join("key-words.ppm"));
    }

    // A click opens a Word Study at the word's Strong's number; the Guide stays put.
    let tabs = h.shell.session().workspace.tabs().count();
    h.click(love.center());
    h.frames(2);
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tabs().count(), tabs + 1);
    let study = ws.focused().unwrap();
    let t = ws.tab(study).unwrap();
    assert_eq!(t.kind, TabKind::Guide(Guide::WordStudy));
    assert_eq!(
        t.entry_history.current(),
        Some(Key::StrongsNumber("G25".parse().unwrap()))
    );
    assert_eq!(ws.tab(guide).unwrap().history.current(), Some(john(3, 16)));

    // Another word reuses that Word Study; Ctrl/Cmd-click opens a new one.
    h.shell.session_mut().workspace.focus(guide);
    h.shell.apply_effects();
    h.frames(2);
    let only = find_in(&h, guide, "monogenes").unwrap().center();
    h.click(only);
    h.frames(2);
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tabs().count(), tabs + 1);
    assert_eq!(
        ws.tab(study).unwrap().entry_history.current(),
        Some(Key::StrongsNumber("G3439".parse().unwrap()))
    );
    h.shell.session_mut().workspace.focus(guide);
    h.shell.apply_effects();
    h.frames(2);
    let world = find_in(&h, guide, "kosmos").unwrap().center();
    h.modifiers = Modifiers::COMMAND;
    h.click(world);
    h.modifiers = Modifiers::NONE;
    h.frames(2);
    assert_eq!(h.shell.session().workspace.tabs().count(), tabs + 2);
    drop(h);
}

#[test]
fn gen_12_1_lists_abram_and_its_link_opens_eastons_abraham() {
    use obs_engine::testing::{dictionary, kjv_genesis_12, tipnr};
    let mut h = Harness::new();
    let (_files, bible, guide) = setup_words(
        &mut h,
        "people",
        &[
            kjv_genesis_12(),
            tipnr(),
            dictionary("easton"),
            dictionary("smith"),
        ],
        "Gen 12:1",
        GuideSection::PeopleAndPlaces,
    );
    until(&mut h, "Abram's row", |h| {
        find_in(h, guide, "Abram").is_some() && find_in(h, guide, "Easton: Abraham").is_some()
    });
    for _ in 0..20 {
        h.paced_frame();
    }
    assert!(find_in(&h, guide, "(Abraham)").is_some(), "{:?}", h.texts());
    assert!(has_prefix_in(&h, guide, "Man · "));
    assert!(find_in(&h, guide, "Father").is_some());
    assert!(find_in(&h, guide, "Smith: Abraham").is_some());
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/guide");
        h.frames(30);
        h.screenshot(&dir.join("people-and-places.ppm"));
    }

    // The link opens Easton at its Abraham article, leaving the Guide and the Bible.
    let tabs = h.shell.session().workspace.tabs().count();
    let link = find_in(&h, guide, "Easton: Abraham").unwrap().center();
    h.click(link);
    h.frames(3);
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tabs().count(), tabs + 1);
    let easton = ws.focused().unwrap();
    let t = ws.tab(easton).unwrap();
    assert_eq!(t.kind, TabKind::Resource("easton".into()));
    assert_eq!(
        t.entry_history.current(),
        Some(Key::Headword("Abraham".into()))
    );
    let gen_12_1 = Verse::new(Book::Genesis, 12, 1);
    assert_eq!(ws.tab(guide).unwrap().history.current(), Some(gen_12_1));
    assert_eq!(ws.tab(bible).unwrap().history.current(), Some(gen_12_1));
    // The Dictionary's header box shows the article.
    assert!(h.find("Abraham").is_some());
    drop(h);
}

/// #149: clicking Abram in People & places opens Abraham's Factbook (family, name meaning,
/// Key Passages); a name typed into its box lists the distinct Johns, and choosing one shows
/// it.
#[test]
fn abram_opens_his_factbook_and_the_john_chooser_lists_the_johns() {
    use obs_engine::testing::{APOSTLE, dictionary, hitchcock, johns, kjv_genesis_12, tipnr};
    let mut h = Harness::new();
    let (_files, _bible, guide) = setup_words(
        &mut h,
        "factbook",
        &[
            kjv_genesis_12(),
            tipnr(),
            dictionary("easton"),
            dictionary("smith"),
            hitchcock(),
            johns(),
        ],
        "Gen 12:1",
        GuideSection::PeopleAndPlaces,
    );
    until(&mut h, "Abram's row", |h| {
        find_in(h, guide, "Abram").is_some() && find_in(h, guide, "Easton: Abraham").is_some()
    });
    for _ in 0..20 {
        h.paced_frame();
    }
    let abram = find_in(&h, guide, "Abram").unwrap().center();
    h.click(abram);
    h.frames(3);
    let ws = &h.shell.session().workspace;
    let factbook = ws.focused().unwrap();
    let t = ws.tab(factbook).unwrap();
    assert_eq!(t.kind, TabKind::Guide(Guide::Factbook));
    assert_eq!(
        t.entry_history.current(),
        Some(Key::Headword("Abraham@Gen.11.26-1Pe".into()))
    );
    until(&mut h, "Abraham's Factbook", |h| {
        find_in(h, factbook, "son of Terah").is_some()
            && find_in(h, factbook, "Terah").is_some()
            && find_in(h, factbook, "“father of a great multitude”").is_some()
            && has_prefix_in(h, factbook, "Genesis · ")
    });
    assert!(
        find_in(&h, factbook, "Abraham").is_some(),
        "{:?}",
        h.texts()
    );
    assert!(find_in(&h, factbook, "Father").is_some());
    assert!(find_in(&h, factbook, "Partners").is_some());
    assert!(find_in(&h, factbook, "Abram").is_some(), "Also called");
    let titles = headers(&h, factbook);
    // Map (#150) sits under the Summary.
    assert_eq!(
        titles[..3],
        [
            "Summary".to_string(),
            "Map".to_string(),
            "Key Passages".to_string()
        ],
        "{titles:?}"
    );
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/guide");
        h.frames(30);
        h.screenshot(&dir.join("factbook-abraham.ppm"));
    }
    // The Dictionary articles are further down (the Map sits above them): wheel down to them.
    h.events
        .push(Event::PointerMoved(pane(&h, factbook).center()));
    for _ in 0..200 {
        if find_in(&h, factbook, "Easton: Abraham").is_some() {
            break;
        }
        h.events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -40.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        });
        h.paced_frame();
    }
    assert!(
        find_in(&h, factbook, "Easton: Abraham").is_some(),
        "{:?}",
        h.texts()
    );

    // A name several people share: the chooser lists each John, told apart by his facts.
    h.shell
        .run("tab.go", &json!({ "tab": factbook.0, "text": "John" }));
    until(&mut h, "the John chooser", |h| {
        find_in(h, factbook, "John (son of Zebedee)").is_some()
    });
    for title in [
        "John (son of Zechariah)",
        "John (son of Zebedee)",
        "John (father of Peter)",
        "John (Tribe of Levi)",
        "Mark (son of Mary)",
    ] {
        assert!(
            find_in(&h, factbook, title).is_some(),
            "{title}: {:?}",
            h.texts()
        );
    }
    assert!(find_in(&h, factbook, "also called John").is_some());
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/guide");
        h.frames(10);
        h.screenshot(&dir.join("factbook-john-chooser.ppm"));
    }
    let apostle = find_in(&h, factbook, "John (son of Zebedee)")
        .unwrap()
        .center();
    h.click(apostle);
    h.frames(3);
    let t = h.shell.session().workspace.tab(factbook).unwrap();
    assert_eq!(
        t.entry_history.current(),
        Some(Key::Headword(APOSTLE.into()))
    );
    until(&mut h, "the apostle's Factbook", |h| {
        find_in(h, factbook, "son of Zebedee").is_some()
    });
    drop(h);
}

/// #149: a Factbook chooser opened first thing (no Guide open, so no Guide job to step) loads
/// the People & places directory by itself: frames step its job and keep coming while it
/// loads.
#[test]
fn a_cold_factbook_chooser_loads_the_directory_by_itself() {
    use obs_engine::testing::johns;
    let mut h = Harness::new();
    let dir = std::env::temp_dir().join(format!("obs-shell-cold-chooser-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    h.shell.st.session.library = Library::new();
    add_file(&mut h, &files, &johns());
    h.shell.reset_layout();
    h.frames(2);
    let tab = TabId(
        h.shell
            .run("guide.factbook", &json!({ "name": "John" }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    let session = &h.shell.st.session;
    assert!(!session.guides.is_busy(), "no Guide job to piggyback on");
    assert!(!session.word_study.is_busy());
    // The chooser's first frame asks for the directory; while it loads, frames keep coming.
    h.frame();
    assert!(h.shell.st.session.factbooks.is_busy());
    assert!(
        h.shell.background_repaint().is_some(),
        "a loading directory keeps frames coming"
    );
    until(&mut h, "the cold John chooser", |h| {
        find_in(h, tab, "John (son of Zebedee)").is_some()
    });
    assert!(find_in(&h, tab, "John (son of Zechariah)").is_some());
    assert!(!h.shell.st.session.factbooks.is_busy());
    drop(h);
    drop(files);
}

/// An Exegetical Guide (#144) beside the first of `resources` (a Bible at `reference`, in
/// Link set A). Returns the Bible and the Guide.
fn setup_exegetical(
    h: &mut Harness,
    name: &str,
    resources: &[obs_model::Resource],
    reference: &str,
) -> (Files, TabId, TabId) {
    let dir = std::env::temp_dir().join(format!("obs-shell-exeg-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    h.shell.st.session.library = Library::new();
    for r in resources {
        add_file(h, &files, r);
    }
    h.shell.reset_layout();
    let id = resources[0].manifest.resource_id.clone();
    let bible = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource(id.clone()))
        .unwrap()
        .id;
    h.shell
        .run("tab.go", &json!({ "tab": bible.0, "text": reference }));
    h.frames(2);
    let guide = h
        .shell
        .run("guide.exegetical_guide", &json!({ "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    h.frames(2);
    (files, bible, TabId(guide))
}

fn preview(h: &mut Harness, name: &str) {
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/guide");
        h.frames(30);
        h.screenshot(&dir.join(format!("{name}.ppm")));
    }
}

#[test]
fn the_exegetical_guide_lists_john_3_16_word_by_word_and_a_word_opens_its_word_study() {
    use obs_engine::testing::{greek_lexicon, kjv_john, tagnt_john_3_16};
    let mut h = Harness::new();
    let (_files, bible, guide) = setup_exegetical(
        &mut h,
        "john",
        &[kjv_john(), tagnt_john_3_16(), greek_lexicon()],
        "Jn 3:16",
    );
    // It opened at the Bible's Passage and follows the Bible's Link set.
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tab(guide).unwrap().history.current(), Some(john(3, 16)));
    assert_eq!(ws.tab(guide).unwrap().link, ws.tab(bible).unwrap().link);
    until(&mut h, "the Word by word rows", |h| {
        find_in(h, guide, "egapesen").is_some() && find_in(h, guide, "John 3:16 (TAGNT)").is_some()
    });
    for _ in 0..20 {
        h.paced_frame();
    }
    // Word by word comes first; the other sections are below the 25 words.
    assert_eq!(
        headers(&h, guide).first().map(String::as_str),
        Some("Word by word")
    );
    // Each word in order: transliteration and Gloss, then Strong's number and Morphology.
    let thus = find_in(&h, guide, "Thus").unwrap();
    let loved = find_in(&h, guide, "loved").unwrap();
    let god = find_in(&h, guide, "God").unwrap();
    assert!(thus.top() < loved.top() && loved.top() < god.top());
    assert!(
        has_prefix_in(&h, guide, "G25 · Verb, aorist active indicative"),
        "{:?}",
        h.texts()
    );
    preview(&mut h, "exegetical-john-3-16");

    // A word opens a Word Study at its Strong's number; the Guide stays put.
    let tabs = h.shell.session().workspace.tabs().count();
    let word = find_in(&h, guide, "egapesen").unwrap().center();
    h.click(word);
    h.frames(2);
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tabs().count(), tabs + 1);
    let study = ws.tab(ws.focused().unwrap()).unwrap();
    assert_eq!(study.kind, TabKind::Guide(Guide::WordStudy));
    assert_eq!(
        study.entry_history.current(),
        Some(Key::StrongsNumber("G25".parse().unwrap()))
    );
    assert_eq!(ws.tab(guide).unwrap().history.current(), Some(john(3, 16)));

    // The Bible moves: the Guide follows its Link set.
    h.shell
        .run("tab.go", &json!({ "tab": bible.0, "text": "Jn 3:17" }));
    h.frames(2);
    assert_eq!(
        h.shell
            .session()
            .workspace
            .tab(guide)
            .unwrap()
            .history
            .current(),
        Some(john(3, 17))
    );
    drop(h);
}

#[test]
fn the_exegetical_guide_at_english_ps_3_1_shows_tahots_ps_3_2() {
    use obs_engine::testing::bibles::{bsb_psalm_3, tahot_psalms};
    let mut h = Harness::new();
    let (_files, _bible, guide) =
        setup_exegetical(&mut h, "psalm", &[bsb_psalm_3(), tahot_psalms()], "Ps 3:1");
    until(&mut h, "the Hebrew verse", |h| {
        find_in(h, guide, "Psalms 3:2 (TAHOT)").is_some() && find_in(h, guide, "O Yahweh").is_some()
    });
    assert!(find_in(&h, guide, "Ps 3:1 in English Bibles").is_some());
    // The Grammatical notes explain the Qal.
    until(&mut h, "the notes", |h| find_in(h, guide, "Qal").is_some());
    for _ in 0..20 {
        h.paced_frame();
    }
    preview(&mut h, "exegetical-psalm-3-1");
    drop(h);
}

/// `eg Ps 3:1-2` and `eg Ps 3` (#144): the Guide shows the whole range or chapter, not only
/// its first Verse.
#[test]
fn a_chapter_or_range_command_shows_its_later_verses() {
    use obs_engine::testing::bibles::{bsb_psalm_3, tahot_psalms};
    let mut h = Harness::new();
    let (_files, _bible, guide) = setup_exegetical(
        &mut h,
        "psalm-range",
        &[bsb_psalm_3(), tahot_psalms()],
        "Ps 3:1",
    );
    until(&mut h, "the first verse", |h| {
        find_in(h, guide, "Psalms 3:2 (TAHOT)").is_some()
    });
    assert!(find_in(&h, guide, "Psalms 3:3 (TAHOT)").is_none());

    let top = h
        .shell
        .st
        .session
        .command_box_suggestions("eg Ps 3:1-2")
        .into_iter()
        .next()
        .unwrap();
    let out = h
        .shell
        .st
        .session
        .run_command_box_action(&top.action, false)
        .unwrap();
    assert_eq!(TabId(out["tab"].as_u64().unwrap()), guide);
    until(&mut h, "the range's second verse", |h| {
        find_in(h, guide, "Psalms 3:1-2").is_some()
            && find_in(h, guide, "Psalms 3:2 (TAHOT)").is_some()
            && find_in(h, guide, "Psalms 3:3 (TAHOT)").is_some()
    });

    h.shell
        .run("guide.exegetical_guide", &json!({ "passage": "Ps 3" }));
    until(&mut h, "the chapter", |h| {
        find_in(h, guide, "Psalms 3").is_some()
    });
    // All four of the excerpt's verses (6 + 7 + 8 + 6 words, to TAHOT's Ps 3:9) are counted;
    // the first ones fill the page.
    until(&mut h, "the chapter's words", |h| {
        find_in(h, guide, "Psalms 3:1 (TAHOT)").is_some() && find_in(h, guide, "27").is_some()
    });
    drop(h);
}

/// #148: the Topic Guide on "Forgiveness" shows Nave's subtopics, Easton's article and the
/// Key Passages; clicking a Passage moves its Link set (the Bible beside it goes there). Typed
/// text shows the chooser with the topical index's synonyms.
#[test]
fn topic_guide_on_forgiveness_shows_naves_subtopics_and_a_passage_moves_the_link_set() {
    use obs_engine::testing::bibles::{bible, v};
    use obs_engine::testing::{easton_topics, topical_index};
    let mut h = Harness::new();
    let dir = std::env::temp_dir().join(format!("obs-shell-topic-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    h.shell.st.session.library = Library::new();
    let kjv = bible(
        "kjv",
        "KJV",
        &[
            (
                v(Book::Genesis, 33, 4),
                "And Esau ran to meet him, and embraced him, and fell on his neck, and kissed \
                 him: and they wept.",
            ),
            (
                v(Book::Luke, 23, 34),
                "Then said Jesus, Father, forgive them; for they know not what they do.",
            ),
            (
                v(Book::Ephesians, 4, 32),
                "And be ye kind one to another, tenderhearted, forgiving one another, even as \
                 God for Christ's sake hath forgiven you.",
            ),
        ],
        None,
        &[],
    );
    for r in [
        kjv,
        topical_index("nave"),
        topical_index("torrey"),
        easton_topics(),
    ] {
        add_file(&mut h, &files, &r);
    }
    h.shell.reset_layout();
    let bible_tab = h
        .shell
        .session()
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("kjv".into()))
        .unwrap()
        .id;
    h.shell
        .run("tab.go", &json!({ "tab": bible_tab.0, "text": "Gen 33:4" }));
    h.frames(2);
    let topic = TabId(
        h.shell
            .run(
                "guide.topic_guide",
                &json!({ "topic": "Forgiveness", "split": true }),
            )
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    let ws = &h.shell.session().workspace;
    let link = ws.tab(bible_tab).unwrap().link;
    assert!(link.is_some());
    assert_eq!(
        ws.tab(topic).unwrap().link,
        link,
        "joins the Bible's Link set"
    );
    until(&mut h, "Nave's subtopics", |h| {
        find_in(h, topic, "OF ENEMIES").is_some()
            && find_in(h, topic, "INSTANCES OF").is_some()
            && find_in(h, topic, "Esau forgives Jacob").is_some()
            && find_in(h, topic, "Nave: FORGIVENESS").is_some()
    });
    assert!(find_in(&h, topic, "Torrey: FORGIVENESS OF INJURIES").is_some());
    assert!(find_in(&h, topic, "(from Sin, Forgiveness of)").is_some());
    preview(&mut h, "topic-guide-forgiveness");
    // The other sections, below: collapse the entries to bring them into view.
    h.shell.run(
        "guide.collapse_section",
        &json!({ "guide": "topic_guide", "section": "topical_entries", "collapsed": true }),
    );
    until(&mut h, "the article and Key Passages", |h| {
        find_in(h, topic, "Easton: Forgiveness of sin").is_some()
            && find_in(h, topic, "cited 3×").is_some()
    });
    preview(&mut h, "topic-guide-key-passages");
    h.shell.run(
        "guide.collapse_section",
        &json!({ "guide": "topic_guide", "section": "topic_passages", "collapsed": true }),
    );
    until(&mut h, "the Search button", |h| {
        find_in(h, topic, "Search your library for “Forgiveness”").is_some()
    });
    assert_eq!(
        headers(&h, topic),
        [
            "Topical Bible entries",
            "Dictionary articles",
            "Key Passages",
            "Search"
        ]
    );
    h.shell.run(
        "guide.collapse_section",
        &json!({ "guide": "topic_guide", "section": "topic_passages", "collapsed": false }),
    );
    until(&mut h, "Key Passages again", |h| {
        find_in(h, topic, "cited 3×").is_some()
    });

    // The first Key Passage (most cited): Eph 4:32. Clicking it moves the Link set.
    let key_row = find_in(&h, topic, "cited 3×").unwrap();
    let eph = h
        .shapes
        .iter()
        .filter(|(s, t)| t == "Eph 4:32" && (s.center().y - key_row.center().y).abs() < 4.0)
        .map(|(s, _)| *s)
        .next()
        .unwrap_or_else(|| panic!("Eph 4:32 beside “cited 3×”: {:?}", h.texts()));
    h.click(eph.center());
    h.frames(3);
    assert_eq!(
        h.shell
            .session()
            .workspace
            .tab(bible_tab)
            .unwrap()
            .history
            .current(),
        Some(v(Book::Ephesians, 4, 32)),
        "the Bible in the Topic Guide's Link set went to Eph 4:32"
    );

    // ▸ shows a subtopic's Passages inline: Esau's row reads Gen 33:4 from the Bible.
    h.shell.run(
        "guide.collapse_section",
        &json!({ "guide": "topic_guide", "section": "topical_entries", "collapsed": false }),
    );
    until(&mut h, "the entries again", |h| {
        find_in(h, topic, "Esau forgives Jacob").is_some()
    });
    let esau = find_in(&h, topic, "Esau forgives Jacob").unwrap();
    let toggle = h
        .shapes
        .iter()
        .filter(|(s, t)| t == "[chevron-right]" && (s.center().y - esau.center().y).abs() < 4.0)
        .map(|(s, _)| *s)
        .next()
        .unwrap();
    h.ctx.enable_accesskit();
    h.click(toggle.center());
    until(&mut h, "Esau's Passage inline", |h| {
        h.tree.as_ref().is_some_and(|tree| {
            tree.nodes.iter().any(|(_, node)| {
                node.role() == egui::accesskit::Role::Label
                    && node
                        .value()
                        .is_some_and(|t| t.starts_with("And Esau ran to meet him"))
            })
        })
    });

    // Torrey has two entries on Forgiveness; showing the first subtopic of one leaves the
    // other's first subtopic closed.
    let arrow_beside = |h: &Harness, label: &str| -> Option<(Rect, String)> {
        let row = find_in(h, topic, label)?;
        h.shapes
            .iter()
            .find(|(s, t)| {
                (t == "[chevron-right]" || t == "[chevron-down]")
                    && (s.center().y - row.center().y).abs() < 4.0
            })
            .cloned()
    };
    // They are below the fold: wheel down to them.
    for _ in 0..60 {
        if arrow_beside(&h, "Christ set an example of").is_some()
            && arrow_beside(&h, "Promised through Christ").is_some()
        {
            break;
        }
        h.events
            .push(egui::Event::PointerMoved(pane(&h, topic).center()));
        h.events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -60.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        });
        h.frames(2);
    }
    h.frames(30);
    until(&mut h, "both Torrey entries", |h| {
        arrow_beside(h, "Christ set an example of").is_some()
            && arrow_beside(h, "Promised through Christ").is_some()
    });
    let (injuries_toggle, _) = arrow_beside(&h, "Christ set an example of").unwrap();
    h.click(injuries_toggle.center());
    until(&mut h, "Torrey's first subtopic open", |h| {
        arrow_beside(h, "Christ set an example of").is_some_and(|(_, a)| a == "[chevron-down]")
    });
    // The open subtopic's Passages may push the other entry below the fold: wheel down to it.
    for _ in 0..20 {
        if arrow_beside(&h, "Promised through Christ").is_some() {
            break;
        }
        h.events
            .push(egui::Event::PointerMoved(pane(&h, topic).center()));
        h.events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -60.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: egui::Modifiers::NONE,
        });
        h.frames(2);
    }
    assert_eq!(
        arrow_beside(&h, "Promised through Christ").map(|(_, a)| a),
        Some("[chevron-right]".to_string()),
        "the other Torrey entry's first subtopic stays closed"
    );

    // The chooser: the topic, longer headwords and the topical index's synonyms.
    h.shell
        .run("tab.go", &json!({ "tab": topic.0, "text": "forgiveness" }));
    until(&mut h, "the topic chooser", |h| {
        find_in(h, topic, "Pardon").is_some()
            && find_in(h, topic, "see Sin, Forgiveness of").is_some()
    });
    assert!(find_in(&h, topic, "Forgiveness of Injuries").is_some());
    preview(&mut h, "topic-guide-chooser");
    let injuries = find_in(&h, topic, "Forgiveness of Injuries").unwrap();
    h.click(injuries.center());
    h.frames(3);
    assert_eq!(
        h.shell
            .session()
            .workspace
            .tab(topic)
            .unwrap()
            .entry_history
            .current(),
        Some(Key::Headword("Forgiveness of Injuries".into()))
    );
    until(&mut h, "Torrey's entry", |h| {
        find_in(h, topic, "Christ set an example of").is_some()
    });
    drop(h);
    drop(files);
}

/// #212 acceptance through the real shell: a Passage Guide at Mark 1:9-11 lists Matthew
/// 3:13-17 and Luke 3:21-22 (with John 1:29-34) under Parallel passages, each with its heading
/// and the ★ Bible's text; clicking Matthew opens Text Comparison aligned on Matthew 3:13-17,
/// and the Guide and the Bible it follows stay at Mark.
#[test]
fn parallel_passages_list_the_baptism_and_a_click_compares_it() {
    use obs_engine::testing::{bsb_parallels, bsb_parallels_bible};
    let mut h = Harness::new();
    let dir = std::env::temp_dir().join(format!("obs-shell-parallels-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    h.shell.st.session.library = Library::new();
    add_file(&mut h, &files, &bsb_parallels_bible());
    add_file(&mut h, &files, &bsb_parallels());
    h.shell.reset_layout();
    let bible = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("bsb".into()))
        .unwrap()
        .id;
    h.shell
        .run("tab.go", &json!({ "tab": bible.0, "text": "Mk 1:9" }));
    h.frames(2);
    let guide = open_guide(&mut h);
    h.shell
        .run("tab.go", &json!({ "tab": guide.0, "text": "Mk 1:9-11" }));
    // Cross-references and Commentaries are empty here; collapse them out of the way.
    for section in ["commentaries", "cross_references"] {
        h.shell.run(
            "guide.collapse_section",
            &json!({ "guide": "passage_guide", "section": section, "collapsed": true }),
        );
    }
    until(&mut h, "the parallel passages", |h| {
        find_in(h, guide, "Matt 3:13-17").is_some() && find_in(h, guide, "Luke 3:21-22").is_some()
    });
    assert!(headers(&h, guide).contains(&"Parallel passages".to_string()));
    assert!(find_in(&h, guide, "John 1:29-34").is_some());
    assert!(
        find_in(&h, guide, "The Baptism of Jesus").is_some(),
        "{:?}",
        h.texts()
    );
    preview(&mut h, "parallel-passages-mark-1-9-11");

    let tabs = h.shell.session().workspace.tabs().count();
    let link = find_in(&h, guide, "Matt 3:13-17").unwrap().center();
    h.click(link);
    h.frames(3);
    let ws = &h.shell.session().workspace;
    assert_eq!(ws.tabs().count(), tabs + 1);
    let compare = ws.focused().unwrap();
    let t = ws.tab(compare).unwrap();
    assert_eq!(t.kind, TabKind::Tool(obs_engine::Tool::TextComparison));
    assert_eq!(t.link, None);
    let matthew = Passage::new(
        Verse::new(Book::Matthew, 3, 13),
        Verse::new(Book::Matthew, 3, 17),
    )
    .unwrap();
    assert_eq!(t.comparison.range, Some(matthew));
    assert_eq!(t.history.current(), Some(Verse::new(Book::Matthew, 3, 13)));
    let mark = Passage::new(Verse::new(Book::Mark, 1, 9), Verse::new(Book::Mark, 1, 11)).unwrap();
    assert_eq!(ws.tab(guide).unwrap().shown_passage(), Some(mark));
    assert_eq!(
        ws.tab(bible).unwrap().history.current(),
        Some(Verse::new(Book::Mark, 1, 9))
    );
    // The Text Comparison shows Matthew's Verses, its range chip naming the parallel.
    until(&mut h, "Matthew 3:13 compared", |h| {
        h.find("Matt 3:13-17").is_some() && h.find("Matthew 3:13").is_some()
    });
    preview(&mut h, "parallel-passages-compare-matthew");
    drop(h);
}

/// #261 GUIDES-3: a Guide at a range shows the whole range in its reference box, and
/// confirming the box keeps it.
#[test]
fn the_reference_box_shows_a_guides_whole_range_and_enter_keeps_it() {
    let mut h = Harness::launch(None);
    let (_files, _bible) = setup(&mut h, "range-box", false);
    let tab = TabId(
        h.shell
            .run(
                "guide.passage_guide",
                &json!({ "passage": "Mark 1:9-11", "split": true }),
            )
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    h.frames(2);
    let text = h.shell.st.location_text(tab);
    assert_eq!(text, "Mark 1:9-11");
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": text }));
    h.frames(2);
    let t = h.shell.session().workspace.tab(tab).unwrap();
    assert_eq!(
        t.shown_passage().map(|p| p.to_string()).as_deref(),
        Some("Mark 1:9-11")
    );
    assert_eq!(h.shell.st.location_text(tab), "Mark 1:9-11");
    // A chapter reads as the chapter, not its title slot or first Verse.
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": "Acts 13" }));
    h.frames(2);
    assert_eq!(h.shell.st.location_text(tab), "Acts 13");
    h.shell
        .run("tab.go", &json!({ "tab": tab.0, "text": "Ps 119" }));
    h.frames(2);
    assert_eq!(h.shell.st.location_text(tab), "Psalms 119");
}

/// #261 GUIDES-12: a reference clicked in an unlinked Guide moves the most recent Bible Tab,
/// click after click, instead of opening one more Bible Tab each time; Ctrl/Cmd still opens
/// a new one.
#[test]
fn an_unlinked_guides_reference_moves_the_most_recent_bible_tab() {
    let mut h = Harness::launch(None);
    let (_files, bible) = setup(&mut h, "unlinked-ref", false);
    let guide = open_guide(&mut h);
    h.shell
        .run("tab.link", &json!({ "tab": guide.0, "set": "" }));
    h.frames(2);
    assert_eq!(h.shell.session().workspace.tab(guide).unwrap().link, None);
    let bibles = |h: &Harness| {
        h.shell
            .session()
            .workspace
            .tabs()
            .filter(|t| t.kind == TabKind::Resource("kjv-sample".into()))
            .count()
    };
    // The sample Bible has John 3:16-17: go back and forth.
    for verse in [17, 16, 17] {
        h.shell
            .st
            .reference_navigate(guide, Passage::verse(john(3, verse)), false);
        assert_eq!(
            h.shell
                .session()
                .workspace
                .tab(bible)
                .unwrap()
                .history
                .current(),
            Some(john(3, verse))
        );
        h.frames(2);
        assert_eq!(bibles(&h), 1, "a click opened another Bible Tab");
    }
    h.shell
        .st
        .reference_navigate(guide, Passage::verse(john(3, 16)), true);
    h.frames(2);
    assert_eq!(bibles(&h), 2, "Ctrl/Cmd-click opens a new Bible Tab");
}
