//! The component catalogue (#378): every shared component in the current Look, with buttons
//! that switch the Look, so the components can be checked in each theme and by screenshot.
//! Open it with `OBS_GPUI_CATALOGUE=1 cargo run -p openbible --features gpui`.

use gpui_kit::base::input::InputState;
use gpui_kit::prelude::*;
use gpui_kit::{AppContext, Context, Entity, IntoElement, Render, SharedString, Window, div, px};
use obs_engine::{PageTone, Theme};

use crate::components as c;
use crate::icons::Icon;
use crate::theme::{self, Look};
use crate::tokens::{self, rgba, size, space};

/// A Look the catalogue can show: a Theme and a Page tone.
pub const LOOKS: [(&str, Theme, PageTone); 5] = [
    ("Light", Theme::Light, PageTone::Paper),
    ("Dark", Theme::Dark, PageTone::Dark),
    ("Sepia", Theme::Light, PageTone::Sepia),
    ("High contrast", Theme::HighContrastLight, PageTone::Paper),
    (
        "High contrast dark",
        Theme::HighContrastDark,
        PageTone::Dark,
    ),
];

/// The Look of one of [`LOOKS`] at Interface size `zoom`.
pub fn look_named(name: &str, zoom: f32) -> Option<Look> {
    let (_, theme_, tone) = LOOKS.iter().find(|l| l.0 == name)?;
    let mut appearance = obs_engine::Appearance {
        theme: *theme_,
        page_tone: *tone,
        ..Default::default()
    };
    appearance.reduce_motion = false;
    let mut l = Look::resolve(
        &appearance,
        &obs_engine::CustomTheme::default(),
        &obs_engine::FontChoices::default(),
        false,
        false,
    );
    l.zoom = zoom;
    Some(l)
}

/// The catalogue's view.
pub struct Catalogue {
    pub name: Entity<InputState>,
    pub search: Entity<InputState>,
    pub toggled: bool,
    pub choice: usize,
    pub clicks: usize,
    pub chips: Vec<SharedString>,
    pub look_name: SharedString,
}

impl Catalogue {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            name: cx.new(|cx| InputState::new(window, cx).placeholder("A Notebook's name")),
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search")),
            toggled: true,
            choice: 0,
            clicks: 0,
            chips: vec!["KJV".into(), "WEB".into(), "BSB".into()],
            look_name: if theme::look(cx).chrome.dark {
                "Dark".into()
            } else {
                "Light".into()
            },
        }
    }

    fn section(title: &str, look: &Look) -> gpui_kit::Div {
        c::card(look).child(c::section_header(title.to_string(), look))
    }
}

