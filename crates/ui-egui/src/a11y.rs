//! Resource text for screen readers (#68, ADR 0004 amendment). Our pipeline paints glyph
//! quads, which assistive technology cannot read, so the reading pane also publishes an
//! AccessKit tree for what is in view:
//!
//! ```text
//! Document (the pane, labelled with the Resource) — carries the text selection
//! └─ Paragraph (one per visible block)
//!    ├─ TextRun (one per laid line: its text, character lengths, bounds)
//!    └─ Link (Tagged words and Scripture references: their words, bounds, description)
//! ```
//!
//! Only visible blocks are published, so the tree stays small however long the Resource is.
//! Nothing is built unless an assistive technology has turned AccessKit on.

use std::ops::Range;
use std::sync::Arc;

use egui::accesskit::{self, Role};
use egui::{Id, Rect, Sense, Ui, UiBuilder, pos2};
use obs_layout::LaidBlock;
use obs_model::SpanKind;
use obs_scripture::{RefStyle, Standard, format_passage_list};

use crate::selection::DocPos;
use crate::view::ReadingView;

/// A link inside a paragraph, in paragraph-text bytes.
struct Link {
    bytes: Range<usize>,
    label: String,
    description: String,
}

fn to_ak(r: Rect) -> accesskit::Rect {
    accesskit::Rect {
        x0: r.min.x.into(),
        y0: r.min.y.into(),
        x1: r.max.x.into(),
        y1: r.max.y.into(),
    }
}

/// The links of block `index`: Tagged words, cross-references and Scripture references.
fn links(view: &ReadingView, index: usize, laid: &LaidBlock) -> Vec<Link> {
    let doc = &view.doc;
    let block = &doc.blocks[index];
    let unit = &doc.units[block.unit];
    let range = &block.source.range;
    let mut out = Vec::new();
    for s in &unit.spans {
        let (start, end) = (s.start as usize, s.end as usize);
        if end <= range.start || start >= range.end || start >= end {
            continue;
        }
        let (label, description) = match &s.kind {
            SpanKind::TaggedWord { parts } => {
                let numbers: Vec<String> = parts.iter().map(|p| p.strongs.to_string()).collect();
                let word = unit.text.get(start..end).unwrap_or_default().to_string();
                (word, format!("Strong's {}", numbers.join(", ")))
            }
            SpanKind::ScriptureRef { targets } | SpanKind::Xref { targets } => {
                let shown = unit
                    .text
                    .get(start..end)
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let refs = format_passage_list(targets, &Standard, RefStyle::Long);
                let label = if shown.is_empty() {
                    refs.clone()
                } else {
                    shown
                };
                (label, format!("Go to {refs}"))
            }
            _ => continue,
        };
        let (Some(a), Some(b)) = (
            laid.from_unit(start.max(range.start)),
            laid.from_unit(end.min(range.end)).or(Some(laid.text.len())),
        ) else {
            continue;
        };
        if a < b {
            out.push(Link {
                bytes: a..b,
                label,
                description,
            });
        }
    }
    out.sort_by_key(|l| l.bytes.start);
    out
}

/// Where a document position falls among the published lines: (run id, character index).
fn text_position(
    view: &ReadingView,
    shown: &[(usize, Arc<LaidBlock>, Vec<Id>)],
    p: DocPos,
    at_start: bool,
) -> Option<accesskit::TextPosition> {
    let (first, last) = (shown.first()?, shown.last()?);
    let block = view.doc.block_at(p.unit, p.byte);
    let run_end = |laid: &LaidBlock, runs: &[Id]| {
        let line = laid.lines.last()?;
        let n = laid.text.get(line.bytes.clone())?.chars().count();
        Some(accesskit::TextPosition {
            node: runs.last()?.accesskit_id(),
            character_index: n,
        })
    };
    let run_start = |runs: &[Id]| {
        Some(accesskit::TextPosition {
            node: runs.first()?.accesskit_id(),
            character_index: 0,
        })
    };
    // Outside the published blocks: clamp to the nearest end of what is published.
    match block {
        Some(b) if b < first.0 => return run_start(&first.2),
        Some(b) if b > last.0 => return run_end(&last.1, &last.2),
        None if at_start => return run_start(&first.2),
        None => return run_end(&last.1, &last.2),
        _ => {}
    }
    let (_, laid, runs) = shown.iter().find(|(i, _, _)| Some(*i) == block)?;
    let para = laid.from_unit(p.byte).unwrap_or(laid.text.len());
    let li = laid
        .lines
        .iter()
        .position(|l| para < l.bytes.end)
        .unwrap_or(laid.lines.len().saturating_sub(1));
    let line = laid.lines.get(li)?;
    let upto = para.clamp(line.bytes.start, line.bytes.end);
    let index = laid.text.get(line.bytes.start..upto)?.chars().count();
    Some(accesskit::TextPosition {
        node: runs.get(li)?.accesskit_id(),
        character_index: index,
    })
}

