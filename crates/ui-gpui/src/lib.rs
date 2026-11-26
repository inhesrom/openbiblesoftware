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
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, Styled, TitlebarOptions, Window, WindowAppearance,
    WindowBounds, WindowOptions, canvas, div, px, size,
};
use obs_engine::launch::Launch;
use obs_engine::{Session, Theme};
use serde_json::Value;

pub mod tokens;

/// Window title until the product is named (the egui app's `APP_TITLE`).
pub const APP_TITLE: &str = "Open Bible Software";

/// The accessible name of the title bar's Quit button.
pub const QUIT_LABEL: &str = "Quit Open Bible Software";

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

/// Set up GPUI Kit and the bundled fonts in `cx`: once per app (or test context), before any
/// window opens.
pub fn install(cx: &mut App) {
    gpui_kit::init(cx);
    if let Err(e) = cx.text_system().add_fonts(bundled_fonts()) {
        log::warn!("bundled fonts: {e}");
    }
}

/// The main window's view: the Session and, for now, an empty Workspace.
pub struct Shell {
    /// The study state the UI draws. Only the theme is drawn until the Workspace is built
    /// (#385); held so it lives, with its background jobs, as long as the window.
    session: Session,
    /// The Quit button was pressed (the app quits; the test platform's quit does nothing).
    quit_requested: bool,
    /// Set when a frame has finished painting (a smoke run waits for it).
    painted: Rc<Cell<Option<Instant>>>,
    /// Tests: colours drawn instead of the theme's (the gallery's seeded change, #379).
    #[cfg(feature = "test-support")]
    chrome_override: Option<tokens::Chrome>,
}

impl Shell {
    pub fn new(session: Session) -> Self {
        Self {
            session,
            quit_requested: false,
            painted: Rc::new(Cell::new(None)),
            #[cfg(feature = "test-support")]
            chrome_override: None,
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// Run a command, as the egui Shell's `run` does: its result, or `None` when it failed
    /// (logged; the toast comes with the GPUI chrome, #378).
    pub fn run(&mut self, id: &str, params: &Value) -> Option<Value> {
        match self.session.run(id, params) {
            Ok(out) => Some(out),
            Err(e) => {
                log::warn!("{id}: {e}");
                None
            }
        }
    }

    /// Whether the Quit button has been pressed.
    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    /// Tests: draw with `chrome` instead of the theme's colours (`None`: the theme's again).
    #[cfg(feature = "test-support")]
    pub fn override_chrome(&mut self, chrome: Option<tokens::Chrome>) {
        self.chrome_override = chrome;
    }

    /// The colours of the theme the Session asks for. Follow system takes the window's
    /// appearance; the High contrast and Custom themes draw as their base until #378.
    fn chrome(&self, window: &Window) -> tokens::Chrome {
        #[cfg(feature = "test-support")]
        if let Some(chrome) = self.chrome_override {
            return chrome;
        }
        let dark = match self.session.appearance.theme {
            Theme::Dark | Theme::HighContrastDark => true,
            Theme::Light | Theme::HighContrastLight | Theme::Custom => false,
            Theme::System => matches!(
                window.appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        };
        if dark {
            tokens::dark()
        } else {
            tokens::light()
        }
    }

    fn title_bar(&self, c: &tokens::Chrome, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(tokens::TITLE_BAR_HEIGHT))
            .px(px(tokens::SPACE_L))
            .gap(px(tokens::SPACE_M))
            .bg(c.surface)
            .border_b_1()
            .border_color(c.rule)
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(tokens::TEXT_WORDMARK))
                    .text_color(c.ink)
                    .child("Open Bible"),
            )
            .child(div().flex_1())
            // The one control so far, so a screen reader has something to reach (#377).
            .child(
                Button::new("quit")
                    .accessibility_label(QUIT_LABEL)
                    .px(px(tokens::SPACE_M))
                    .py(px(tokens::SPACE_S / 2.0))
                    .rounded(px(tokens::RADIUS_CONTROL))
                    .border_1()
                    .border_color(c.rule)
                    .text_color(c.ink)
                    .hover(|s| s.bg(c.hover))
                    // The keyboard focus: the outline in ink instead of the rule's grey.
                    .focus_visible(|s| s.border_color(c.ink))
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.quit_requested = true;
                        cx.quit();
                    }))
                    .child("Quit"),
            )
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = self.chrome(window);
        let painted = self.painted.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(c.shell)
            // The interface font is the bundled Gentium Plus until #378 chooses one, so the
            // goldens draw the same on every computer (ADR 0019 rule 4).
            .font_family("Gentium Plus")
            .text_size(px(tokens::TEXT_BODY))
            .text_color(c.ink)
            .child(self.title_bar(&c, cx))
            // The Workspace: its Panels and Tabs come with #385.
            .child(
                div()
                    .flex_1()
                    .m(px(tokens::SPACE_S))
                    .p(px(tokens::SPACE_XL))
                    .bg(c.surface)
                    .border_1()
                    .border_color(c.rule)
                    .rounded(px(tokens::RADIUS_CONTROL))
                    .text_color(c.muted)
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

/// The main window's options: `bounds` (the app's default size, centred, when `None`) and
/// the app's title.
pub fn main_window_options(bounds: Option<Bounds<gpui_kit::Pixels>>, cx: &App) -> WindowOptions {
    let bounds = bounds.unwrap_or_else(|| {
        Bounds::centered(
            None,
            size(px(tokens::WINDOW_WIDTH), px(tokens::WINDOW_HEIGHT)),
            cx,
        )
    });
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some(APP_TITLE.into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Open the main window over `session` with `options` ([`main_window_options`]); [`install`]
/// must have run.
pub fn open_main_window(
    session: Session,
    options: WindowOptions,
    cx: &mut App,
) -> gpui_kit::Result<(AnyWindowHandle, Entity<Shell>)> {
    gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| {
            // Follow system draws in the window's appearance: redraw when it changes.
            cx.observe_window_appearance(window, |_, _, cx| cx.notify())
                .detach();
            Shell::new(session)
        })
    })
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
        install(cx);
        let Some(session) = session.take() else {
            return;
        };
        let window_options = main_window_options(None, cx);
        let shell = match open_main_window(session, window_options, cx) {
            Ok((_, shell)) => shell,
            Err(e) => {
                result.set(Some(Outcome::SmokeFailed(format!(
                    "the window did not open: {e}"
                ))));
                cx.quit();
                return;
            }
        };
        let painted = shell.read(cx).painted.clone();
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
