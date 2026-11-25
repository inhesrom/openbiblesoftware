//! The headless test driver (#379) on GPUI's TestPlatform: the Shell over a real Session of
//! fixtures, driven by engine commands, pointer and keyboard input, and found by its
//! accessible labels and roles. No display, GPU or screen reader is involved, so these run on
//! Linux, macOS and Windows alike.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Role, TestAppContext, point, px, size,
};
use obs_engine::Theme;
use obs_ui_gpui::{QUIT_LABEL, Shell};
use serde_json::json;
use support::Driver;

/// The Shell over the fixtures' Session, in a window the size of the app's.
fn open(cx: &mut TestAppContext, name: &str) -> (AnyWindowHandle, Entity<Shell>, support::Lab) {
    let (session, lab) = support::session(name);
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

#[gpui_kit::test]
fn the_quit_button_is_found_by_its_accessible_label_and_clicked(cx: &mut TestAppContext) {
    let (handle, shell, _lab) = open(cx, "quit");
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        // The AccessKit node a screen reader is given for it.
        let quit = window.find_by_label(QUIT_LABEL);
        assert_eq!(quit.role(), Some(Role::Button));
        assert_eq!(quit.label(), Some(QUIT_LABEL));
        assert_eq!(quit.disabled(), None);
        assert!(quit.visible(), "the Quit button is in view");
        // A pointer down and up at its centre, with real hit testing.
        let at = quit.bounds().center();
        window.pointer_move(at, cx);
        window.pointer_down(at, cx);
        window.pointer_up(at, cx);
    })
    .unwrap();
    assert!(shell.read_with(cx, |s, _| s.quit_requested()));
}

#[gpui_kit::test]
fn the_title_bar_and_the_empty_workspace_are_drawn(cx: &mut TestAppContext) {
    let (handle, _shell, _lab) = open(cx, "nodes");
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        // Every node the frame exposes: one Button so far, and it is labelled.
        let buttons = window.find_all_by_role(Role::Button);
        assert_eq!(buttons.len(), 1, "{:?}", window.elements());
        assert!(
            window
                .elements()
                .iter()
                .filter(|e| e.role() == Some(Role::Button))
                .all(|e| e.label().is_some_and(|l| !l.is_empty())),
            "an unnamed button: {:?}",
            window.elements()
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn engine_commands_run_through_the_shell(cx: &mut TestAppContext) {
    let (handle, shell, _lab) = open(cx, "commands");
    // The fixtures' Library is in the Session the window draws.
    let ids: Vec<String> = shell.read_with(cx, |s, _| {
        s.session()
            .library
            .entries()
            .iter()
            .map(|e| e.manifest.resource_id.clone())
            .collect()
    });
    assert!(ids.iter().any(|id| id == "bsb"), "{ids:?}");
    assert!(ids.iter().any(|id| id == "kjv-strongs"), "{ids:?}");
    // A command, as the egui tests run them; its effect is drawn on the next frame.
    let out = shell.update(cx, |s, cx| {
        let out = s.run("view.theme", &json!({ "value": "dark" }));
        cx.notify();
        out
    });
    assert!(out.is_some(), "view.theme failed");
    assert_eq!(
        shell.read_with(cx, |s, _| s.session().appearance.theme),
        Theme::Dark
    );
    // A command that does not exist fails without a panic.
    assert_eq!(
        shell.update(cx, |s, _| s.run("no.such_command", &json!({}))),
        None
    );
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        assert_eq!(window.find_by_label(QUIT_LABEL).role(), Some(Role::Button));
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_keyboard_reaches_and_presses_the_quit_button(cx: &mut TestAppContext) {
    let (handle, shell, _lab) = open(cx, "keys");
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        // The first tab stop is the one control (GPUI Kit's Root binds Tab only once
        // something has focus, as at launch the app will give it); Enter presses it.
        window.focus_next(cx);
        window.frame(cx);
        assert_eq!(
            window.find_by_label(QUIT_LABEL).focused(),
            Some(true),
            "{:?}",
            window.elements()
        );
        window.press("enter", cx);
    })
    .unwrap();
    assert!(shell.read_with(cx, |s, _| s.quit_requested()));
}

#[gpui_kit::test]
fn the_fixed_clock_holds_in_tests(cx: &mut TestAppContext) {
    let (_handle, _shell, _lab) = open(cx, "clock");
    assert_eq!(obs_store::clock::now_ms(), support::NOW_MS);
}
