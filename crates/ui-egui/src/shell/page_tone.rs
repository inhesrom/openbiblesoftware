//! Page tone and the theme (#240). With Page tone Auto the pages follow the theme on the next
//! frame (the palette is resolved each frame in `apply_appearance`). A light tone the user
//! chose (Paper or Sepia) is kept in a dark theme, so the first time the theme turns dark
//! with one, a hint offers Auto; Settings says which tone the pages are in now.

use egui::{RichText, vec2};
use obs_engine::PageTone;
use serde_json::json;

use super::State;
use crate::theme;

/// The Page tone hint, while it is open: the light tone the user chose.
#[derive(Debug, Default)]
pub(crate) struct PageToneHint {
    open: Option<PageTone>,
    /// Whether the chrome was dark last frame (`None` before the first frame).
    was_dark: Option<bool>,
}

/// The hint's question.
pub(crate) fn hint_text(tone: PageTone) -> String {
    format!(
        "Page tone is set to {}. Use Auto to follow the theme?",
        tone.label()
    )
}

/// Settings' line under the Page tone choices: the tone the pages are in now.
pub(crate) fn effective_text(tone: PageTone, dark: bool) -> String {
    if tone == PageTone::Auto {
        format!(
            "Showing {} pages now: Auto follows the theme.",
            tone.resolve(dark).label()
        )
    } else {
        format!(
            "Showing {} pages in every theme. Choose Auto to follow the theme.",
            tone.label()
        )
    }
}

impl State {
    /// After this frame's theme is known: open the hint when the chrome has just turned dark
    /// over a light Page tone the user chose (the Session says whether it is still due), and
    /// close it once the Page tone is no longer that tone.
    pub(super) fn follow_page_tone_hint(&mut self, dark: bool) {
        let tone = self.session.appearance.page_tone;
        let turned_dark = self.page_tone_hint.was_dark == Some(false) && dark;
        self.page_tone_hint.was_dark = Some(dark);
        if turned_dark && self.session.page_tone_hint_due() {
            self.page_tone_hint.open = Some(tone);
        }
        if self.page_tone_hint.open.is_some_and(|t| t != tone) {
            self.page_tone_hint.open = None;
        }
    }

    /// The open hint, over the bottom of the window until answered: Use Auto runs View →
    /// Page tone → Auto; Keep closes it.
    pub(super) fn page_tone_hint_ui(&mut self, ctx: &egui::Context) {
        let Some(tone) = self.page_tone_hint.open else {
            return;
        };
        let mut use_auto = None;
        egui::Area::new(egui::Id::unique("page-tone-hint"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_BOTTOM, vec2(0.0, -96.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(hint_text(tone)).color(theme::c().ink));
                        if ui.button("Use Auto").clicked() {
                            use_auto = Some(true);
                        }
                        if ui.button(format!("Keep {}", tone.label())).clicked() {
                            use_auto = Some(false);
                        }
                    });
                });
            });
        match use_auto {
            Some(true) => {
                self.page_tone_hint.open = None;
                self.run("view.page_tone", &json!({ "value": "auto" }));
            }
            Some(false) => self.page_tone_hint.open = None,
            None => {}
        }
    }
}
