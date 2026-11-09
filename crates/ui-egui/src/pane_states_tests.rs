//! Empty, loading and error states (#223), through the shell's real-event headless harness:
//! each surface's states as the Screenshot gallery draws them ([`pane_states`]), an error
//! that shows its plain message, Retry and Details but not the error itself until Details is
//! clicked, Retry that asks again, and no Tab left blank in an empty Library.

use std::sync::Arc;

use obs_engine::sword_repo::{Fetch, FetchError, Progress};
use obs_engine::{
    ErrorField, Guide, GuideSection, InlineError, JobContext, Library, SectionData, SectionSource,
    TabId,
};
use obs_model::Key;
use serde_json::Value;

use super::a11y_audit::{Overlay, every_tab_kind, open_tab};
use super::gallery::{run_split, settle_search};
use super::missing_resource::{settle, setup};
use super::*;
use crate::components::{DETAILS, HIDE_DETAILS, RETRY};

/// What every injected failure reports: the error itself, never on screen until Details.
const DETAIL: &str = "disk read error at block 7";

/// Every Guide section fails, as when a Resource's file cannot be read.
#[derive(Debug)]
struct FailingSections;

impl SectionSource for FailingSections {
    fn compute(
        &self,
        _: &Library,
        _: &Key,
        _: GuideSection,
        _: &JobContext,
    ) -> Result<SectionData, String> {
        Err(DETAIL.into())
    }
}

/// No network: every fetch fails.
#[derive(Debug)]
struct Offline;

impl Fetch for Offline {
    fn get(&self, _: &str, _: Progress<'_>) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Network(DETAIL.into()))
    }

    fn size(&self, _: &str) -> Result<Option<u64>, FetchError> {
        Err(FetchError::Network(DETAIL.into()))
    }
}

fn shows(h: &Harness, text: &str) -> bool {
    h.find(text).is_some()
}

fn shows_start(h: &Harness, start: &str) -> bool {
    h.texts().iter().any(|t| t.starts_with(start))
}

/// The focused Tab is of `kind`.
fn focused(h: &Harness, kind: &TabKind) -> bool {
    let ws = &h.shell.session().workspace;
    ws.focused().and_then(|t| ws.tab(t)).map(|t| &t.kind) == Some(kind)
}

fn focused_tab(h: &Harness) -> TabId {
    h.shell
        .session()
        .workspace
        .focused()
        .expect("a focused Tab")
}

/// An error state: its Retry and Details.
fn error_shown(h: &Harness) -> bool {
    shows(h, RETRY) && shows(h, DETAILS)
}

/// A loading state's line: what is loading.
fn loading_shown(h: &Harness) -> bool {
    [
        "Loading",
        "Looking up",
        "Checking",
        "Searching",
        "Finding",
        "Reading",
    ]
    .iter()
    .any(|s| shows_start(h, s))
}

/// The Library with nothing in it.
fn empty_library(h: &mut Harness) {
    h.shell.st.session.library = Library::new();
    h.frames(2);
}

/// Frames leave Guide sections, Word Study headers and the directories loading, and the
/// Cross-references, places and Timeline queries pending.
fn hold(h: &mut Harness) {
    h.shell.hold_jobs = true;
    h.shell.st.references.hold();
}

fn open_split(h: &mut Harness, id: &str, mut params: Value) {
    params["split"] = json!(true);
    h.shell.run(id, &params);
    h.frames(2);
}

/// A Tab of `kind` with nothing to show yet: no Passage, Strong's number, name or topic.
fn open_unkeyed(h: &mut Harness, kind: TabKind) {
    let title = match &kind {
        TabKind::Guide(g) => g.title().to_string(),
        TabKind::Tool(t) => t.title().to_string(),
        other => panic!("{other:?} has no Key"),
    };
    let request = obs_engine::OpenRequest {
        panel: None,
        split: true,
    };
    // Opened from the Library Tab, so it joins no Link set and has nowhere to be yet.
    let library = h.tab_id("Library");
    h.shell.st.session.workspace.user_focused(library);
    let tab = h.shell.st.session.workspace.open(kind, title, request);
    h.shell.st.session.workspace.user_focused(tab);
    h.frames(2);
}

