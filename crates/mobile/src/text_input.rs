//! The text input and accessibility answers of a reader's hosted text view (UIKit's
//! `UITextInput`, `UIAccessibilityElement`; `docs/dev/mobile-text-view.md`). Positions are
//! *text offsets*: each unit's bytes in document order, one more between units. The core
//! decides what each text unit covers: a word is the word under the offset (never a space);
//! a sentence, line or paragraph is the Verse in a Bible, the block elsewhere.

use obs_layout::UnitPos;
use obs_reading::{DocPos, ReadingView, Selection};
use obs_scripture::{Passage, RefStyle, format_passage};

use crate::frame::FrameState;
use crate::{MobileApp, MobileError, ReaderId};

/// The text units the shell's text input asks about (UIKit's `UITextGranularity`); what each
/// covers is decided here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextUnitKind {
    Character,
    Word,
    Sentence,
    Paragraph,
    Line,
    Document,
}

/// A range of text, as text offsets (see [`MobileApp::text_offset`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSpan {
    pub start: u64,
    pub end: u64,
}

/// A rectangle in content points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentRect {
    pub x: f32,
    pub y: f64,
    pub width: f32,
    pub height: f32,
}

/// One accessibility element: a Verse in a Bible, a block (a heading) otherwise.
#[derive(Debug, Clone, PartialEq)]
pub struct AccessibilityItem {
    /// Stable for the same text: `Gen.1.1` for a Verse, `block.12` for a block.
    pub id: String,
    /// What VoiceOver reads: the Verse's reference, then its text.
    pub label: String,
    /// The box around its text, in content points.
    pub frame: ContentRect,
    /// Whether it is a heading rather than reading text.
    pub heading: bool,
}

/// The window block under content y, else the nearest.
pub(crate) fn block_under(state: &FrameState, y: f64) -> Option<usize> {
    if state.window.is_empty() {
        return None;
    }
    let i = (state.window.iter())
        .position(|(_, top, laid)| y < top + f64::from(laid.height))
        .unwrap_or(state.window.len() - 1);
    Some(i)
}

impl MobileApp {
    fn reader_parts(
        &mut self,
        reader: ReaderId,
    ) -> Result<(&mut ReadingView, &mut FrameState), MobileError> {
        let r = self.reader_at_mut(reader)?;
        Ok((&mut r.view, &mut r.frame))
    }

    fn unit_starts(view: &ReadingView, state: &mut FrameState) {
        if state.unit_starts.len() == view.doc.units.len() + 1 {
            return;
        }
        let mut at = 0u64;
        state.unit_starts = Vec::with_capacity(view.doc.units.len() + 1);
        for u in &view.doc.units {
            state.unit_starts.push(at);
            at += u.text.len() as u64 + 1;
        }
        state.unit_starts.push(at);
    }

