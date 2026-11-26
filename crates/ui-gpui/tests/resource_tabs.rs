//! Resource Tabs (#380 in the Workspace of #385): each Resource Tab's reading pane, kept across
//! frames, moved by the Tab's navigation; and the egui `pane_states_tests` behaviours of a
//! Resource Tab (#223), driven headlessly on the TestPlatform.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use gpui_kit::{AnyWindowHandle, AppContext as _, Bounds, Entity, TestAppContext, point, px, size};
use obs_engine::{TabId, TabKind};
use obs_model::Key;
use obs_scripture::Verse;
use obs_ui_gpui::Shell;
use obs_ui_gpui::workspace::{DETAILS, HIDE_DETAILS, RETRY};
use serde_json::json;
use support::Driver;

fn open(cx: &mut TestAppContext, name: &str) -> (AnyWindowHandle, Entity<Shell>, support::Lab) {
    open_with(cx, name, &[])
}

/// [`open`], with `extra` in the Library too.
fn open_with(
    cx: &mut TestAppContext,
    name: &str,
    extra: &[obs_model::Resource],
) -> (AnyWindowHandle, Entity<Shell>, support::Lab) {
    let (mut session, lab) = support::session(name);
    for r in extra {
        support::add_resource(&mut session, &lab, r);
    }
    session.appearance.reduce_motion = true;
    let (handle, shell) = cx.update(|cx| {
        obs_ui_gpui::install(cx);
        let bounds = Bounds {
            origin: point(px(0.), px(0.)),
            size: size(
                px(obs_ui_gpui::tokens::WINDOW_WIDTH),
                px(obs_ui_gpui::tokens::WINDOW_HEIGHT),
            ),
        };
        let options = obs_ui_gpui::main_window_options(Some(bounds), cx);
        obs_ui_gpui::open_main_window(session, options, cx).unwrap()
    });
    (handle, shell, lab)
}

fn close(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    shell: Entity<Shell>,
    lab: support::Lab,
) {
    drop(shell);
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    drop(lab);
}

fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    for _ in 0..4 {
        cx.update_window(handle, |_, window, cx| window.frame(cx))
            .unwrap();
        cx.run_until_parked();
    }
}

fn run(cx: &mut TestAppContext, shell: &Entity<Shell>, id: &str, params: serde_json::Value) {
    cx.update(|cx| {
        shell.update(cx, |s, cx| {
            s.run(id, &params);
            cx.notify();
        })
    });
}

fn open_tab(cx: &mut TestAppContext, shell: &Entity<Shell>, id: &str) -> TabId {
    cx.update(|cx| {
        shell.update(cx, |s, cx| {
            cx.notify();
            s.run("resource.open", &json!({ "id": id, "split": true }))
        })
    })
    .and_then(|v| v["tab"].as_u64())
    .map(TabId)
    .unwrap_or_else(|| panic!("{id} did not open"))
}

fn title(cx: &mut TestAppContext, shell: &Entity<Shell>, tab: TabId) -> String {
    cx.update(|cx| {
        let ws = &shell.read(cx).session().workspace;
        ws.tab(tab).unwrap().title.clone()
    })
}

/// Where Tab `tab`'s pane is, and where the engine has the Tab.
fn at(
    cx: &mut TestAppContext,
    shell: &Entity<Shell>,
    tab: TabId,
) -> (Option<Verse>, Option<Verse>) {
    cx.update(|cx| {
        let pane = shell.read(cx).pane(tab).expect("the Tab's pane").clone();
        let shown = pane.update(cx, |p, _| p.view_mut().current_location());
        let ws = &shell.read(cx).session().workspace;
        (shown, ws.tab(tab).unwrap().history.current())
    })
}

/// The unit at the top of Tab `tab`'s pane (the egui tests' `shown_unit`).
fn shown_unit(cx: &mut TestAppContext, shell: &Entity<Shell>, tab: TabId) -> String {
    cx.update(|cx| {
        let pane = shell.read(cx).pane(tab).expect("the Tab's pane").clone();
        pane.update(cx, |p, _| {
            let view = p.view_mut();
            let top = view.visible(400.0)[0].index;
            view.doc.units[view.doc.blocks[top].unit].id.to_string()
        })
    })
}