/// A held query of a Tool opened with `id`, then failed.
fn failed_query(h: &mut Harness, id: &str, params: Value) {
    hold(h);
    open_split(h, id, params);
    h.shell.st.references.fail_held(DETAIL);
    h.frames(2);
}

/// A Guide opened with `id` whose sections all fail.
fn failed_sections(h: &mut Harness, id: &str, params: Value) {
    h.shell
        .st
        .session
        .guides
        .set_source(Arc::new(FailingSections));
    open_split(h, id, params);
    settle(h);
}

/// Where an unreadable Resource's file is moved to.
fn moved_away(path: &std::path::Path) -> std::path::PathBuf {
    path.with_extension("obr-away")
}

/// Resource `id`'s file can no longer be read (it is moved away), as when a disk fails.
fn unreadable(h: &mut Harness, id: &str) {
    let path = h.shell.st.session.library.get(id).expect(id).path.clone();
    std::fs::rename(&path, moved_away(&path)).unwrap();
    h.shell.st.docs.remove(id);
}

/// Resource `id`'s file is back where it was.
fn readable_again(h: &Harness, id: &str) {
    let path = h.shell.st.session.library.get(id).expect(id).path.clone();
    std::fs::rename(moved_away(&path), &path).unwrap();
}

/// The error states whose surface reads a Resource's file on the UI thread (so Retry reads it
/// again at once, with no loading state): the state, and the Resource made unreadable.
const UNREADABLE: [(&str, &str); 2] = [
    ("Resource: error", "mhc"),
    ("Text Comparison: error", "kjv-strongs"),
];

/// The error the focused Tab's error state was given: the read error of an unreadable
/// Resource, or [`DETAIL`].
fn the_error(h: &Harness) -> String {
    let t = &h.shell.st.tabs[&focused_tab(h)];
    (t.open_error.clone())
        .or_else(|| t.comparison_errors.values().next().cloned())
        .unwrap_or_else(|| DETAIL.into())
}

/// A Resource Tab for `id` with its Contents pane open.
fn open_contents(h: &mut Harness, id: &str) {
    let out = h
        .shell
        .run("resource.open", &json!({ "id": id, "split": true }));
    let tab = out.unwrap()["tab"].as_u64().unwrap();
    h.shell
        .run("tab.contents", &json!({ "tab": tab, "on": true }));
    h.frames(2);
}

