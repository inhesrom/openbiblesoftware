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

use gpui_kit::base::dock::DockArea;
use gpui_kit::base::input::InputState;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyWindowHandle, App, Bounds, Context, Entity, FontWeight, Render, TitlebarOptions, Window,
    WindowBounds, WindowOptions, canvas, div, px, size,
};
use obs_engine::Session;
use obs_engine::launch::Launch;
use serde_json::Value;

pub mod catalogue;
pub mod components;
pub mod icons;
pub mod theme;
pub mod tokens;
pub mod workspace;

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
pub fn bundled_fonts() -> Vec<Cow<'static, [u8]>> {
    obs_fonts::FaceId::ALL
        .iter()
        .filter_map(|face| face.bundled_bytes())
        .map(Cow::Borrowed)
        // The default Interface font (#138): egui's own proportional face, so both UIs draw
        // the chrome alike.
        .chain([Cow::Borrowed(epaint_default_fonts::UBUNTU_LIGHT)])
        .collect()
}

/// Set up GPUI for the app (or a test context), once, before any window opens: GPUI Kit's
/// layers and the bundled fonts. The icon set's assets are the application's
/// ([`icons::Assets`]).
pub fn install(cx: &mut App) {
    gpui_kit::init(cx);
    if let Err(e) = cx.text_system().add_fonts(bundled_fonts()) {
        log::warn!("bundled fonts: {e}");
    }
}

/// The main window's view: the Session and, for now, an empty Workspace.
pub struct Shell {
    /// The study state the UI draws. Only its appearance is drawn until the Workspace is built
    /// (#385); held so it lives, with its background jobs, as long as the window.
    session: Session,
    /// The component catalogue (#378), drawn instead of the Workspace when asked for.
    catalogue: Option<Entity<catalogue::Catalogue>>,
    /// The Look the Session last asked for, so a change of appearance is made current once.
    look_asked: Option<theme::Look>,
    /// The Quit button was pressed (the app quits; the test platform's quit does nothing).
    quit_requested: bool,
    /// Set when a frame has finished painting (a smoke run waits for it).
    painted: Rc<Cell<Option<Instant>>>,
    /// Tests: colours drawn instead of the theme's (the gallery's seeded change, #379).
    #[cfg(feature = "test-support")]
    chrome_override: Option<tokens::Chrome>,
    /// The Workspace's dock (#385), made with the first frame (it needs the window).
    dock: Option<Entity<DockArea>>,
    /// Which dock group is which engine Panel, and which dock panel which engine Tab.
    map: workspace::DockMap,
    /// The Panels the engine was last told the dock shows.
    shown: Vec<(obs_engine::PanelId, Vec<obs_engine::TabId>)>,
    /// The latest message for the user (a toast), until the next one.
    toast: Option<String>,
    /// The + menu is open, for this dock group.
    pub(crate) plus_menu: Option<gpui_kit::base::dock::NodeId>,
    /// The Layout-name prompt: the Layout being renamed (none: saving a new one) and the box.
    layout_prompt: Option<(Option<String>, Entity<InputState>)>,
    /// Write the Last session as the Workspace changes. Off in the app unless
    /// `OBS_GPUI_SAVE_SESSION=1`: while egui ships, the GPUI app reads the egui app's Last
    /// session but does not overwrite it (docs/dev/ui-gpui.md).
    save_session: bool,
}

impl Shell {
    pub fn new(session: Session) -> Self {
        Self {
            session,
            catalogue: None,
            look_asked: None,
            quit_requested: false,
            painted: Rc::new(Cell::new(None)),
            #[cfg(feature = "test-support")]
            chrome_override: None,
            dock: None,
            map: workspace::DockMap::default(),
            shown: Vec::new(),
            toast: None,
            plus_menu: None,
            layout_prompt: None,
            save_session: std::env::var("OBS_GPUI_SAVE_SESSION").as_deref() == Ok("1"),
        }
    }

    /// Whether the Workspace writes the Last session as it changes (see the field).
    pub fn set_save_session(&mut self, on: bool) {
        self.save_session = on;
    }

    /// The latest message shown to the user (a toast).
    pub fn toast(&self) -> Option<&str> {
        self.toast.as_deref()
    }

    /// The Workspace's dock, once the first frame has made it.
    pub fn dock(&self) -> Option<&Entity<DockArea>> {
        self.dock.as_ref()
    }

