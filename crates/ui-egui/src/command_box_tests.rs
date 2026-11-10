//! The Command box (#139) through the headless harness: Ctrl+L, typing, the dropdown, ↑/↓,
//! Enter, Alt+Enter, Esc, a click on a suggestion, and the recent entries after a restart.

use egui::Key;
use obs_engine::{Guide, TabId};
use obs_scripture::{Book, Verse};

use super::*;
use crate::shell::command_box::{ADD_A_BIBLE, BOX_ID, HINT, Row, show_all_label};
use crate::tokens::{share, size};

/// Ctrl as Linux and Windows report it.
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    command: true,
    ..Modifiers::NONE
};

fn key(h: &mut Harness, key: Key, modifiers: Modifiers) {
    h.modifiers = modifiers;
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    h.frame();
    h.events.push(Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers,
    });
    h.modifiers = Modifiers::NONE;
    h.frames(2);
}

fn type_text(h: &mut Harness, text: &str) {
    h.events.push(Event::Text(text.into()));
    h.frames(2);
}

fn box_focused(h: &Harness) -> bool {
    h.ctx.memory(|m| m.has_focus(egui::Id::unique(BOX_ID)))
}

fn tb_tabs(h: &Harness) -> Vec<TabId> {
    h.shell
        .st
        .session
        .workspace
        .tabs()
        .filter(|t| t.kind == TabKind::Resource("test-bible".into()))
        .map(|t| t.id)
        .collect()
}

fn location(h: &mut Harness, t: TabId) -> Option<Verse> {
    h.shell
        .st
        .tabs
        .get_mut(&t)
        .and_then(|t| t.view.as_mut())
        .and_then(|v| v.current_location())
}

fn ge(c: u16, v: u16) -> Verse {
    Verse::new(Book::Genesis, c, v)
}

#[test]
fn ctrl_l_focuses_the_box_typing_suggests_and_enter_goes_to_the_reference() {
    let mut h = Harness::new();
    assert!(
        h.find(&format!("{HINT} (Ctrl+L)")).is_some(),
        "{:?}",
        h.texts()
    );
    let tb = tb_tabs(&h)[0];
    assert!(!box_focused(&h));
    key(&mut h, Key::L, CTRL);
    assert!(box_focused(&h));

    type_text(&mut h, "gen 32 5");
    // The dropdown: the reference in the focused Bible first, Search last.
    let top = h.at("Genesis 32:5");
    assert!(h.find("Go to · TB").is_some(), "{:?}", h.texts());
    let search = h.at("Search for “gen 32 5”");
    assert!(top.y < search.y);

    key(&mut h, Key::Enter, Modifiers::NONE);
    h.frames(2);
    assert_eq!(location(&mut h, tb), Some(ge(32, 5)));
    assert_eq!(tb_tabs(&h), [tb], "Enter moved the focused Bible");
    // The box is empty again and the keyboard is back in the Workspace.
    assert!(!box_focused(&h));
    assert!(h.shell.st.command_box.text.is_empty());
    assert!(h.find("Search for “gen 32 5”").is_none());
}

#[test]
fn arrows_pick_a_suggestion_and_alt_enter_opens_a_new_tab() {
    let mut h = Harness::new();
    let tb = tb_tabs(&h)[0];
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "gen 31:3");
    assert_eq!(h.shell.st.command_box.selected, 0);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.shell.st.command_box.selected, 1);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.shell.st.command_box.selected, 1, "stops at the last");
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(h.shell.st.command_box.selected, 0);

    // Alt+Enter: the reference in a new Tab of the focused Bible. It joins TB's Link set
    // (A), so TB follows the jump, as with any Tab opened from a linked one.
    key(&mut h, Key::Enter, Modifiers::ALT);
    h.frames(3);
    let tabs = tb_tabs(&h);
    assert_eq!(tabs.len(), 2, "{tabs:?}");
    let new = tabs.into_iter().find(|t| *t != tb).unwrap();
    assert_eq!(location(&mut h, new), Some(ge(31, 3)));
    let ws = &h.shell.st.session.workspace;
    assert_eq!(ws.tab(new).unwrap().link, ws.tab(tb).unwrap().link);
    assert_eq!(location(&mut h, tb), Some(ge(31, 3)));

    // ↓ then Enter runs the second suggestion: here, Search.
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "gen 33:1");
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    let search = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| t.id)
        .expect("a Search Tab");
    assert_eq!(
        h.shell.st.session.search_tab(search).unwrap().query,
        "gen 33:1"
    );
}