/// Each surface's empty, loading and error states, as the Screenshot gallery draws them: a
/// name, what brings the state about, and what shows it is on screen.
pub(super) fn pane_states() -> Vec<Overlay> {
    vec![
        (
            "Library: empty",
            |h| {
                empty_library(h);
                open_split(h, "library.open", json!({}));
            },
            |h| {
                focused(h, &TabKind::Library)
                    && shows(h, crate::shell::missing_resource::ADD_STARTER)
            },
        ),
        (
            "Library: nothing matches",
            |h| {
                open_split(h, "library.open", json!({}));
                let tab = focused_tab(h);
                h.shell.st.tabs.get_mut(&tab).unwrap().filter = "zzyzx".into();
            },
            |h| focused(h, &TabKind::Library) && shows(h, "Clear the filter"),
        ),
        (
            "Get Resources: loading",
            |h| {
                hold(h);
                open_split(h, "library.get_resources", json!({}));
                h.shell.run("getbibles.refresh", &json!({}));
            },
            |h| shows(h, "Checking SWORD repositories for Resources…"),
        ),
        (
            "Get Resources: offline",
            |h| {
                h.shell.st.session.set_fetch(Arc::new(Offline));
                open_split(h, "library.get_resources", json!({}));
                h.shell.run("getbibles.refresh", &json!({}));
                settle(h);
            },
            |h| focused(h, &TabKind::GetBibles) && error_shown(h),
        ),
        (
            "Get Resources: nothing matches",
            |h| {
                open_split(h, "library.get_resources", json!({}));
                settle(h);
                h.shell.st.get_bibles_ui.query = "zzyzx".into();
            },
            |h| shows(h, "Show everything"),
        ),
        (
            "Starter library: loading",
            |h| {
                hold(h);
                open_split(h, "library.starter", json!({}));
                h.shell.run("starter.check", &json!({}));
            },
            |h| shows(h, "Loading the Starter library list…"),
        ),
        (
            "Starter library: error",
            |h| {
                h.shell.st.session.set_fetch(Arc::new(Offline));
                open_split(h, "library.starter", json!({}));
                h.shell.run("starter.check", &json!({}));
                settle(h);
            },
            |h| focused(h, &TabKind::StarterLibrary) && error_shown(h),
        ),
        (
            "Search: empty",
            |h| open_split(h, "tool.search", json!({})),
            |h| shows(h, "Search for “love”"),
        ),
        (
            "Search: searching",
            |h| {
                hold(h);
                open_split(h, "tool.search", json!({}));
                h.shell.run("search.run", &json!({ "text": "God" }));
                h.frames(2);
            },
            |h| shows(h, "Searching…"),
        ),
        (
            "Search: error",
            |h| {
                open_split(h, "tool.search", json!({}));
                let tab = focused_tab(h);
                let t = h.shell.st.session.workspace.tab_mut(tab).unwrap();
                t.search.query = "God".into();
                t.search.error = Some(InlineError {
                    field: ErrorField::Search,
                    message: DETAIL.into(),
                    span: None,
                });
            },
            |h| focused(h, &TabKind::Tool(Tool::Search)) && error_shown(h),
        ),
        (
            "Search: nothing matches",
            |h| {
                // The launch's search cache is in its own folder: build its indexes first.
                h.shell.st.session.refresh_search_indexes();
                settle_search(h);
                run_split(h, "tool.search", json!({}));
                h.shell.run("search.run", &json!({ "text": "zzyzx" }));
                settle_search(h);
            },
            |h| shows(h, "Edit the search"),
        ),
        (
            "Notes: empty",
            |h| open_split(h, "tool.notes", json!({})),
            |h| focused(h, &TabKind::Tool(Tool::Notes)) && shows(h, "Open the Library"),
        ),
        (
            "Passage Guide: empty",
            |h| open_unkeyed(h, TabKind::Guide(Guide::PassageGuide)),
            |h| shows(h, "Show John 3:16-18"),
        ),
        (
            "Passage Guide: loading",
            |h| {
                hold(h);
                open_split(h, "guide.passage_guide", json!({ "passage": "Jn 3:16" }));
            },
            |h| focused(h, &TabKind::Guide(Guide::PassageGuide)) && loading_shown(h),
        ),
        (
            "Passage Guide: error",
            |h| failed_sections(h, "guide.passage_guide", json!({ "passage": "Jn 3:16" })),
            |h| focused(h, &TabKind::Guide(Guide::PassageGuide)) && error_shown(h),
        ),
        (
            "Exegetical Guide: empty",
            |h| open_unkeyed(h, TabKind::Guide(Guide::ExegeticalGuide)),
            |h| {
                focused(h, &TabKind::Guide(Guide::ExegeticalGuide)) && shows(h, "Show John 3:16-18")
            },
        ),
        (
            "Exegetical Guide: loading",
            |h| {
                hold(h);
                open_split(h, "guide.exegetical_guide", json!({ "passage": "Jn 3:16" }));
            },
            |h| focused(h, &TabKind::Guide(Guide::ExegeticalGuide)) && loading_shown(h),
        ),
        (
            "Exegetical Guide: error",
            |h| failed_sections(h, "guide.exegetical_guide", json!({ "passage": "Jn 3:16" })),
            |h| focused(h, &TabKind::Guide(Guide::ExegeticalGuide)) && error_shown(h),
        ),
        (
            "Word Study: empty",
            |h| open_unkeyed(h, TabKind::Guide(Guide::WordStudy)),
            |h| shows(h, "Show G26 (love)"),
        ),
        (
            "Word Study: loading",
            |h| {
                hold(h);
                open_split(h, "guide.word_study", json!({ "strongs": "G26" }));
            },
            |h| focused(h, &TabKind::Guide(Guide::WordStudy)) && loading_shown(h),
        ),
        (
            "Word Study: error",
            |h| failed_sections(h, "guide.word_study", json!({ "strongs": "G26" })),
            |h| focused(h, &TabKind::Guide(Guide::WordStudy)) && error_shown(h),
        ),
        (
            "Word Study chooser: loading",
            |h| {
                hold(h);
                open_split(h, "word_study.choose", json!({ "words": "love" }));
            },
            |h| shows(h, "Looking up “love”…"),
        ),
        (
            "Word Study chooser: error",
            |h| {
                hold(h);
                open_split(h, "word_study.choose", json!({ "words": "love" }));
                h.shell.st.session.fail_pending_word_studies(DETAIL);
            },
            |h| shows(h, "“love” could not be looked up.") && error_shown(h),
        ),
        (
            "Factbook: empty",
            |h| open_unkeyed(h, TabKind::Guide(Guide::Factbook)),
            |h| shows(h, "Show Abraham"),
        ),
        (
            "Factbook: loading",
            |h| {
                hold(h);
                open_split(h, "guide.factbook", json!({ "name": "John" }));
            },
            |h| focused(h, &TabKind::Guide(Guide::Factbook)) && loading_shown(h),
        ),
        (
            "Factbook: error",
            |h| {
                let entity = json!({ "entity": "Abraham@Gen.11.26-1Pe" });
                failed_sections(h, "guide.factbook", entity);
            },
            |h| focused(h, &TabKind::Guide(Guide::Factbook)) && error_shown(h),
        ),
        (
            "Topic Guide: empty",
            |h| open_unkeyed(h, TabKind::Guide(Guide::TopicGuide)),
            |h| shows(h, "Show Forgiveness"),
        ),
        (
            "Topic Guide: loading",
            |h| {
                hold(h);
                open_split(h, "guide.topic_guide", json!({ "topic": "Forgiveness" }));
            },
            |h| focused(h, &TabKind::Guide(Guide::TopicGuide)) && loading_shown(h),
        ),
        (
            "Topic Guide: error",
            |h| failed_sections(h, "guide.topic_guide", json!({ "topic": "Forgiveness" })),
            |h| focused(h, &TabKind::Guide(Guide::TopicGuide)) && error_shown(h),
        ),
        (
            "Cross-references: empty",
            |h| open_unkeyed(h, TabKind::Tool(Tool::CrossReferences)),
            |h| shows(h, "Show John 3:16"),
        ),
        (
            "Cross-references: loading",
            |h| {
                hold(h);
                open_split(h, "tool.cross_references", json!({ "passage": "Jn 3:16" }));
            },
            |h| shows(h, "Loading Cross-references…"),
        ),
        (
            "Cross-references: error",
            |h| failed_query(h, "tool.cross_references", json!({ "passage": "Jn 3:16" })),
            |h| focused(h, &TabKind::Tool(Tool::CrossReferences)) && error_shown(h),
        ),
        (
            "Atlas: empty",
            |h| open_unkeyed(h, TabKind::Tool(Tool::Atlas)),
            |h| shows(h, "Show Acts 13"),
        ),
        (
            "Atlas: nothing in the library",
            |h| {
                empty_library(h);
                open_split(h, "tool.atlas", json!({ "passage": "Acts 13" }));
            },
            |h| {
                focused(h, &TabKind::Tool(Tool::Atlas))
                    && shows(h, crate::shell::missing_resource::ADD_STARTER)
            },
        ),
        (
            "Atlas: loading",
            |h| {
                hold(h);
                open_split(h, "tool.atlas", json!({ "passage": "Acts 13" }));
            },
            |h| shows(h, "Finding the places of Acts 13…"),
        ),
        (
            "Atlas: error",
            |h| failed_query(h, "tool.atlas", json!({ "passage": "Acts 13" })),
            |h| focused(h, &TabKind::Tool(Tool::Atlas)) && error_shown(h),
        ),
        (
            "Timeline: nothing in the library",
            |h| {
                empty_library(h);
                open_split(h, "tool.timeline", json!({}));
                settle(h);
            },
            |h| {
                focused(h, &TabKind::Tool(Tool::Timeline))
                    && shows(h, crate::shell::missing_resource::ADD_STARTER)
            },
        ),
        (
            "Timeline: loading",
            |h| {
                hold(h);
                open_split(h, "tool.timeline", json!({}));
            },
            |h| shows(h, "Reading the Timeline…"),
        ),
        (
            "Timeline: error",
            |h| failed_query(h, "tool.timeline", json!({})),
            |h| focused(h, &TabKind::Tool(Tool::Timeline)) && error_shown(h),
        ),
        (
            "Resource: nothing to read",
            |h| open_split(h, "resource.open", json!({ "id": "tipnr" })),
            |h| shows_start(h, "TIPNR has no text to read") && shows(h, "Show Abraham"),
        ),
        (
            "Resource: error",
            |h| {
                unreadable(h, "mhc");
                open_split(h, "resource.open", json!({ "id": "mhc" }));
            },
            |h| shows(h, "MHC could not be opened.") && error_shown(h),
        ),
        (
            "Contents: loading",
            |h| {
                h.shell.st.contents.hold = true;
                open_contents(h, "kjv-strongs");
            },
            |h| shows(h, "Loading contents…"),
        ),
        (
            "Contents: empty",
            |h| open_contents(h, "tipnr"),
            |h| shows(h, "Close Contents"),
        ),
        (
            "Text Comparison: error",
            |h| {
                unreadable(h, "kjv-strongs");
                open_split(h, "tool.text_comparison", json!({ "passage": "Gen 1:1" }));
            },
            |h| {
                focused(h, &TabKind::Tool(Tool::TextComparison))
                    && shows(h, "KJV could not be opened.")
                    && error_shown(h)
            },
        ),
        (
            "Favorites: empty",
            |h| open_split(h, "tool.favorites", json!({})),
            |h| focused(h, &TabKind::Tool(Tool::Favorites)) && shows(h, "Open the Library"),
        ),
        (
            "History: empty",
            |h| {
                h.shell.run("history.clear", &json!({}));
                open_split(h, "tool.history", json!({}));
            },
            |h| focused(h, &TabKind::Tool(Tool::History)) && shows(h, "Open the Library"),
        ),
    ]
}

