//! Scripture references in a reading pane (#381): what a reference span points at in the
//! Standard versification, and the preview shown while the pointer is on it (the egui Shell's
//! `references.rs`, `reference_popup`). The preview is read from the ★ Bible
//! ([`obs_engine::cross_references::preferred_preview_cached`]) once per reference hovered and
//! laid out by our text pipeline ([`crate::reading::SnippetElement`]), as ADR 0004 asks of
//! hover popups.

use gpui_kit::{Pixels, Point};
use obs_engine::TabId;
use obs_engine::cross_references::{self, BiblePreview, BiblePreviewCache};
use obs_model::SpanKind;
use obs_reading::ReadingDoc;
use obs_scripture::{Passage, PassageList, RefStyle, Standard, Verse, format_passage};

/// The line under a preview that leaves text out.
pub const PREVIEW_SHORTENED: &str = "Preview shortened — click the reference to read";

/// The most written targets a preview shows.
const PREVIEW_TARGETS: usize = 3;

/// A reference span's written targets in the Standard versification (the egui
/// `standard_targets`): a Scripture reference in a Bible's text is read in the Bible's own
/// numbering and goes through its Verse map (ADR 0002); a Cross-reference already is
/// Standard.
pub fn reference_targets(doc: &ReadingDoc, unit: usize, span: usize) -> Vec<PassageList> {
    let Some(span) = doc.units.get(unit).and_then(|u| u.spans.get(span)) else {
        return Vec::new();
    };
    let (written, native) = match &span.kind {
        SpanKind::Xref { targets } => (targets, false),
        SpanKind::ScriptureRef { targets } => (targets, true),
        _ => return Vec::new(),
    };
    let map = if native { doc.verse_map.as_ref() } else { None };
    written
        .iter()
        .map(|p| {
            let Some(map) = map else {
                return vec![*p].into();
            };
            map.entries()
                .iter()
                .filter_map(|entry| {
                    let end = Verse::new(entry.verse.book, entry.verse.chapter, entry.last);
                    (entry.verse <= p.end() && p.start() <= end)
                        .then_some(entry.standard)
                        .flatten()
                })
                .collect::<PassageList>()
                .merged(&Standard)
        })
        .collect()
}

/// One target's part of a preview: its reference and the ★ Bible's text of it.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewPart {
    pub label: String,
    pub preview: Result<BiblePreview, String>,
}

/// The preview of the reference under the pointer in one Tab's pane.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferencePreview {
    pub tab: TabId,
    /// The reference: unit and span indexes.
    pub span: (usize, usize),
    /// Where the pointer is, in window points.
    pub at: Point<Pixels>,
    pub parts: Vec<PreviewPart>,
    /// More Passages than it shows, or text left out.
    pub shortened: bool,
    /// The reference has no Standard equivalent.
    pub unmapped: bool,
}

impl ReferencePreview {
    /// The preview of reference `span` of `doc`, read from the ★ Bible of `library`.
    pub fn read(
        tab: TabId,
        doc: &ReadingDoc,
        span: (usize, usize),
        at: Point<Pixels>,
        library: &obs_engine::Library,
        cache: &mut BiblePreviewCache,
    ) -> Self {
        let targets = reference_targets(doc, span.0, span.1);
        let mut parts = Vec::new();
        let mut shortened = targets.len() > PREVIEW_TARGETS;
        for passage in targets.iter().take(PREVIEW_TARGETS).flat_map(|t| t.iter()) {
            if parts.len() >= PREVIEW_TARGETS {
                shortened = true;
                break;
            }
            let preview = cross_references::preferred_preview_cached(library, *passage, cache);
            shortened |= preview.as_ref().is_ok_and(|p| p.truncated);
            parts.push(PreviewPart {
                label: format_passage(*passage, &Standard, RefStyle::Short),
                preview,
            });
        }
        let unmapped = targets.iter().all(|t| t.is_empty());
        Self {
            tab,
            span,
            at,
            parts,
            shortened,
            unmapped,
        }
    }
}

/// The first Verse a reference points at in the Standard versification, with its label.
pub fn first_target(doc: &ReadingDoc, unit: usize, span: usize) -> Option<(Passage, String)> {
    let targets = reference_targets(doc, unit, span);
    let passage = targets.iter().flat_map(|l| l.iter()).next().copied()?;
    Some((passage, format_passage(passage, &Standard, RefStyle::Short)))
}

impl crate::Shell {
    /// The preview of the reference under the pointer, beside it: each target's reference and
    /// the ★ Bible's text of it, laid out by our pipeline.
    pub(crate) fn reference_preview_popover(
        &self,
        look: &crate::theme::Look,
        cx: &gpui_kit::App,
    ) -> Option<gpui_kit::AnyElement> {
        use gpui_kit::base::TestSupportExt;
        use gpui_kit::prelude::*;
        use gpui_kit::{div, px};

        use crate::tokens::{self, size, space, text};
        let p = self.reference_preview.as_ref()?;
        let ch = look.chrome;
        let palette = crate::reading::palette_for(&self.session, look);
        let settings = self.session.text;
        let width = look.pt(size::POPOVER) - 2.0 * look.pt(space::M);
        let mut body = crate::components::popover_frame("Reference preview", look)
            .absolute()
            .left(p.at.x + px(look.pt(space::M)))
            .top(p.at.y + px(look.pt(space::L)))
            .w(px(look.pt(size::POPOVER)))
            .p(px(look.pt(space::M)))
            .bg(tokens::rgba(palette.page))
            .test_support();
        for part in &p.parts {
            body = body.child(
                div()
                    .font_weight(gpui_kit::FontWeight::BOLD)
                    .child(part.label.clone()),
            );
            body = match &part.preview {
                Ok(preview) => body.child(crate::reading::SnippetElement::new(
                    &format!("{} ({})", preview.text, preview.abbreviation),
                    preview.direction == obs_model::Direction::Rtl,
                    width,
                    &settings,
                    palette,
                    cx,
                )),
                Err(e) => body.child(div().text_color(tokens::rgba(ch.muted)).child(e.clone())),
            };
        }
        if p.unmapped {
            body = body.child("No Standard equivalent");
        }
        if p.shortened {
            body = body.child(
                div()
                    .text_size(px(look.pt(text::SMALL)))
                    .text_color(tokens::rgba(ch.muted))
                    .child(PREVIEW_SHORTENED),
            );
        }
        Some(body.into_any_element())
    }
}