/// The Key Tab `tab` is at (its header box), and the entry at the top of its pane.
fn entry_at(
    cx: &mut TestAppContext,
    shell: &Entity<Shell>,
    tab: TabId,
) -> (Option<Key>, Option<Key>) {
    cx.update(|cx| {
        let pane = shell.read(cx).pane(tab).expect("the Tab's pane").clone();
        let shown = pane.read(cx).view().current_entry();
        let ws = &shell.read(cx).session().workspace;
        (ws.tab(tab).unwrap().entry_history.current(), shown)
    })
}

fn strongs(number: &str) -> Option<Key> {
    Some(Key::StrongsNumber(number.parse().unwrap()))
}

/// Open Resource `id` in a Resource Tab with the keyboard in its pane: the Tab.
fn open_reading(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    shell: &Entity<Shell>,
    id: &str,
) -> TabId {
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        shell.update(cx, |s, cx| {
            s.open_reading(id, None, window, cx).unwrap();
            s.session().workspace.focused().unwrap()
        })
    })
    .unwrap()
}

fn genesis(chapter: u16, verse: u16) -> Option<Verse> {
    Some(Verse::new(obs_scripture::Book::Genesis, chapter, verse))
}

/// Port of resource_tabs_tests `typing_a_reference_in_the_header_box_goes_there` and
/// the Back and Forward buttons, on the pane: the header box, Back and Forward move the
/// Tab's pane, and the same pane is kept across frames.
#[gpui_kit::test]
fn tab_navigation_moves_the_tabs_pane(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "rt-navigate");
    let bsb = cx
        .update_window(handle, |_, window, cx| {
            window.frame(cx);
            shell.update(cx, |s, cx| {
                let pane = s.open_reading("bsb", Some("Gen 1:1"), window, cx).unwrap();
                (pane, s.session().workspace.focused().unwrap())
            })
        })
        .unwrap();
    let (pane, tab) = bsb;
    settle(cx, handle);
    assert_eq!(at(cx, &shell, tab), (genesis(1, 1), genesis(1, 1)));
    let name = title(cx, &shell, tab);
    let label = format!("Go to a reference in {name}");
    cx.update_window(handle, |_, window, cx| {
        window.click_label(&label, cx);
        window.press("secondary-a", cx);
        window.type_text("Gen 1:3", cx);
        window.press("enter", cx);
    })
    .unwrap();
    settle(cx, handle);
    assert_eq!(at(cx, &shell, tab), (genesis(1, 3), genesis(1, 3)));
    cx.update_window(handle, |_, window, cx| {
        window.click_label(&format!("Back in {name}"), cx);
    })
    .unwrap();
    settle(cx, handle);
    assert_eq!(at(cx, &shell, tab), (genesis(1, 1), genesis(1, 1)));
    cx.update_window(handle, |_, window, cx| {
        window.click_label(&format!("Forward in {name}"), cx);
    })
    .unwrap();
    settle(cx, handle);
    assert_eq!(at(cx, &shell, tab), (genesis(1, 3), genesis(1, 3)));
    // `tab.go` from a command moves it too.
    run(
        cx,
        &shell,
        "tab.go",
        json!({ "tab": tab.0, "text": "Gen 1:2" }),
    );
    settle(cx, handle);
    assert_eq!(at(cx, &shell, tab), (genesis(1, 2), genesis(1, 2)));
    let same = cx.update(|cx| shell.read(cx).pane(tab).unwrap().entity_id());
    assert_eq!(same, pane.entity_id(), "one pane for the Tab, kept");
    drop(pane);
    close(cx, handle, shell, lab);
}

/// Port of resource_tabs_tests `scrolling_updates_the_location`: the user scrolls the pane, and
/// the Tab's location (its header box) follows.
#[gpui_kit::test]
fn scrolling_the_pane_moves_the_tabs_location(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "rt-scroll");
    let tab = cx
        .update_window(handle, |_, window, cx| {
            window.frame(cx);
            shell.update(cx, |s, cx| {
                s.open_reading("bsb", Some("Gen 1:1"), window, cx).unwrap();
                s.session().workspace.focused().unwrap()
            })
        })
        .unwrap();
    settle(cx, handle);
    for _ in 0..3 {
        cx.update_window(handle, |_, window, cx| window.press("pagedown", cx))
            .unwrap();
        settle(cx, handle);
    }
    let (shown, engine) = at(cx, &shell, tab);
    assert!(shown > genesis(1, 1), "the pane moved: {shown:?}");
    assert_eq!(engine, shown, "the Tab's location follows its pane");
    close(cx, handle, shell, lab);
}