#[test]
fn clicking_a_suggestion_runs_it_and_esc_leaves_the_box() {
    let mut h = Harness::new();
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "zzz");
    assert!(h.find("Search for “zzz”").is_some());
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!box_focused(&h));
    assert!(h.find("Search for “zzz”").is_none(), "the dropdown closed");

    key(&mut h, Key::L, CTRL);
    assert!(box_focused(&h));
    // Ctrl+L selected the text left in the box, so typing replaces it.
    type_text(&mut h, "pg gen 31");
    assert_eq!(h.shell.st.command_box.text, "pg gen 31");
    assert!(h.find("Passage Guide").is_some(), "{:?}", h.texts());
    h.click_text("Genesis 31");
    let ws = &h.shell.st.session.workspace;
    let guide = ws
        .tabs()
        .find(|t| t.kind == TabKind::Guide(Guide::PassageGuide))
        .expect("a Passage Guide");
    assert_eq!(guide.history.current(), Some(ge(31, 1)));
    assert_eq!(ws.focused(), Some(guide.id));
}

#[test]
fn an_empty_box_lists_recent_entries_kept_across_a_restart() {
    let db = temp_db("command-box-recent");
    {
        let mut h = Harness::launch(Some(&db));
        for text in ["gen 30:2", "gen 32 5"] {
            key(&mut h, Key::L, CTRL);
            type_text(&mut h, text);
            key(&mut h, Key::Enter, Modifiers::NONE);
        }
        h.shell.save_session(true);
    }
    let mut h = Harness::launch(Some(&db));
    let tb = tb_tabs(&h)[0];
    h.shell
        .run("tab.go", &json!({ "tab": tb.0, "text": "Gen 33:1" }));
    h.frames(2);
    key(&mut h, Key::L, CTRL);
    // Newest first.
    let newest = h.at("gen 32 5");
    let older = h.at("gen 30:2");
    assert!(newest.y < older.y);
    assert!(h.find("Recent").is_some());
    h.click_text("gen 30:2");
    h.frames(2);
    assert_eq!(location(&mut h, tb), Some(ge(30, 2)));
    assert_eq!(
        h.shell.st.session.command_box_recent(),
        ["gen 30:2", "gen 32 5"]
    );
}

/// The box's placeholder names the key the user has for it (#177), or none once cleared.
#[test]
fn the_hint_names_the_current_command_box_key() {
    let mut h = Harness::new();
    h.shell.run(
        "keys.set",
        &json!({ "command": "command_box.focus", "key": "Ctrl+Shift+K" }),
    );
    h.frames(2);
    assert!(
        h.find(&format!("{HINT} (Ctrl+Shift+K)")).is_some(),
        "{:?}",
        h.texts()
    );
    h.shell
        .run("keys.clear", &json!({ "command": "command_box.focus" }));
    h.frames(2);
    assert!(h.find(HINT).is_some(), "{:?}", h.texts());
}

/// Light and dark previews of the dropdown for "love" over a library whose ★ Bible has
/// more hits than the Search preview shows (#295):
/// `UPDATE_PREVIEWS=1 cargo test -p obs-ui-egui command_box_results_previews` writes
/// `target/previews/command-box/results-{light,dark}.ppm`, and All commands (`>`) as
/// `commands-{light,dark}.ppm`; then the hits in the Dark theme beside a Paper Page tone and in
/// the Light theme beside a Dark one, `results-{dark-paper,light-dark}.ppm`.
#[test]
fn command_box_results_previews() {
    if std::env::var_os("UPDATE_PREVIEWS").is_none() {
        return;
    }
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/previews/command-box");
    for theme in ["light", "dark"] {
        let (mut h, _files) = super::search::harness_of(
            &format!("command-box-preview-{theme}"),
            &[
                obs_engine::testing::kjv_love(),
                obs_engine::testing::bibles::psalms(12),
            ],
        );
        h.shell.run("view.theme", &json!({ "value": theme }));
        h.frames(2);
        key(&mut h, Key::L, CTRL);
        type_text(&mut h, "love");
        settle_verses(&mut h);
        h.screenshot(&out.join(format!("results-{theme}.ppm")));
        // All commands: more rows than fit.
        key(&mut h, Key::L, CTRL);
        type_text(&mut h, ">");
        h.frames(10);
        h.screenshot(&out.join(format!("commands-{theme}.ppm")));
    }
    for (theme, tone) in [("dark", "paper"), ("light", "dark")] {
        let (mut h, _files) = super::search::harness_of(
            &format!("command-box-preview-{theme}-{tone}"),
            &[
                obs_engine::testing::kjv_love(),
                obs_engine::testing::bibles::psalms(12),
            ],
        );
        h.shell.run("view.theme", &json!({ "value": theme }));
        h.shell.run("view.page_tone", &json!({ "value": tone }));
        h.frames(2);
        key(&mut h, Key::L, CTRL);
        type_text(&mut h, "love");
        settle_verses(&mut h);
        h.screenshot(&out.join(format!("results-{theme}-{tone}.ppm")));
    }
    // A right-to-left ★ Bible: its Verses laid out from the right by the text pipeline.
    for theme in ["light", "dark"] {
        let (mut h, _files) = super::search::harness_of(
            &format!("command-box-preview-hebrew-{theme}"),
            &[
                obs_engine::testing::bibles::hebrew_love(),
                obs_engine::testing::bibles::psalms(12),
            ],
        );
        h.shell.run("view.theme", &json!({ "value": theme }));
        h.frames(2);
        key(&mut h, Key::L, CTRL);
        type_text(&mut h, "love");
        settle_verses(&mut h);
        h.screenshot(&out.join(format!("results-hebrew-{theme}.ppm")));
    }
}

