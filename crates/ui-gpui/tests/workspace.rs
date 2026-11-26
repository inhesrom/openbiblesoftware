//! The Workspace on GPUI (#385): the engine's Panels and Tabs in `gpui-base`'s dock, driven
//! headlessly (#379). Each test is a port of an egui test's behaviour, named in its doc line.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use gpui_kit::base::dock::{DockPlacement, InsertTarget, PanelId as DockPanelId};
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Role, TestAppContext, point, px, size,
};
use obs_engine::persist::LayoutNode;
use obs_engine::{TabId, TabKind};
use obs_ui_gpui::Shell;
use obs_ui_gpui::workspace::groups_below;
use serde_json::json;
use support::Driver;

fn open_with(
    cx: &mut TestAppContext,
    session: obs_engine::Session,
) -> (AnyWindowHandle, Entity<Shell>) {
    cx.update(|cx| {
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
    })
}

fn open(cx: &mut TestAppContext, name: &str) -> (AnyWindowHandle, Entity<Shell>, support::Lab) {
    let (session, lab) = support::session(name);
    let (handle, shell) = open_with(cx, session);
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

/// Draw frames until the dock and the engine have settled.
fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    for _ in 0..3 {
        cx.update_window(handle, |_, window, cx| window.frame(cx))
            .unwrap();
        cx.run_until_parked();
    }
}

fn run(
    cx: &mut TestAppContext,
    shell: &Entity<Shell>,
    id: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    cx.update(|cx| {
        shell.update(cx, |s, cx| {
            let out = s.run(id, &params);
            cx.notify();
            out
        })
    })
    .unwrap_or_else(|| panic!("{id} failed"))
}

/// The dock's tab groups, each as its engine Tabs, in order.
fn dock_groups(cx: &mut TestAppContext, shell: &Entity<Shell>) -> Vec<Vec<TabId>> {
    cx.update(|cx| {
        let s = shell.read(cx);
        let dock = s
            .dock()
            .expect("the dock is made with the first frame")
            .read(cx);
        let tree = dock.layout(DockPlacement::Center).unwrap();
        groups_below(tree.root())
            .into_iter()
            .map(|(_, panels, _)| {
                panels
                    .iter()
                    .filter_map(|p| s.dock_map().tabs.get(p).copied())
                    .collect::<Vec<_>>()
            })
            .filter(|g| !g.is_empty())
            .collect()
    })
}

/// The engine's Panels, each as its Tabs, in the dock's order.
fn engine_groups(cx: &mut TestAppContext, shell: &Entity<Shell>) -> Vec<Vec<TabId>> {
    let mut groups: Vec<Vec<TabId>> = cx.update(|cx| {
        shell
            .read(cx)
            .session()
            .workspace
            .panels()
            .map(|(_, t)| t.to_vec())
            .collect()
    });
    groups.sort();
    groups
}

fn sorted(mut v: Vec<Vec<TabId>>) -> Vec<Vec<TabId>> {
    v.sort();
    v
}

fn tab_of(cx: &mut TestAppContext, shell: &Entity<Shell>, kind: TabKind) -> TabId {
    cx.update(|cx| {
        shell
            .read(cx)
            .session()
            .workspace
            .tabs()
            .find(|t| t.kind == kind)
            .map(|t| t.id)
            .unwrap_or_else(|| panic!("no {kind:?} Tab"))
    })
}

