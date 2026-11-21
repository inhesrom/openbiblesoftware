//! Annotations as a reading pane draws them (#62, #63): each Highlight or Note that shows in
//! the pane's Resource becomes a [`Mark`] over document positions. The painter resolves a
//! Mark's colour from the Page tone's palette each frame, so a theme switch never re-lays
//! anything out (#21 rule 4), and its rectangles come from the laid-out clusters, so Marks
//! follow scrolling and relayout (text size, width) exactly like the selection does.
//!
//! Where an Annotation shows (#17 rules 14–17, ADR 0001, 0002):
//!
//! - **In the Resource of its Text range** (the origin Bible of a Passage anchor, or the one
//!   Resource of a Text-range anchor): exact to the word.
//! - **In every other Bible**, a Passage anchor is mapped from the Standard versification
//!   through that Bible's Verse map. A Highlight that covered its Verses whole (or has no
//!   Text range) highlights those Verses; one that covered part of a Verse shows only a
//!   margin mark beside the number of each Verse it touches, because word offsets differ
//!   between translations. A Note shows one marker, beside its first Verse's number.
//! - Text-range anchors (non-Bible Resources, Unmapped verses) show only in their Resource.

use std::collections::HashMap;

use obs_engine::{Annotation, Content, HighlightStyle, StyleAppearance};
use obs_model::{ResourceType, UnitId};
use obs_scripture::Passage;

use crate::doc::ReadingDoc;
use crate::selection::DocPos;

/// What each Highlight style looks like, by id (#181): Marks carry the look, so drawing
/// needs no lookup, and a style edit (which moves the annotation revision) remakes them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HighlightLooks(HashMap<String, StyleAppearance>);

impl HighlightLooks {
    pub fn new(styles: &[HighlightStyle]) -> Self {
        Self(
            styles
                .iter()
                .map(|s| (s.id.clone(), s.appearance))
                .collect(),
        )
    }

    /// The five built-in styles (as a new User database has them).
    pub fn built_in() -> Self {
        Self::new(&obs_store::built_in_styles())
    }

    /// Style `id`'s look; a style no row has (a database edited by hand) shows as the first
    /// built-in, so its Highlight is never lost from view.
    pub fn get(&self, id: &str) -> StyleAppearance {
        self.0
            .get(id)
            .copied()
            .unwrap_or_else(obs_store::fallback_appearance)
    }
}

/// What a Mark draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkKind {
    /// A Highlight in its style's look: a background behind the glyphs, the glyphs in its
    /// colour, or a line or box over them.
    Highlight(StyleAppearance),
    /// A small marker at the end of the range; hovering it previews the Note.
    Note,
    /// A Highlight made over part of a Verse in another Bible: a bar in the style's colour
    /// in the margin on the verse number's line, for each Verse the range touches.
    Margin(StyleAppearance),
    /// A Note from another Bible (or one without a Text range): its marker in the margin on
    /// the line of the verse number of the Verse its range starts at.
    VerseNote,
}

impl MarkKind {
    /// Whether it is drawn beside a verse number rather than over the range.
    pub fn at_verse_number(self) -> bool {
        matches!(self, MarkKind::Margin(_) | MarkKind::VerseNote)
    }

    /// Whether hovering it previews a Note.
    pub fn is_note(self) -> bool {
        matches!(self, MarkKind::Note | MarkKind::VerseNote)
    }
}

/// One Annotation drawn in one Resource, oldest first so the newest is drawn on top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    /// The Annotation's id.
    pub id: String,
    /// The range covered; for marks at a verse number, the whole Verse whose number it is.
    pub start: DocPos,
    pub end: DocPos,
    pub kind: MarkKind,
}

