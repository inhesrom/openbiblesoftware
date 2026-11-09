//! The High contrast themes (#224) through the shell's real-event headless harness and their
//! colour tokens: text at WCAG AAA in the chrome and on every Page tone (the rendered matrix
//! of every surface is `a11y_audit_tests.rs`), a heavy focus ring, and cues that are not
//! colour alone: borders on controls and links underlined at rest.

use egui::{Key, Shape};
use obs_engine::{HighlightLook, Rgb, StyleAppearance, StyleTone};

use super::*;
use crate::Palette;
use crate::palette::{AAA_TEXT_CONTRAST, MARK_CONTRAST, contrast};
use crate::theme::{HIGH_CONTRAST_DARK, HIGH_CONTRAST_LIGHT};
use crate::tokens::stroke;

const HIGH_CONTRAST_PAGES: [Palette; 3] = [
    Palette::PAPER_HIGH_CONTRAST,
    Palette::SEPIA_HIGH_CONTRAST,
    Palette::DARK_HIGH_CONTRAST,
];

/// Every text colour of each High contrast page reads at 7:1 on the page and on every
/// overlay (selection, hover, search hit, Find match, jump flash); the hovered word's
/// underline and the current Find match's outline show at 3:1.
#[test]
fn every_high_contrast_page_tone_reads_at_wcag_aaa() {
    for p in HIGH_CONTRAST_PAGES {
        assert!(p.high_contrast);
        assert_eq!(p.text_contrast(), AAA_TEXT_CONTRAST);
        for (role, ink) in p.inks() {
            for (name, bg) in p.overlays() {
                let c = contrast(ink, bg);
                assert!(
                    c >= AAA_TEXT_CONTRAST,
                    "{:?}: {role:?} on {name}: {c:.2}:1",
                    p.tone
                );
            }
        }
        for (what, mark) in [("underline", p.underline), ("Find outline", p.find_current)] {
            let c = contrast(mark, p.page);
            assert!(c >= MARK_CONTRAST, "{:?}: {what} {c:.2}:1", p.tone);
        }
    }
}

/// Highlight styles keep their contrast check per tone in High contrast: whatever colour a
/// user picks, its derived colour lets every ink read at 7:1 as a background (and reads at
/// 7:1 as a text colour), and its margin mark shows at 3:1.
#[test]
fn highlights_are_derived_to_aaa_on_high_contrast_pages() {
    let steps = [0u8, 85, 170, 255];
    for p in HIGH_CONTRAST_PAGES {
        let mut colours = Vec::new();
        for r in steps {
            for g in steps {
                for b in steps {
                    colours.push((r, g, b));
                }
            }
        }
        for (r, g, b) in colours {
            for look in HighlightLook::ALL {
                let a = StyleAppearance::new(look, Rgb::new(r, g, b));
                let paint = p.paint(&a);
                assert_eq!(p.check(look, paint.color), Ok(()), "{:?} {look:?}", p.tone);
                if look == HighlightLook::Background {
                    for (role, ink) in p.inks() {
                        let c = contrast(ink, paint.color);
                        assert!(c >= AAA_TEXT_CONTRAST, "{:?} {role:?} {c:.2}", p.tone);
                    }
                }
                assert!(contrast(paint.mark, p.page) >= MARK_CONTRAST);
            }
        }
    }
    // A style's adjustment for a tone that reads at AA but not AAA is drawn derived instead.
    let mut yellow = StyleAppearance::new(HighlightLook::Background, Rgb::new(255, 236, 140));
    yellow.paper = Some(Rgb::new(255, 200, 0));
    let normal = Palette::PAPER.paint(&yellow).color;
    assert_eq!(normal, egui::Color32::from_rgb(255, 200, 0), "AA in Paper");
    let high = Palette::PAPER_HIGH_CONTRAST.paint(&yellow).color;
    assert_ne!(high, normal);
    for (_, ink) in Palette::PAPER_HIGH_CONTRAST.inks() {
        assert!(contrast(ink, high) >= AAA_TEXT_CONTRAST);
    }
    assert_eq!(StyleTone::ALL.len(), HIGH_CONTRAST_PAGES.len());
}

/// The rectangles stroked last frame in `color`, with their stroke widths.
fn strokes(h: &Harness, color: egui::Color32) -> Vec<(Rect, f32)> {
    fn walk(shape: &Shape, f: &mut impl FnMut(&Shape)) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, f)),
            s => f(s),
        }
    }
    let mut out = Vec::new();
    for c in &h.last_shapes {
        walk(&c.shape, &mut |s| {
            if let Shape::Rect(r) = s
                && r.stroke.color == color
                && r.stroke.width > 0.0
            {
                out.push((r.rect, r.stroke.width));
            }
        });
    }
    out
}

/// The horizontal lines drawn last frame in `color`.
fn hlines(h: &Harness, color: egui::Color32) -> Vec<Rect> {
    let mut out = Vec::new();
    for c in &h.last_shapes {
        if let Shape::LineSegment { points, stroke } = &c.shape
            && stroke.color == color
            && points[0].y == points[1].y
        {
            out.push(Rect::from_two_pos(points[0], points[1]));
        }
    }
    out
}