/// Port of shell_tests `default_layout_has_bible_and_library_panels`: the default arrangement
/// is the ★ Bible in one Panel, in Link set A, and the Library and Word Info in a Panel to its
/// right, with Home on top on the first run (no Last session, #178); the dock shows what the
/// engine has.
#[gpui_kit::test]
fn the_default_layout_puts_the_bible_beside_the_library_and_word_info(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "ws-default");
    cx.update(|cx| shell.update(cx, |s, _| s.restore_at_launch()));
    settle(cx, handle);
    let groups = dock_groups(cx, &shell);
    assert_eq!(sorted(groups.clone()), engine_groups(cx, &shell));
    assert!(groups.len() >= 2, "two Panels at least: {groups:?}");
    let (bible, link, focused) = cx.update(|cx| {
        let ws = &shell.read(cx).session().workspace;
        let bible = ws.tabs().find(|t| t.kind.is_reading()).unwrap();
        (bible.id, bible.link, ws.focused())
    });
    assert_eq!(link, Some('A'));
    let home = tab_of(cx, &shell, TabKind::Home);
    assert_eq!(focused, Some(home), "Home opens on top at the first launch");
    assert!(
        groups[0].contains(&bible),
        "the Bible's Panel is first: {groups:?}"
    );
    let library = tab_of(cx, &shell, TabKind::Library);
    assert!(groups[1..].iter().any(|g| g.contains(&library)));
    close(cx, handle, shell, lab);
}

/// Port of resource_tabs_tests `split_opens_beside_the_focused_panel`: `split` opens a new
/// Panel to the right of the focused one; a plain open goes into the Panel it is remembered in.
#[gpui_kit::test]
fn opening_with_split_makes_a_new_panel_to_the_right(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "ws-split");
    let kjv = run(cx, &shell, "resource.open", json!({ "id": "kjv-strongs" }))["tab"]
        .as_u64()
        .unwrap();
    settle(cx, handle);
    let bsb = run(
        cx,
        &shell,
        "resource.open",
        json!({ "id": "bsb", "split": true }),
    )["tab"]
        .as_u64()
        .unwrap();
    settle(cx, handle);
    let groups = dock_groups(cx, &shell);
    assert_eq!(groups, vec![vec![TabId(kjv)], vec![TabId(bsb)]]);
    assert_eq!(sorted(groups), engine_groups(cx, &shell));
    // The arrangement saved in Layouts: a row of the two Panels.
    let arrangement = cx.update(|cx| shell.read(cx).session().workspace.arrangement.clone());
    assert!(
        matches!(&arrangement, Some(LayoutNode::Row { children }) if children.len() == 2),
        "{arrangement:?}"
    );
    close(cx, handle, shell, lab);
}

/// Port of recently_closed_tests `close_then_reopen_restores_tab_in_its_panel`: the Tab's ×
/// closes it (the engine is told, and Recently closed has it); Reopen closed Tab brings it
/// back into the dock.
#[gpui_kit::test]
fn the_close_button_closes_a_tab_and_reopen_brings_it_back(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "ws-close");
    run(cx, &shell, "resource.open", json!({ "id": "kjv-strongs" }));
    let bsb = run(cx, &shell, "resource.open", json!({ "id": "bsb" }))["tab"]
        .as_u64()
        .unwrap();
    settle(cx, handle);
    let title = cx.update(|cx| {
        shell
            .read(cx)
            .session()
            .workspace
            .tab(TabId(bsb))
            .unwrap()
            .title
            .clone()
    });
    cx.update_window(handle, |_, window, cx| {
        window.click_label(&format!("Close {title}"), cx);
    })
    .unwrap();
    settle(cx, handle);
    let gone = cx.update(|cx| shell.read(cx).session().workspace.tab(TabId(bsb)).is_none());
    assert!(gone, "the × closed the Tab in the engine");
    assert!(
        dock_groups(cx, &shell)
            .iter()
            .all(|g| !g.contains(&TabId(bsb)))
    );
    run(cx, &shell, "tab.reopen_closed", json!({}));
    settle(cx, handle);
    let bsb_again = tab_of(cx, &shell, TabKind::Resource("bsb".into()));
    assert!(
        dock_groups(cx, &shell)
            .iter()
            .any(|g| g.contains(&bsb_again))
    );
    assert_eq!(sorted(dock_groups(cx, &shell)), engine_groups(cx, &shell));
    close(cx, handle, shell, lab);
}