// ------------------------------------------------------------- the results panel (#295)

/// Draw `n` frames, running the queued jobs (Inline mode) before each.
fn pump_frames(h: &mut Harness, n: usize) {
    for _ in 0..n {
        h.shell.st.session.jobs().pump(16);
        h.frame();
    }
}

fn dropdown(h: &Harness) -> Rect {
    h.shell
        .st
        .command_box
        .dropdown
        .expect("the dropdown is open")
}

/// The window as egui sees it, in points (smaller at a larger Interface size).
fn window(h: &Harness) -> Rect {
    h.ctx.content_rect()
}

/// The library's ★ Bible is a Psalm whose 12 Verses all say "love", its Search index
/// built; KJV is second.
fn love_harness(name: &str) -> (Harness, super::search::Files) {
    super::search::harness_of(
        name,
        &[
            obs_engine::testing::bibles::psalms(12),
            obs_engine::testing::kjv_love(),
        ],
    )
}

/// The scroll bar's handle: a filled rectangle at most a scroll bar wide at the dropdown's
/// right edge.
fn scroll_handle(h: &Harness) -> Option<Rect> {
    let d = dropdown(h);
    h.rects.iter().map(|(r, _)| *r).find(|r| {
        d.contains_rect(*r)
            && r.width() <= size::SCROLLBAR
            && r.height() >= size::ROW
            && r.right() > d.right() - size::ROW
    })
}

/// The dropdown is a results panel (#295): min(640 pt, half the window) wide and up to 60%
/// of the window tall, right edge under the box, inside the window at every Toolbar position,
/// in a small window and at every Interface size.
#[test]
fn the_dropdown_is_wider_and_taller_and_stays_in_the_window() {
    let mut h = Harness::new();
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, ">");
    let d = dropdown(&h);
    let screen = window(&h);
    // Before: as wide as the box or 360 pt, and 12 rows (≈ 350 pt) tall.
    assert_eq!(d.width(), size::RESULTS, "{d:?}");
    assert!(
        (d.height() - screen.height() * share::RESULTS_HEIGHT).abs() < 1.0,
        "{d:?} in {screen:?}"
    );
    // The box ends the menu bar, at the window's right edge.
    assert!(
        screen.right() - d.right() < size::ROW,
        "under the box: {d:?}"
    );

    for position in ["bottom", "left", "right", "top"] {
        h.shell
            .run("view.toolbar_position", &json!({ "value": position }));
        h.frames(2);
        key(&mut h, Key::L, CTRL);
        type_text(&mut h, ">");
        let d = dropdown(&h);
        assert!(window(&h).contains_rect(d), "{position}: {d:?}");
        assert_eq!(d.width(), size::RESULTS, "{position}");
        key(&mut h, Key::Escape, Modifiers::NONE);
    }

    // A small window: half its width, still inside it.
    h.screen = vec2(700.0, 420.0);
    h.frames(2);
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, ">");
    let d = dropdown(&h);
    let screen = window(&h);
    assert!(screen.contains_rect(d), "{d:?} in {screen:?}");
    assert!(
        (d.width() - screen.width() * share::RESULTS_WIDTH).abs() < 1.0,
        "{d:?}"
    );
    key(&mut h, Key::Escape, Modifiers::NONE);

    // Interface size scales it with everything else: the same points, more pixels.
    h.screen = vec2(1400.0, 900.0);
    for size in ["larger", "smaller", "default"] {
        h.shell
            .run("view.interface_size", &json!({ "value": size }));
        h.frames(3);
        key(&mut h, Key::L, CTRL);
        type_text(&mut h, ">");
        let d = dropdown(&h);
        let screen = window(&h);
        assert!(screen.contains_rect(d), "{size}: {d:?} in {screen:?}");
        let wide = size::RESULTS.min(screen.width() * share::RESULTS_WIDTH);
        assert!((d.width() - wide).abs() < 1.0, "{size}: {d:?}");
        key(&mut h, Key::Escape, Modifiers::NONE);
    }
}

