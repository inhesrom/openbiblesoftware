//! "None in your library" with a way out (#135): an empty Guide section, the Cross-references
//! Tool and the Word Study chooser draw their message and, from the engine's
//! [`MissingResource`], the buttons that get the missing Resource — Get Resources at the
//! right filter, the Starter library when it has one, and Import a file… as a secondary link.

use egui::{RichText, Ui};
use obs_engine::{MissingResource, StarterOffer};
use serde_json::{Value, json};

use super::State;
use crate::theme;

/// The Starter library button's label.
pub(super) const ADD_STARTER: &str = "Add the Starter library…";
/// The secondary Import link's label.
pub(super) const IMPORT_FILE: &str = "Import a file…";

impl State {
    /// Draw `message` and the buttons that would fill it. Returns the command a click asks
    /// for; the caller runs it after drawing.
    pub(super) fn missing_resource_ui(
        &self,
        ui: &mut Ui,
        message: &str,
        missing: &MissingResource,
    ) -> Option<(&'static str, Value)> {
        ui.label(RichText::new(message).color(theme::c().muted));
        let mut action = None;
        ui.horizontal_wrapped(|ui| {
            if let (Some(label), Some(id)) = (
                missing.get_label(),
                missing.get_resources.and_then(|f| f.id()),
            ) && ui
                .button(label)
                .on_hover_text("Open Get Resources: download one in a click")
                .clicked()
            {
                action = Some(("library.get_resources", json!({ "type": id })));
            }
            if missing.offers_starter()
                && ui
                    .button(ADD_STARTER)
                    .on_hover_text(self.starter_hover(&missing.starter))
                    .clicked()
            {
                action = Some(("library.starter", json!({})));
            }
            if missing.import && self.session.is_enabled("library.import") {
                // On a row of its own when it does not fit: a link wrapped across rows is two
                // widgets, and screen readers get no name for the second (#182).
                let font = egui::TextStyle::Small.resolve(ui.style());
                let width = (ui.painter())
                    .layout_no_wrap(IMPORT_FILE.into(), font, egui::Color32::PLACEHOLDER)
                    .size()
                    .x
                    + ui.spacing().item_spacing.x;
                if ui.available_size_before_wrap().x < width {
                    ui.end_row();
                }
                if ui
                    .link(RichText::new(IMPORT_FILE).small())
                    .on_hover_text("Import a USFM or SWORD file you already have")
                    .clicked()
                {
                    action = Some(("library.import", json!({})));
                }
            }
        });
        action
    }

    /// What the Starter library button would add, by abbreviation.
    fn starter_hover(&self, offer: &StarterOffer) -> String {
        let StarterOffer::Resources(ids) = offer else {
            return "Open the Starter library: the open Resources made for this app".into();
        };
        let manifest = self.session.starter.manifest();
        let names: Vec<&str> = ids
            .iter()
            .map(|id| {
                manifest
                    .and_then(|m| m.get(id))
                    .map(|i| i.abbreviation.as_str())
                    .filter(|a| !a.is_empty())
                    .unwrap_or(id)
            })
            .collect();
        format!(
            "Open the Starter library, which has {} for this",
            names.join(", ")
        )
    }
}
