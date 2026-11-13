//! Check boxes and radio buttons show their edge (#306): an unchecked box or circle is
//! filled with the Field colour, so on a Field-coloured dialog or page only its outline shows
//! it is there. The shared Toggle and Radio components draw that outline in Muted, which
//! reads at more than the 3:1 a control's edge needs on every background of every built-in
//! chrome, and the check mark as thick as the focus ring.

use egui::{RawInput, Shape};

use super::*;
use crate::palette::{MARK_CONTRAST, contrast};
use crate::theme::{Chrome, DARK, HIGH_CONTRAST_DARK, HIGH_CONTRAST_LIGHT, LIGHT};
use crate::tokens::stroke;

/// The outlines (stroke colour and width) of last frame's check boxes and radio buttons:
/// square rectangles and circles `side` across, with a stroke.
fn control_edges(shapes: &[egui::epaint::ClippedShape], side: f32) -> Vec<(egui::Color32, f32)> {
    fn walk(shape: &Shape, f: &mut impl FnMut(&Shape)) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, f)),
            s => f(s),
        }
    }
    let near = |a: f32| (a - side).abs() <= stroke::THICK * 2.0;
    let mut out = Vec::new();
    for c in shapes {
        walk(&c.shape, &mut |s| match s {
            Shape::Rect(r) if near(r.rect.width()) && near(r.rect.height()) => {
                out.push((r.stroke.color, r.stroke.width));
            }
            Shape::Circle(c) if near(c.radius * 2.0) => {
                out.push((c.stroke.color, c.stroke.width));
            }
            _ => {}
        });
    }
    out
}

/// The check marks of last frame: the three-point lines.
fn check_marks(shapes: &[egui::epaint::ClippedShape]) -> Vec<egui::Stroke> {
    let mut out = Vec::new();
    for c in shapes {
        if let Shape::Path(p) = &c.shape
            && p.points.len() == 3
            && !p.closed
            && let egui::epaint::ColorMode::Solid(color) = p.stroke.color
        {
            out.push(egui::Stroke::new(p.stroke.width, color));
        }
    }
    out
}

/// One frame of an unchecked and a checked Toggle and an unselected Radio on a page of the
/// Field colour, in `chrome`; its shapes and the box's side.
fn draw_controls(chrome: Chrome) -> (Vec<egui::epaint::ClippedShape>, f32) {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    crate::theme::use_chromes(&ctx, chrome, chrome);
    ctx.set_theme(if chrome.dark {
        egui::ThemePreference::Dark
    } else {
        egui::ThemePreference::Light
    });
    let mut shapes = Vec::new();
    let mut side = 0.0;
    for _ in 0..3 {
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            crate::theme::set_for(ui.ctx());
            egui::CentralPanel::default()
                .frame(crate::components::page_frame())
                .show(ui, |ui| {
                    side = ui.spacing().icon_width;
                    crate::components::toggle(ui, &mut false, "Open Home at launch");
                    crate::components::toggle(ui, &mut true, "Auto-hide");
                    crate::components::radio(ui, false, "Icons only");
                });
        });
        out.textures_delta.clear();
        shapes = out.shapes;
    }
    (shapes, side)
}

/// Acceptance (#306): in every built-in chrome, an unchecked check box and radio button are
/// outlined at 3:1 or more against their Field-coloured fill and against every background
/// they sit on (dialogs, cards, the shell), and a check mark is drawn in Ink, as thick as the
/// focus ring. Dark failed before: its Rule border reads at 1.4:1 on the Field.
#[test]
fn an_unchecked_check_box_and_radio_button_show_their_edge_in_every_built_in_theme() {
    assert!(
        contrast(DARK.rule, DARK.field) < MARK_CONTRAST,
        "the Rule border the boxes had in Dark"
    );
    for (name, chrome) in [
        ("light", LIGHT),
        ("dark", DARK),
        ("high contrast light", HIGH_CONTRAST_LIGHT),
        ("high contrast dark", HIGH_CONTRAST_DARK),
    ] {
        let (shapes, side) = draw_controls(chrome);
        let edges = control_edges(&shapes, side);
        assert_eq!(edges.len(), 3, "{name}: two boxes and a circle: {edges:?}");
        for (color, width) in edges {
            assert_eq!(color, chrome.muted, "{name}");
            assert!(width >= stroke::HAIR, "{name}");
            for bg in [chrome.field, chrome.surface, chrome.shell] {
                let ratio = contrast(color, bg);
                assert!(ratio >= MARK_CONTRAST, "{name}: edge at {ratio:.2}:1");
            }
        }
        let marks = check_marks(&shapes);
        assert_eq!(marks.len(), 1, "{name}: the checked box's mark");
        assert_eq!(marks[0].color, chrome.ink, "{name}");
        assert!(marks[0].width >= stroke::THICK, "{name}");
    }
}

/// Acceptance (#306), on the real surfaces: Home's unchecked "Open Home at launch" and the
/// Customize toolbar dialog's check boxes and radio buttons are outlined in Muted in Dark.
#[test]
fn home_and_customize_toolbar_outline_their_check_boxes_in_dark() {
    let mut h = Harness::new();
    h.shell.run("view.theme", &json!({ "value": "dark" }));
    h.shell.run("home.open", &json!({}));
    h.frames(3);
    assert_eq!(crate::theme::c(), DARK);
    let side = h.ctx.global_style().spacing.icon_width;
    let label = h.at("Open Home at launch");
    let edges: Vec<_> = control_edges(&h.last_shapes, side);
    assert!(
        edges.contains(&(DARK.muted, stroke::HAIR)),
        "Home's check box beside {label:?}: {edges:?}"
    );
    h.shell.run("view.toolbar_customize", &json!({}));
    h.frames(3);
    assert!(
        h.find("Auto-hide: show the toolbar when the pointer reaches its edge or Alt is held")
            .is_some()
    );
    let edges: Vec<_> = control_edges(&h.last_shapes, side);
    let outlined = edges.iter().filter(|e| e.0 == DARK.muted).count();
    // Each Launcher's box, the Buttons show and Position choices, and Auto-hide.
    assert!(outlined >= 8, "{edges:?}");
    assert!(
        !edges.iter().any(|e| e.0 == DARK.rule),
        "no control keeps the Rule edge: {edges:?}"
    );
}