/// Rows that do not fit show a scroll bar and, at the bottom edge, how many more follow;
/// with every row shown there is neither (#295).
#[test]
fn rows_that_overflow_show_a_scroll_bar_and_how_many_more_follow() {
    let mut h = Harness::new();
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, ">");
    h.frames(2);
    let more = h.shell.st.command_box.more.expect("the commands overflow");
    assert!(more > 10, "{more}");
    assert!(h.find(&format!("{more} more")).is_some(), "{:?}", h.texts());
    assert!(scroll_handle(&h).is_some(), "an always-visible scroll bar");
    // At the end nothing more follows: the bar stays, the cue goes.
    key(&mut h, Key::End, Modifiers::NONE);
    h.frames(3);
    assert_eq!(h.shell.st.command_box.more, Some(0));
    assert!(!h.texts().iter().any(|t| t.ends_with(" more")));
    assert!(scroll_handle(&h).is_some());

    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "zzz");
    assert!(h.find("Search for “zzz”").is_some());
    assert_eq!(h.shell.st.command_box.more, None);
    assert!(!h.texts().iter().any(|t| t.ends_with(" more")));
    assert!(
        scroll_handle(&h).is_none(),
        "no scroll bar when everything shows"
    );
}

/// Suggestions sit under headings with their counts, the best one first (#295).
#[test]
fn suggestions_are_grouped_under_headings_with_counts() {
    let mut h = Harness::new();
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "gen 31:3");
    let rows = &h.shell.st.command_box.rows;
    assert_eq!(
        rows[..2],
        [
            Row::Heading {
                title: "Go to".into(),
                count: 1
            },
            Row::Suggestion(0)
        ]
    );
    let go_to = h.at("Go to");
    let genesis = h.at("Genesis 31:3");
    let search = h.at("Search for “gen 31:3”");
    assert!(go_to.y < genesis.y && genesis.y < search.y);
    assert!(rows.contains(&Row::Heading {
        title: "Search".into(),
        count: 1
    }));
}

/// Long labels are cut with "…" before their detail and shown whole on hover; a Resource
/// title fits whole at a normal window width (#295).
#[test]
fn long_labels_are_cut_and_shown_whole_on_hover() {
    let title = "The Psalms of Ascent with the Hebrew text beside it, two commentaries, a lexicon and notes on every verse";
    let mut psalms = obs_engine::testing::bibles::psalms(12);
    psalms.manifest.title = title.to_string();
    let (mut h, _files) = super::search::harness_of(
        "command-box-long",
        &[psalms, obs_engine::testing::kjv_love()],
    );
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "king james");
    let whole = "KJV — King James Version with Strong's (test)";
    assert!(h.find(whole).is_some(), "{:?}", h.texts());
    key(&mut h, Key::Escape, Modifiers::NONE);

    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "psa");
    let long = format!("PSA — {title}");
    let long = long.as_str();
    let label = (h.find(long)).unwrap_or_else(|| panic!("the Bible's row: {:?}", h.texts()));
    let detail = h.find("Open Bible").expect("its detail");
    let d = dropdown(&h);
    assert!(
        label.right() < detail.left(),
        "cut before the detail: {label:?} {detail:?}"
    );
    assert!(d.contains_rect(label));
    h.events.push(Event::PointerMoved(label.center()));
    h.frames(60);
    let shown = h.texts().iter().filter(|t| **t == long).count();
    assert_eq!(shown, 2, "the row and its tooltip: {:?}", h.texts());
}

