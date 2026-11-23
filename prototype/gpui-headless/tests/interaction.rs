//! PROTOTYPE (#374): drive the surface headlessly with gpui-kit's test API on GPUI's
//! `TestPlatform`. No display server, GPU or screen reader is involved.

use std::time::Instant;

use gpui_kit::test::{TestEventExt, TestInput, TestQueryExt, TestWindowExt};
use gpui_kit::{
    AppContext as _, Bounds, ElementInputHandler, Entity, MouseButton, Role, TestAppContext,
    WindowBounds, WindowOptions, base::Root, point, px, size,
};
use obs_prototype_gpui_headless::{BOOKS, Surface, register_fonts};

fn open(cx: &mut TestAppContext) -> (gpui_kit::AnyWindowHandle, Entity<Surface>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        register_fonts(cx);
        let (window, view) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    size: size(px(480.), px(260.)),
                })),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| Surface::new(false, window, cx)),
        )
        .expect("open window");
        let _ = window.downcast::<Root>().expect("Base Root");
        (window, view)
    })
}

#[test]
fn runs_without_a_display_server() {
    // `run.sh` sets OBS_PROTO_NO_DISPLAY=1 and unsets DISPLAY and WAYLAND_DISPLAY.
    if std::env::var("OBS_PROTO_NO_DISPLAY").as_deref() == Ok("1") {
        assert!(std::env::var_os("DISPLAY").is_none(), "DISPLAY is set");
        assert!(
            std::env::var_os("WAYLAND_DISPLAY").is_none(),
            "WAYLAND_DISPLAY is set"
        );
    }
}

#[gpui_kit::test]
fn a_pointer_click_on_the_labelled_button_changes_state(cx: &mut TestAppContext) {
    let t = Instant::now();
    let (handle, view) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let save = window.find_by_label("Save");
        assert_eq!(save.role(), Some(Role::Button));
        // Real hit testing: a pointer down and up at the button's centre.
        let at = save.bounds().center();
        window.pointer_move(at, None, cx);
        window.pointer_down(at, MouseButton::Left, cx);
        window.pointer_up(at, MouseButton::Left, cx);
        // And once more through the kit's click-by-id helper.
        window.click("save", cx);
    })
    .unwrap();
    assert_eq!(view.read_with(cx, |s, _| s.saves), 2);
    eprintln!("timing: click test {:?}", t.elapsed());
}

#[gpui_kit::test]
fn clicking_a_list_row_selects_it(cx: &mut TestAppContext) {
    let (handle, view) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("book", 2usize), cx);
    })
    .unwrap();
    assert_eq!(view.read_with(cx, |s, _| s.selected), Some(2));
}

#[gpui_kit::test]
fn typing_into_the_input_changes_its_value(cx: &mut TestAppContext) {
    let (handle, view) = open(cx);
    let input = view.read_with(cx, |s, _| s.input.clone());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // Focus with a pointer click on the field, then type and edit with keys.
        let field = window.find_by_label("Reference");
        let at = field.bounds().center();
        window.pointer_down(at, MouseButton::Left, cx);
        window.pointer_up(at, MouseButton::Left, cx);
        window.input("John 3:16x", cx);
        window.press("backspace", cx);
        window.render_frame(cx);
        assert_eq!(window.find_by_label("Reference").value(), Some("John 3:16"));
        assert_eq!(window.find_by_label("Reference").focused(), Some(true));
    })
    .unwrap();
    assert_eq!(input.read_with(cx, |s, _| s.value().to_string()), "John 3:16");
}

#[gpui_kit::test]
fn ime_composition_marks_then_commits_text(cx: &mut TestAppContext) {
    let (handle, view) = open(cx);
    let input = view.read_with(cx, |s, _| s.input.clone());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let bounds = window.find_by_label("Reference").bounds();
        input.update(cx, |s, cx| s.focus(window, cx));
        let mut ime = TestInput::new(ElementInputHandler::new(bounds, input.clone()), window);
        // A two-step composition (as a Hebrew or CJK IME would send), then the commit.
        ime.compose_text(None, "ש", Some(1..1), cx);
        assert_eq!(ime.marked_text_range(cx), Some(0..1));
        ime.compose_text(None, "שָׁ", Some(3..3), cx);
        assert_eq!(ime.marked_text_range(cx), Some(0..3));
        ime.commit_text("שָׁלוֹם", cx);
        assert_eq!(ime.marked_text_range(cx), None);
    })
    .unwrap();
    assert_eq!(input.read_with(cx, |s, _| s.value().to_string()), "שָׁלוֹם");
}

#[gpui_kit::test]
fn controls_are_found_by_accessible_label_and_role(cx: &mut TestAppContext) {
    let (handle, _) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let rows = window.find_all_by_role(Role::ListItem);
        let labels: Vec<_> = rows.iter().filter_map(|r| r.label()).collect();
        assert_eq!(labels, BOOKS);
        let reference = window.find_by_label("Reference");
        assert_eq!(reference.role(), Some(Role::TextInput));
        assert_eq!(reference.placeholder(), Some("Go to a reference"));
        assert_eq!(window.find_all_by_role(Role::Button).len(), 1);
        // Everything the frame exposed, as the kit's snapshot of each node.
        for e in window.elements() {
            eprintln!("a11y: {:?} {:?} {:?}", e.role(), e.label(), e.value());
        }
        // The full AccessKit tree GPUI would send to a screen reader is built only once a
        // platform adapter activates accessibility, which GPUI's TestPlatform never does.
        assert_eq!(window.debug_a11y_tree_json(), None);
    })
    .unwrap();
}
