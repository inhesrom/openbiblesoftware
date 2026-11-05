//! Selection in document coordinates (#14 rule 9): `(unit, byte)` positions, the same shape
//! as a Text range, so a selection survives scrolling and virtualization. Rects are computed
//! per cluster of what is laid out in view, which also handles bidi runs; copying reads the
//! unit text, never painted glyphs.

use std::ops::Range;

use obs_layout::UnitPos;
use obs_scripture::{Passage, RefStyle, format_passage};
use unicode_segmentation::UnicodeSegmentation;

use crate::doc::ReadingDoc;
use crate::view::ReadingView;

/// A position in the document: a unit (index into the document's units) and a byte in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DocPos {
    pub unit: usize,
    pub byte: usize,
}

/// What a drag extends by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Granularity {
    #[default]
    Grapheme,
    Word,
    Verse,
}

/// A selection: where it started (`anchor`) and where the pointer is (`head`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: DocPos,
    pub head: DocPos,
    pub granularity: Granularity,
}

impl Selection {
    pub fn caret(at: DocPos) -> Self {
        Self {
            anchor: at,
            head: at,
            granularity: Granularity::Grapheme,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// Start and end in document order, expanded to the granularity's units.
    pub fn range(&self, doc: &ReadingDoc) -> (DocPos, DocPos) {
        let (a, b) = if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        };
        match self.granularity {
            Granularity::Grapheme => (a, b),
            Granularity::Word => (
                DocPos {
                    byte: word_at(doc, a).start,
                    ..a
                },
                DocPos {
                    byte: word_at(doc, b).end,
                    ..b
                },
            ),
            Granularity::Verse => (
                DocPos {
                    byte: doc
                        .verse_range_at(a.unit, a.byte)
                        .map_or(a.byte, |r| r.start),
                    ..a
                },
                DocPos {
                    byte: doc.verse_range_at(b.unit, b.byte).map_or(b.byte, |r| r.end),
                    ..b
                },
            ),
        }
    }

    pub fn contains(&self, doc: &ReadingDoc, p: DocPos) -> bool {
        let (a, b) = self.range(doc);
        a <= p && p < b
    }
}

/// The word (or space or punctuation run) around a position.
pub fn word_at(doc: &ReadingDoc, p: DocPos) -> Range<usize> {
    let text = &doc.units[p.unit].text;
    for (i, w) in text.split_word_bound_indices() {
        if p.byte < i + w.len() {
            return i..i + w.len();
        }
    }
    text.len()..text.len()
}

/// The exact source text a selection covers; units are joined with a newline.
pub fn selected_text(doc: &ReadingDoc, sel: &Selection) -> String {
    let (a, b) = sel.range(doc);
    let mut out = String::new();
    for u in a.unit..=b.unit {
        let text = &doc.units[u].text;
        let start = if u == a.unit { a.byte } else { 0 };
        let end = if u == b.unit { b.byte } else { text.len() };
        if let Some(s) = text.get(start.min(end)..end.min(text.len())) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(s);
        }
    }
    out
}

/// The selection as copied: the text, then its reference and the Resource's abbreviation,
/// e.g. `John 3:16 (KJV)`. References are in the Bible's own numbering.
pub fn copy_text(doc: &ReadingDoc, sel: &Selection) -> String {
    let text = selected_text(doc, sel);
    let text = text.trim();
    let (a, b) = sel.range(doc);
    let first = doc.verse_containing(a.unit, a.byte);
    let last = doc.verse_containing(b.unit, b.byte.saturating_sub(1).max(a.byte));
    let abbrev = &doc.manifest.abbreviation;
    match (first, last) {
        (Some(f), Some(l)) => {
            let passage = Passage::new(f, l.max(f)).unwrap_or(Passage::verse(f));
            let reference = format_passage(passage, &doc.versification, RefStyle::Long);
            format!("{text}\n{reference} ({abbrev})")
        }
        _ => format!("{text}\n({abbrev})"),
    }
}

/// Highlight rectangles `[x0, y0, x1, y1]` for the selection, relative to the text's left
/// edge and the viewport top, for the blocks laid out in a viewport `height` tall.
pub fn selection_rects(view: &ReadingView, sel: &Selection, height: f32) -> Vec<[f32; 4]> {
    let range = sel.range(&view.doc);
    range_rects(view, &[range], height)
        .into_iter()
        .next()
        .unwrap_or_default()
}

/// [`selection_rects`] for many document ranges `[start, end)` at once (Highlights, Note
/// ranges): one list of rectangles per range, from one pass over the clusters in view. Each
/// block only tests the ranges that reach into it.
pub fn range_rects(
    view: &ReadingView,
    ranges: &[(DocPos, DocPos)],
    height: f32,
) -> Vec<Vec<[f32; 4]>> {
    let doc = &view.doc;
    let mut out: Vec<Vec<[f32; 4]>> = vec![Vec::new(); ranges.len()];
    if ranges.iter().all(|(a, b)| a >= b) {
        return out;
    }
    for (index, top, laid) in view.laid_in_view(height) {
        let block = &doc.blocks[index];
        let unit = block.unit;
        let first = DocPos {
            unit,
            byte: block.source.range.start,
        };
        let last = DocPos {
            unit,
            byte: block.source.range.end,
        };
        // Ranges reaching into this block (an empty block still has its start position).
        let active: Vec<usize> = (0..ranges.len())
            .filter(|&i| {
                let (a, b) = ranges[i];
                a < b && a <= last && b > first
            })
            .collect();
        if active.is_empty() {
            continue;
        }
        for line in &laid.lines {
            for item in &line.items {
                let mut runs: Vec<Option<[f32; 4]>> = vec![None; active.len()];
                for (bytes, x0, x1) in item.clusters() {
                    let byte = match laid.to_unit(bytes.start) {
                        Some(UnitPos::Text(b)) => b,
                        Some(UnitPos::Object { anchor, .. }) => anchor,
                        None => continue,
                    };
                    let p = DocPos { unit, byte };
                    for (k, &i) in active.iter().enumerate() {
                        let (start, end) = ranges[i];
                        let inside = start <= p && p < end;
                        let run = &mut runs[k];
                        match (run.as_mut(), inside) {
                            (Some(r), true)
                                if (r[2] - x0).abs() < 0.5 || (r[0] - x1).abs() < 0.5 =>
                            {
                                r[0] = r[0].min(x0);
                                r[2] = r[2].max(x1);
                            }
                            (_, true) => {
                                if let Some(r) = run.take() {
                                    out[i].push(r);
                                }
                                *run = Some([x0, top + line.top, x1, top + line.bottom]);
                            }
                            (_, false) => {
                                if let Some(r) = run.take() {
                                    out[i].push(r);
                                }
                            }
                        }
                    }
                }
                for (k, run) in runs.into_iter().enumerate() {
                    if let Some(r) = run {
                        out[active[k]].push(r);
                    }
                }
            }
        }
    }
    out
}
