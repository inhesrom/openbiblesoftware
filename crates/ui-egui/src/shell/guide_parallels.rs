//! The Passage Guide's Parallel passages section (#212): the other accounts of the Passage,
//! grouped by the part of it they are parallel to, each with the heading that pairs them and
//! a one-line preview from the ★ Bible. Clicking one opens Text Comparison aligned on it
//! (`compare.parallel`); the Guide stays where it is.

use egui::{RichText, Ui};
use obs_engine::ParallelPassages;
use obs_scripture::{Passage, RefStyle, Standard, format_passage};

use super::State;
use crate::theme;

/// What the hint under each link says.
const COMPARE_HINT: &str =
    "Click to compare it in Text Comparison · Ctrl/Cmd-click opens a new one";

impl State {
    /// The section's rows; returns the parallel clicked this frame (to compare).
    pub(super) fn parallel_passages_section(
        &mut self,
        ui: &mut Ui,
        found: &ParallelPassages,
        shown: Passage,
    ) -> Option<Passage> {
        let mut clicked = None;
        let short = |p: Passage| format_passage(p, &Standard, RefStyle::Short);
        // With several parts, a line naming every parallel in all comes first: 2 Samuel 24 is
        // parallel to "1 Chr 21" across four sections.
        if found.groups.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.label(RichText::new("In all:").strong().color(theme::c().ink));
                for (i, p) in found.all.iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new("·").color(theme::c().muted));
                    }
                    let r = super::unbroken_link(ui, short(*p)).on_hover_text(COMPARE_HINT);
                    if r.clicked() {
                        clicked = Some(*p);
                    }
                    if r.hovered() {
                        self.reference_popup(ui, &[vec![*p].into()]);
                    }
                }
            });
            ui.add_space(6.0);
        }
        for group in &found.groups {
            // The part of the Guide's Passage the rows are parallel to, when it is not the
            // whole of it.
            if group.passage != shown {
                ui.label(
                    RichText::new(format!("Beside {}", short(group.passage)))
                        .small()
                        .color(theme::c().muted),
                );
            }
            for row in &group.rows {
                let reference = short(row.passage);
                let r = ui
                    .horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let r = super::unbroken_link(ui, &reference).on_hover_text(format!(
                            "{} · {}\n{COMPARE_HINT}",
                            row.title, row.source_title
                        ));
                        ui.label(RichText::new(&row.title).color(theme::c().muted));
                        r
                    })
                    .inner;
                if r.clicked() {
                    clicked = Some(row.passage);
                }
                if r.hovered() {
                    self.reference_popup(ui, &[vec![row.passage].into()]);
                }
                self.preview(ui, Passage::verse(row.passage.start()), true);
                ui.add_space(4.0);
            }
            ui.separator();
        }
        clicked
    }
}