/// Port of drag_drop_tests `dragging_a_tab_onto_another_panel_moves_it`: a Tab dropped on
/// another Panel moves there (the dock's own drop, as a drag ends), and the engine follows.
#[gpui_kit::test]
fn a_tab_dropped_on_another_panel_moves_there(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "ws-drag");
    let kjv = TabId(
        run(cx, &shell, "resource.open", json!({ "id": "kjv-strongs" }))["tab"]
            .as_u64()
            .unwrap(),
    );
    let bsb = TabId(
        run(
            cx,
            &shell,
            "resource.open",
            json!({ "id": "bsb", "split": true }),
        )["tab"]
            .as_u64()
            .unwrap(),
    );
    settle(cx, handle);
    cx.update_window(handle, |_, window, cx| {
        let (dock, id, node) = {
            let s = shell.read(cx);
            let dock = s.dock().unwrap().clone();
            let id = DockPanelId::from(s.dock_map().views[&bsb].entity_id());
            let kjv_id = DockPanelId::from(s.dock_map().views[&kjv].entity_id());
            let node = dock
                .read(cx)
                .layout(DockPlacement::Center)
                .unwrap()
                .find_panel_node(kjv_id)
                .unwrap();
            (dock, id, node)
        };
        dock.update(cx, |d, cx| {
            d.move_panel(
                id,
                InsertTarget::Tabs {
                    node,
                    ix: None,
                    activate: true,
                },
                window,
                cx,
            );
        });
    })
    .unwrap();
    settle(cx, handle);
    assert_eq!(dock_groups(cx, &shell), vec![vec![kjv, bsb]]);
    assert_eq!(engine_groups(cx, &shell), vec![vec![kjv, bsb]]);
    close(cx, handle, shell, lab);
}

/// Port of resource_tabs_tests `typing_a_reference_in_the_header_box_goes_there`: the header
/// box takes a reference and Enter goes to it (`tab.go`).
#[gpui_kit::test]
fn the_header_box_goes_to_a_typed_reference(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "ws-go");
    let bsb = TabId(
        run(cx, &shell, "resource.open", json!({ "id": "bsb" }))["tab"]
            .as_u64()
            .unwrap(),
    );
    settle(cx, handle);
    let title = cx.update(|cx| {
        shell
            .read(cx)
            .session()
            .workspace
            .tab(bsb)
            .unwrap()
            .title
            .clone()
    });
    let label = format!("Go to a reference in {title}");
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        let field = window.find_by_label(&label);
        assert_eq!(field.role(), Some(Role::TextInput));
        window.click_label(&label, cx);
        // Select all: Cmd-A on macOS, Ctrl-A elsewhere.
        window.press("secondary-a", cx);
        window.type_text("Gen 1:3", cx);
        window.press("enter", cx);
    })
    .unwrap();
    settle(cx, handle);
    let at = cx.update(|cx| {
        shell
            .read(cx)
            .session()
            .workspace
            .tab(bsb)
            .unwrap()
            .history
            .current()
    });
    assert_eq!(at.map(|v| (v.chapter, v.verse)), Some((1, 3)));
    close(cx, handle, shell, lab);
}