    /// Which dock group is which engine Panel and which dock panel which engine Tab.
    pub fn dock_map(&self) -> &workspace::DockMap {
        &self.map
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// Run a command, as the egui Shell's `run` does: its result, or `None` when it failed
    /// (logged; the toast comes with the Workspace's chrome, #385).
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

    /// The Look this frame draws in. The catalogue draws in the Look it switches to; otherwise
    /// the Session's appearance decides it (Follow system takes the window's appearance), and
    /// when that changes it is made current ([`theme::set_look`]) for every view.
    fn look(&mut self, window: &Window, cx: &mut App) -> theme::Look {
        if self.catalogue.is_none() {
            self.follow_session(window, cx);
        }
        let look = theme::look(cx);
        #[cfg(feature = "test-support")]
        let look = match self.chrome_override {
            Some(chrome) => theme::Look { chrome, ..look },
            None => look,
        };
        look
    }

    fn follow_session(&mut self, window: &Window, cx: &mut App) {
        let asked = theme::session_look(&self.session, theme::is_dark(window.appearance()));
        if self.look_asked.as_ref() == Some(&asked) {
            return;
        }
        self.look_asked = Some(asked.clone());
        theme::set_look(asked, cx);
    }

    fn title_bar(&self, look: &theme::Look, cx: &mut Context<Self>) -> impl IntoElement {
        let ch = look.chrome;
        div()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(look.pt(tokens::TITLE_BAR_HEIGHT)))
            .px(px(look.pt(tokens::inset::MENU_BAR.0)))
            .gap(px(look.pt(tokens::space::M)))
            .bg(tokens::rgba(ch.surface))
            .border_b_1()
            .border_color(tokens::rgba(ch.rule))
            .child(
                div()
                    .font_family(theme::DISPLAY_FAMILY)
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(look.pt(tokens::text::TITLE)))
                    .text_color(tokens::rgba(ch.ink))
                    .child("Open Bible"),
            )
            .child(div().flex_1())
            // The one control so far, so a screen reader has something to reach (#377).
            .child(
                components::button("quit", "Quit", look)
                    .accessibility_label(QUIT_LABEL)
                    .on_click(cx.listener(|shell, _, _, cx| {
                        shell.quit_requested = true;
                        cx.quit();
                    })),
            )
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let painted = self.painted.clone();
        let look = self.look(window, cx);
        let ch = look.chrome;
        if self.catalogue.is_none() {
            self.ensure_dock(window, cx);
            self.apply_effects(window, cx);
        }
        let overlays = self.overlays(&look, window, cx);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(tokens::rgba(ch.shell))
            .font_family(look.interface_family.clone())
            .font_weight(look.interface_weight)
            .text_size(px(look.pt(tokens::text::BODY)))
            .text_color(tokens::rgba(ch.ink))
            .child(self.title_bar(&look, cx))
            .map(|this| match &self.catalogue {
                Some(catalogue) => {
                    this.child(div().flex_1().min_h(px(0.0)).child(catalogue.clone()))
                }
                // The Workspace (#385): the engine's Panels and Tabs in the dock.
                None => this.child(
                    div()
                        .flex_1()
                        .min_h(px(0.0))
                        .p(px(look.pt(tokens::space::XS)))
                        .children(self.dock.clone()),
                ),
            })
            .children(overlays)
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
    open_shell(session, options, false, cx)
}

/// Open the main window, with the component catalogue in place of the Workspace when
/// `catalogue` is set.
fn open_shell(
    session: Session,
    options: WindowOptions,
    catalogue: bool,
    cx: &mut App,
) -> gpui_kit::Result<(AnyWindowHandle, Entity<Shell>)> {
    gpui_kit::open_window(options, cx, move |window, cx| {
        let catalogue = catalogue.then(|| cx.new(|cx| catalogue::Catalogue::new(window, cx)));
        cx.new(|cx| {
            // Follow system draws in the window's appearance: redraw when it changes.
            cx.observe_window_appearance(window, |_, _, cx| cx.notify())
                .detach();
            Shell {
                catalogue,
                ..Shell::new(session)
            }
        })
    })
}

/// `OBS_GPUI_CATALOGUE`: open the component catalogue instead of the Workspace; its value
/// (`Light`, `Dark`, `Sepia`, `High contrast`, `High contrast dark`) chooses the Look it opens
/// in, and `OBS_GPUI_ZOOM` its Interface size.
fn catalogue_look() -> Option<theme::Look> {
    let name = std::env::var("OBS_GPUI_CATALOGUE").ok()?;
    let zoom = std::env::var("OBS_GPUI_ZOOM")
        .ok()
        .and_then(|z| z.parse().ok())
        .unwrap_or(1.0);
    catalogue::look_named(&name, zoom).or_else(|| catalogue::look_named("Light", zoom))
}

/// Open the app window over `launch`'s Session and run until it closes (or, for a smoke run,
/// until a frame has been drawn). `started` is when the process started, for the smoke run's
/// first-frame time.
pub fn run(launch: Launch, options: Options, started: Instant) -> Outcome {
    let outcome: Rc<Cell<Option<Outcome>>> = Rc::new(Cell::new(None));
    let result = outcome.clone();
    let mut session = Some(launch.session);
    gpui_kit::application()
        .with_assets(icons::Assets)
        .run(move |cx: &mut App| {
            install(cx);
            let Some(session) = session.take() else {
                return;
            };
            let catalogue_look = catalogue_look();
            let catalogue = catalogue_look.is_some();
            if let Some(look) = catalogue_look {
                theme::set_look(look, cx);
            }
            let window_options = main_window_options(None, cx);
            let shell = match open_shell(session, window_options, catalogue, cx) {
                Ok((_, shell)) => shell,
                Err(e) => {
                    result.set(Some(Outcome::SmokeFailed(format!(
                        "the window did not open: {e}"
                    ))));
                    cx.quit();
                    return;
                }
            };
            // The Workspace comes back as it was: the Last session, else the default one.
            if !catalogue {
                shell.update(cx, |s, _| s.restore_at_launch());
            }
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
