//! Word Study (#55) through the shell's real-event headless harness: the English-word chooser
//! from the Guide's box, the header, Lexicon entries in Preferred order (first expanded), and
//! the ways in: Word Info's button, Strong's links in Lexicon entries and in a Lexicon Tab.

use obs_engine::{Guide, Library, TabId};
use obs_model::Key;
use obs_scripture::StrongsNumber;

use super::*;

struct Files(std::path::PathBuf);
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn n(text: &str) -> StrongsNumber {
    text.parse().unwrap()
}

/// A library of real `.obr` files: the KJV-style Bible with Strong's numbers, the Hebrew
/// Bible and the Starter Lexicon excerpts (TBESH, TBESG, Strong's Hebrew, BDB).
fn setup(h: &mut Harness, name: &str) -> Files {
    let mut resources = vec![
        obs_engine::testing::kjv_love(),
        obs_engine::testing::hebrew_bara(),
    ];
    resources.extend(obs_engine::testing::starter_lexicon_excerpts());
    setup_with(h, name, &resources)
}

/// A library of real `.obr` files made of `resources`.
fn setup_with(h: &mut Harness, name: &str, resources: &[obs_model::Resource]) -> Files {
    let dir = std::env::temp_dir().join(format!(
        "obs-shell-word-study-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let files = Files(dir);
    h.shell.st.session.library = Library::new();
    for r in resources {
        let path = files.0.join(format!("{}.obr", r.manifest.resource_id));
        obs_store::write_obr(r, &path).unwrap();
        h.shell.st.session.library.add_file(&path).unwrap();
    }
    h.shell.reset_layout();
    h.frames(2);
    files
}

fn open_word_study(h: &mut Harness) -> TabId {
    let tab = h
        .shell
        .run("guide.word_study", &json!({ "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    h.frames(2);
    TabId(tab)
}

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
    tiles.rect(tiles.find_pane(&tab).unwrap()).unwrap()
}

fn find_in(h: &Harness, tab: TabId, text: &str) -> Option<Rect> {
    let r = pane(h, tab);
    h.shapes
        .iter()
        .find(|(s, t)| t == text && r.contains(s.center()))
        .map(|(s, _)| *s)
}

/// Frames until every Resource-text snippet is laid out (so nothing moves under a click).
fn settle(h: &mut Harness) {
    for _ in 0..600 {
        h.frame();
        if !h.shell.st.snippets.has_pending() {
            h.frames(2);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn key_of(h: &Harness, tab: TabId) -> Option<Key> {
    h.shell
        .session()
        .workspace
        .tab(tab)
        .and_then(|t| t.entry_history.current())
}

/// Type into a Tab's header box (found by what it shows) and press Enter.
fn type_in_box(h: &mut Harness, tab: TabId, shown: &str, text: &str) {
    let boxed = h
        .shapes
        .iter()
        .filter(|(r, t)| t == shown && pane(h, tab).contains(r.center()))
        .map(|(r, _)| *r)
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .unwrap_or_else(|| panic!("no box showing {shown:?}: {:?}", h.texts()));
    h.click(boxed.center());
    h.events.push(Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    h.events.push(Event::Text(text.into()));
    h.frame();
    h.events.push(Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.frames(3);
}

#[test]
fn typing_love_offers_g25_g26_and_g5368_with_counts() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "love");
    let tab = open_word_study(&mut h);
    assert!(
        h.texts()
            .iter()
            .any(|t| t.starts_with("Type a Strong's number")),
        "{:?}",
        h.texts()
    );
    type_in_box(&mut h, tab, "Strong's number or word", "love");
    until(&mut h, "the chooser rows", |h| {
        find_in(h, tab, "G5368").is_some()
    });
    // Most frequent first, each with its count in the tagged English Bible.
    let rows: Vec<(f32, &str)> = ["G25", "G26", "G5368"]
        .iter()
        .map(|t| (find_in(&h, tab, t).unwrap().top(), *t))
        .collect();
    assert!(rows[0].0 < rows[1].0 && rows[1].0 < rows[2].0, "{rows:?}");
    for (top, count) in [(rows[0].0, "×3"), (rows[1].0, "×2"), (rows[2].0, "×2")] {
        assert!(
            h.shapes.iter().any(|(r, t)| t == count
                && pane(&h, tab).contains(r.center())
                && (r.center().y - top).abs() < 16.0),
            "no {count} beside its number: {:?}",
            h.texts()
        );
    }
    assert!(
        h.texts().iter().any(|t| t.contains("in the KJV")),
        "{:?}",
        h.texts()
    );
    // The box keeps the typed words while choosing.
    assert!(find_in(&h, tab, "love").is_some());
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/word-study");
        settle(&mut h);
        h.screenshot(&dir.join("word-study-chooser.ppm"));
    }

    // Choose G26: the Guide studies it, with its header and Lexicon entries.
    let g26 = find_in(&h, tab, "G26").unwrap().center();
    h.click(g26);
    h.frames(2);
    assert_eq!(key_of(&h, tab), Some(Key::StrongsNumber(n("G26"))));
    until(&mut h, "the G26 header and its TBESG entry", |h| {
        find_in(h, tab, "2 occurrences in KJV").is_some()
            && find_in(h, tab, "Brief lexicon of Extended Strongs for Greek").is_some()
    });
    assert!(find_in(&h, tab, "agape").is_some(), "{:?}", h.texts());
    assert!(find_in(&h, tab, "Lexicon entries").is_some());
    drop(h);
}

#[test]
fn h1254a_shows_tbesh_then_bdb_then_strongs_with_the_first_expanded() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "order");
    let tab = open_word_study(&mut h);
    type_in_box(&mut h, tab, "Strong's number or word", "H1254A");
    assert_eq!(key_of(&h, tab), Some(Key::StrongsNumber(n("H1254A"))));
    let tbesh = "Brief lexicon of Extended Strongs for Hebrew";
    let bdb = "Brown-Driver-Briggs";
    let strongs = "Strong's Hebrew Dictionary";
    until(&mut h, "the Lexicon entries", |h| {
        [tbesh, bdb, strongs]
            .iter()
            .all(|t| find_in(h, tab, t).is_some())
    });
    let top = |t: &str| find_in(&h, tab, t).unwrap().top();
    assert!(top(tbesh) < top(bdb) && top(bdb) < top(strongs));
    // Extended number first, then the base number.
    assert!(find_in(&h, tab, "H1254").is_some(), "{:?}", h.texts());
    // The header: Strong's number, part of speech, transliteration, gloss, occurrences.
    until(&mut h, "the header", |h| {
        find_in(h, tab, "3 occurrences in HOT").is_some()
    });
    for t in ["H1254A", "verb", "bara", "“to create”", "— TBESH"] {
        assert!(find_in(&h, tab, t).is_some(), "no {t}: {:?}", h.texts());
    }
    // The first entry is expanded (its text and Open button lie between it and BDB); the
    // others are collapsed (BDB's title sits right above Strong's).
    settle(&mut h);
    let title_gap = |h: &Harness, a: &str, b: &str| {
        find_in(h, tab, b).unwrap().top() - find_in(h, tab, a).unwrap().top()
    };
    assert!(title_gap(&h, tbesh, bdb) > 80.0);
    assert!(title_gap(&h, bdb, strongs) < 45.0);
    assert!(find_in(&h, tab, "Open in TBESH").is_some());
    assert!(find_in(&h, tab, "Open in BDB").is_none());
    // Expanding BDB is the user's choice for this number.
    let at = find_in(&h, tab, bdb).unwrap().center();
    h.click(at);
    settle(&mut h);
    // Its text now pushes Strong's down (off screen: BDB's entry is long).
    let bdb_top = find_in(&h, tab, bdb).unwrap().top();
    assert!(
        find_in(&h, tab, strongs).is_none_or(|r| r.top() - bdb_top > 80.0),
        "{:?}",
        h.texts()
    );
    assert!(find_in(&h, tab, "Open in TBESH").is_some());
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/word-study");
        h.frames(30);
        h.screenshot(&dir.join("word-study-h1254a.ppm"));
    }
    drop(h);
}

#[test]
fn word_info_and_strongs_links_open_word_study() {
    let mut h = Harness::new();
    let _files = setup(&mut h, "ways-in");
    // Word Info shows a word: its Word Study of this word button opens a Word Study at its number.
    let mut part = obs_model::MorphemePart::new(n("H1254A"));
    part.gloss = Some("he created".into());
    let word =
        obs_engine::ShownWord::new("בָּרָא", true, "Genesis 1:1 (HOT)".into(), &[part], |_| None)
            .unwrap();
    h.shell.st.session.word_info.pin(word);
    let info = h.tab_at("Word Info");
    h.click(info);
    h.frames(2);
    h.click_text("Word Study of this word");
    h.frames(2);
    let ws = &h.shell.session().workspace;
    let study = ws.focused().unwrap();
    assert_eq!(
        ws.tab(study).unwrap().kind,
        TabKind::Guide(Guide::WordStudy)
    );
    assert_eq!(key_of(&h, study), Some(Key::StrongsNumber(n("H1254A"))));

    // A Strong's link in a Lexicon entry moves the Word Study to that number.
    h.shell.run("word_study.open", &json!({ "strongs": "H2" }));
    h.frames(2);
    until(&mut h, "Strong's link H1G in TBESH's H2 entry", |h| {
        find_in(h, study, "H1G").is_some()
    });
    settle(&mut h);
    // A wrapped row's text shapes start at the row's left edge: aim at the link's end.
    let r = find_in(&h, study, "H1G").unwrap();
    h.click(pos2(r.right() - 6.0, r.center().y));
    h.frames(2);
    assert_eq!(key_of(&h, study), Some(Key::StrongsNumber(n("H1G"))));

    // In a Lexicon Tab, clicking a Strong's link opens its Word Study too.
    let lexicon = h
        .shell
        .run("resource.open", &json!({ "id": "tbesh", "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .map(TabId)
        .unwrap();
    h.frames(2);
    h.shell
        .run("tab.go", &json!({ "tab": lexicon.0, "text": "H1H" }));
    h.frames(6);
    let spot = super::word_info::word_spot(&h, lexicon, "H2438H");
    h.click(spot);
    h.frames(2);
    assert_eq!(key_of(&h, study), Some(Key::StrongsNumber(n("H2438H"))));
    assert_eq!(h.shell.session().workspace.focused(), Some(study));
    drop(h);
}

/// #56: Translated as from the KJV, Occurrences from TAGNT with KJV previews, and Forms
/// (collapsed at first) filtering Occurrences; a rendering hands its query to Search.
#[test]
fn g5485_translated_as_occurrences_and_forms_filter() {
    let mut h = Harness::new();
    let _files = setup_with(
        &mut h,
        "grace",
        &[
            obs_engine::testing::kjv_grace(),
            obs_engine::testing::tagnt_grace(),
        ],
    );
    let tab = open_word_study(&mut h);
    type_in_box(&mut h, tab, "Strong's number or word", "G5485");
    assert_eq!(key_of(&h, tab), Some(Key::StrongsNumber(n("G5485"))));
    // Translated as, from KJV+Strong's: grace, then favour.
    until(&mut h, "the renderings", |h| {
        find_in(h, tab, "grace ×6").is_some() && find_in(h, tab, "favour ×3").is_some()
    });
    let top = |h: &Harness, t: &str| find_in(h, tab, t).map(|r| (r.top(), r.left()));
    assert!(top(&h, "grace ×6") <= top(&h, "favour ×3"));
    // Forms starts collapsed: its header shows, its forms do not.
    let dative = "noun, dat. fem. sing. ×2";
    until(&mut h, "the Forms header", |h| {
        find_in(h, tab, "Forms").is_some()
    });
    assert!(find_in(&h, tab, dative).is_none(), "{:?}", h.texts());
    // Drag-free arrangement change: Forms to the top so everything fits on screen.
    h.shell.run(
        "guide.move_section",
        &json!({ "guide": "word_study", "section": "forms", "to": 0 }),
    );
    // No Lexicon here: its empty state (and its buttons, #135) would push Occurrences down.
    h.shell.run(
        "guide.collapse_section",
        &json!({ "guide": "word_study", "section": "lexicon_entries", "collapsed": true }),
    );
    h.frames(2);
    let header = find_in(&h, tab, "Forms").unwrap().center();
    h.click(header);
    until(&mut h, "the forms", |h| find_in(h, tab, dative).is_some());
    for form in [
        "noun, acc. fem. sing. ×4",
        "noun, gen. fem. sing. ×2",
        "noun, nom. fem. sing. ×1",
    ] {
        assert!(
            find_in(&h, tab, form).is_some(),
            "no {form}: {:?}",
            h.texts()
        );
    }
    // Occurrences: every Verse (the first in view), with a bar per Bible book.
    until(&mut h, "the Occurrences rows", |h| {
        find_in(h, tab, "Luke 1:30").is_some()
    });
    for book in ["Luke", "John", "Acts", "Rom", "Eph"] {
        assert!(find_in(&h, tab, book).is_some(), "no {book} bar");
    }
    assert!(
        find_in(&h, tab, "9 occurrences in 8 Verses of the TAGNT").is_some(),
        "{:?}",
        h.texts()
    );
    settle(&mut h);
    if std::env::var_os("UPDATE_PREVIEWS").is_some() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/previews/word-study");
        h.frames(30);
        h.screenshot(&dir.join("word-study-g5485.ppm"));
    }

    // Clicking the dative filters Occurrences to Luke 2:52 and Ephesians 2:8.
    let at = find_in(&h, tab, dative).unwrap().center();
    h.click(at);
    h.frames(3);
    assert!(
        find_in(&h, tab, "2 occurrences in 2 Verses of the TAGNT").is_some(),
        "{:?}",
        h.texts()
    );
    assert!(find_in(&h, tab, "Form: noun, dat. fem. sing.").is_some());
    for shown in ["Luke 2:52", "Eph 2:8"] {
        assert!(
            find_in(&h, tab, shown).is_some(),
            "no {shown}: {:?}",
            h.texts()
        );
    }
    for hidden in [
        "Luke 1:30",
        "Luke 2:40",
        "John 1:14",
        "Acts 2:47",
        "Rom 1:5",
        "John",
    ] {
        assert!(find_in(&h, tab, hidden).is_none(), "{hidden} still listed");
    }
    // Clicking it again shows every form.
    let at = find_in(&h, tab, dative).unwrap().center();
    h.click(at);
    h.frames(3);
    assert!(find_in(&h, tab, "Luke 1:30").is_some());

    // A rendering opens Search for its Verses in the KJV.
    let at = find_in(&h, tab, "favour ×3").unwrap().center();
    h.click(at);
    h.frames(2);
    let ws = &h.shell.session().workspace;
    let search = ws
        .tabs()
        .find(|t| t.kind == TabKind::Tool(obs_engine::Tool::Search))
        .map(|t| t.id)
        .expect("a Search Tab");
    let st = h.shell.session().search_tab(search).unwrap();
    assert_eq!(st.query, "strong:G5485 \"favour\"");
    assert_eq!(st.bibles.as_deref(), Some(&["kjv-grace".to_string()][..]));
    drop(h);
}
