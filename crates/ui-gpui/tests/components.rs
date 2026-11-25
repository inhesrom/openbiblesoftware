//! The shared components work through the UI (#378): each is found by its accessible name and
//! role, activates by click, takes keyboard focus, and the text field types, composes with an
//! IME, undoes, copies and pastes. Driven headlessly with gpui-kit's test API.

use gpui_kit::base::input::InputState;
use gpui_kit::test::{TestInput, TestQueryExt, TestWindowExt};
use gpui_kit::{
    AppContext, Bounds, ClipboardItem, Context, ElementInputHandler, Entity, IntoElement,
    ParentElement, Point, Render, Role, SharedString, Styled, TestAppContext, Window, WindowBounds,
    WindowOptions, div, px, size,
};
use obs_ui_gpui::catalogue::{Catalogue, look_named};
use obs_ui_gpui::components as c;
use obs_ui_gpui::icons::Icon;
use obs_ui_gpui::theme;

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
        window.render_frame(cx);
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
        window.render_frame(cx);
        window.click("save", cx);
        window.click("save", cx);
        assert_eq!(view.read(cx).clicks, 2);
        assert!(view.read(cx).toggled);
        window.click("red-letter", cx);
        assert!(!view.read(cx).toggled);
        window.click(("flow", 1usize), cx);
        assert_eq!(view.read(cx).choice, 1);
        assert_eq!(
            window.find_by_label("Remove WEB").role(),
            Some(Role::Button)
        );
        window.click("close-WEB", cx);
        let chips: Vec<SharedString> = view.read(cx).chips.clone();
        assert_eq!(chips, vec![SharedString::from("KJV"), "BSB".into()]);
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_click_focuses_a_control_and_tab_moves_the_focus(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, |window, cx| cx.new(|cx| Catalogue::new(window, cx)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("save", cx);
        assert_eq!(window.find("save").focused(), Some(true));
        window.press("tab", cx);
        assert_eq!(window.find("save").focused(), Some(false));
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
        window.render_frame(cx);
        window.click("field", cx);
        let f = window.find("field");
        assert_eq!(f.focused(), Some(true));
        assert_eq!(f.label(), Some("Notebook name"));
        assert_eq!(f.role(), Some(Role::TextInput));
        window.input("Romans ", cx);
        assert_eq!(window.find("field").value(), Some("Romans "));

        // IME: a marked composition, then committed as one undo step.
        let state = view.read(cx).state.clone();
        let bounds = window.find("field").bounds();
        let mut ime = TestInput::new(ElementInputHandler::new(bounds, state), window);
        ime.compose_text(None, "בָּ", Some(1..1), cx);
        assert!(ime.marked_text_range(cx).is_some());
        ime.commit_text("בָּרָא", cx);
        assert_eq!(ime.marked_text_range(cx), None);
        drop(ime);
        assert_eq!(window.find("field").value(), Some("Romans בָּרָא"));
        window.press(undo_key(), cx);
        assert_eq!(window.find("field").value(), Some("Romans "));

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
        assert_eq!(window.find("field").value(), Some("Romans 8:28"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn switching_the_look_redraws_in_the_same_frame(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, |window, cx| cx.new(|cx| Catalogue::new(window, cx)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!theme::look(cx).chrome.dark);
        window.click(("look", 1usize), cx);
        assert!(
            theme::look(cx).chrome.dark,
            "Dark is current after the click"
        );
        window.click(("look", 3usize), cx);
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