/// Port of pane_states_tests "Resource: nothing to read" and
/// `empty_states_offer_a_button_that_fills_the_tab`: a Resource with no text says where its
/// data shows, and its Button goes there.
#[gpui_kit::test]
fn a_resource_with_nothing_to_read_offers_where_it_shows(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "rt-nothing");
    open_tab(cx, &shell, "tipnr");
    settle(cx, handle);
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        window.click_label("Show Abraham", cx);
    })
    .unwrap();
    settle(cx, handle);
    let focused = cx.update(|cx| {
        let ws = &shell.read(cx).session().workspace;
        ws.focused().and_then(|t| ws.tab(t)).map(|t| t.kind.clone())
    });
    assert_eq!(
        focused,
        Some(TabKind::Guide(obs_engine::Guide::Factbook)),
        "Show Abraham opened the Factbook"
    );
    close(cx, handle, shell, lab);
}

/// Port of pane_states_tests "Resource: error" and
/// `error_states_hide_the_error_until_details_and_retry_asks_again`: an unreadable Resource
/// says so in plain words with Retry and Details; the error itself shows only after Details;
/// Retry reads the Resource again.
#[gpui_kit::test]
fn an_unreadable_resource_hides_its_error_until_details_and_retry_reads_it_again(
    cx: &mut TestAppContext,
) {
    let (handle, shell, lab) = open(cx, "rt-error");
    let path = cx.update(|cx| {
        shell
            .read(cx)
            .session()
            .library
            .get("mhc")
            .unwrap()
            .path
            .clone()
    });
    let away = path.with_extension("obr-away");
    std::fs::rename(&path, &away).unwrap();
    let tab = open_tab(cx, &shell, "mhc");
    settle(cx, handle);
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        let shown = window.find_by_label("MHC could not be opened.");
        assert!(shown.visible());
        let raw = |window: &gpui_kit::Window| {
            window
                .elements()
                .iter()
                .any(|e| e.label().is_some_and(|l| l.contains("obr")))
        };
        assert!(!raw(window), "the raw error is on screen");
        window.click_label(DETAILS, cx);
        window.frame(cx);
        assert!(raw(window), "Details shows the error");
        window.click_label(HIDE_DETAILS, cx);
        window.frame(cx);
        assert!(!raw(window), "Hide details hides it");
    })
    .unwrap();
    std::fs::rename(&away, &path).unwrap();
    cx.update_window(handle, |_, window, cx| window.click_label(RETRY, cx))
        .unwrap();
    settle(cx, handle);
    let read = cx.update(|cx| shell.read(cx).pane(tab).is_some());
    assert!(read, "Retry read the Resource again");
    close(cx, handle, shell, lab);
}

/// Port of recently_closed_tests: a closed Resource Tab's pane goes with it, and the Tab
/// reopened has a pane again.
#[gpui_kit::test]
fn a_closed_tabs_pane_goes_and_comes_back_when_reopened(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "rt-close");
    let tab = open_tab(cx, &shell, "bsb");
    settle(cx, handle);
    assert!(cx.update(|cx| shell.read(cx).pane(tab).is_some()));
    run(cx, &shell, "tab.close", json!({ "tab": tab.0 }));
    settle(cx, handle);
    assert!(cx.update(|cx| shell.read(cx).pane(tab).is_none()));
    run(cx, &shell, "tab.reopen_closed", json!({}));
    settle(cx, handle);
    let again = cx.update(|cx| {
        let s = shell.read(cx);
        let ws = &s.session().workspace;
        let t = ws
            .tabs()
            .find(|t| t.kind == TabKind::Resource("bsb".into()))
            .map(|t| t.id)
            .unwrap();
        s.pane(t).is_some()
    });
    assert!(again, "the reopened Tab has its pane");
    close(cx, handle, shell, lab);
}