impl Render for Catalogue {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = theme::look(cx);
        let ch = look.chrome;
        let looks = div()
            .flex()
            .flex_wrap()
            .gap(px(look.pt(space::S)))
            .children(LOOKS.iter().enumerate().map(|(i, (name, _, _))| {
                let name = *name;
                let b = if self.look_name.as_ref() == name {
                    c::primary_button(("look", i), name, &look)
                } else {
                    c::button(("look", i), name, &look)
                };
                b.on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(l) = look_named(name, theme::look(cx).zoom) {
                        this.look_name = name.into();
                        theme::set_look(l, cx);
                    }
                }))
            }));
        let buttons = Self::section("Buttons", &look).child(
            div()
                .flex()
                .items_center()
                .gap(px(look.pt(space::M)))
                .child(
                    c::button("save", "Save", &look).on_click(cx.listener(|this, _, _, cx| {
                        this.clicks += 1;
                        cx.notify();
                    })),
                )
                .child(c::primary_button("open", "Open", &look))
                .child(c::icon_button(
                    "star",
                    Icon::Star,
                    "Add to Favorites",
                    &look,
                ))
                .child(c::icon_button("more", Icon::More, "More", &look))
                .child(c::hint(format!("Saved {} times", self.clicks), &look)),
        );
        let fields = Self::section("Text fields", &look).child(
            div()
                .flex()
                .items_center()
                .gap(px(look.pt(space::L)))
                .child(c::text_field(
                    "name",
                    &self.name,
                    "Notebook name",
                    &look,
                    window,
                    cx,
                ))
                .child(c::search_field(
                    "search",
                    &self.search,
                    "Search",
                    &look,
                    window,
                    cx,
                )),
        );
        let choices = Self::section("Toggles and choices", &look)
            .child(c::toggle(
                "red-letter",
                self.toggled,
                "Words of Jesus in red",
                &look,
                {
                    let e = cx.entity();
                    move |on, _, _, cx| {
                        e.update(cx, |this, cx| {
                            this.toggled = on;
                            cx.notify();
                        })
                    }
                },
            ))
            .child(
                div().flex().gap(px(look.pt(space::L))).children(
                    ["Paragraphs", "Verse per line"]
                        .iter()
                        .enumerate()
                        .map(|(i, l)| {
                            c::radio(("flow", i), self.choice == i, *l, &look, {
                                let e = cx.entity();
                                move |_, _, _, cx| {
                                    e.update(cx, |this, cx| {
                                        this.choice = i;
                                        cx.notify();
                                    })
                                }
                            })
                        }),
                ),
            );
        let chips =
            Self::section("Chips", &look).child(div().flex().gap(px(look.pt(space::S))).children(
                self.chips.iter().cloned().enumerate().map(|(i, label)| {
                    let entity = cx.entity();
                    c::chip(
                        ("chip", i),
                        label.clone(),
                        &look,
                        Some(Box::new(move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.chips.retain(|l| *l != label);
                                cx.notify();
                            });
                        })),
                    )
                }),
            ));
        let floating = Self::section("Menus, popovers, tooltips, toasts", &look).child(
            div()
                .flex()
                .items_start()
                .gap(px(look.pt(space::L)))
                .child(
                    c::menu("Tab menu", &look)
                        .child(c::menu_item("m-float", "Float", None, &look))
                        .child(c::menu_item("m-close", "Close Tab", Some("Ctrl+W"), &look)),
                )
                .child(
                    c::popover_frame("Word Info", &look)
                        .child(c::section_header("בָּרָא bara", &look))
                        .child(c::hint("to create, shape, form", &look)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(look.pt(space::M)))
                        .child(c::tooltip("Add to Favorites", &look))
                        .child(c::toast("Layout saved", &look)),
                ),
        );
        let pages = Self::section("Dialog, notices and states", &look)
            .child(
                c::dialog("Remove KJV?", &look)
                    .child(c::hint(
                        "Its Highlights and Notes stay in the User database.",
                        &look,
                    ))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(look.pt(space::M)))
                            .child(c::button("cancel", "Cancel", &look))
                            .child(c::primary_button("remove", "Remove", &look)),
                    ),
            )
            .child(c::notice("An update is ready.", ch.lapis, &look))
            .child(c::notice("You are offline.", ch.warn, &look))
            .child(c::empty_state(
                "No Notes yet.",
                c::button("new-note", "New Note", &look),
                &look,
            ))
            .child(c::loading_state(
                "Building the Search index…",
                Some(0.4),
                &look,
            ))
            .child(c::error_state(
                "KJV could not be opened.",
                "kjv.obr: the file is damaged.",
                &look,
            ));
        let page = Self::section("Page tone", &look).child(
            div()
                .p(px(look.pt(space::L)))
                .rounded(px(look.pt(tokens::radius::M)))
                .bg(rgba(look.page.page))
                .text_color(rgba(look.page.ink))
                .font_family(theme::DISPLAY_FAMILY)
                .text_size(px(look.pt(tokens::text::SUBTITLE)))
                .child(
                    div()
                        .flex()
                        .gap(px(look.pt(space::XS)))
                        .child(div().text_color(rgba(look.page.verse_number)).child("1"))
                        .child("In the beginning God created the heaven and the earth."),
                ),
        );
        let icons = Self::section("Icons", &look).child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(look.pt(space::S)))
                .children(
                    Icon::ALL
                        .iter()
                        .map(|i| i.element(look.pt(size::ICON), rgba(ch.ink))),
                ),
        );
        div()
            .size_full()
            .flex()
            .bg(rgba(ch.shell))
            .font_family(look.interface_family.clone())
            .text_size(px(look.pt(tokens::text::BODY)))
            .child(
                div().id("catalogue").flex_1().overflow_y_scroll().child(
                    c::page(&look)
                        .bg(rgba(ch.shell))
                        .child(c::title("Components", &look))
                        .child(looks)
                        .child(
                            div()
                                .flex()
                                .gap(px(look.pt(space::L)))
                                .child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap(px(look.pt(space::L)))
                                        .child(buttons)
                                        .child(fields)
                                        .child(choices)
                                        .child(chips)
                                        .child(icons),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap(px(look.pt(space::L)))
                                        .child(page)
                                        .child(floating)
                                        .child(pages),
                                ),
                        ),
                ),
            )
            .child(c::scroll_bar("Components", 0.0, 0.6, &look))
    }
}
