//! The right-click menu of the GPUI reading pane (#381): what it lists for a click on a word,
//! a Verse and a Tagged word, and its commands, run through the engine. Each test names the
//! egui test it ports (`crates/ui-egui/src/context_menu_tests.rs`, and
//! `crates/ui-egui/src/copy_export_tests.rs` for copying from a pane).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use gpui_kit::{TestAppContext, point, px};
use obs_engine::{Guide, TabKind, Tool};
use obs_model::Key;
use obs_scripture::{Book, Verse};
use support::pane::{self, Open};

/// The egui context-menu tests' library: KJV John with Strong's, TAGNT John 3:16 and the
/// Greek Lexicon.
fn open_at_john_3_16(cx: &mut TestAppContext, name: &str, id: &str) -> Open {
    pane::open(
        cx,
        name,
        &[
            obs_engine::testing::kjv_john(),
            obs_engine::testing::tagnt_john_3_16(),
            obs_engine::testing::greek_lexicon(),
        ],
        id,
        "John 3:16",
    )
}

fn menu_open(cx: &mut TestAppContext, o: &Open) -> bool {
    o.shell.read_with(cx, |s, _| s.context_menu().is_some())
}

fn focused_kind(cx: &mut TestAppContext, o: &Open) -> TabKind {
    o.shell.read_with(cx, |s, _| {
        let ws = &s.session().workspace;
        ws.tab(ws.focused().unwrap()).unwrap().kind.clone()
    })
}

fn focused_key(cx: &mut TestAppContext, o: &Open) -> Option<Key> {
    o.shell.read_with(cx, |s, _| {
        let ws = &s.session().workspace;
        ws.tab(ws.focused().unwrap())
            .unwrap()
            .entry_history
            .current()
    })
}

fn search_query(cx: &mut TestAppContext, o: &Open) -> (String, Option<Vec<String>>) {
    o.shell.read_with(cx, |s, _| {
        let ws = &s.session().workspace;
        let search = ws
            .tabs()
            .find(|t| t.kind == TabKind::Tool(Tool::Search))
            .map(|t| t.id)
            .expect("a Search Tab");
        let st = s.session().search_tab(search).unwrap();
        (st.query.clone(), st.bibles.clone())
    })
}

/// Rest the pointer on a menu row (hovering a target chooses it).
fn hover(cx: &mut TestAppContext, o: &Open, label: &str) {
    let at = cx
        .update_window(o.handle, |_, window, cx| {
            use support::Driver;
            window.frame(cx);
            window.find_by_label(label).bounds().center()
        })
        .unwrap();
    pane::pointer_move(cx, o, at);
    pane::frames(cx, o.handle, 1);
}

/// Back to the Bible's Tab, as the user clicking it.
fn refocus(cx: &mut TestAppContext, o: &Open) {
    o.shell.update(cx, |s, cx| {
        s.session_mut().workspace.focus(o.tab);
        cx.notify();
    });
    pane::frames(cx, o.handle, 2);
}

/// Ports `context_menu_tests.rs::right_click_loved_in_john_3_16_kjv_opens_word_study_on_g25_in_one_click`.
#[gpui_kit::test]
fn right_click_loved_in_john_3_16_kjv_opens_word_study_on_g25_in_one_click(
    cx: &mut TestAppContext,
) {
    let o = open_at_john_3_16(cx, "menu-loved", "kjv");
    let loved = pane::word_spot(cx, &o, "loved", 0);
    pane::right_click(cx, &o, loved);
    assert!(menu_open(cx, &o));
    // The left column: the selection, the Verse, the Lemma (from the Lexicon: KJV+Strong's
    // tags only numbers) and the Strong's number; no Morphology.
    for t in [
        "Selection: “loved”",
        "Verse: John 3:16",
        "Lemma: ἀγαπάω",
        "Strong's number: G25",
    ] {
        assert!(pane::shows(cx, &o, t), "no {t}: {:?}", pane::labels(cx, &o));
    }
    let labels = pane::labels(cx, &o);
    assert!(
        !labels.iter().any(|l| l.starts_with("Morphology")),
        "{labels:?}"
    );
    // The word is chosen: Word Study is one click away.
    pane::click_label(cx, &o, "Word Study");
    assert!(!menu_open(cx, &o));
    assert_eq!(focused_kind(cx, &o), TabKind::Guide(Guide::WordStudy));
    assert_eq!(
        focused_key(cx, &o),
        Some(Key::StrongsNumber("G25".parse().unwrap()))
    );

    // Strong's: Search Bible for G25 searches this Bible.
    refocus(cx, &o);
    pane::right_click(cx, &o, loved);
    hover(cx, &o, "Strong's number: G25");
    pane::click_label(cx, &o, "Search Bible for G25");
    let (query, bibles) = search_query(cx, &o);
    assert_eq!(query, "strong:G25");
    assert_eq!(bibles.as_deref(), Some(&["kjv".to_string()][..]));

    // Open in preferred Lexicon: the Lexicon at G25.
    refocus(cx, &o);
    pane::right_click(cx, &o, loved);
    pane::click_label(cx, &o, "Open in preferred Lexicon");
    assert_eq!(focused_kind(cx, &o), TabKind::Resource("lexicon".into()));
    assert_eq!(
        focused_key(cx, &o),
        Some(Key::StrongsNumber("G25".parse().unwrap()))
    );
    pane::close(cx, o);
}

