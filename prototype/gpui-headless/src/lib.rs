//! PROTOTYPE — throwaway (#374). A tiny surface drawn with GPUI through `gpui-kit`'s unstyled
//! layer (`gpui-base`): a header row with a labelled Button and a text Input, a short list,
//! and a paragraph of English and Hebrew in the app's bundled fonts. The tests in
//! `tests/` drive it headlessly and capture it as pixels.

use std::borrow::Cow;

use gpui_kit::TestSupportExt as _;
use gpui_kit::base::Button;
use gpui_kit::base::input::{Input, InputAccessibility, InputState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Hsla, IntoElement, ParentElement as _, Render, Role,
    SharedString, Styled as _, Window, div, prelude::*, px, rgb,
};

/// The list's rows.
pub const BOOKS: [&str; 3] = ["Genesis", "Psalms", "John"];

/// English, then Hebrew (Gen 1:1, with points and cantillation).
pub const ENGLISH: &str = "In the beginning God created the heaven and the earth.";
pub const HEBREW: &str = "בְּרֵאשִׁ֖ית בָּרָ֣א אֱלֹהִ֑ים אֵ֥ת הַשָּׁמַ֖יִם וְאֵ֥ת הָאָֽרֶץ׃";

/// Register Gentium Plus (four faces) and Ezra SIL with GPUI's text system.
pub fn register_fonts(cx: &mut App) {
    let fonts = obs_fonts::FaceId::ALL
        .iter()
        .filter_map(|f| f.bundled_bytes())
        .map(Cow::Borrowed)
        .collect();
    cx.text_system()
        .add_fonts(fonts)
        .unwrap_or_else(|e| panic!("bundled fonts: {e}"));
}

struct Colors {
    page: Hsla,
    ink: Hsla,
    muted: Hsla,
    rule: Hsla,
    field: Hsla,
    accent: Hsla,
    selected: Hsla,
}

fn colors(dark: bool) -> Colors {
    let c = |v: u32| -> Hsla { rgb(v).into() };
    // The app's Paper and Dark tokens (crates/ui-egui/src/tokens.rs, palette.rs).
    if dark {
        Colors {
            page: c(0x16161a),
            ink: c(0xdee2e4),
            muted: c(0xa0aab0),
            rule: c(0x3a4046),
            field: c(0x282d32),
            accent: c(0x84a8e2),
            selected: c(0x1c2c4a),
        }
    } else {
        Colors {
            page: c(0xfafbf8),
            ink: c(0x202629),
            muted: c(0x505b60),
            rule: c(0xcdd5d3),
            field: c(0xfffffd),
            accent: c(0x2d4e86),
            selected: c(0xd3e0f4),
        }
    }
}

/// The surface under test.
pub struct Surface {
    pub dark: bool,
    pub saves: usize,
    pub selected: Option<usize>,
    pub input: Entity<InputState>,
}

impl Surface {
    pub fn new(dark: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("Go to a reference");
            state.set_accessibility(
                InputAccessibility::new(Some(Role::TextInput))
                    .with_label(Some(SharedString::from("Reference")))
                    .with_placeholder(Some(SharedString::from("Go to a reference"))),
            );
            state
        });
        Self {
            dark,
            saves: 0,
            selected: None,
            input,
        }
    }
}

impl Render for Surface {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(self.dark);
        let this = cx.entity().downgrade();
        let save = Button::new("save")
            .accessibility_label("Save")
            .px(px(10.))
            .py(px(5.))
            .rounded(px(4.))
            .bg(c.accent)
            .text_color(c.page)
            .child(format!("Save ({})", self.saves))
            .on_click({
                let this = this.clone();
                move |_, _, cx| {
                    let _ = this.update(cx, |s, cx| {
                        s.saves += 1;
                        cx.notify();
                    });
                }
            });
        let rows = BOOKS.iter().enumerate().map(|(i, name)| {
            let this = this.clone();
            div()
                .id(("book", i))
                .test_support()
                .role(Role::ListItem)
                .aria_label(*name)
                .px(px(6.))
                .py(px(2.))
                .when(self.selected == Some(i), |d| d.bg(c.selected))
                .child(*name)
                .on_click(move |_, _, cx| {
                    let _ = this.update(cx, |s, cx| {
                        s.selected = Some(i);
                        cx.notify();
                    });
                })
        });
        div()
            .id("surface")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(12.))
            .bg(c.page)
            .text_color(c.ink)
            .font_family("Gentium Plus")
            .text_size(px(16.))
            .child(
                div()
                    .id("header")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(save)
                    .child(
                        div()
                            .id("reference-frame")
                            .w(px(220.))
                            .px(px(6.))
                            .py(px(3.))
                            .bg(c.field)
                            .border_1()
                            .border_color(c.rule)
                            .rounded(px(4.))
                            .child(Input::new(&self.input)),
                    ),
            )
            .child(
                div()
                    .id("books")
                    .test_support()
                    .role(Role::List)
                    .aria_label("Books")
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(c.rule)
                    .children(rows),
            )
            .child(
                div()
                    .id("text")
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().child(ENGLISH))
                    .child(
                        div()
                            .font_family("Ezra SIL")
                            .text_size(px(21.))
                            .text_right()
                            .child(HEBREW),
                    )
                    .child(div().text_color(c.muted).text_size(px(13.)).child("Gentium Plus and Ezra SIL, drawn by GPUI's own text system.")),
            )
    }
}