/// Publish the pane's AccessKit nodes. `pane` is the pane's response id; `origin` is where
/// the text's top-left corner is drawn; `height` the viewport height.
pub(crate) fn publish(ui: &mut Ui, pane: Id, view: &ReadingView, origin: [f32; 2], height: f32) {
    let ctx = ui.ctx().clone();
    let title = view.doc.manifest.title.clone();
    let on = ctx
        .accesskit_node_builder(pane, |n| {
            n.set_role(Role::Document);
            n.set_label(title);
        })
        .is_some();
    if !on {
        return;
    }
    let [ox, oy] = origin;
    let mut shown: Vec<(usize, Arc<LaidBlock>, Vec<Id>)> = Vec::new();
    for (index, top, laid) in view.laid_in_view(height) {
        let rect = Rect::from_min_size(pos2(ox, oy + top), egui::vec2(laid.width, laid.height));
        let para = ui.new_child(
            UiBuilder::new()
                .id_salt(("a11y-paragraph", index))
                .max_rect(rect)
                .sense(Sense::hover())
                .accessibility_parent(pane),
        );
        let para_id = para.unique_id();
        ctx.accesskit_node_builder(para_id, |n| {
            n.set_role(Role::Paragraph);
            n.set_bounds(to_ak(rect));
        });
        // One TextRun per line, in logical order.
        let mut runs = Vec::with_capacity(laid.lines.len());
        for (li, line) in laid.lines.iter().enumerate() {
            let text = laid.text.get(line.bytes.clone()).unwrap_or_default();
            let lr = Rect::from_min_max(
                pos2(ox, oy + top + line.top),
                pos2(ox + laid.width, oy + top + line.bottom),
            );
            let run = para_id.with(("run", li));
            para.interact(lr, run, Sense::hover());
            ctx.accesskit_node_builder(run, |n| {
                n.set_role(Role::TextRun);
                n.set_value(text.to_string());
                let lengths: Vec<u8> = text.chars().map(|c| c.len_utf8() as u8).collect();
                n.set_character_lengths(lengths);
                n.set_text_direction(if laid.rtl {
                    accesskit::TextDirection::RightToLeft
                } else {
                    accesskit::TextDirection::LeftToRight
                });
                n.set_bounds(to_ak(lr));
            });
            runs.push(run);
        }
        // Links, with bounds around their clusters.
        let links = links(view, index, &laid);
        let mut bounds: Vec<Option<Rect>> = vec![None; links.len()];
        for line in &laid.lines {
            for item in &line.items {
                for (bytes, x0, x1) in item.clusters() {
                    let i = links.partition_point(|l| l.bytes.start <= bytes.start);
                    let Some(k) = i.checked_sub(1) else { continue };
                    if bytes.start >= links[k].bytes.end {
                        continue;
                    }
                    let r = Rect::from_min_max(
                        pos2(ox + x0, oy + top + line.top),
                        pos2(ox + x1, oy + top + line.bottom),
                    );
                    bounds[k] = Some(bounds[k].map_or(r, |b| b.union(r)));
                }
            }
        }
        for (k, link) in links.iter().enumerate() {
            let Some(r) = bounds[k] else { continue };
            let id = para_id.with(("link", k));
            para.interact(r, id, Sense::hover());
            ctx.accesskit_node_builder(id, |n| {
                n.set_role(Role::Link);
                n.set_label(link.label.clone());
                n.set_description(link.description.clone());
                n.set_bounds(to_ak(r));
            });
        }
        shown.push((index, laid, runs));
    }
    // The selection, as a text selection on the Document.
    let selection = view.selection.and_then(|sel| {
        let (a, b) = sel.range(&view.doc);
        let (a, b) = if sel.anchor <= sel.head {
            (a, b)
        } else {
            (b, a)
        };
        let forward = sel.anchor <= sel.head;
        let anchor = text_position(view, &shown, a, forward)?;
        let focus = text_position(view, &shown, b, !forward)?;
        Some(accesskit::TextSelection { anchor, focus })
    });
    ctx.accesskit_node_builder(pane, |n| match selection {
        Some(s) => n.set_text_selection(s),
        None => n.clear_text_selection(),
    });
}
