//! The GPUI tokens are the egui app's (ADR 0015): every value copied into
//! `obs_ui_gpui::tokens` must equal the one in `obs_ui_egui::tokens` / `Palette`.

use obs_engine::{CustomTheme, PageTone, Rgb, Theme, ThemeRole};
use obs_ui_egui::tokens as e;
use obs_ui_gpui::theme::{self, Look};
use obs_ui_gpui::tokens as g;

macro_rules! from_egui {
    ($c:expr) => {{
        let c = $c;
        Rgb::new(c.r(), c.g(), c.b())
    }};
}

fn chrome_of(c: &e::Chrome) -> g::Chrome {
    g::Chrome {
        dark: c.dark,
        high_contrast: c.high_contrast,
        shell: from_egui!(c.shell),
        surface: from_egui!(c.surface),
        field: from_egui!(c.field),
        rule: from_egui!(c.rule),
        ink: from_egui!(c.ink),
        muted: from_egui!(c.muted),
        lapis: from_egui!(c.lapis),
        verdigris: from_egui!(c.verdigris),
        violet: from_egui!(c.violet),
        gilt: from_egui!(c.gilt),
        selection: from_egui!(c.selection),
        hover: from_egui!(c.hover),
        hover_rule: from_egui!(c.hover_rule),
        warn: from_egui!(c.warn),
        unlinked: from_egui!(c.unlinked),
    }
}

#[test]
fn chromes_equal_the_egui_ones() {
    assert_eq!(g::LIGHT, chrome_of(&e::LIGHT));
    assert_eq!(g::DARK, chrome_of(&e::DARK));
    assert_eq!(g::HIGH_CONTRAST_LIGHT, chrome_of(&e::HIGH_CONTRAST_LIGHT));
    assert_eq!(g::HIGH_CONTRAST_DARK, chrome_of(&e::HIGH_CONTRAST_DARK));
}

#[test]
fn pages_equal_the_egui_palettes() {
    use obs_ui_egui::Palette;
    for (tone, hc) in [
        (PageTone::Paper, false),
        (PageTone::Sepia, false),
        (PageTone::Dark, false),
        (PageTone::Paper, true),
        (PageTone::Sepia, true),
        (PageTone::Dark, true),
    ] {
        let p = Palette::for_theme(tone, hc);
        let ours = g::page_for(tone, hc);
        assert_eq!(ours.page, from_egui!(p.page), "{tone:?} {hc}");
        assert_eq!(ours.ink, from_egui!(p.ink), "{tone:?} {hc}");
        assert_eq!(
            ours.verse_number,
            from_egui!(p.verse_number),
            "{tone:?} {hc}"
        );
        assert_eq!(ours.link, from_egui!(p.link), "{tone:?} {hc}");
    }
}

