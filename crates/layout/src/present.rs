//! Text unit + standoff spans → blocks of styled runs (#14 rules 6 and 7).
//!
//! A **block** is one `para` span, one `heading` or one `psalm_title`; poetry lines are their
//! own `para` spans. Text no block covers becomes a plain block, so every visible byte is in
//! exactly one block.
//!
//! | span | run style |
//! |---|---|
//! | `divine_name` | small caps, first letter of each word kept as a capital (`LORD`) |
//! | `woj` | `WordsOfJesus` role when red letter is on |
//! | `added`, `italic`, `emphasis`, `selah` | italic |
//! | `bold` | bold |
//! | `scripture_ref`, non-empty `xref` | `Link` role |
//! | `verse` start | verse number object, superscript; verse 1 shows the chapter number |
//! | zero-width `note_ref` | note marker object `†` |
//!
//! Hebrew runs (by character) use Ezra SIL at `hebrew_scale`; everything else Gentium Plus.

use std::ops::Range;

use obs_fonts::{FaceId, Script};
use obs_model::{Direction, ParaStyle, Span, SpanKind, TextUnit};
use unicode_script::UnicodeScript;

use crate::paragraph::{Align, InlineObject, Paragraph, Run, RunSource};
use crate::style::{ColorRole, RunStyle, TextSettings};

/// What kind of block a range of a unit is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph(ParaStyle),
    Heading(u8),
    PsalmTitle,
    /// Text outside any structural span.
    Plain,
}

/// One block of a unit, before styling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockSource {
    pub kind: BlockKind,
    /// Unit bytes the block covers.
    pub range: Range<usize>,
}

/// The blocks of a unit, in order and not overlapping.
pub fn unit_blocks(unit: &TextUnit) -> Vec<BlockSource> {
    let mut structural: Vec<BlockSource> = unit
        .spans
        .iter()
        .filter_map(|s| {
            let kind = match &s.kind {
                SpanKind::Para { style } => BlockKind::Paragraph(*style),
                SpanKind::Heading { level } => BlockKind::Heading(*level),
                SpanKind::PsalmTitle => BlockKind::PsalmTitle,
                _ => return None,
            };
            Some(BlockSource {
                kind,
                range: s.range(),
            })
        })
        .collect();
    structural.sort_by_key(|b| (b.range.start, std::cmp::Reverse(b.range.end)));
    let mut out: Vec<BlockSource> = Vec::new();
    let mut pos = 0;
    let push_plain = |out: &mut Vec<BlockSource>, a: usize, b: usize| {
        if unit
            .text
            .get(a..b)
            .is_some_and(|t| t.chars().any(|c| !c.is_whitespace()))
        {
            out.push(BlockSource {
                kind: BlockKind::Plain,
                range: a..b,
            });
        }
    };
    for b in structural {
        // A block that starts inside the previous one (nested markup) adds nothing.
        if b.range.start < pos {
            continue;
        }
        push_plain(&mut out, pos, b.range.start);
        pos = b.range.end.max(pos);
        out.push(b);
    }
    push_plain(&mut out, pos, unit.text.len());
    out
}

/// Whether a unit's character is Hebrew (letters, points, punctuation).
fn is_hebrew(c: char) -> bool {
    c.script() == Script::Hebrew
}

/// Style flags accumulated from the spans over a piece of text.
#[derive(Debug, Clone, Copy, Default)]
struct Flags {
    bold: bool,
    italic: bool,
    small_caps: bool,
    role: Option<ColorRole>,
}