/// Ports `context_menu_tests.rs::right_click_in_a_greek_nt_lists_lemma_strongs_and_morphology`.
#[gpui_kit::test]
fn right_click_in_a_greek_nt_lists_lemma_strongs_and_morphology(cx: &mut TestAppContext) {
    let o = open_at_john_3_16(cx, "menu-tagnt", "tagnt");
    let word = pane::word_spot(cx, &o, "ἠγάπησεν", 0);
    pane::right_click(cx, &o, word);
    let morphology = obs_engine::word_info::ShownWord::new(
        "ἠγάπησεν",
        false,
        String::new(),
        &[obs_model::MorphemePart {
            strongs: "G25".parse().unwrap(),
            lemma: Some("ἀγαπάω".into()),
            morph: Some("V-AAI-3S".into()),
            gloss: None,
        }],
        |_| None,
    )
    .unwrap()
    .main
    .morphology
    .unwrap();
    for t in [
        "Lemma: ἀγαπάω".to_string(),
        "Strong's number: G25".to_string(),
        format!("Morphology: {morphology}"),
    ] {
        assert!(
            pane::shows(cx, &o, &t),
            "no {t}: {:?}",
            pane::labels(cx, &o)
        );
    }
    // Lemma: the right column searches by Lemma.
    hover(cx, &o, "Lemma: ἀγαπάω");
    pane::click_label(cx, &o, "Search Bible for ἀγαπάω");
    let (query, bibles) = search_query(cx, &o);
    assert_eq!(query, "lemma:ἀγαπάω");
    assert_eq!(bibles.as_deref(), Some(&["tagnt".to_string()][..]));

    // Morphology: Pin in Word Info pins the word there.
    refocus(cx, &o);
    pane::right_click(cx, &o, word);
    hover(cx, &o, &format!("Morphology: {morphology}"));
    assert!(pane::shows(cx, &o, "Copy"), "{:?}", pane::labels(cx, &o));
    pane::click_label(cx, &o, "Pin in Word Info");
    let (pinned, written) = o.shell.read_with(cx, |s, _| {
        let info = &s.session().word_info;
        (info.pinned, info.shown.as_ref().unwrap().written.clone())
    });
    assert!(pinned);
    assert_eq!(written, "ἠγάπησεν");
    assert_eq!(focused_kind(cx, &o), TabKind::Tool(Tool::WordInfo));
    pane::close(cx, o);
}

/// Ports `context_menu_tests.rs::the_verse_target_opens_passage_keyed_tools_at_the_verse`.
#[gpui_kit::test]
fn the_verse_target_opens_passage_keyed_tools_at_the_verse(cx: &mut TestAppContext) {
    let o = open_at_john_3_16(cx, "menu-verse", "kjv");
    let loved = pane::word_spot(cx, &o, "loved", 0);
    for (action, kind) in [
        ("Passage Guide", TabKind::Guide(Guide::PassageGuide)),
        ("Exegetical Guide", TabKind::Guide(Guide::ExegeticalGuide)),
        ("Cross-references", TabKind::Tool(Tool::CrossReferences)),
        ("Text Comparison", TabKind::Tool(Tool::TextComparison)),
    ] {
        pane::right_click(cx, &o, loved);
        hover(cx, &o, "Verse: John 3:16");
        assert!(pane::shows(cx, &o, "Copy with reference"));
        pane::click_label(cx, &o, action);
        assert_eq!(focused_kind(cx, &o), kind, "{action}");
        let at = o.shell.read_with(cx, |s, _| {
            let ws = &s.session().workspace;
            ws.tab(ws.focused().unwrap()).unwrap().history.current()
        });
        assert_eq!(at, Some(Verse::new(Book::John, 3, 16)), "{action}");
        refocus(cx, &o);
    }
    pane::close(cx, o);
}

