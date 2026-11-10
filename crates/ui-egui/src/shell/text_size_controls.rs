//! A Resource's own Text size (#292): the row in a Tab's ⋯ menu and the Aa button in its
//! header, which opens the same row on its own. Both change the Tab's Text size Resource
//! (`Session::text_size_resource`): a Resource Tab's Resource, or in a Text Comparison or
//! Search Tab the one last clicked into. Every change runs a command at once, so the text is
//! laid out again at the new size while the slider is dragged and the menu stays open.

use egui::{RichText, Ui};
use obs_engine::appearance::{TEXT_SIZE_MAX, TEXT_SIZE_MIN, TEXT_SIZE_STEP};
use obs_engine::TabId;
use serde_json::{Value, json};

use super::State;
use crate::theme;
use crate::tokens::{alpha, size, space};

/// What the Text size row calls its controls, for screen readers and tests.
pub(crate) fn slider_name(resource: &str, size: f32) -> String {
    format!("Text size for {resource}, {size:.0} points")
}

impl State {
    /// The Text size row for Tab `tab`'s Text size Resource, if it has one: what it is for
    /// and whether it has its own size, then A−, the slider, A+, the size and Reset.
    /// Returns whether a row was drawn.
    pub(super) fn text_size_row(&mut self, ui: &mut Ui, tab: TabId) -> bool {
        let Some(resource) = self.session.text_size_resource(tab) else {
            return false;
        };
        let name = self.session.text_size_name(&resource);
        let size = self.session.text_size_of(&resource);
        let own = self.session.resource_text_sizes().get(&resource).is_some();
        let mut run: Option<(&'static str, Value)> = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Text size · {name}")).color(theme::c().ink));
            let note = if own { "own size" } else { "follows Text size" };
            let tip = if own {
                format!("{name} has its own Text size; Reset makes it follow the Text size")
            } else {
                format!(
                    "{name} follows the Text size ({:.0} pt); a change here gives it its own",
                    self.session.text.size
                )
            };
            let note = if own {
                RichText::new(note).color(theme::c().lapis)
            } else {
                crate::components::hint_text(note)
            };
            ui.label(note.size(crate::tokens::text::SMALL))
                .on_hover_text(tip);
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::XS;
            // In a menu, buttons have no border or padding and the slider's rail is the menu's
            // colour: A− and A+ get theirs back, and the rail is muted ink, filled in lapis up
            // to the handle, in every theme.
            let global = ui.ctx().global_style();
            ui.spacing_mut().button_padding = global.spacing.button_padding;
            let widgets = &mut ui.visuals_mut().widgets;
            widgets.inactive.bg_stroke = global.visuals.widgets.inactive.bg_stroke;
            widgets.inactive.bg_fill = theme::c().muted.gamma_multiply(alpha::HALF);
            ui.visuals_mut().slider_trailing_fill = true;
            ui.visuals_mut().selection.bg_fill = theme::c().lapis;
            let smaller = format!("Smaller text for {name}");
            if ui
                .add_enabled(size > TEXT_SIZE_MIN, crate::components::button("A−"))
                .accessible_name(&smaller)
                .on_hover_text(super::with_key(
                    &self.session,
                    &smaller,
                    "view.resource_text_smaller",
                ))
                .clicked()
            {
                run = Some(("view.resource_text_smaller", json!({ "tab": tab.0 })));
            }
            let mut value = size;
            ui.spacing_mut().slider_width = size::FIELD_NARROW;
            let slider = ui
                .add(
                    egui::Slider::new(&mut value, TEXT_SIZE_MIN..=TEXT_SIZE_MAX)
                        .step_by(f64::from(TEXT_SIZE_STEP))
                        .show_value(false),
                )
                .accessible_name(slider_name(&name, size))
                .on_hover_text(format!("Text size for {name}"));
            if slider.changed() && value.round() != size {
                run = Some((
                    "view.resource_text_size",
                    json!({ "tab": tab.0, "value": value.round() as i64 }),
                ));
            }
            let larger = format!("Larger text for {name}");
            if ui
                .add_enabled(size < TEXT_SIZE_MAX, crate::components::button("A+"))
                .accessible_name(&larger)
                .on_hover_text(super::with_key(
                    &self.session,
                    &larger,
                    "view.resource_text_larger",
                ))
                .clicked()
            {
                run = Some(("view.resource_text_larger", json!({ "tab": tab.0 })));
            }
            ui.label(RichText::new(format!("{size:.0} pt")).color(theme::c().ink));
            let reset = crate::components::link_enabled(ui, own, "Reset")
                .accessible_name(format!("Reset the Text size of {name}"))
                .on_hover_text(super::with_key(
                    &self.session,
                    &format!("{name} follows the Text size again"),
                    "view.resource_text_reset",
                ))
                .on_disabled_hover_text(format!("{name} follows the Text size"));
            if reset.clicked() {
                run = Some(("view.resource_text_reset", json!({ "tab": tab.0 })));
            }
        });
        if let Some((id, params)) = run {
            // The commands act on the focused Tab's Resource when the keys run them.
            self.session.workspace.user_focused(tab);
            self.run(id, &params);
        }
        true
    }

    /// The Aa button of Tab `tab`'s header, when it has a Text size Resource: one click opens
    /// the Text size row on its own, which stays open while it is used.
    pub(super) fn text_size_button(&mut self, ui: &mut Ui, tab: TabId) {
        let Some(resource) = self.session.text_size_resource(tab) else {
            return;
        };
        let name = self.session.text_size_name(&resource);
        let size = self.session.text_size_of(&resource);
        let aa = RichText::new("Aa").color(theme::c().muted);
        crate::components::menu_staying_open(ui, aa, |ui| {
            ui.set_min_width(size::FIELD_WIDE);
            self.text_size_row(ui, tab);
        })
        .response
        .accessible_name(format!("Text size for {name}"))
            .on_hover_text(format!("Text size for {name}: {size:.0} pt"));
    }
}