#[test]
fn measurements_equal_the_egui_ones() {
    let pairs: &[(&str, f32, f32)] = &[
        ("space::PX", g::space::PX, e::space::PX),
        ("space::XXS", g::space::XXS, e::space::XXS),
        ("space::XS", g::space::XS, e::space::XS),
        ("space::S", g::space::S, e::space::S),
        ("space::M", g::space::M, e::space::M),
        ("space::L", g::space::L, e::space::L),
        ("space::XL", g::space::XL, e::space::XL),
        ("space::XXL", g::space::XXL, e::space::XXL),
        ("space::XXXL", g::space::XXXL, e::space::XXXL),
        ("text::CAPTION", g::text::CAPTION, e::text::CAPTION),
        ("text::SMALL", g::text::SMALL, e::text::SMALL),
        ("text::LABEL", g::text::LABEL, e::text::LABEL),
        ("text::BODY", g::text::BODY, e::text::BODY),
        ("text::SUBTITLE", g::text::SUBTITLE, e::text::SUBTITLE),
        ("text::TITLE", g::text::TITLE, e::text::TITLE),
        ("text::DISPLAY", g::text::DISPLAY, e::text::DISPLAY),
        ("text::MONO", g::text::MONO, e::text::MONO),
        ("text::LABEL_LINE", g::text::LABEL_LINE, e::text::LABEL_LINE),
        (
            "text::CAPTION_LINE",
            g::text::CAPTION_LINE,
            e::text::CAPTION_LINE,
        ),
        ("radius::XS", g::radius::XS, e::radius::XS),
        ("radius::S", g::radius::S, e::radius::S),
        ("radius::M", g::radius::M, e::radius::M),
        ("radius::L", g::radius::L, e::radius::L),
        ("radius::XL", g::radius::XL, e::radius::XL),
        ("radius::PILL", g::radius::PILL, e::radius::PILL),
        ("stroke::HAIR", g::stroke::HAIR, e::stroke::HAIR),
        ("stroke::MEDIUM", g::stroke::MEDIUM, e::stroke::MEDIUM),
        ("stroke::THICK", g::stroke::THICK, e::stroke::THICK),
        ("stroke::HEAVY", g::stroke::HEAVY, e::stroke::HEAVY),
        ("alpha::WASH", g::alpha::WASH, e::alpha::WASH),
        ("alpha::TINT", g::alpha::TINT, e::alpha::TINT),
        ("alpha::HALF", g::alpha::HALF, e::alpha::HALF),
        ("alpha::SOFT", g::alpha::SOFT, e::alpha::SOFT),
        ("alpha::STRONG", g::alpha::STRONG, e::alpha::STRONG),
        ("size::ICON_SMALL", g::size::ICON_SMALL, e::size::ICON_SMALL),
        ("size::ICON", g::size::ICON, e::size::ICON),
        ("size::ICON_LARGE", g::size::ICON_LARGE, e::size::ICON_LARGE),
        ("size::CONTROL", g::size::CONTROL, e::size::CONTROL),
        (
            "size::CONTROL_COMPACT",
            g::size::CONTROL_COMPACT,
            e::size::CONTROL_COMPACT,
        ),
        ("size::CHIP", g::size::CHIP, e::size::CHIP),
        (
            "size::ROW_COMPACT",
            g::size::ROW_COMPACT,
            e::size::ROW_COMPACT,
        ),
        ("size::ROW", g::size::ROW, e::size::ROW),
        ("size::TAB_BAR", g::size::TAB_BAR, e::size::TAB_BAR),
        ("size::SCROLLBAR", g::size::SCROLLBAR, e::size::SCROLLBAR),
        ("size::THUMB_MIN", g::size::THUMB_MIN, e::size::THUMB_MIN),
        (
            "size::FIELD_NARROW",
            g::size::FIELD_NARROW,
            e::size::FIELD_NARROW,
        ),
        ("size::FIELD", g::size::FIELD, e::size::FIELD),
        ("size::FIELD_WIDE", g::size::FIELD_WIDE, e::size::FIELD_WIDE),
        ("size::PROSE", g::size::PROSE, e::size::PROSE),
        ("size::POPOVER", g::size::POPOVER, e::size::POPOVER),
        ("size::TOAST_RISE", g::size::TOAST_RISE, e::size::TOAST_RISE),
        (
            "size::DIALOG_WIDTH",
            g::size::DIALOG_WIDTH,
            e::size::DIALOG_WIDTH,
        ),
        (
            "size::MENU_COLUMN",
            g::size::MENU_COLUMN,
            e::size::MENU_COLUMN,
        ),
        ("size::PAGE_MAX", g::size::PAGE_MAX, e::size::PAGE_MAX),
    ];
    for (name, ours, theirs) in pairs {
        assert_eq!(ours, theirs, "{name}");
    }
}

#[test]
fn insets_motion_and_elevation_equal_the_egui_ones() {
    for (name, ours, theirs) in [
        ("PAGE", g::inset::PAGE, e::inset::PAGE),
        ("CARD", g::inset::CARD, e::inset::CARD),
        ("NOTICE", g::inset::NOTICE, e::inset::NOTICE),
        ("POPOVER", g::inset::POPOVER, e::inset::POPOVER),
        ("WINDOW", g::inset::WINDOW, e::inset::WINDOW),
        ("BAR", g::inset::BAR, e::inset::BAR),
        ("CHIP", g::inset::CHIP, e::inset::CHIP),
        ("BAND", g::inset::BAND, e::inset::BAND),
        ("MENU_BAR", g::inset::MENU_BAR, e::inset::MENU_BAR),
        ("WORKSPACE", g::inset::WORKSPACE, e::inset::WORKSPACE),
    ] {
        assert_eq!(
            (ours.0 as i8, ours.1 as i8, ours.0 as i8, ours.1 as i8),
            (theirs.left, theirs.top, theirs.right, theirs.bottom),
            "inset::{name}"
        );
    }
    assert_eq!(g::motion::POPOVER, e::motion::POPOVER);
    assert_eq!(g::motion::POPOVER_SCALE, e::motion::POPOVER_SCALE);
    assert_eq!(g::motion::PANEL, e::motion::PANEL);
    assert_eq!(g::motion::JUMP, e::motion::JUMP);
    assert_eq!(g::motion::SCROLL, e::motion::SCROLL);
    assert_eq!(g::motion::FLASH, e::motion::FLASH);
    assert_eq!(g::motion::SHIMMER, e::motion::SHIMMER);
    for dark in [false, true] {
        for (ours, theirs) in [
            (g::elevation::WINDOW, e::elevation::window(dark)),
            (g::elevation::POPUP, e::elevation::popup(dark)),
        ] {
            assert_eq!(ours.0, f32::from(theirs.offset[1]));
            assert_eq!(ours.1, f32::from(theirs.blur));
            assert_eq!(if dark { ours.3 } else { ours.2 }, theirs.color.a());
        }
    }
}