/// ↑/↓, PgUp/PgDn and Home/End reach every row and keep the highlight in view; Esc closes;
/// screen readers hear where it is ("3 of 99") (#295).
#[test]
fn the_keys_reach_every_row_and_screen_readers_hear_the_count() {
    let mut h = Harness::new();
    h.ctx.enable_accesskit();
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, ">");
    h.frames(2);
    let n = h.shell.st.command_box.suggestions.len();
    let in_view = |h: &Harness| {
        let cb = &h.shell.st.command_box;
        let Some(Row::Suggestion(i)) = cb.selected_row() else {
            panic!("a suggestion is highlighted");
        };
        let label = &cb.suggestions[*i].label;
        let r = h.find(label).unwrap_or_else(|| panic!("{label} drawn"));
        dropdown(h).contains_rect(r)
    };
    key(&mut h, Key::PageDown, Modifiers::NONE);
    let page = h.shell.st.command_box.selected;
    assert!(page > 10, "a page is the rows that fit: {page}");
    assert!(in_view(&h));
    key(&mut h, Key::End, Modifiers::NONE);
    h.frames(2);
    assert_eq!(h.shell.st.command_box.selected, n - 1);
    assert!(in_view(&h), "the last row scrolled into view");
    let names: Vec<String> = (h.tree.as_ref().unwrap().nodes.iter())
        .filter(|(_, node)| node.role() == egui::accesskit::Role::ListBoxOption)
        .filter_map(|(_, node)| node.label().map(str::to_string))
        .collect();
    assert!(
        names.iter().any(|l| l.ends_with(&format!(", {n} of {n}"))),
        "{names:?}"
    );
    key(&mut h, Key::PageUp, Modifiers::NONE);
    assert_eq!(h.shell.st.command_box.selected, n - 1 - page);
    key(&mut h, Key::Home, Modifiers::NONE);
    h.frames(2);
    assert_eq!(h.shell.st.command_box.selected, 0);
    assert!(in_view(&h));
    assert!(
        h.find("Commands").is_some(),
        "the heading scrolled back too"
    );
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(h.shell.st.command_box.selected, 0);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.shell.st.command_box.dropdown.is_none());
}

/// The Search preview (#295): the ★ Bible's first five Verses with "love" highlighted show
/// under the Search row by the frame after the debounce ends (on the harness's 60 Hz clock;
/// the `search-preview` bench scenario gates the wall-clock time), with Show all; a click on
/// one goes there, and Show all opens the Search Tool.
#[test]
fn the_search_preview_shows_the_star_bibles_hits_as_you_type() {
    let (mut h, _files) = love_harness("command-box-preview");
    key(&mut h, Key::L, CTRL);
    let typed_at = h.time;
    type_text(&mut h, "love");
    // Not yet: the typing may go on.
    assert!(h.find("Psalms 119:1").is_none(), "{:?}", h.texts());
    let mut shown_at = None;
    for _ in 0..30 {
        pump_frames(&mut h, 1);
        if h.find("Psalms 119:1").is_some() {
            shown_at = Some(h.time);
            break;
        }
    }
    let shown_at = shown_at.expect("the hits came");
    assert!(
        shown_at - typed_at <= 0.15 + 1.0 / 60.0,
        "{}",
        shown_at - typed_at
    );
    for text in [
        "Search for “love”",
        "Verses in PSA",
        "12",
        "Psalms 119:1",
        "Psalms 119:5",
        &show_all_label(12),
    ] {
        assert!(h.find(text).is_some(), "{text}: {:?}", h.texts());
    }
    assert!(h.find("Psalms 119:6").is_none(), "five only");
    let search = h.at("Search for “love”");
    let first = h.at("Psalms 119:1");
    assert!(search.y < first.y, "under the Search row");
    assert_eq!(
        show_all_label(1),
        "Show all 1 result in Search",
        "one result"
    );

    // The keys reach the hits: Search, then the five Verses.
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.shell.st.command_box.selected_row(), Some(&Row::Hit(1)));
    key(&mut h, Key::Enter, Modifiers::NONE);
    pump_frames(&mut h, 3);
    let ps = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Resource("psalms".into()))
        .map(|t| t.id)
        .expect("the ★ Bible opened");
    assert_eq!(location(&mut h, ps), Some(Verse::new(Book::Psalms, 119, 2)));
    assert!(h.shell.st.command_box.text.is_empty());
    assert!(!box_focused(&h));

    // A click on a hit, then Show all.
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "love");
    pump_frames(&mut h, 10);
    h.click_text("Psalms 119:4");
    pump_frames(&mut h, 2);
    assert_eq!(location(&mut h, ps), Some(Verse::new(Book::Psalms, 119, 4)));
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "love");
    pump_frames(&mut h, 10);
    h.click_text(&show_all_label(12));
    pump_frames(&mut h, 2);
    let tab = h
        .shell
        .st
        .session
        .workspace
        .tabs()
        .find(|t| t.kind == TabKind::Tool(Tool::Search))
        .map(|t| t.id)
        .expect("a Search Tab");
    assert_eq!(h.shell.st.session.search_tab(tab).unwrap().query, "love");
}

