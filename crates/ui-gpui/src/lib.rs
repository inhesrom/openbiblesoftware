//! The desktop UI on GPUI (ADR 0025), built beside `obs-ui-egui` until the cutover (#401).
//!
//! For now it opens the app window over the Session the egui app opens
//! ([`obs_engine::launch::open_session`]) and draws an empty Workspace: the title bar and an
//! empty Panel area (#377). The surfaces arrive with the map's tickets (#371). Build and run it
//! with `cargo run -p openbible --features gpui` (docs/dev/ui-gpui.md).

use std::borrow::Cow;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::base::Button;
use gpui_kit::{
    App, AppContext, Bounds, Context, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Render, Styled, TitlebarOptions, Window, WindowBounds, WindowOptions, canvas, div, px, size,
};
use obs_engine::Session;
use obs_engine::launch::Launch;

pub mod tokens;

/// Window title until the product is named (the egui app's `APP_TITLE`).
pub const APP_TITLE: &str = "Open Bible Software";

/// How long a smoke run ([`Options::smoke`]) waits for the first frame before it fails.
pub const SMOKE_TIMEOUT: Duration = Duration::from_secs(60);

/// How the app runs.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Open the window, wait until a frame has been drawn, and quit (the CI smoke run).
    pub smoke: bool,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The user closed the window or quit.
    Closed,
    /// A smoke run drew a frame and quit.
    SmokePassed {
        /// From [`run`] being called to the end of the first frame's paint.
        first_frame: Duration,
    },
    /// A smoke run failed: the window did not open, or no frame was drawn in time.
    SmokeFailed(String),
}

/// The fonts the app draws with, registered with GPUI's text system: the bundled faces of
/// `obs-fonts` (Gentium Plus and Ezra SIL), so text looks the same on every computer.
fn bundled_fonts() -> Vec<Cow<'static, [u8]>> {
    obs_fonts::FaceId::ALL
        .iter()
        .filter_map(|face| face.bundled_bytes())
        .map(Cow::Borrowed)
        .collect()
}

/// The main window's view: the Session and, for now, an empty Workspace.
struct Shell {
    /// The study state the UI draws. Unused until the Workspace is built (#385); held so it
    /// lives, with its background jobs, as long as the window.
    _session: Session,
    /// Set when a frame has finished painting (a smoke run waits for it).
    painted: Rc<Cell<Option<Instant>>>,
}

impl Shell {
    fn title_bar(&self) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(tokens::TITLE_BAR_HEIGHT))
            .px(px(tokens::SPACE_L))
            .gap(px(tokens::SPACE_M))
            .bg(tokens::surface())
            .border_b_1()
            .border_color(tokens::rule())
            .child(
                div()
                    .font_family("Gentium Plus")
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(tokens::TEXT_WORDMARK))
                    .text_color(tokens::ink())
                    .child("Open Bible"),
            )
            .child(div().flex_1())
            // The one control so far, so a screen reader has something to reach (#377).
            .child(
                Button::new("quit")
                    .accessibility_label(format!("Quit {APP_TITLE}"))
                    .px(px(tokens::SPACE_M))
                    .py(px(tokens::SPACE_S / 2.0))
                    .rounded(px(tokens::RADIUS_CONTROL))
                    .border_1()
                    .border_color(tokens::rule())
                    .text_color(tokens::ink())
                    .hover(|s| s.bg(tokens::hover()))
                    .on_click(|_, _, cx| cx.quit())
                    .child("Quit"),
            )
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let painted = self.painted.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(tokens::shell())
            .text_size(px(tokens::TEXT_BODY))
            .text_color(tokens::ink())
            .child(self.title_bar())
            // The Workspace: its Panels and Tabs come with #385.
            .child(
                div()
                    .flex_1()
                    .m(px(tokens::SPACE_S))
                    .p(px(tokens::SPACE_XL))
                    .bg(tokens::surface())
                    .border_1()
                    .border_color(tokens::rule())
                    .rounded(px(tokens::RADIUS_CONTROL))
                    .text_color(tokens::muted())
                    .child("The GPUI Workspace is not built yet."),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, _, _| {
                        if painted.get().is_none() {
                            painted.set(Some(Instant::now()));
                        }
                    },
                )
                .absolute()
                .size(px(0.0)),
            )
    }
}

/// Open the app window over `launch`'s Session and run until it closes (or, for a smoke run,
/// until a frame has been drawn). `started` is when the process started, for the smoke run's
/// first-frame time.
pub fn run(launch: Launch, options: Options, started: Instant) -> Outcome {
    for problem in &launch.problems {
        log::warn!("{problem}");
    }
    let outcome: Rc<Cell<Option<Outcome>>> = Rc::new(Cell::new(None));
    let result = outcome.clone();
    let mut session = Some(launch.session);
    gpui_kit::application().run(move |cx: &mut App| {
        gpui_kit::init(cx);
        if let Err(e) = cx.text_system().add_fonts(bundled_fonts()) {
            log::warn!("bundled fonts: {e}");
        }
        let painted: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
        let shell_painted = painted.clone();
        let bounds = Bounds::centered(
            None,
            size(px(tokens::WINDOW_WIDTH), px(tokens::WINDOW_HEIGHT)),
            cx,
        );
        let options_for_window = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some(APP_TITLE.into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let Some(session) = session.take() else {
            return;
        };
        let opened = gpui_kit::open_window(options_for_window, cx, move |_, cx| {
            cx.new(|_| Shell {
                _session: session,
                painted: shell_painted,
            })
        });
        if let Err(e) = opened {
            result.set(Some(Outcome::SmokeFailed(format!(
                "the window did not open: {e}"
            ))));
            cx.quit();
            return;
        }
        // Closing the last window quits the app.
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        if options.smoke {
            let result = result.clone();
            cx.spawn(async move |cx| {
                let opened_at = Instant::now();
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                    if let Some(at) = painted.get() {
                        result.set(Some(Outcome::SmokePassed {
                            first_frame: at.duration_since(started),
                        }));
                        break;
                    }
                    if opened_at.elapsed() > SMOKE_TIMEOUT {
                        result.set(Some(Outcome::SmokeFailed(format!(
                            "no frame was drawn within {} s",
                            SMOKE_TIMEOUT.as_secs()
                        ))));
                        break;
                    }
                }
                cx.update(|cx| cx.quit());
            })
            .detach();
        }
    });
    outcome.take().unwrap_or(Outcome::Closed)
}