#[test]
fn the_theme_chooses_the_chrome_as_the_egui_app_does() {
    let custom = CustomTheme::default();
    for t in [
        Theme::Light,
        Theme::Dark,
        Theme::HighContrastLight,
        Theme::HighContrastDark,
    ] {
        let ([light, dark], ground) = obs_ui_egui::theme::chromes_of(t, &custom);
        let egui_chrome = if ground == Some(egui::Theme::Dark) {
            dark
        } else {
            light
        };
        assert_eq!(
            theme::chrome_of(t, &custom, false),
            chrome_of(&egui_chrome),
            "{t:?}"
        );
    }
    assert_eq!(theme::chrome_of(Theme::System, &custom, true), g::DARK);
    assert_eq!(theme::chrome_of(Theme::System, &custom, false), g::LIGHT);
}

#[test]
fn a_custom_theme_is_its_base_with_the_changed_roles() {
    let mut custom = CustomTheme {
        base: Theme::Dark,
        ..CustomTheme::default()
    };
    custom.colors.insert(ThemeRole::Lapis, Rgb::new(1, 2, 3));
    custom.colors.insert(ThemeRole::Shell, Rgb::new(9, 8, 7));
    let ours = theme::chrome_of(Theme::Custom, &custom, false);
    assert_eq!(ours, chrome_of(&obs_ui_egui::theme::custom_chrome(&custom)));
    assert_eq!(
        theme::role_color(&ours, ThemeRole::Lapis),
        Rgb::new(1, 2, 3)
    );
    assert_eq!(ours.surface, g::DARK.surface);
}

#[test]
fn the_look_follows_the_appearance() {
    let mut a = obs_engine::Appearance::default();
    let fonts = obs_engine::FontChoices::default();
    let custom = CustomTheme::default();
    a.theme = Theme::Dark;
    a.page_tone = PageTone::Auto;
    let l = Look::resolve(&a, &custom, &fonts, false, false);
    assert_eq!(
        l.page,
        g::PAGE_DARK,
        "Auto with a dark theme is the Dark page"
    );
    a.page_tone = PageTone::Sepia;
    a.interface_size = obs_engine::InterfaceSize::Larger;
    let l = Look::resolve(&a, &custom, &fonts, false, false);
    assert_eq!(l.page, g::SEPIA);
    assert_eq!(l.zoom, 1.25);
    assert_eq!(l.pt(g::size::CONTROL), 35.0);
    assert!(!l.reduce_motion);
    assert!(
        Look::resolve(&a, &custom, &fonts, false, true).reduce_motion,
        "the system's"
    );
    a.reduce_motion = true;
    assert!(
        Look::resolve(&a, &custom, &fonts, false, false).reduce_motion,
        "the app's"
    );
    a.theme = Theme::HighContrastLight;
    let l = Look::resolve(&a, &custom, &fonts, false, false);
    assert_eq!(l.focus_width(), g::stroke::HEAVY);
    assert_eq!(l.page, g::SEPIA_HIGH_CONTRAST);
    assert_eq!(l.interface_family, theme::DEFAULT_INTERFACE_FAMILY);
}

#[test]
fn every_icon_of_the_egui_set_is_here_by_the_same_file() {
    let egui: Vec<&str> = obs_ui_egui::icons::Icon::ALL
        .iter()
        .map(|i| i.file())
        .collect();
    let ours: Vec<&str> = obs_ui_gpui::icons::Icon::ALL
        .iter()
        .map(|i| i.file())
        .collect();
    assert_eq!(ours, egui);
    use gpui_kit::AssetSource;
    for i in obs_ui_gpui::icons::Icon::ALL {
        let bytes = obs_ui_gpui::icons::Assets.load(&i.path()).ok().flatten();
        assert!(bytes.is_some_and(|b| b.starts_with(b"<svg")), "{i:?}");
    }
}