/// High contrast light and dark from the View menu's Theme: the app draws in the High
/// contrast chrome and pages at once, Page tone Auto follows its ground, egui's controls get
/// borders that read at 3:1, the keyboard's focus ring is heavy and links on Home are
/// underlined at rest. Light again draws as before.
#[test]
fn the_high_contrast_themes_draw_aaa_chrome_heavy_focus_rings_and_underlined_links() {
    let mut h = Harness::new();
    h.ctx.enable_accesskit();
    h.shell.run("home.open", &json!({}));
    h.frames(3);
    let link = h.find("Open the Library").expect("Home's link");
    let light_lapis = crate::theme::LIGHT.lapis;
    assert!(
        !hlines(&h, light_lapis)
            .iter()
            .any(|l| (l.center().x - link.center().x).abs() < link.width()),
        "Light underlines links only when hovered"
    );
    for (id, chrome, page) in [
        (
            "high_contrast_light",
            HIGH_CONTRAST_LIGHT,
            Palette::PAPER_HIGH_CONTRAST,
        ),
        (
            "high_contrast_dark",
            HIGH_CONTRAST_DARK,
            Palette::DARK_HIGH_CONTRAST,
        ),
    ] {
        h.shell.run("view.theme", &json!({ "value": id }));
        h.frames(2);
        assert_eq!(crate::theme::c(), chrome, "{id}");
        assert_eq!(h.shell.st.palette, page, "{id}: Page tone Auto");
        let style = h.ctx.global_style();
        let border = style.visuals.widgets.inactive.bg_stroke;
        assert_eq!(border.color, chrome.rule, "{id}");
        assert_eq!(border.width, stroke::MEDIUM, "{id}");
        assert!(contrast(chrome.rule, chrome.shell) >= MARK_CONTRAST);

        // Links show as links without their colour.
        let link = h.find("Open the Library").expect("Home's link");
        assert!(
            hlines(&h, chrome.lapis)
                .iter()
                .any(|l| l.left() <= link.center().x
                    && l.right() >= link.center().x
                    && (l.top() - link.bottom()).abs() <= stroke::THICK),
            "{id}: Open the Library is not underlined"
        );

        // A Sepia page the user chose stays Sepia, in High contrast.
        h.shell.run("view.page_tone", &json!({ "value": "sepia" }));
        h.frame();
        assert_eq!(h.shell.st.palette, Palette::SEPIA_HIGH_CONTRAST, "{id}");
        h.shell.run("view.page_tone", &json!({ "value": "auto" }));
        h.frame();
    }

    // The focus ring, reached with the keyboard.
    super::a11y_audit::key(&mut h, Key::Tab, Modifiers::NONE);
    super::a11y_audit::key(&mut h, Key::Tab, Modifiers::NONE);
    assert!(super::a11y_audit::focused(&h).is_some());
    super::a11y_audit::assert_focus_ring(&h);
    let rings = strokes(&h, HIGH_CONTRAST_DARK.lapis);
    assert!(
        rings.iter().any(|(_, w)| *w == stroke::HEAVY),
        "no heavy focus ring: {rings:?}"
    );
    h.shell.run("view.theme", &json!({ "value": "light" }));
    h.frames(2);
    assert_eq!(crate::theme::c(), crate::theme::LIGHT);
    assert_eq!(h.shell.st.palette, Palette::PAPER);
    assert_eq!(crate::theme::focus_ring_width(), stroke::THICK);
}

/// A selected selectable button (a chosen choice, a selected row) shows in High contrast by
/// its frame in the Borders colour, not by its fill alone; an unselected one has none.
#[test]
fn a_selected_choice_shows_a_border_in_high_contrast() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    crate::theme::use_chromes(&ctx, HIGH_CONTRAST_LIGHT, HIGH_CONTRAST_DARK);
    ctx.set_theme(egui::ThemePreference::Light);
    let mut rects = (Rect::NOTHING, Rect::NOTHING);
    let mut shapes = Vec::new();
    for _ in 0..3 {
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let on = ui.add(egui::Button::selectable(true, "Chosen"));
                let off = ui.add(egui::Button::selectable(false, "Not chosen"));
                rects = (on.rect, off.rect);
            });
        });
        out.textures_delta.clear();
        shapes = out.shapes;
    }
    let mut framed = Vec::new();
    for c in &shapes {
        if let Shape::Rect(r) = &c.shape
            && r.stroke.color == HIGH_CONTRAST_LIGHT.rule
            && r.stroke.width >= stroke::MEDIUM
        {
            framed.push(r.rect);
        }
    }
    let around = |b: Rect| {
        framed
            .iter()
            .any(|f| f.expand(stroke::THICK).contains_rect(b))
    };
    assert!(around(rects.0), "the chosen one is framed: {framed:?}");
    assert!(!around(rects.1), "the other is not: {framed:?}");
}