    /// The text offset of a document position: the shell's text input counts positions this
    /// way (each unit's bytes, then one between units).
    pub fn text_offset(&mut self, reader: ReaderId, pos: DocPos) -> Result<u64, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        Self::unit_starts(view, state);
        Ok(state.unit_starts.get(pos.unit).copied().unwrap_or(0) + pos.byte as u64)
    }

    /// The document position of a text offset, on a character boundary at or before it.
    fn doc_pos(view: &ReadingView, state: &mut FrameState, offset: u64) -> DocPos {
        Self::unit_starts(view, state);
        let units = view.doc.units.len();
        if units == 0 {
            return DocPos { unit: 0, byte: 0 };
        }
        let unit = (state.unit_starts.partition_point(|&s| s <= offset))
            .saturating_sub(1)
            .min(units - 1);
        let text = &view.doc.units[unit].text;
        let mut byte = ((offset - state.unit_starts[unit]) as usize).min(text.len());
        while !text.is_char_boundary(byte) {
            byte -= 1;
        }
        DocPos { unit, byte }
    }

    fn offset_of(state: &FrameState, pos: DocPos) -> u64 {
        state.unit_starts.get(pos.unit).copied().unwrap_or(0) + pos.byte as u64
    }

    /// The last text offset (the end of the document).
    pub fn text_end(&mut self, reader: ReaderId) -> Result<u64, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        Self::unit_starts(view, state);
        Ok(state
            .unit_starts
            .last()
            .copied()
            .unwrap_or(1)
            .saturating_sub(1))
    }

    /// `offset` moved by `by` characters, within the document.
    pub fn move_offset(
        &mut self,
        reader: ReaderId,
        offset: u64,
        by: i64,
    ) -> Result<u64, MobileError> {
        let end = self.text_end(reader)?;
        let (view, state) = self.reader_parts(reader)?;
        if view.doc.units.is_empty() {
            return Ok(0);
        }
        let mut pos = Self::doc_pos(view, state, offset.min(end));
        let mut at = Self::offset_of(state, pos);
        for _ in 0..by.unsigned_abs().min(1 << 20) {
            let text = &view.doc.units[pos.unit].text;
            let next = if by > 0 {
                text[pos.byte..]
                    .chars()
                    .next()
                    .map(|c| pos.byte + c.len_utf8())
            } else {
                text[..pos.byte]
                    .chars()
                    .next_back()
                    .map(|c| pos.byte - c.len_utf8())
            };
            match next {
                Some(b) => pos.byte = b,
                None if by > 0 && pos.unit + 1 < view.doc.units.len() => {
                    pos = DocPos {
                        unit: pos.unit + 1,
                        byte: 0,
                    }
                }
                None if by < 0 && pos.unit > 0 => {
                    let unit = pos.unit - 1;
                    pos = DocPos {
                        unit,
                        byte: view.doc.units[unit].text.len(),
                    }
                }
                None => break,
            }
            at = Self::offset_of(state, pos);
        }
        Ok(at)
    }

    /// The text offset nearest a content point, among the text laid out around the view.
    pub fn closest_offset(&mut self, reader: ReaderId, x: f32, y: f64) -> Result<u64, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        Self::unit_starts(view, state);
        let Some(hit) = block_under(state, y) else {
            return Ok(0);
        };
        let (index, top, laid) = &state.window[hit];
        let p = laid.caret_at(x - state.left, (y - top) as f32);
        let unit = view.doc.blocks[*index].unit;
        let byte = match laid.to_unit(p) {
            Some(UnitPos::Text(b)) => b,
            Some(UnitPos::Object { anchor, .. }) => anchor,
            None => view.doc.blocks[*index].source.range.start,
        };
        Ok(Self::offset_of(state, DocPos { unit, byte }))
    }

    /// The span of text of kind `kind` around `offset`: a word is the word (or space) there;
    /// a sentence, line or paragraph is the Verse in a Bible (the block elsewhere).
    pub fn enclosing_span(
        &mut self,
        reader: ReaderId,
        offset: u64,
        kind: TextUnitKind,
    ) -> Result<Option<TextSpan>, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let pos = Self::doc_pos(view, state, offset);
        let doc = &view.doc;
        let Some(unit) = doc.units.get(pos.unit) else {
            return Ok(None);
        };
        let range = match kind {
            TextUnitKind::Character => {
                let len = unit.text[pos.byte..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8);
                pos.byte..pos.byte + len
            }
            TextUnitKind::Word => {
                let w = obs_reading::word_at(doc, pos);
                if unit.text[w.clone()].trim().is_empty() {
                    return Ok(None);
                }
                w
            }
            TextUnitKind::Sentence | TextUnitKind::Line | TextUnitKind::Paragraph => {
                match doc.verse_range_at(pos.unit, pos.byte) {
                    Some(r) if r.contains(&pos.byte) => r,
                    _ => match doc.block_at(pos.unit, pos.byte) {
                        Some(b) => doc.blocks[b].source.range.clone(),
                        None => 0..unit.text.len(),
                    },
                }
            }
            TextUnitKind::Document => {
                let end = state
                    .unit_starts
                    .last()
                    .copied()
                    .unwrap_or(1)
                    .saturating_sub(1);
                return Ok(Some(TextSpan { start: 0, end }));
            }
        };
        let start = Self::offset_of(
            state,
            DocPos {
                unit: pos.unit,
                byte: range.start,
            },
        );
        Ok(Some(TextSpan {
            start,
            end: start + (range.end - range.start) as u64,
        }))
    }

    fn selection(view: &ReadingView, state: &mut FrameState, span: TextSpan) -> Selection {
        let a = Self::doc_pos(view, state, span.start.min(span.end));
        let b = Self::doc_pos(view, state, span.start.max(span.end));
        Selection {
            anchor: a,
            head: b,
            granularity: obs_reading::Granularity::Grapheme,
        }
    }

    /// The text of a span, as written (units joined by a newline).
    pub fn text_in(&mut self, reader: ReaderId, span: TextSpan) -> Result<String, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let sel = Self::selection(view, state, span);
        Ok(obs_reading::selected_text(&view.doc, &sel))
    }

    /// A span as Copy puts it on the pasteboard: the text, then its reference.
    pub fn copy_text(&mut self, reader: ReaderId, span: TextSpan) -> Result<String, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let sel = Self::selection(view, state, span);
        Ok(view.copy_text(&sel))
    }

    /// The rectangles covering a span's text among the text laid out around the view.
    pub fn span_rects(
        &mut self,
        reader: ReaderId,
        span: TextSpan,
    ) -> Result<Vec<ContentRect>, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let sel = Self::selection(view, state, span);
        let range = (sel.anchor, sel.head);
        let mut out = Vec::new();
        for (index, top, laid) in &state.window {
            let rects =
                obs_reading::selection::block_range_rects(&view.doc, *index, laid, &[range]);
            for [x0, y0, x1, y1] in rects.into_iter().flatten() {
                out.push(ContentRect {
                    x: state.left + x0,
                    y: top + f64::from(y0),
                    width: x1 - x0,
                    height: y1 - y0,
                });
            }
        }
        Ok(out)
    }

    /// The caret's rectangle at `offset`: a thin box at the leading edge of the character
    /// there (the trailing edge of the one before, at the end of a unit).
    pub fn caret_rect(
        &mut self,
        reader: ReaderId,
        offset: u64,
    ) -> Result<ContentRect, MobileError> {
        let next = self.move_offset(reader, offset, 1)?;
        let rtl = (self.readers.get(reader.0 as usize))
            .is_some_and(|r| r.view.doc.direction() == obs_model::Direction::Rtl);
        let rects = self.span_rects(
            reader,
            TextSpan {
                start: offset,
                end: next,
            },
        )?;
        let caret = |r: &ContentRect, leading: bool| ContentRect {
            x: if leading != rtl { r.x } else { r.x + r.width },
            width: 2.0,
            ..*r
        };
        if let Some(r) = rects.first() {
            return Ok(caret(r, true));
        }
        let before = self.move_offset(reader, offset, -1)?;
        let rects = self.span_rects(
            reader,
            TextSpan {
                start: before,
                end: offset,
            },
        )?;
        Ok(rects.last().map_or(
            ContentRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            |r| caret(r, false),
        ))
    }

    /// The accessibility elements of the text laid out around the view, in reading order:
    /// one per Verse in a Bible (a Verse split across paragraphs is one element, framed
    /// around all of it), one per block without Verses (a heading).
    pub fn accessibility_items(
        &mut self,
        reader: ReaderId,
    ) -> Result<Vec<AccessibilityItem>, MobileError> {
        let (view, state) = self.reader_parts(reader)?;
        let doc = &view.doc;
        let mut out: Vec<AccessibilityItem> = Vec::new();
        let union = |a: ContentRect, b: ContentRect| {
            let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
            let x1 = (a.x + a.width).max(b.x + b.width);
            let y1 = (a.y + f64::from(a.height)).max(b.y + f64::from(b.height));
            ContentRect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: (y1 - y0) as f32,
            }
        };
        for (index, top, laid) in &state.window {
            let block = &doc.blocks[*index];
            let unit = block.unit;
            let range = block.source.range.clone();
            let n = doc.verses.len();
            let first = partition(n, |i| doc.verse_unit(i) < Some(unit));
            let mut verses = Vec::new();
            for i in first..n {
                if doc.verse_unit(i) != Some(unit) {
                    break;
                }
                let v = &doc.verses[i];
                let (s, e) = (v.start as usize, v.end as usize);
                if s < range.end && e > range.start {
                    verses.push((v.verse, s.max(range.start), e.min(range.end)));
                }
            }
            let frame_of = |a: usize, b: usize| {
                let rects = obs_reading::selection::block_range_rects(
                    doc,
                    *index,
                    laid,
                    &[(DocPos { unit, byte: a }, DocPos { unit, byte: b })],
                );
                (rects.into_iter().flatten())
                    .map(|[x0, y0, x1, y1]| ContentRect {
                        x: state.left + x0,
                        y: top + f64::from(y0),
                        width: x1 - x0,
                        height: y1 - y0,
                    })
                    .reduce(union)
            };
            if verses.is_empty() {
                let text = doc.units[unit].text.get(range.clone()).unwrap_or_default();
                let label = clean(text);
                if let Some(frame) = frame_of(range.start, range.end).filter(|_| !label.is_empty())
                {
                    out.push(AccessibilityItem {
                        id: format!("block.{index}"),
                        label,
                        frame,
                        heading: true,
                    });
                }
                continue;
            }
            for (verse, a, b) in verses {
                let Some(frame) = frame_of(a, b) else {
                    continue;
                };
                let id = verse.osis();
                if let Some(last) = out.last_mut()
                    && last.id == id
                {
                    last.frame = union(last.frame, frame);
                    continue;
                }
                let reference =
                    format_passage(Passage::verse(verse), &doc.versification, RefStyle::Long);
                let text = clean(doc.verse_text(verse).unwrap_or_default());
                out.push(AccessibilityItem {
                    id,
                    label: format!("{reference}. {text}"),
                    frame,
                    heading: false,
                });
            }
        }
        Ok(out)
    }
}

/// The first `i` in `0..n` where `pred` is false (`pred` true then false).
fn partition(n: usize, pred: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi) = (0, n);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if pred(mid) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Text as VoiceOver reads it: whitespace runs as one space, no control characters.
pub(crate) fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split(|c: char| c.is_whitespace() || c.is_control()) {
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}