/// The Search preview's Verses arriving under the Search row move the highlight with the row
/// it was on, and Enter pressed in the frame they arrive runs the row the user saw
/// highlighted, not the Verse now in its place (#295).
#[test]
fn the_search_preview_arriving_keeps_the_highlighted_row() {
    let word_study_tabs = |h: &Harness| {
        (h.shell.st.session.workspace.tabs())
            .filter(|t| t.kind == TabKind::Guide(Guide::WordStudy))
            .count()
    };
    for enter_as_they_arrive in [false, true] {
        let (mut h, _files) = love_harness(&format!(
            "command-box-preview-arriving-{enter_as_they_arrive}"
        ));
        key(&mut h, Key::L, CTRL);
        type_text(&mut h, "love");
        // Before the Verses: Search, then Word Study for the English word.
        key(&mut h, Key::ArrowDown, Modifiers::NONE);
        let cb = &h.shell.st.command_box;
        let word_study = cb.selected_row().cloned();
        let Some(Row::Suggestion(i)) = word_study else {
            panic!("a suggestion is highlighted: {:?}", cb.rows);
        };
        assert_eq!(
            cb.suggestions[i].action,
            obs_engine::CommandBoxAction::WordStudyWords("love".into())
        );
        assert_eq!(cb.selected, 1);
        // The typing stopped: the search is queued, its Verses not yet back.
        h.frames(15);
        assert!(h.shell.st.session.search_preview_running());
        assert!(!h.shell.st.command_box.rows.contains(&Row::Hit(0)));
        h.shell.st.session.jobs().pump(16);
        if enter_as_they_arrive {
            key(&mut h, Key::Enter, Modifiers::NONE);
            pump_frames(&mut h, 3);
            assert_eq!(word_study_tabs(&h), 1, "Word Study ran");
            let psalms = (h.shell.st.session.workspace.tabs())
                .any(|t| t.kind == TabKind::Resource("psalms".into()));
            assert!(!psalms, "no Verse was opened");
        } else {
            h.frames(2);
            let cb = &h.shell.st.command_box;
            assert!(cb.rows.contains(&Row::Hit(0)), "{:?}", cb.rows);
            assert_eq!(cb.selected_row().cloned(), word_study);
            // Search, five Verses and Show all above it now.
            assert_eq!(cb.selected, 7);
            assert_eq!(word_study_tabs(&h), 0);
        }
    }
}

/// The Search preview's Verses read in every Theme beside every Page tone (#295): their ink
/// and the tint behind their matches are a pair (WCAG AA, AAA in High contrast) that also
/// reads on the panel, whatever the Page tone.
#[test]
fn the_search_preview_highlights_read_in_every_theme_and_page_tone() {
    let (mut h, _files) = love_harness("command-box-hit-contrast");
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "love");
    pump_frames(&mut h, 20);
    assert!(h.find("Psalms 119:1").is_some(), "{:?}", h.texts());
    let mut checked = 0;
    for theme in ["light", "dark", "high_contrast_light", "high_contrast_dark"] {
        for (_, tone, _) in obs_engine::PageTone::ALL {
            h.shell.run("view.theme", &json!({ "value": theme }));
            h.shell.run("view.page_tone", &json!({ "value": tone }));
            settle_verses(&mut h);
            let chrome = crate::theme::c();
            assert_eq!(chrome.high_contrast, theme.starts_with("high_contrast"));
            let needs = if chrome.high_contrast { 7.0 } else { 4.5 };
            let palette = crate::shell::command_box::hit_palette(&chrome);
            let (glyphs, rects) = verse_paint(&h);
            let tints: Vec<Rect> = (rects.iter())
                .filter(|(_, fill)| *fill == palette.search_hit)
                .map(|(r, _)| *r)
                .collect();
            assert!(tints.len() >= 5, "{theme}, {tone}: the matches are tinted");
            assert!(!glyphs.is_empty(), "{theme}, {tone}: no Verse text");
            for (glyph, ink) in &glyphs {
                assert_eq!(*ink, palette.ink, "{theme}, {tone}: {glyph:?}");
            }
            let marked = (glyphs.iter())
                .filter(|(g, _)| tints.iter().any(|t| t.contains(g.center())))
                .count();
            assert!(
                marked >= 5 * 4,
                "{theme}, {tone}: \"love\" on its tint: {marked}"
            );
            for (on, background) in [("tint", palette.search_hit), ("panel", chrome.field)] {
                let ratio = crate::palette::contrast(palette.ink, background);
                assert!(
                    ratio >= needs,
                    "{theme}, {tone}: {:?} on the {on} {background:?} is {ratio:.2}:1",
                    palette.ink
                );
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 4 * obs_engine::PageTone::ALL.len());
}

/// Draw frames until the Search preview's Verses are laid out and drawn (the text pipeline
/// lays them out on worker threads).
fn settle_verses(h: &mut Harness) {
    for _ in 0..200 {
        h.shell.st.session.jobs().pump(16);
        h.paced_frame();
        let hits = match h.shell.st.session.search_preview() {
            obs_engine::SearchPreview::Hits(p) => p.hits.len(),
            _ => 0,
        };
        if hits > 0 && h.shell.st.command_box.verses_drawn == hits {
            h.frames(2);
            return;
        }
    }
    panic!("the Verses were never drawn");
}

/// What the dropdown's list drew last frame between the Search preview's heading and Show
/// all: glyph quads of the text pipeline with their ink, and filled rectangles with their fill.
#[allow(clippy::type_complexity)]
fn verse_paint(h: &Harness) -> (Vec<(Rect, egui::Color32)>, Vec<(Rect, egui::Color32)>) {
    let heading = (h.shapes.iter()).find(|(_, t)| t.starts_with("Verses in "));
    let show_all = (h.shapes.iter()).find(|(_, t)| t.starts_with("Show all "));
    let (Some((heading, _)), Some((show_all, _))) = (heading, show_all) else {
        return (Vec::new(), Vec::new());
    };
    let between = |r: &Rect| r.center().y > heading.bottom() && r.center().y < show_all.top();
    let (glyphs, rects) = list_paint(h);
    (
        glyphs.into_iter().filter(|(r, _)| between(r)).collect(),
        rects.into_iter().filter(|(r, _)| between(r)).collect(),
    )
}

/// What the dropdown's list drew last frame (its shapes are clipped to the list, or to a line
/// of it, inside the dropdown; a Panel's under it are clipped further right): textured quads (glyphs and icons) with their colour, and filled rectangles with their fill.
#[allow(clippy::type_complexity)]
fn list_paint(h: &Harness) -> (Vec<(Rect, egui::Color32)>, Vec<(Rect, egui::Color32)>) {
    fn walk(
        shape: &egui::Shape,
        glyphs: &mut Vec<(Rect, egui::Color32)>,
        rects: &mut Vec<(Rect, egui::Color32)>,
    ) {
        match shape {
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, glyphs, rects)),
            egui::Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => {
                for q in m.vertices.chunks(4) {
                    let at: Vec<Pos2> = q.iter().map(|v| v.pos).collect();
                    glyphs.push((Rect::from_points(&at), q[0].color));
                }
            }
            egui::Shape::Rect(r) => rects.push((r.rect, r.fill)),
            _ => {}
        }
    }
    let Some(d) = h.shell.st.command_box.dropdown else {
        return (Vec::new(), Vec::new());
    };
    let (mut glyphs, mut rects) = (Vec::new(), Vec::new());
    for clipped in &h.last_shapes {
        let clip = clipped.clip_rect;
        if d.contains_rect(clip) && clip.left() - d.left() < size::ROW {
            walk(&clipped.shape, &mut glyphs, &mut rects);
        }
    }
    (glyphs, rects)
}

