//! Annotations as a reading pane draws them (#62): each Highlight or Note whose Text range is
//! in the pane's Resource becomes a [`Mark`] over document positions. The painter resolves a
//! Mark's colour from the Page tone's palette each frame, so a theme switch never re-lays
//! anything out (#21 rule 4), and its rectangles come from the laid-out clusters, so Marks
//! follow scrolling and relayout (text size, width) exactly like the selection does.

use std::collections::HashMap;

use obs_engine::{Annotation, Content, HighlightStyle};
use obs_model::UnitId;
use obs_scripture::{Passage, Verse};

use crate::doc::ReadingDoc;
use crate::selection::DocPos;

/// What a Mark draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkKind {
    /// A background behind the glyphs, or the red underline over them.
    Highlight(HighlightStyle),
    /// A small marker at the end of the range; hovering it previews the Note.
    Note,
}

/// One Annotation drawn in one Resource, oldest first so the newest is drawn on top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    /// The Annotation's id.
    pub id: String,
    pub start: DocPos,
    pub end: DocPos,
    pub kind: MarkKind,
}

/// The Marks of `doc`'s Resource among `annotations`: every Annotation whose Text range (a
/// Text-range anchor, or a Passage anchor's origin) lies in it, in creation order. A range
/// whose text no longer reads as its quote is not drawn over other words: a Passage anchor
/// falls back to its whole Verse(s) (ADR 0006), and a bare Text range is left out until
/// re-anchoring (#65) moves or orphans it.
pub fn marks_for(doc: &ReadingDoc, annotations: &[Annotation]) -> Vec<Mark> {
    let units: HashMap<&UnitId, usize> = doc
        .units
        .iter()
        .enumerate()
        .map(|(i, u)| (&u.id, i))
        .collect();
    let mut marks: Vec<(i64, Mark)> = annotations
        .iter()
        .filter_map(|a| {
            let anchor = a.anchor.as_ref()?;
            let range = anchor.text_range()?;
            if range.resource_id != doc.manifest.resource_id {
                return None;
            }
            let (start, end) = (range.start as usize, range.end as usize);
            let exact = units.get(&range.unit).copied().filter(|&unit| {
                start < end && doc.units[unit].text.get(start..end) == Some(range.quote.as_str())
            });
            let (start, end) = match exact {
                Some(unit) => (DocPos { unit, byte: start }, DocPos { unit, byte: end }),
                None => passage_span(doc, &units, anchor.passage()?)?,
            };
            let kind = match a.content {
                Content::Highlight { style } => MarkKind::Highlight(style),
                Content::Note { .. } => MarkKind::Note,
            };
            Some((
                a.created_at,
                Mark {
                    id: a.id.clone(),
                    start,
                    end,
                    kind,
                },
            ))
        })
        .collect();
    // UUIDv7 ids sort by creation too; they break ties within one millisecond.
    marks.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
    marks.into_iter().map(|(_, m)| m).collect()
}

/// Where `doc` shows a Standard Passage: from the start of the first of its Bible Verses that
/// correspond to it to the end of the last, whole Verses.
fn passage_span(
    doc: &ReadingDoc,
    units: &HashMap<&UnitId, usize>,
    passage: Passage,
) -> Option<(DocPos, DocPos)> {
    let mut span: Option<(DocPos, DocPos)> = None;
    for v in &doc.verses {
        let standard = match &doc.verse_map {
            Some(map) => map.to_standard(v.verse),
            None => Passage::new(v.verse, Verse::new(v.verse.book, v.verse.chapter, v.last)).ok(),
        };
        let Some(standard) = standard else {
            continue;
        };
        if standard.end() < passage.start() || passage.end() < standard.start() {
            continue;
        }
        let Some(&unit) = units.get(&v.unit) else {
            continue;
        };
        let (a, b) = (
            DocPos {
                unit,
                byte: v.start as usize,
            },
            DocPos {
                unit,
                byte: v.end as usize,
            },
        );
        span = Some(match span {
            Some((s, e)) => (s.min(a), e.max(b)),
            None => (a, b),
        });
    }
    span.filter(|(a, b)| a < b)
}