/// A launch with the gallery's fixtures, in the state `name` of [`pane_states`].
fn open_state(name: &str) -> (Harness, super::missing_resource::Lab) {
    let (_, open, is_open) = pane_states()
        .into_iter()
        .find(|(n, ..)| *n == name)
        .unwrap_or_else(|| panic!("no state {name}"));
    // A folder name on every platform (no `:`).
    let folder: String = (name.chars())
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let (mut h, lab) =
        super::gallery::launch(&format!("states-{folder}"), &super::gallery::LOOKS[0]);
    open(&mut h);
    h.frames(4);
    assert!(is_open(&h), "{name} is not on screen: {:?}", h.texts());
    (h, lab)
}

/// Acceptance (#223): every error state says what failed in plain words, with Retry and
/// Details; the error itself shows only after Details, and Retry asks again (the surface is
/// loading, not failed).
#[test]
fn error_states_hide_the_error_until_details_and_retry_asks_again() {
    let errors: Vec<&str> = pane_states()
        .into_iter()
        .map(|(n, ..)| n)
        .filter(|n| n.ends_with(": error") || n.ends_with(": offline"))
        .collect();
    assert_eq!(errors.len(), 14, "{errors:?}");
    for name in errors {
        let (mut h, _lab) = open_state(name);
        let error = the_error(&h);
        let raw = |h: &Harness| h.texts().iter().any(|t| t.contains(&error));
        assert!(
            !raw(&h),
            "{name}: the raw error is on screen: {:?}",
            h.texts()
        );
        h.click_text(DETAILS);
        assert!(raw(&h), "{name}: Details does not show the error");
        assert!(shows(&h, HIDE_DETAILS), "{name}: no Hide details");
        h.click_text(HIDE_DETAILS);
        assert!(!raw(&h), "{name}: Hide details leaves the error shown");
        if let Some((_, id)) = UNREADABLE.iter().find(|(n, _)| *n == name) {
            // Read on the UI thread: Retry reads the file again at once.
            readable_again(&h, id);
            h.click_text(RETRY);
            h.frames(2);
            assert!(
                !error_shown(&h),
                "{name}: Retry did not read the Resource again: {:?}",
                h.texts()
            );
            continue;
        }
        // Retry, with nothing running the new attempt yet.
        h.shell.hold_jobs = true;
        h.click_text(RETRY);
        h.frames(2);
        assert!(
            loading_shown(&h),
            "{name}: Retry did not ask again: {:?}",
            h.texts()
        );
    }
}

