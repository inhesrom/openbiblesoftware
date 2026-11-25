//! The Look: which chrome, page and interface font the GPUI UI draws with, from the Session's
//! appearance (View → Theme, Page tone, Interface size, Fonts, Reduce motion), as the egui
//! app's `theme.rs` and `palette.rs` choose them.
//!
//! The Look is a GPUI global. [`set_look`] replaces it and refreshes every window, so a change
//! of theme shows in the next frame drawn, the same frame the change is made in.

use gpui_kit::{App, FontWeight, Global, WindowAppearance};
use obs_engine::{Appearance, CustomTheme, FontChoices, FontSlot, PageTone, Rgb, Theme, ThemeRole};

use crate::tokens::{self, Chrome, DARK, HIGH_CONTRAST_DARK, HIGH_CONTRAST_LIGHT, LIGHT, Page};

/// The bundled interface font when none is chosen: egui's default proportional face, so both
/// UIs draw the chrome alike.
pub const DEFAULT_INTERFACE_FAMILY: &str = "Ubuntu";
/// The weight of the bundled interface face, Ubuntu Light, asked for by name so a computer
/// with the Ubuntu family installed draws the bundled face, not its own Regular.
pub const DEFAULT_INTERFACE_WEIGHT: FontWeight = FontWeight::LIGHT;
/// The display face of headings and the wordmark (the egui app's `DISPLAY` family).
pub const DISPLAY_FAMILY: &str = "Gentium Plus";

/// Everything that decides how the chrome looks in this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub chrome: Chrome,
    pub page: Page,
    /// The interface font's family name (#138).
    pub interface_family: String,
    /// The interface font's weight: [`DEFAULT_INTERFACE_WEIGHT`] for the bundled face, normal
    /// for a font the user chose.
    pub interface_weight: FontWeight,
    /// Interface size: every chrome measurement is multiplied by this.
    pub zoom: f32,
    /// The app's own Reduce motion setting. Whether a frame reduces motion is
    /// `cx.reduce_motion()`: this, or else the operating system's setting, as gpui-base keeps
    /// it current ([`set_look`]).
    pub reduce_motion: bool,
}

impl Global for Look {}

impl Default for Look {
    fn default() -> Self {
        Self {
            chrome: LIGHT,
            page: tokens::PAPER,
            interface_family: DEFAULT_INTERFACE_FAMILY.into(),
            interface_weight: DEFAULT_INTERFACE_WEIGHT,
            zoom: 1.0,
            reduce_motion: false,
        }
    }
}

impl Look {
    /// The Look of `appearance`. `system_dark`: the operating system's appearance, for Follow
    /// system.
    pub fn resolve(
        appearance: &Appearance,
        custom: &CustomTheme,
        fonts: &FontChoices,
        system_dark: bool,
    ) -> Self {
        let chrome = chrome_of(appearance.theme, custom, system_dark);
        let tone = match appearance.page_tone {
            PageTone::Auto if chrome.dark => PageTone::Dark,
            PageTone::Auto => PageTone::Paper,
            t => t,
        };
        let (interface_family, interface_weight) = match fonts.get(FontSlot::Interface) {
            Some(f) => (f.family.clone(), FontWeight::NORMAL),
            None => (
                DEFAULT_INTERFACE_FAMILY.to_string(),
                DEFAULT_INTERFACE_WEIGHT,
            ),
        };
        Self {
            chrome,
            page: tokens::page_for(tone, chrome.high_contrast),
            interface_family,
            interface_weight,
            zoom: appearance.interface_size.zoom(),
            reduce_motion: appearance.reduce_motion,
        }
    }

    /// A measurement token at the Interface size.
    pub fn pt(&self, v: f32) -> f32 {
        v * self.zoom
    }

    /// The keyboard focus ring's width: heavy in a High contrast theme (#224).
    pub fn focus_width(&self) -> f32 {
        if self.chrome.high_contrast {
            tokens::stroke::HEAVY
        } else {
            tokens::stroke::THICK
        }
    }
}

