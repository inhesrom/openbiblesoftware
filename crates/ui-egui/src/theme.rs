//! The Study desk visual system for the chrome (chosen in #13's polish follow-up): cool linen
//! surfaces, iron ink, lapis navigation, verdigris study accents and Gentium display headings.
//! The colours and measurements themselves are design tokens ([`crate::tokens`], #221);
//! this module installs them as egui's style and says which theme is current. Resource text
//! colours live in [`crate::Palette`].
//!
//! The Theme chooses a pair of chromes, one for a light and one for a dark ground: Light and
//! Dark (and Follow system) the Study desk's, High contrast its own (#224), Custom the user's
//! colours over a built-in theme's ([`custom_chrome`]). [`contrast_shorts`] is the one check
//! of a chrome's colours: the a11y audit runs it on the built-in themes and the Theme editor
//! shows its findings as warnings while the user edits.

use std::sync::Arc;

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle};
use obs_fonts::FaceId;

use obs_engine::{CustomTheme, Theme, ThemeRole};

use crate::palette::{AAA_TEXT_CONTRAST, MARK_CONTRAST, TEXT_CONTRAST, contrast, to_color};
use crate::tokens::{self, corner, elevation, inset, radius, size, stroke, text};

pub use crate::tokens::{Chrome, DARK, HIGH_CONTRAST_DARK, HIGH_CONTRAST_LIGHT, LIGHT};

thread_local! {
    static CURRENT: std::cell::Cell<Chrome> = const { std::cell::Cell::new(LIGHT) };
}

/// The chrome palette of the frame being drawn.
pub fn c() -> Chrome {
    CURRENT.with(std::cell::Cell::get)
}

/// Set the chrome palette for this frame (from the context's current theme).
pub fn set_for(ctx: &egui::Context) {
    let [light, dark] = installed(ctx);
    let chrome = if ctx.theme() == egui::Theme::Dark {
        dark
    } else {
        light
    };
    CURRENT.with(|cur| cur.set(chrome));
}

/// The chromes installed for a light and a dark ground.
fn installed(ctx: &egui::Context) -> [Chrome; 2] {
    ctx.data(|d| d.get_temp::<[Chrome; 2]>(installed_id()))
        .unwrap_or([LIGHT, DARK])
}

fn installed_id() -> egui::Id {
    egui::Id::unique("obs-installed-chromes")
}

/// Use `light` and `dark` as the chromes of egui's light and dark themes, restyling egui only
/// when they change.
pub fn use_chromes(ctx: &egui::Context, light: Chrome, dark: Chrome) {
    if ctx.data(|d| d.get_temp::<[Chrome; 2]>(installed_id())) == Some([light, dark]) {
        return;
    }
    ctx.style_mut_of(egui::Theme::Light, |s| style(s, &light));
    ctx.style_mut_of(egui::Theme::Dark, |s| style(s, &dark));
    ctx.data_mut(|d| d.insert_temp(installed_id(), [light, dark]));
}

/// The chromes a Theme draws with, for a light and a dark ground, and the ground it asks egui
/// for (`None`: follow the operating system).
pub fn chromes_of(theme: Theme, custom: &CustomTheme) -> ([Chrome; 2], Option<egui::Theme>) {
    match theme {
        Theme::System => ([LIGHT, DARK], None),
        Theme::Light => ([LIGHT, DARK], Some(egui::Theme::Light)),
        Theme::Dark => ([LIGHT, DARK], Some(egui::Theme::Dark)),
        Theme::HighContrastLight | Theme::HighContrastDark => (
            [HIGH_CONTRAST_LIGHT, HIGH_CONTRAST_DARK],
            Some(if theme == Theme::HighContrastDark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            }),
        ),
        Theme::Custom => {
            let ch = custom_chrome(custom);
            let ground = if ch.dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            };
            ([ch, ch], Some(ground))
        }
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