/// Acceptance (#223): every empty state explains itself and offers one Button; clicking it
/// changes what the Tab shows.
#[test]
fn empty_states_offer_a_button_that_fills_the_tab() {
    let cases = [
        ("Passage Guide: empty", "Show John 3:16-18", "John 3:16-18"),
        (
            "Exegetical Guide: empty",
            "Show John 3:16-18",
            "John 3:16-18",
        ),
        ("Word Study: empty", "Show G26 (love)", "G26"),
        ("Atlas: empty", "Show Acts 13", "Acts 13"),
        ("Cross-references: empty", "Show John 3:16", "John 3:16"),
    ];
    for (name, button, shown) in cases {
        let (mut h, _lab) = open_state(name);
        h.click_text(button);
        settle(&mut h);
        assert!(!shows(&h, button), "{name}: still empty: {:?}", h.texts());
        assert!(
            shows_start(&h, shown) || h.texts().iter().any(|t| t.contains(shown)),
            "{name}: {shown} not shown: {:?}",
            h.texts()
        );
    }
    let (mut h, _lab) = open_state("Library: nothing matches");
    h.click_text("Clear the filter");
    assert!(
        shows(&h, "KJV"),
        "the Library's rows came back: {:?}",
        h.texts()
    );
    let (mut h, _lab) = open_state("Library: empty");
    h.click_text(crate::shell::missing_resource::ADD_STARTER);
    assert!(focused(&h, &TabKind::StarterLibrary));
    let (mut h, _lab) = open_state("Resource: nothing to read");
    h.click_text("Show Abraham");
    settle(&mut h);
    assert!(focused(&h, &TabKind::Guide(Guide::Factbook)));
    let (mut h, _lab) = open_state("Contents: empty");
    let tab = focused_tab(&h);
    h.click_text("Close Contents");
    let ws = &h.shell.session().workspace;
    assert!(
        !ws.tab(tab).unwrap().contents,
        "the Contents pane is still open"
    );
}