/// Lay a block's text out as runs.
pub fn block_paragraph(
    unit: &TextUnit,
    block: &BlockSource,
    direction: Direction,
    settings: &TextSettings,
) -> Paragraph {
    let size = settings.size;
    let (base, space_before, space_after, indent, inset, align) = match block.kind {
        BlockKind::Heading(level) => {
            let scale = if level <= 1 { 1.12 } else { 1.0 };
            (
                Flags {
                    bold: true,
                    role: Some(ColorRole::Accent),
                    ..Flags::default()
                },
                size * 0.9,
                size * 0.3,
                0.0,
                0.0,
                (Align::Center, scale),
            )
        }
        BlockKind::PsalmTitle => (
            Flags {
                italic: true,
                ..Flags::default()
            },
            size * 0.4,
            size * 0.3,
            0.0,
            0.0,
            (Align::Start, 0.95),
        ),
        BlockKind::Paragraph(style) => {
            let (indent, inset) = match style {
                ParaStyle::Prose => (size * 1.2, 0.0),
                ParaStyle::Poetry1 => (0.0, size * 1.5),
                ParaStyle::Poetry2 => (0.0, size * 3.0),
                ParaStyle::Poetry3 => (0.0, size * 4.5),
                ParaStyle::List => (0.0, size * 1.5),
                ParaStyle::Blank => (0.0, 0.0),
            };
            let after = match style {
                ParaStyle::Prose | ParaStyle::List => size * 0.5,
                _ => size * 0.1,
            };
            (
                Flags::default(),
                0.0,
                after,
                indent,
                inset,
                (Align::Start, 1.0),
            )
        }
        BlockKind::Plain => (
            Flags::default(),
            0.0,
            size * 0.5,
            0.0,
            0.0,
            (Align::Start, 1.0),
        ),
    };
    let (align, scale) = align;
    let body = size * scale;
    let range = block.range.clone();
    // Only spans touching the block matter; scanning all of a book's spans per piece is what
    // made whole-book layout slow.
    let local: Vec<&Span> = unit
        .spans
        .iter()
        .filter(|s| s.start as usize <= range.end && s.end as usize >= range.start)
        .collect();

    let mut cuts: Vec<usize> = vec![range.start, range.end];
    for s in &local {
        for p in [s.range().start, s.range().end] {
            if range.contains(&p) {
                cuts.push(p);
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();

    let mut runs: Vec<Run> = Vec::new();
    let pieces = cuts
        .windows(2)
        .map(|w| (w[0], w[1]))
        .chain(std::iter::once((range.end, range.end)));
    for (a, b) in pieces {
        push_objects(unit, &local, a, &range, settings, body, &mut runs);
        if b <= a {
            continue;
        }
        let mut flags = base;
        for s in &local {
            if s.start as usize <= a && s.end as usize >= b && s.end > s.start {
                match &s.kind {
                    SpanKind::Added | SpanKind::Italic | SpanKind::Emphasis | SpanKind::Selah => {
                        flags.italic = true;
                    }
                    SpanKind::Bold => flags.bold = true,
                    SpanKind::DivineName => flags.small_caps = true,
                    SpanKind::Woj if settings.red_letter => {
                        flags.role = flags.role.or(Some(ColorRole::WordsOfJesus));
                    }
                    SpanKind::ScriptureRef { .. } | SpanKind::Xref { .. } => {
                        flags.role = Some(ColorRole::Link);
                    }
                    _ => {}
                }
            }
        }
        let Some(text) = unit.text.get(a..b) else {
            continue;
        };
        let text = if flags.small_caps {
            small_caps_text(&unit.text, a, text)
        } else {
            text.to_string()
        };
        push_text(&mut runs, &text, a, flags, body, settings);
    }
    Paragraph {
        runs,
        rtl: direction == Direction::Rtl,
        line_spacing: settings.line_spacing,
        indent,
        inset,
        space_before,
        space_after,
        align,
        hebrew_line_height: settings.hebrew_line_height,
    }
}

/// `LORD` → `Lord` (drawn with `smcp`, so it reads as L + small capitals). Only ASCII changes
/// case, so byte offsets are kept. A letter starts a word if the unit character before it is
/// not a letter.
fn small_caps_text(unit_text: &str, start: usize, text: &str) -> String {
    let mut prev_letter = unit_text[..start]
        .chars()
        .next_back()
        .is_some_and(char::is_alphabetic);
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        out.push(if prev_letter {
            c.to_ascii_lowercase()
        } else {
            c
        });
        prev_letter = c.is_alphabetic();
    }
    out
}

/// A note marker at `a` belongs to the block whose text it follows (`start < a <= end`); one at
/// the very start of a block belongs to it only if no block ends there.
fn owns_marker(unit: &TextUnit, block: &Range<usize>, a: usize) -> bool {
    if block.start < a && a <= block.end {
        return true;
    }
    a == block.start && unit.text[..a].chars().next_back().is_none_or(|c| c == '\n')
}

/// Verse numbers and note markers anchored at byte `a`.
fn push_objects(
    unit: &TextUnit,
    spans: &[&Span],
    a: usize,
    block: &Range<usize>,
    settings: &TextSettings,
    body: f32,
    runs: &mut Vec<Run>,
) {
    for s in spans {
        let start = s.start as usize;
        if start != a {
            continue;
        }
        match &s.kind {
            SpanKind::NoteRef { note, note_kind } if owns_marker(unit, block, a) => {
                runs.push(Run {
                    text: "†".into(),
                    style: RunStyle {
                        face: FaceId::Regular,
                        size: body * 0.7,
                        small_caps: false,
                        raise: body * 0.35,
                        role: ColorRole::NoteMarker,
                    },
                    source: RunSource::Object {
                        anchor: a,
                        object: InlineObject::NoteMarker {
                            note: note.clone(),
                            kind: *note_kind,
                        },
                    },
                });
            }
            SpanKind::Verse { verse, .. }
                if settings.verse_numbers && verse.verse > 0 && block.contains(&a) =>
            {
                let (text, style) = if verse.verse == 1 {
                    (
                        format!("{}\u{00A0}", verse.chapter),
                        RunStyle {
                            face: FaceId::Bold,
                            size: body * 1.3,
                            small_caps: false,
                            raise: 0.0,
                            role: ColorRole::Accent,
                        },
                    )
                } else {
                    (
                        format!("{}\u{202F}", verse.verse),
                        RunStyle {
                            face: FaceId::Bold,
                            size: body * 0.62,
                            small_caps: false,
                            raise: body * 0.38,
                            role: ColorRole::VerseNumber,
                        },
                    )
                };
                runs.push(Run {
                    text,
                    style,
                    source: RunSource::Object {
                        anchor: a,
                        object: InlineObject::VerseNumber(*verse),
                    },
                });
            }
            _ => {}
        }
    }
}

/// Split `text` into runs by script, so Hebrew gets Ezra SIL; neutral characters (spaces,
/// punctuation, digits) stay with the run before them.
fn push_text(
    runs: &mut Vec<Run>,
    text: &str,
    unit_start: usize,
    flags: Flags,
    body: f32,
    settings: &TextSettings,
) {
    let style_for = |hebrew: bool| {
        let script = if hebrew {
            Script::Hebrew
        } else {
            Script::Latin
        };
        RunStyle {
            face: FaceId::for_script(script, flags.bold, flags.italic),
            size: if hebrew {
                body * settings.hebrew_scale
            } else {
                body
            },
            small_caps: flags.small_caps && !hebrew,
            raise: 0.0,
            role: flags.role.unwrap_or(ColorRole::Ink),
        }
    };
    let mut start = 0;
    let mut current: Option<bool> = None;
    for (i, c) in text.char_indices() {
        let hebrew = if is_hebrew(c) && !c.is_whitespace() && c != '\u{05BE}' && c != '\u{05C3}' {
            Some(true)
        } else if c.is_alphanumeric() && !c.is_ascii_digit() {
            Some(false)
        } else {
            None
        };
        if let (Some(h), Some(cur)) = (hebrew, current)
            && h != cur
        {
            runs.push(Run {
                text: text[start..i].to_string(),
                style: style_for(cur),
                source: RunSource::Text {
                    unit_start: unit_start + start,
                },
            });
            start = i;
        }
        if hebrew.is_some() {
            current = hebrew;
        }
    }
    if start < text.len() {
        runs.push(Run {
            text: text[start..].to_string(),
            style: style_for(current.unwrap_or(false)),
            source: RunSource::Text {
                unit_start: unit_start + start,
            },
        });
    }
}

/// A one-off paragraph of plain text (inline-snippet mode: previews, popups, Word Info).
pub fn snippet_paragraph(
    text: &str,
    role: ColorRole,
    rtl: bool,
    settings: &TextSettings,
) -> Paragraph {
    let mut runs = Vec::new();
    let flags = Flags {
        role: Some(role),
        ..Flags::default()
    };
    push_text(&mut runs, text, 0, flags, settings.size, settings);
    Paragraph {
        runs,
        rtl,
        line_spacing: settings.line_spacing,
        indent: 0.0,
        inset: 0.0,
        space_before: 0.0,
        space_after: 0.0,
        align: Align::Start,
        hebrew_line_height: settings.hebrew_line_height,
    }
}