/// The Custom theme's chrome: its base's, with the colours the user changed.
pub fn custom_chrome(custom: &CustomTheme) -> Chrome {
    let mut ch = built_in_chrome(custom.base);
    for (role, c) in &custom.colors {
        *role_mut(&mut ch, *role) = to_color(*c);
    }
    ch
}

/// The colour of `role` in `ch`.
pub fn role_color(ch: &Chrome, role: ThemeRole) -> Color32 {
    let mut ch = *ch;
    *role_mut(&mut ch, role)
}

fn role_mut(ch: &mut Chrome, role: ThemeRole) -> &mut Color32 {
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

/// What text needs against what it is drawn on in `ch`: WCAG AA, or AAA in High contrast.
pub fn text_contrast(ch: &Chrome) -> f32 {
    if ch.high_contrast {
        AAA_TEXT_CONTRAST
    } else {
        TEXT_CONTRAST
    }
}

/// The width of the keyboard focus ring: heavy in High contrast.
pub fn focus_ring_width() -> f32 {
    if c().high_contrast {
        stroke::HEAVY
    } else {
        stroke::THICK
    }
}

/// A pair of a chrome's colours that does not reach what it needs.
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeShort {
    /// What is drawn: a role's name, or what is drawn in it ("Focus ring").
    pub what: String,
    /// What it is drawn on.
    pub on: &'static str,
    pub ratio: f32,
    pub needed: f32,
}

impl std::fmt::Display for ChromeShort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} on {}: {:.1}:1, needs {:.1}:1",
            self.what, self.on, self.ratio, self.needed
        )
    }
}

/// The colour roles the chrome draws text in.
const TEXT_ROLES: [ThemeRole; 5] = [
    ThemeRole::Ink,
    ThemeRole::Muted,
    ThemeRole::Lapis,
    ThemeRole::Verdigris,
    ThemeRole::Warn,
];

/// Whether every text colour of `ch` reads on `ground` as [`contrast_shorts`] asks of the
/// chrome's own backgrounds: a Guide's page is the Page tone only when it does (#306).
pub fn text_reads_on(ch: &Chrome, ground: Color32) -> bool {
    let needed = text_contrast(ch);
    TEXT_ROLES
        .into_iter()
        .all(|r| contrast(role_color(ch, r), ground) >= needed)
}

/// The width of a control's border: medium in High contrast, so controls show their edges
/// without colour (#224), else a hairline.
pub fn border_width(ch: &Chrome) -> f32 {
    if ch.high_contrast {
        stroke::MEDIUM
    } else {
        stroke::HAIR
    }
}

/// Every pair of `ch`'s colours that does not reach WCAG (#182, #224): each text colour on
/// each background it can sit on (AA, AAA in High contrast), the star and the focus ring at
/// 3:1, the text on each chip and badge, and in High contrast the borders at 3:1 so controls
/// show their edges without colour.
pub fn contrast_shorts(ch: &Chrome) -> Vec<ChromeShort> {
    let label = ThemeRole::label;
    let backgrounds = [
        ThemeRole::Shell,
        ThemeRole::Surface,
        ThemeRole::Field,
        ThemeRole::Hover,
        ThemeRole::Selection,
    ];
    let text = text_contrast(ch);
    let mut drawn: Vec<(String, Color32, f32)> = TEXT_ROLES
        .into_iter()
        .map(|r| (label(r).to_string(), role_color(ch, r), text))
        .collect();
    drawn.push(("Star marks".to_string(), ch.gilt, MARK_CONTRAST));
    drawn.push(("Focus ring".to_string(), ch.lapis, MARK_CONTRAST));
    if ch.high_contrast {
        drawn.push((label(ThemeRole::Rule).to_string(), ch.rule, MARK_CONTRAST));
    }
    let mut out = Vec::new();
    for bg in backgrounds {
        for (what, fg, needed) in &drawn {
            let ratio = contrast(*fg, role_color(ch, bg));
            if ratio < *needed {
                out.push(ChromeShort {
                    what: what.clone(),
                    on: label(bg),
                    ratio,
                    needed: *needed,
                });
            }
        }
    }
    if ch.high_contrast {
        let ratio = contrast(ch.hover_rule, ch.hover);
        if ratio < MARK_CONTRAST {
            out.push(ChromeShort {
                what: label(ThemeRole::HoverRule).to_string(),
                on: label(ThemeRole::Hover),
                ratio,
                needed: MARK_CONTRAST,
            });
        }
    }
    // Chips and badges: their text is chosen for their fill.
    for fill in [
        ThemeRole::Lapis,
        ThemeRole::Verdigris,
        ThemeRole::Violet,
        ThemeRole::Gilt,
        ThemeRole::Unlinked,
    ] {
        let color = role_color(ch, fill);
        let ratio = contrast(text_on_in(ch, color), color);
        if ratio < text {
            out.push(ChromeShort {
                what: "Chip text".to_string(),
                on: label(fill),
                ratio,
                needed: text,
            });
        }
    }
    out
}