/// Acceptance (#223): with nothing in the Library, no Tab is blank: each says something in
/// its own pane (a Resource and its About need a Resource, so they are left out).
#[test]
fn no_tab_is_blank_in_an_empty_library() {
    for (i, kind) in every_tab_kind().into_iter().enumerate() {
        if matches!(kind, TabKind::Resource(_) | TabKind::About(_)) {
            continue;
        }
        let (mut h, lab) = setup(&format!("states-blank-{i}"), &[]);
        open_tab(&mut h, &lab, &kind);
        settle(&mut h);
        let tab = focused_tab(&h);
        let tiles = &h.shell.tree.tiles;
        let pane = tiles
            .find_pane(&tab)
            .and_then(|p| tiles.rect(p))
            .expect("the Tab's pane");
        let body = egui::Rect::from_min_max(
            egui::pos2(pane.left(), pane.top() + crate::tokens::size::TAB_BAR),
            pane.max,
        );
        let said: Vec<&str> = (h.shapes.iter())
            .filter(|(r, t)| body.contains(r.center()) && !t.trim().is_empty())
            .map(|(_, t)| t.as_str())
            .collect();
        assert!(!said.is_empty(), "{kind:?} is blank in an empty Library");
    }
}

/// Acceptance (#223): a Cross-reference whose preview cannot be read (the ★ Bible's file is
/// gone) says so in plain words, never with the read error.
#[test]
fn an_unreadable_cross_reference_preview_says_so_plainly() {
    let (mut h, _lab) = super::gallery::launch("states-preview", &super::gallery::LOOKS[0]);
    unreadable(&mut h, "kjv-strongs");
    let passage =
        obs_scripture::Passage::verse(obs_scripture::Verse::new(obs_scripture::Book::John, 3, 16));
    let error =
        obs_engine::cross_references::preferred_preview(&h.shell.st.session.library, passage)
            .expect_err("the ★ Bible cannot be read");
    open_split(
        &mut h,
        "tool.cross_references",
        json!({ "passage": "Jn 3:16" }),
    );
    settle(&mut h);
    assert!(
        shows(&h, "The preview could not be read."),
        "{:?}",
        h.texts()
    );
    assert!(
        !h.texts().iter().any(|t| t.contains(&error)),
        "the raw error is on screen: {error}"
    );
}