/// Port of resource_tabs_tests
/// `g26_typed_in_lexicon_tab_renders_entry_with_shared_pipeline_and_entry_navigation` and
/// `dictionary_headword_alias_and_saved_lexicon_location_reopen_correct_entries`, on the pane:
/// a Lexicon Tab's pane starts at its first entry; the Key box, Back and Forward move it; and
/// the Workspace restored reopens it at the Tab's Key.
#[gpui_kit::test]
fn the_key_box_back_and_forward_move_a_lexicon_tabs_pane(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open_with(cx, "rt-lexicon", &[support::study_lexicon()]);
    let tab = open_reading(cx, handle, &shell, "test-lexicon");
    settle(cx, handle);
    assert_eq!(shown_unit(cx, &shell, tab), "G25");
    assert_eq!(
        entry_at(cx, &shell, tab).0,
        strongs("G25"),
        "the Tab is at its first entry"
    );
    let name = title(cx, &shell, tab);
    let label = format!("Go to an entry in {name}");
    for number in ["G26", "G27"] {
        cx.update_window(handle, |_, window, cx| {
            window.click_label(&label, cx);
            window.press("secondary-a", cx);
            window.type_text(number, cx);
            window.press("enter", cx);
        })
        .unwrap();
        settle(cx, handle);
        assert_eq!(shown_unit(cx, &shell, tab), number, "typed {number}");
        assert_eq!(entry_at(cx, &shell, tab).0, strongs(number));
    }
    for (button, number) in [("Back", "G26"), ("Back", "G25"), ("Forward", "G26")] {
        cx.update_window(handle, |_, window, cx| {
            window.click_label(&format!("{button} in {name}"), cx);
        })
        .unwrap();
        settle(cx, handle);
        assert_eq!(shown_unit(cx, &shell, tab), number, "{button} to {number}");
        assert_eq!(entry_at(cx, &shell, tab).0, strongs(number));
    }
    // The header box shows the Key.
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        assert_eq!(window.find_by_label(&label).value(), Some("G26"));
    })
    .unwrap();
    // A Layout or the Last session restored reads the pane again, at the Tab's Key.
    cx.update(|cx| {
        shell.update(cx, |s, cx| {
            let snapshot = s.session().workspace.snapshot();
            s.session_mut().workspace.restore(snapshot);
            cx.notify();
        })
    });
    settle(cx, handle);
    let tab = cx.update(|cx| {
        let ws = &shell.read(cx).session().workspace;
        ws.tabs()
            .find(|t| t.kind == TabKind::Resource("test-lexicon".into()))
            .map(|t| t.id)
            .unwrap()
    });
    assert_eq!(
        shown_unit(cx, &shell, tab),
        "G26",
        "restored at the Tab's Key"
    );
    close(cx, handle, shell, lab);
}

/// Port of the egui Shell's `user_scrolled_entry`: the user scrolls a Lexicon Tab's pane, and
/// the Tab's Key (its header box, where Back returns from) is the entry at the top.
#[gpui_kit::test]
fn scrolling_a_lexicon_tabs_pane_moves_its_key(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open_with(cx, "rt-lexicon-scroll", &[support::long_lexicon()]);
    let tab = open_reading(cx, handle, &shell, "long-lexicon");
    settle(cx, handle);
    assert_eq!(entry_at(cx, &shell, tab), (strongs("G1"), strongs("G1")));
    for _ in 0..3 {
        cx.update_window(handle, |_, window, cx| window.press("pagedown", cx))
            .unwrap();
        settle(cx, handle);
    }
    let (key, shown) = entry_at(cx, &shell, tab);
    assert_ne!(shown, strongs("G1"), "the pane moved");
    assert_eq!(key, shown, "the Tab's Key follows its pane");
    close(cx, handle, shell, lab);
}

/// A pane not shown (its Tab behind another in its Panel) is never asked where it is, so it
/// lays no text out on the UI thread outside a frame's budget (`SYNC_BUDGET`).
#[gpui_kit::test]
fn a_pane_not_shown_lays_nothing_out_between_frames(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open_with(cx, "rt-hidden", &[support::long_lexicon()]);
    let hidden = open_reading(cx, handle, &shell, "long-lexicon");
    settle(cx, handle);
    let panel = cx.update(|cx| {
        let ws = &shell.read(cx).session().workspace;
        ws.panel_of(hidden).unwrap()
    });
    run(
        cx,
        &shell,
        "resource.open",
        json!({ "id": "bsb", "panel": panel.0 }),
    );
    settle(cx, handle);
    let shown = cx.update(|cx| shell.read(cx).session().workspace.active_in(panel));
    assert_ne!(shown, Some(hidden), "the Lexicon's Tab is behind the BSB's");
    let pane = cx.update(|cx| shell.read(cx).pane(hidden).unwrap().clone());
    // Moved far down while hidden, to text never laid out.
    let laid = pane.update(cx, |p, _| {
        let view = p.view_mut();
        let total = view.position().1;
        view.scroll_to_position(total * 0.9);
        view.layouts
    });
    settle(cx, handle);
    let after = pane.read_with(cx, |p, _| p.view().layouts);
    assert_eq!(after, laid, "the hidden pane laid text out");
    drop(pane);
    close(cx, handle, shell, lab);
}