/// A Search preview Verse is Resource text (ADR 0004): drawn by the text pipeline, never as
/// egui text, and a right-to-left Bible's laid out from the right: in a Hebrew Verse that
/// begins with the Latin \"love\", the tinted \"love\" is at the right end of the line and the
/// Hebrew to its left (#295).
#[test]
fn a_right_to_left_bibles_hits_are_laid_out_from_the_right() {
    let (mut h, _files) = super::search::harness_of(
        "command-box-rtl",
        &[
            obs_engine::testing::bibles::hebrew_love(),
            obs_engine::testing::bibles::psalms(12),
        ],
    );
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "love");
    settle_verses(&mut h);
    assert!(h.find("Verses in HEB").is_some(), "{:?}", h.texts());
    assert!(
        !h.texts()
            .iter()
            .any(|t| t.contains('\u{05d0}') || t.contains("love \u{05d1}")),
        "no Verse text through egui: {:?}",
        h.texts()
    );
    let palette = crate::shell::command_box::hit_palette(&crate::theme::c());
    let d = dropdown(&h);
    let (glyphs, rects) = verse_paint(&h);
    let reference = h.find("Genesis 1:1").expect("the first Verse");
    let next = h.find("Genesis 1:2").expect("the second Verse");
    let line = |r: &Rect| r.center().y > reference.bottom() && r.center().y < next.top();
    let glyphs: Vec<Rect> = (glyphs.iter()).map(|(r, _)| *r).filter(line).collect();
    let tint = (rects.iter())
        .find(|(r, fill)| *fill == palette.search_hit && line(r))
        .map(|(r, _)| *r)
        .expect("\"love\" is tinted");
    let (marked, hebrew): (Vec<Rect>, Vec<Rect>) =
        (glyphs.iter()).partition(|g| tint.contains(g.center()));
    assert_eq!(marked.len(), 4, "l o v e: {marked:?}");
    assert!(hebrew.len() >= 10, "{hebrew:?}");
    let right = glyphs.iter().map(|g| g.right()).fold(f32::MIN, f32::max);
    let left = glyphs.iter().map(|g| g.left()).fold(f32::MAX, f32::min);
    assert!(
        d.right() - right < size::ROW * 2.0,
        "flush right: {right} in {d:?}"
    );
    assert!(
        left > d.center().x,
        "a short line, not from the left: {left} in {d:?}"
    );
    for g in &hebrew {
        assert!(
            g.right() <= tint.left() + 1.0,
            "the Hebrew left of \"love\": {g:?} {tint:?}"
        );
    }

    // A left-to-right Bible's Verses start at the left.
    h.shell
        .st
        .session
        .library
        .set_preferred_order(["psalms", "heb-love"].map(String::from).to_vec());
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "law");
    settle_verses(&mut h);
    assert!(h.find("Verses in PSA").is_some(), "{:?}", h.texts());
    let (glyphs, _) = verse_paint(&h);
    let left = glyphs
        .iter()
        .map(|(g, _)| g.left())
        .fold(f32::MAX, f32::min);
    assert!(
        left - d.left() < size::ROW * 2.0,
        "flush left: {left} in {d:?}"
    );
}