/// Port of shell_tests `layout_save_asks_for_a_name`: saving a Layout without a name asks for
/// one; the name typed is saved; restoring it rebuilds the dock; renaming and deleting work.
#[gpui_kit::test]
fn layouts_save_restore_rename_and_delete(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "ws-layouts");
    let kjv = TabId(
        run(cx, &shell, "resource.open", json!({ "id": "kjv-strongs" }))["tab"]
            .as_u64()
            .unwrap(),
    );
    run(
        cx,
        &shell,
        "resource.open",
        json!({ "id": "bsb", "split": true }),
    );
    settle(cx, handle);
    // No name: the prompt.
    cx.update(|cx| {
        shell.update(cx, |s, cx| {
            s.run("layout.save", &json!({}));
            cx.notify();
        })
    });
    settle(cx, handle);
    cx.update_window(handle, |_, window, cx| {
        window.click_label("Layout name", cx);
        window.type_text("Study", cx);
        window.click_label("Save", cx);
    })
    .unwrap();
    settle(cx, handle);
    let names = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            shell
                .read(cx)
                .session()
                .user
                .as_ref()
                .unwrap()
                .layouts()
                .unwrap()
        })
    };
    assert!(names(cx).contains(&"Study".to_string()), "{:?}", names(cx));
    // Another arrangement, then the Layout back.
    run(cx, &shell, "tab.close", json!({ "tab": kjv.0 }));
    settle(cx, handle);
    assert_eq!(dock_groups(cx, &shell).len(), 1);
    run(cx, &shell, "layout.restore", json!({ "name": "Study" }));
    settle(cx, handle);
    let groups = dock_groups(cx, &shell);
    assert_eq!(groups.len(), 2, "{groups:?}");
    assert_eq!(sorted(groups), engine_groups(cx, &shell));
    run(
        cx,
        &shell,
        "layout.rename",
        json!({ "name": "Study", "to": "Reading" }),
    );
    assert!(names(cx).contains(&"Reading".to_string()));
    run(cx, &shell, "layout.delete", json!({ "name": "Reading" }));
    assert!(!names(cx).contains(&"Reading".to_string()));
    close(cx, handle, shell, lab);
}

/// Layout compatibility: a Layout written by the engine as the egui Shell writes it (its
/// `record_layout` sets the arrangement from egui_tiles: a row of Panels with egui's shares,
/// Panel ids its own) restores in GPUI with the same Panels and Tabs.
#[gpui_kit::test]
fn a_layout_saved_by_the_egui_shell_restores_in_gpui(cx: &mut TestAppContext) {
    let (mut session, lab) = support::session("ws-egui-layout");
    let a = session
        .run("resource.open", &json!({ "id": "kjv-strongs" }))
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    let b = session
        .run("resource.open", &json!({ "id": "bsb", "split": true }))
        .unwrap()["tab"]
        .as_u64()
        .unwrap();
    let c = session.run("library.open", &json!({})).unwrap()["tab"]
        .as_u64()
        .unwrap();
    // What egui's `dock_node` writes: Panel ids from its tiles, shares 2:1, a column inside.
    let panel_of = |t: u64| session.workspace.panel_of(TabId(t)).unwrap().0;
    session.workspace.arrangement = Some(LayoutNode::Row {
        children: vec![
            (
                LayoutNode::Panel {
                    panel: panel_of(a),
                    tabs: vec![a],
                    active: Some(a),
                },
                2.0,
            ),
            (
                LayoutNode::Column {
                    children: vec![
                        (
                            LayoutNode::Panel {
                                panel: panel_of(b),
                                tabs: vec![b],
                                active: Some(b),
                            },
                            1.0,
                        ),
                        (
                            LayoutNode::Panel {
                                panel: panel_of(c),
                                tabs: vec![c],
                                active: Some(c),
                            },
                            1.0,
                        ),
                    ],
                },
                1.0,
            ),
        ],
    });
    session
        .run("layout.save", &json!({ "name": "From egui" }))
        .unwrap();
    let (handle, shell) = open_with(cx, session);
    settle(cx, handle);
    run(cx, &shell, "layout.restore", json!({ "name": "From egui" }));
    settle(cx, handle);
    let groups = dock_groups(cx, &shell);
    assert_eq!(groups.len(), 3, "{groups:?}");
    assert_eq!(sorted(groups), engine_groups(cx, &shell));
    let arrangement = cx.update(|cx| shell.read(cx).session().workspace.arrangement.clone());
    assert!(
        matches!(&arrangement, Some(LayoutNode::Row { children })
            if children.len() == 2 && matches!(children[1].0, LayoutNode::Column { .. })),
        "{arrangement:?}"
    );
    // The proportions come back too: 2:1 on screen, and 2:1 in the arrangement saved again.
    let Some(LayoutNode::Row { children }) = &arrangement else {
        unreachable!()
    };
    let ratio = children[0].1 / children[1].1;
    assert!((ratio - 2.0).abs() < 0.1, "shares {children:?}");
    let title_b = cx.update(|cx| {
        let ws = &shell.read(cx).session().workspace;
        ws.tab(TabId(b)).unwrap().title.clone()
    });
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        let x = f32::from(
            window
                .find_by_label(&format!("Back in {title_b}"))
                .bounds()
                .origin
                .x,
        );
        let width = obs_ui_gpui::tokens::WINDOW_WIDTH;
        assert!(
            (x / width - 2.0 / 3.0).abs() < 0.05,
            "the second Panel starts at {x} of {width}"
        );
    })
    .unwrap();
    close(cx, handle, shell, lab);
}