/// Ports the copying half of `context_menu_tests.rs::the_verse_target_copies_with_reference_and_adds_a_note_on_the_verse`
/// (Add Note on verse comes with Notes, #382), and the pane's part of
/// `copy_export_tests.rs::copy_verses_copies_jn_3_16_18_in_kjv_and_bsb_with_a_preset_as_markdown_and_rich_text`:
/// Copy verses with formatting copies the selection's Verses through the engine's
/// `copy_verses`, with the Copy verses options in use.
#[gpui_kit::test]
fn the_verse_target_copies_with_reference_and_the_selection_copies_its_verses(
    cx: &mut TestAppContext,
) {
    let o = open_at_john_3_16(cx, "menu-copy", "kjv");
    let loved = pane::word_spot(cx, &o, "loved", 0);
    pane::right_click(cx, &o, loved);
    hover(cx, &o, "Verse: John 3:16");
    pane::click_label(cx, &o, "Copy with reference");
    let copied = pane::clipboard(cx).unwrap_or_default();
    assert!(
        copied.starts_with("For God so loved the world")
            && copied.ends_with("everlasting life.\nJohn 3:16 (KJV)"),
        "{copied:?}"
    );
    // The selection's Copy: the word with its reference.
    pane::right_click(cx, &o, loved);
    hover(cx, &o, "Selection: “loved”");
    pane::click_label(cx, &o, "Copy");
    assert_eq!(pane::clipboard(cx).unwrap(), "loved\nJohn 3:16 (KJV)");
    // Copy verses with formatting: the Verse as Copy verses lays it out.
    let expected = o.shell.read_with(cx, |s, cx| {
        let pane = s.pane(o.tab).unwrap().read(cx);
        let doc = std::sync::Arc::clone(&pane.view().doc);
        let list =
            obs_scripture::parse_reference("John 3:16", &obs_scripture::Standard, None).unwrap();
        let bibles: [&dyn obs_engine::CopiedBible; 1] = [doc.as_ref()];
        obs_engine::copy_verses(&list, &bibles, s.session().copy_verses.options).plain_text()
    });
    pane::right_click(cx, &o, loved);
    hover(cx, &o, "Selection: “loved”");
    pane::click_label(cx, &o, "Copy verses with formatting");
    let copied = pane::clipboard(cx).unwrap();
    assert_eq!(copied, expected);
    assert!(copied.contains("For God so loved the world") && copied.contains("John 3:16"));
    pane::close(cx, o);
}

/// Ports `context_menu_tests.rs::a_right_click_with_nothing_to_act_on_never_shows_the_last_menu`
/// for a reading pane: a right-click on blank space below the text opens no menu, and not the
/// last one.
#[gpui_kit::test]
fn a_right_click_with_nothing_to_act_on_never_shows_the_last_menu(cx: &mut TestAppContext) {
    let o = open_at_john_3_16(cx, "menu-blank", "kjv");
    let loved = pane::word_spot(cx, &o, "loved", 0);
    pane::right_click(cx, &o, loved);
    assert!(menu_open(cx, &o));
    pane::press(cx, &o, "escape");
    assert!(!menu_open(cx, &o));
    let bounds = o.pane.read_with(cx, |p, _| p.bounds());
    let blank = point(
        bounds.origin.x + px(60.0),
        bounds.origin.y + bounds.size.height - px(20.0),
    );
    pane::right_click(cx, &o, blank);
    assert!(!menu_open(cx, &o), "{:?}", pane::labels(cx, &o));
    assert!(!pane::shows(cx, &o, "Copy with reference"));
    pane::close(cx, o);
}

/// Ports `context_menu_tests.rs::shift_f10_and_the_menu_key_open_the_menu_at_the_hovered_word_or_the_selection`
/// (GPUI has no Menu key): Shift+F10 opens it at the word under the pointer, and with the
/// pointer away, on the selection.
#[gpui_kit::test]
fn shift_f10_opens_the_menu_at_the_hovered_word_or_the_selection(cx: &mut TestAppContext) {
    let o = open_at_john_3_16(cx, "menu-keys", "kjv");
    let loved = pane::word_spot(cx, &o, "loved", 0);
    pane::pointer_move(cx, &o, loved);
    pane::press(cx, &o, "shift-f10");
    assert!(menu_open(cx, &o));
    assert!(
        pane::shows(cx, &o, "Strong's number: G25"),
        "{:?}",
        pane::labels(cx, &o)
    );
    pane::press(cx, &o, "escape");
    assert!(!menu_open(cx, &o));
    // The selection, the pointer off the text.
    let world = pane::word_spot(cx, &o, "world", 0);
    pane::multi_click(cx, &o, world, 2);
    let bounds = o.pane.read_with(cx, |p, _| p.bounds());
    pane::pointer_move(
        cx,
        &o,
        point(
            bounds.origin.x + px(60.0),
            bounds.origin.y + bounds.size.height - px(20.0),
        ),
    );
    pane::press(cx, &o, "shift-f10");
    assert!(menu_open(cx, &o));
    assert!(
        pane::shows(cx, &o, "Selection: “world”"),
        "{:?}",
        pane::labels(cx, &o)
    );
    pane::close(cx, o);
}

/// The menu on a Hebrew Tagged word (#381: Hebrew must work): TAHOT's בָּרָא lists its Strong's
/// number and Morphology.
#[gpui_kit::test]
fn right_click_on_a_hebrew_tagged_word_lists_its_strongs_number(cx: &mut TestAppContext) {
    let o = pane::open(
        cx,
        "menu-hebrew",
        &[obs_engine::testing::bibles::tahot()],
        "tahot",
        "Gen 1:1",
    );
    let bara = pane::tagged_spot(cx, &o, "H1254A");
    pane::right_click(cx, &o, bara);
    assert!(menu_open(cx, &o));
    assert!(
        pane::shows(cx, &o, "Strong's number: H1254A"),
        "{:?}",
        pane::labels(cx, &o)
    );
    assert!(
        pane::shows(cx, &o, "Verse: Gen 1:1"),
        "{:?}",
        pane::labels(cx, &o)
    );
    pane::close(cx, o);
}