/// A Verse too long for its row shows its first line ended with "…"; hovering it shows the
/// whole Verse, drawn by the text pipeline too (#295).
#[test]
fn a_cut_hit_is_shown_whole_on_hover_through_the_text_pipeline() {
    use obs_engine::testing::bibles::{bible, v};
    let long = "Blessed are the undefiled in the way, who walk in the law of the LORD, who love it \
                and keep his testimonies, and that seek him with the whole heart, all the day long, \
                every morning and every evening, in the house and by the way, love without end.";
    let verses = [(v(Book::Psalms, 119, 1), long)];
    let (mut h, _files) = super::search::harness_of(
        "command-box-cut",
        &[bible("long", "LNG", &verses, None, &[])],
    );
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "love");
    settle_verses(&mut h);
    let reference = h.find("Psalms 119:1").expect("the Verse");
    let cut = (h.shapes.iter()).any(|(r, t)| {
        t == "…" && r.top() >= reference.bottom() - 1.0 && r.top() < reference.bottom() + size::ROW
    });
    assert!(cut, "the line ends with \"…\": {:?}", h.texts());
    let (row, _) = verse_paint(&h);
    let before = h.glyphs.len();
    h.events.push(Event::PointerMoved(reference.center()));
    let mut shown = 0;
    for _ in 0..120 {
        h.paced_frame();
        shown = h.glyphs.len() - before.min(h.glyphs.len());
        if shown > row.len() {
            break;
        }
    }
    assert!(
        shown > row.len(),
        "the whole Verse ({shown} more glyphs) beyond the row's {}",
        row.len()
    );
    assert!(
        !h.texts().iter().any(|t| t.contains("undefiled")),
        "not as egui text: {:?}",
        h.texts()
    );
}

/// Without a Bible the preview offers Get Bibles; while the ★ Bible is indexed it says so
/// and fills in when the index is ready. The Search row stays in both (#295).
#[test]
fn the_search_preview_without_a_bible_and_while_indexing() {
    let (mut h, _files) = super::search::harness_of("command-box-no-bible", &[]);
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "love");
    pump_frames(&mut h, 10);
    assert!(h.find("Search for “love”").is_some());
    h.click_text(ADD_A_BIBLE);
    pump_frames(&mut h, 2);
    let ws = &h.shell.st.session.workspace;
    let focused = ws.focused().and_then(|t| ws.tab(t)).map(|t| t.kind.clone());
    assert_eq!(focused, Some(TabKind::GetBibles));

    // A library whose ★ Bible's index is being built in the background.
    let root =
        std::env::temp_dir().join(format!("obs-command-box-indexing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    obs_engine::testing::bibles::write_library(
        &root.join("library"),
        &[obs_engine::testing::bibles::psalms(12)],
    );
    let mut h = Harness::new();
    let s = &mut h.shell.st.session;
    s.library = obs_engine::Library::new();
    assert!(s.library.scan(&root.join("library")).is_empty());
    s.set_search_cache_dir(root.join("cache"));
    s.refresh_search_indexes();
    assert!(s.search.is_building());
    key(&mut h, Key::L, CTRL);
    type_text(&mut h, "love");
    h.frames(10);
    let indexing = "Indexing PSA… results will appear here";
    assert!(h.find(indexing).is_some(), "{:?}", h.texts());
    assert!(h.find("Search for “love”").is_some());
    for _ in 0..200 {
        pump_frames(&mut h, 1);
        if h.find("Psalms 119:1").is_some() {
            break;
        }
    }
    assert!(h.find("Psalms 119:1").is_some(), "{:?}", h.texts());
    assert!(h.find(indexing).is_none());
    drop(h);
    let _ = std::fs::remove_dir_all(root);
}