/// Floating windows come with #388: a Tab in a floating window of an egui Last session or
/// Layout is docked into the main window, not closed, and shown once.
#[gpui_kit::test]
fn a_floating_windows_tabs_are_docked_not_lost(cx: &mut TestAppContext) {
    let (mut session, lab) = support::session("ws-floating");
    let a = TabId(
        session
            .run("resource.open", &json!({ "id": "kjv-strongs" }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    let b = TabId(
        session
            .run("resource.open", &json!({ "id": "bsb", "split": true }))
            .unwrap()["tab"]
            .as_u64()
            .unwrap(),
    );
    assert!(session.workspace.float(b, None).is_some());
    let (handle, shell) = open_with(cx, session);
    settle(cx, handle);
    let groups = dock_groups(cx, &shell);
    let shown: Vec<TabId> = groups.iter().flatten().copied().collect();
    assert_eq!(shown.len(), 2, "each Tab once: {groups:?}");
    assert!(shown.contains(&a) && shown.contains(&b), "{groups:?}");
    assert_eq!(sorted(groups), engine_groups(cx, &shell));
    let floating = cx.update(|cx| shell.read(cx).session().workspace.floating.len());
    assert_eq!(floating, 0);
    // Floated while running: it stays in the main window.
    run(cx, &shell, "resource.open", json!({ "id": "bsb" }));
    cx.update(|cx| {
        shell.update(cx, |s, cx| {
            s.run("tab.float", &json!({ "tab": a.0 }));
            cx.notify();
        })
    });
    settle(cx, handle);
    let groups = dock_groups(cx, &shell);
    let shown: Vec<TabId> = groups.iter().flatten().copied().collect();
    assert!(shown.contains(&a) && shown.contains(&b), "{groups:?}");
    let mut once = shown.clone();
    once.dedup();
    assert_eq!(once.len(), shown.len(), "{groups:?}");
    assert_eq!(sorted(groups), engine_groups(cx, &shell));
    close(cx, handle, shell, lab);
}

/// `with_every_tab`: a Tab the arrangement leaves out goes into its Panel's node, or a new
/// Panel at the right, so the dock shows every Tab the engine has.
#[test]
fn an_arrangement_missing_a_tab_gets_it_back() {
    use obs_engine::PanelId;
    use obs_ui_gpui::workspace::with_every_tab;
    let arrangement = Some(LayoutNode::Panel {
        panel: 1,
        tabs: vec![10],
        active: Some(10),
    });
    let panels = vec![
        (PanelId(1), vec![TabId(10), TabId(11)], Some(TabId(10))),
        (PanelId(2), vec![TabId(12)], Some(TabId(12))),
    ];
    let node = with_every_tab(arrangement, &panels).unwrap();
    assert_eq!(
        node.panels(),
        vec![(1, vec![10, 11], Some(10)), (2, vec![12], Some(12))]
    );
    assert!(matches!(node, LayoutNode::Row { .. }));
}

/// The Panel's Tab strip: each Tab's tab is a Tab (AccessKit) named by its title, and the
/// keyboard reaches it (its focus ring shows then, `focus_visible`).
#[gpui_kit::test]
fn the_tab_strip_has_named_tabs_the_keyboard_reaches(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "ws-strip");
    let bsb = TabId(
        run(cx, &shell, "resource.open", json!({ "id": "bsb" }))["tab"]
            .as_u64()
            .unwrap(),
    );
    settle(cx, handle);
    let title = cx.update(|cx| {
        let ws = &shell.read(cx).session().workspace;
        ws.tab(bsb).unwrap().title.clone()
    });
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        let tabs = window.find_all_by_role(Role::Tab);
        assert!(
            tabs.iter().any(|t| t.label() == Some(title.as_str())),
            "a Tab named {title}"
        );
        let focused = |window: &mut gpui_kit::Window| {
            window
                .find_all_by_role(Role::Tab)
                .into_iter()
                .find(|t| t.label() == Some(title.as_str()))
                .and_then(|t| t.focused())
        };
        window.click_label(&format!("Back in {title}"), cx);
        assert_eq!(focused(window), Some(false));
        // Back along the keyboard order, from the header to the strip's tab.
        let mut reached = false;
        for _ in 0..6 {
            window.press("shift-tab", cx);
            window.frame(cx);
            if focused(window) == Some(true) {
                reached = true;
                break;
            }
        }
        assert!(reached, "Shift-Tab moves the keyboard to the strip's tab");
    })
    .unwrap();
    close(cx, handle, shell, lab);
}