/// The text colour for a label on a solid `fill` (a Link set chip, a badge, a heat cell):
/// white or near-black, whichever reads better, and black when neither meets WCAG AA (#182),
/// or AAA in High contrast (#224).
pub fn text_on(fill: Color32) -> Color32 {
    text_on_in(&c(), fill)
}

/// [`text_on`] in chrome `ch`, which need not be the current one (the Theme editor's
/// preview).
pub fn text_on_in(ch: &Chrome, fill: Color32) -> Color32 {
    let contrast = |c| crate::palette::contrast(c, fill);
    let best = if contrast(tokens::INK_WHITE) >= contrast(LIGHT.ink) {
        tokens::INK_WHITE
    } else {
        LIGHT.ink
    };
    if contrast(best) < text_contrast(ch) && contrast(tokens::INK_BLACK) > contrast(best) {
        tokens::INK_BLACK
    } else {
        best
    }
}

/// Use `chrome` for the frame being drawn on this thread, as [`set_for`] does from a context.
#[cfg(test)]
pub(crate) fn set_for_test(chrome: Chrome) {
    CURRENT.with(|cur| cur.set(chrome));
}

/// Each Link set's colour.
pub fn link_color(link: Option<char>) -> Color32 {
    let ch = c();
    match link {
        Some('A') => ch.lapis,
        Some('B') => ch.verdigris,
        Some('C') => ch.violet,
        Some(_) => ch.gilt,
        None => ch.unlinked,
    }
}

const DISPLAY: &str = "display";

/// Gentium Plus Bold, for headings and the app name.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

/// Install the fonts and the Study desk style on a context.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx, None);
    ctx.style_mut_of(egui::Theme::Light, |s| style(s, &LIGHT));
    ctx.style_mut_of(egui::Theme::Dark, |s| style(s, &DARK));
    ctx.data_mut(|d| d.insert_temp(installed_id(), [LIGHT, DARK]));
}

const INTERFACE: &str = "interface";

const FALLBACK: &str = "gentium-regular";