/// The Marks of `doc`'s Resource among `annotations`, in creation order (see the module
/// docs for where each Annotation shows). An Orphaned annotation is not drawn (it is listed
/// under Needs attention, #65). A Text range in this Resource whose text no longer reads as
/// its quote is never drawn over other words: a Passage anchor falls back to its whole
/// Verse(s) (ADR 0006), and a bare Text range is left out. Re-anchoring normally fixes both
/// before the Resource is shown; this guards Resources replaced outside the app. A Passage
/// anchor whose origin re-anchoring did not find shows as whole Verses everywhere.
pub fn marks_for(
    doc: &ReadingDoc,
    annotations: &[Annotation],
    looks: &HighlightLooks,
) -> Vec<Mark> {
    let units: HashMap<&UnitId, usize> = doc
        .units
        .iter()
        .enumerate()
        .map(|(i, u)| (&u.id, i))
        .collect();
    let bible = doc.manifest.resource_type == ResourceType::Bible && !doc.verses.is_empty();
    let mut marks: Vec<(i64, Mark)> = Vec::new();
    for a in annotations {
        let Some(anchor) = a.anchor.as_ref().filter(|x| !x.is_orphaned()) else {
            continue;
        };
        let mut push = |start: DocPos, end: DocPos, kind: MarkKind| {
            marks.push((
                a.created_at,
                Mark {
                    id: a.id.clone(),
                    start,
                    end,
                    kind,
                },
            ));
        };
        let here = match &a.content {
            Content::Highlight { style } => MarkKind::Highlight(looks.get(style)),
            Content::Note { .. } => MarkKind::Note,
        };
        // A Passage anchor's origin that re-anchoring did not find is kept for later but not
        // drawn: the Annotation shows as whole Verses, in every Bible.
        let range = anchor.text_range().filter(|r| !r.orphaned);
        match range {
            // Its own Resource: exact, or whole Verses when the range went stale.
            Some(range) if range.resource_id == doc.manifest.resource_id => {
                let (start, end) = (range.start as usize, range.end as usize);
                let exact = units.get(&range.unit).copied().filter(|&unit| {
                    start < end
                        && doc.units[unit].text.get(start..end) == Some(range.quote.as_str())
                });
                if let Some(unit) = exact {
                    push(
                        DocPos { unit, byte: start },
                        DocPos { unit, byte: end },
                        here,
                    );
                    continue;
                }
                let Some(passage) = anchor.passage() else {
                    continue;
                };
                let runs = verse_runs(doc, passage);
                match here {
                    MarkKind::Highlight(_) => {
                        for (s, e) in runs {
                            push(s, e, here);
                        }
                    }
                    // One marker, at the end of the last Verse.
                    _ => {
                        if let (Some((s, _)), Some((_, e))) = (runs.first(), runs.last()) {
                            push(*s, *e, here);
                        }
                    }
                }
            }
            // Another Bible: Passage anchors only, mapped through this Bible's Verse map.
            _ => {
                let Some(passage) = anchor.passage().filter(|_| bible) else {
                    continue;
                };
                let whole = range.is_none_or(|r| r.whole_verses);
                match here {
                    MarkKind::Highlight(_) if whole => {
                        for (s, e) in verse_runs(doc, passage) {
                            push(s, e, here);
                        }
                    }
                    // A margin mark at each Verse it touches: one over 3:16-18 marks 3:17
                    // and 3:18 too, so every Verse with marked words says so.
                    MarkKind::Highlight(style) => {
                        for i in doc.verses_for_standard(passage) {
                            if let Some((s, e)) = verse_span(doc, i) {
                                push(s, e, MarkKind::Margin(style));
                            }
                        }
                    }
                    _ => {
                        if let Some((s, e)) = first_verse(doc, passage) {
                            push(s, e, MarkKind::VerseNote);
                        }
                    }
                }
            }
        }
    }
    // UUIDv7 ids sort by creation too; they break ties within one millisecond.
    marks.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
    // One margin mark per style and Verse is enough: more would only widen the margin.
    let mut seen = std::collections::HashSet::new();
    marks
        .into_iter()
        .map(|(_, m)| m)
        .filter(|m| match m.kind {
            MarkKind::Margin(style) => seen.insert((m.start, style)),
            _ => true,
        })
        .collect()
}

/// Where Bible Verse `i` of `doc` is: its start and end.
fn verse_span(doc: &ReadingDoc, i: usize) -> Option<(DocPos, DocPos)> {
    let v = doc.verses.get(i)?;
    let unit = doc.verse_unit(i)?;
    let (start, end) = (v.start as usize, v.end as usize);
    (start < end).then_some((DocPos { unit, byte: start }, DocPos { unit, byte: end }))
}

/// The first of `doc`'s Verses that correspond to a Standard Passage.
fn first_verse(doc: &ReadingDoc, passage: Passage) -> Option<(DocPos, DocPos)> {
    doc.verses_for_standard(passage)
        .into_iter()
        .find_map(|i| verse_span(doc, i))
}

/// Where `doc` shows a Standard Passage, in whole Verses: one range per run of adjacent
/// Verses (only white space between them), so a heading or a Verse that is not part of the
/// Passage is never covered.
fn verse_runs(doc: &ReadingDoc, passage: Passage) -> Vec<(DocPos, DocPos)> {
    let mut runs: Vec<(DocPos, DocPos)> = Vec::new();
    for (s, e) in doc
        .verses_for_standard(passage)
        .into_iter()
        .filter_map(|i| verse_span(doc, i))
    {
        if let Some(last) = runs.last_mut()
            && last.1.unit == s.unit
            && last.1.byte <= s.byte
            && doc.units[s.unit]
                .text
                .get(last.1.byte..s.byte)
                .is_some_and(|gap| gap.trim().is_empty())
        {
            last.1 = e;
            continue;
        }
        runs.push((s, e));
    }
    runs
}