/// The Last session: the GPUI app does not write it unless allowed (while egui ships, it
/// must not overwrite the egui app's); allowed, it is written and a new Shell restores it.
#[gpui_kit::test]
fn the_last_session_is_written_only_when_allowed(cx: &mut TestAppContext) {
    let (handle, shell, lab) = open(cx, "ws-session");
    run(cx, &shell, "resource.open", json!({ "id": "bsb" }));
    settle(cx, handle);
    let saved = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            shell
                .read(cx)
                .session()
                .user
                .as_ref()
                .unwrap()
                .last_session()
                .unwrap()
        })
    };
    assert_eq!(saved(cx), None, "not written by default");
    cx.update(|cx| {
        shell.update(cx, |s, cx| {
            s.set_save_session(true);
            cx.notify();
        })
    });
    run(
        cx,
        &shell,
        "resource.open",
        json!({ "id": "kjv-strongs", "split": true }),
    );
    settle(cx, handle);
    let json = saved(cx).expect("written once allowed");
    assert!(json.contains("bsb"));
    close(cx, handle, shell, lab);
}

/// Port of narrow_panels_tests `header_fits_a_narrow_panel`: in a narrow window the header's
/// controls stay reachable (each still exposed with its accessible name).
#[gpui_kit::test]
fn a_narrow_panel_keeps_its_header_controls(cx: &mut TestAppContext) {
    let (session, lab) = support::session("ws-narrow");
    let (handle, shell) = cx.update(|cx| {
        obs_ui_gpui::install(cx);
        let bounds = Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(420.), px(600.)),
        };
        let options = obs_ui_gpui::main_window_options(Some(bounds), cx);
        obs_ui_gpui::open_main_window(session, options, cx).unwrap()
    });
    let bsb = TabId(
        run(cx, &shell, "resource.open", json!({ "id": "bsb" }))["tab"]
            .as_u64()
            .unwrap(),
    );
    run(
        cx,
        &shell,
        "resource.open",
        json!({ "id": "kjv-strongs", "split": true }),
    );
    settle(cx, handle);
    let title = cx.update(|cx| {
        shell
            .read(cx)
            .session()
            .workspace
            .tab(bsb)
            .unwrap()
            .title
            .clone()
    });
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        for label in [
            format!("Back in {title}"),
            format!("Go to a reference in {title}"),
            format!("More for {title}"),
            format!("Close {title}"),
        ] {
            let e = window.find_by_label(&label);
            assert!(e.visible(), "{label} is drawn");
        }
    })
    .unwrap();
    close(cx, handle, shell, lab);
}