/// The chrome a Theme draws with (the egui app's `chromes_of`, with the ground resolved).
pub fn chrome_of(theme: Theme, custom: &CustomTheme, system_dark: bool) -> Chrome {
    match theme {
        Theme::System if system_dark => DARK,
        Theme::System | Theme::Light => LIGHT,
        Theme::Dark => DARK,
        Theme::HighContrastLight => HIGH_CONTRAST_LIGHT,
        Theme::HighContrastDark => HIGH_CONTRAST_DARK,
        Theme::Custom => custom_chrome(custom),
    }
}

/// A built-in theme's chrome (Follow system and Custom read as Light).
pub fn built_in_chrome(theme: Theme) -> Chrome {
    match theme {
        Theme::Dark => DARK,
        Theme::HighContrastLight => HIGH_CONTRAST_LIGHT,
        Theme::HighContrastDark => HIGH_CONTRAST_DARK,
        Theme::Light | Theme::System | Theme::Custom => LIGHT,
    }
}

/// The Custom theme's chrome: its base's, with the colours the user changed (ADR 0018).
pub fn custom_chrome(custom: &CustomTheme) -> Chrome {
    let mut ch = built_in_chrome(custom.base);
    for (role, c) in &custom.colors {
        *role_mut(&mut ch, *role) = *c;
    }
    ch
}

/// The colour of `role` in `ch`.
pub fn role_color(ch: &Chrome, role: ThemeRole) -> Rgb {
    let mut ch = *ch;
    *role_mut(&mut ch, role)
}

fn role_mut(ch: &mut Chrome, role: ThemeRole) -> &mut Rgb {
    match role {
        ThemeRole::Shell => &mut ch.shell,
        ThemeRole::Surface => &mut ch.surface,
        ThemeRole::Field => &mut ch.field,
        ThemeRole::Rule => &mut ch.rule,
        ThemeRole::Ink => &mut ch.ink,
        ThemeRole::Muted => &mut ch.muted,
        ThemeRole::Lapis => &mut ch.lapis,
        ThemeRole::Verdigris => &mut ch.verdigris,
        ThemeRole::Violet => &mut ch.violet,
        ThemeRole::Gilt => &mut ch.gilt,
        ThemeRole::Selection => &mut ch.selection,
        ThemeRole::Hover => &mut ch.hover,
        ThemeRole::HoverRule => &mut ch.hover_rule,
        ThemeRole::Warn => &mut ch.warn,
        ThemeRole::Unlinked => &mut ch.unlinked,
    }
}

/// Whether the operating system asks for a dark appearance.
pub fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

/// The current Look (the default until [`set_look`] is called).
pub fn look(cx: &App) -> Look {
    cx.try_global::<Look>().cloned().unwrap_or_default()
}

/// Make `look` current, and draw every window again so the change shows in the next frame.
/// gpui-base's reduce motion preference is the app's own: Reduce when the Look asks for it,
/// else the operating system's setting (System), which gpui-base keeps current. Read
/// `cx.reduce_motion()` where motion is drawn.
pub fn set_look(look: Look, cx: &mut App) {
    use gpui_kit::base::{ReduceMotionPreference, set_reduce_motion_preference};
    if look.reduce_motion {
        set_reduce_motion_preference(ReduceMotionPreference::Reduce, cx);
    } else {
        // From GPUI's default (motion), so the app's earlier Reduce does not stay where the
        // system's setting is not known; System then applies the system's, when it is.
        cx.set_reduce_motion(false);
        set_reduce_motion_preference(ReduceMotionPreference::System, cx);
    }
    cx.set_global(look);
    cx.refresh_windows();
}

/// The Look the Session's appearance asks for, in a window that appears dark or not
/// (`system_dark`, for Follow system).
pub fn session_look(session: &obs_engine::Session, system_dark: bool) -> Look {
    Look::resolve(
        &session.appearance,
        &session.custom_theme,
        &session.fonts,
        system_dark,
    )
}
