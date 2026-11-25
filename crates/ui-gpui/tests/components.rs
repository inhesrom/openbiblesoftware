//! The shared components work through the UI (#378): each is found by its accessible name and
//! role, activates by click, takes keyboard focus, and the text field types, composes with an
//! IME, undoes, copies and pastes. Driven headlessly by the test driver (`tests/support`, #379).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use gpui_kit::base::input::InputState;
use gpui_kit::{
    AppContext, Bounds, ClipboardItem, Context, ElementInputHandler, Entity, InputHandler as _,
    IntoElement, ParentElement, Point, Render, Role, SharedString, Styled, TestAppContext, Window,
    WindowBounds, WindowOptions, div, px, size,
};
use obs_ui_gpui::catalogue::{Catalogue, look_named};
use obs_ui_gpui::components as c;
use obs_ui_gpui::icons::Icon;
use obs_ui_gpui::theme;
use support::Driver;

fn open<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut gpui_kit::App) -> Entity<V>,
) -> (gpui_kit::AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        obs_ui_gpui::install(cx);
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1280.), px(1400.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            build,
        )
        .unwrap()
    })
}

fn undo_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    }
}

fn key(k: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{k}")
    } else {
        format!("ctrl-{k}")
    }
}

#[gpui_kit::test]
fn every_catalogue_control_is_found_by_its_name_and_role(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, |window, cx| cx.new(|cx| Catalogue::new(window, cx)));
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        for (label, role) in [
            ("Save", Role::Button),
            ("Open", Role::Button),
            ("Add to Favorites", Role::Button),
            ("More", Role::Button),
            ("Notebook name", Role::TextInput),
            ("Search", Role::TextInput),
            ("Words of Jesus in red", Role::Switch),
            ("Paragraphs", Role::RadioButton),
            ("Verse per line", Role::RadioButton),
            ("Remove KJV", Role::Button),
            ("Float", Role::MenuItem),
            ("Close Tab", Role::MenuItem),
            ("Tab menu", Role::Menu),
            ("Word Info", Role::Dialog),
            ("Remove KJV?", Role::Dialog),
            ("Buttons", Role::Heading),
            ("An update is ready.", Role::Status),
            ("Layout saved", Role::Status),
            ("Go back (Alt+Left)", Role::Tooltip),
            ("You are offline.", Role::Status),
            ("Building the Search index…", Role::ProgressIndicator),
            ("KJV could not be opened.", Role::Alert),
        ] {
            let found = window.find_by_label(label);
            assert_eq!(found.role(), Some(role), "{label}");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn buttons_toggles_radios_and_chips_act_on_a_click(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, |window, cx| cx.new(|cx| Catalogue::new(window, cx)));
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        window.click_label("Save", cx);
        window.click_label("Save", cx);
        assert_eq!(view.read(cx).clicks, 2);
        assert!(view.read(cx).toggled);
        window.click_label("Words of Jesus in red", cx);
        assert!(!view.read(cx).toggled);
        window.click_label("Verse per line", cx);
        assert_eq!(view.read(cx).choice, 1);
        assert_eq!(
            window.find_by_label("Remove WEB").role(),
            Some(Role::Button)
        );
        window.click_label("Remove WEB", cx);
        let chips: Vec<SharedString> = view.read(cx).chips.clone();
        assert_eq!(chips, vec![SharedString::from("KJV"), "BSB".into()]);
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_click_focuses_a_control_and_tab_moves_the_focus(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, |window, cx| cx.new(|cx| Catalogue::new(window, cx)));
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        window.click_label("Save", cx);
        assert_eq!(window.find_by_label("Save").focused(), Some(true));
        window.press("tab", cx);
        assert_eq!(window.find_by_label("Save").focused(), Some(false));
    })
    .unwrap();
}

struct Field {
    state: Entity<InputState>,
}

impl Render for Field {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = theme::look(cx);
        div().p(px(20.)).child(c::text_field(
            "field",
            &self.state,
            "Notebook name",
            &look,
            window,
            cx,
        ))
    }
}

#[gpui_kit::test]
fn the_text_field_types_composes_undoes_copies_and_pastes(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, |window, cx| {
        cx.new(|cx| Field {
            state: cx.new(|cx| InputState::new(window, cx)),
        })
    });
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        window.click_label("Notebook name", cx);
        let f = window.find_by_label("Notebook name");
        assert_eq!(f.focused(), Some(true));
        assert_eq!(f.label(), Some("Notebook name"));
        assert_eq!(f.role(), Some(Role::TextInput));
        window.type_text("Romans ", cx);
        assert_eq!(
            window.find_by_label("Notebook name").value(),
            Some("Romans ")
        );

        // IME: a marked composition, then committed as one undo step.
        let state = view.read(cx).state.clone();
        let bounds = window.find_by_label("Notebook name").bounds();
        // The text-input protocol a platform IME drives, on the field's own input handler.
        let mut ime = ElementInputHandler::new(bounds, state);
        ime.replace_and_mark_text_in_range(None, "בָּ", Some(1..1), window, cx);
        window.frame(cx);
        assert!(ime.marked_text_range(window, cx).is_some());
        ime.replace_text_in_range(None, "בָּרָא", window, cx);
        window.frame(cx);
        assert_eq!(ime.marked_text_range(window, cx), None);
        assert_eq!(
            window.find_by_label("Notebook name").value(),
            Some("Romans בָּרָא")
        );
        window.press(undo_key(), cx);
        assert_eq!(
            window.find_by_label("Notebook name").value(),
            Some("Romans ")
        );

        // Copy and paste through the clipboard.
        window.press(&key("a"), cx);
        window.press(&key("c"), cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
            Some("Romans ")
        );
        cx.write_to_clipboard(ClipboardItem::new_string("8:28".into()));
        window.press("end", cx);
        window.press(&key("v"), cx);
        assert_eq!(
            window.find_by_label("Notebook name").value(),
            Some("Romans 8:28")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn switching_the_look_redraws_in_the_same_frame(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, |window, cx| cx.new(|cx| Catalogue::new(window, cx)));
    cx.update_window(handle, |_, window, cx| {
        window.frame(cx);
        assert!(!theme::look(cx).chrome.dark);
        window.click_label("Dark", cx);
        assert!(
            theme::look(cx).chrome.dark,
            "Dark is current after the click"
        );
        window.click_label("High contrast", cx);
        let l = theme::look(cx);
        assert!(l.chrome.high_contrast);
        assert_eq!(l.focus_width(), obs_ui_gpui::tokens::stroke::HEAVY);
    })
    .unwrap();
    assert!(look_named("Sepia", 1.0).is_some_and(|l| l.page == obs_ui_gpui::tokens::SEPIA));
}

#[gpui_kit::test]
fn reduce_motion_reaches_gpui(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let mut l = look_named("Light", 1.0).unwrap();
        l.reduce_motion = true;
        theme::set_look(l, cx);
        assert!(cx.reduce_motion());
    });
}

#[test]
fn every_icon_has_a_path_under_icons() {
    assert!(Icon::ALL.iter().all(|i| i.path().starts_with("icons/")));
}