/// Install the chrome's fonts: egui's defaults, Gentium Plus Bold for display headings and,
/// when one is chosen (View → Fonts, #138), the interface font ahead of egui's proportional
/// fonts. Those stay behind it as fallbacks, so emoji and symbols still draw. Gentium Plus
/// Regular comes last in the interface and monospace families, for what egui's fonts lack:
/// arrows (→ in "Edit → Undo", #262), polytonic Greek, IPA. The modifier symbols of shortcuts
/// (⌘ ⌥ ⇧ ⌃) are drawn by [`crate::key_labels::modifier_rasterizer`].
pub fn install_fonts(ctx: &egui::Context, interface: Option<egui::FontData>) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(bold) = FaceId::Bold.bundled_bytes() {
        fonts.font_data.insert(
            "gentium-bold".into(),
            Arc::new(egui::FontData::from_static(bold)),
        );
    }
    if let Some(regular) = FaceId::Regular.bundled_bytes() {
        fonts.font_data.insert(
            FALLBACK.into(),
            Arc::new(egui::FontData::from_static(regular)),
        );
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push(FALLBACK.into());
        }
    }
    if let Some(data) = interface {
        fonts.font_data.insert(INTERFACE.into(), Arc::new(data));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, INTERFACE.into());
    }
    let mut family = vec!["gentium-bold".to_string()];
    if let Some(p) = fonts.families.get(&FontFamily::Proportional) {
        family.extend(p.iter().filter(|f| *f != INTERFACE).cloned());
    }
    fonts
        .families
        .insert(FontFamily::Name(DISPLAY.into()), family);
    ctx.set_fonts(fonts);
    ctx.add_glyph_rasterizer(crate::key_labels::modifier_rasterizer());
}

fn style(style: &mut egui::Style, ch: &Chrome) {
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(text::BODY));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(text::BODY));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(text::SMALL));
    style
        .text_styles
        .insert(TextStyle::Heading, display(text::TITLE));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(text::MONO));
    style.spacing.item_spacing = tokens::ITEM_SPACING;
    style.spacing.button_padding = tokens::BUTTON_PADDING;
    style.spacing.interact_size.y = size::CONTROL;
    style.spacing.menu_margin = inset::POPOVER;
    style.spacing.window_margin = inset::WINDOW;

    let mut v = if ch.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = ch.shell;
    v.window_fill = ch.field;
    v.extreme_bg_color = ch.field;
    v.faint_bg_color = ch.surface;
    v.code_bg_color = ch.surface;
    v.hyperlink_color = ch.lapis;
    // Hints and shortcuts: egui's default fades the ink, below AA on the fields (#182).
    v.weak_text_color = Some(ch.muted);
    v.selection.bg_fill = ch.selection;
    v.selection.stroke = Stroke::new(stroke::HAIR, ch.lapis);
    v.window_stroke = Stroke::new(stroke::HAIR, ch.rule);
    v.window_corner_radius = corner(radius::XL);
    v.menu_corner_radius = corner(radius::L);
    v.window_shadow = elevation::window(ch.dark);
    v.popup_shadow = elevation::popup(ch.dark);
    let control = corner(radius::M);
    // High contrast (#224): controls show their edges in a border that reads at 3:1, not by
    // a fill colour alone.
    let border = border_width(ch);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = ch.surface;
    w.noninteractive.weak_bg_fill = ch.surface;
    w.noninteractive.bg_stroke = Stroke::new(stroke::HAIR, ch.rule);
    w.noninteractive.fg_stroke = Stroke::new(stroke::HAIR, ch.ink);
    w.noninteractive.corner_radius = control;
    w.inactive.bg_fill = ch.field;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::new(border, ch.rule);
    w.inactive.fg_stroke = Stroke::new(stroke::HAIR, ch.ink);
    w.inactive.corner_radius = control;
    w.hovered.bg_fill = ch.hover;
    w.hovered.weak_bg_fill = ch.hover;
    w.hovered.bg_stroke = Stroke::new(border, ch.hover_rule);
    w.hovered.fg_stroke = Stroke::new(stroke::HAIR, ch.ink);
    w.hovered.corner_radius = control;
    w.hovered.expansion = 0.0;
    w.active.bg_fill = ch.selection;
    w.active.weak_bg_fill = ch.selection;
    w.active.bg_stroke = Stroke::new(border, ch.lapis);
    w.active.fg_stroke = Stroke::new(stroke::MEDIUM, ch.lapis);
    w.active.corner_radius = control;
    w.active.expansion = 0.0;
    w.open = w.active;
    style.visuals = v;
}
