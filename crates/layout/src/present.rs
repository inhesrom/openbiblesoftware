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
//! | `tagged_word` in an `entry` unit (a Lexicon's Strong's link) | `Link` role |
//! | `verse` start | verse number object, superscript; verse 1 shows the chapter number |
//! | zero-width `note_ref` (footnote) | Footnote marker object: a raised italic letter, a … z then aa …, counted from each chapter's first (#208) |
//! | zero-width `note_ref` (cross-reference) | the `↗` of its zero-width `xref` beside it, else a raised `‡` |
//!
//! With `footnote_markers` off, no zero-width `note_ref`, `xref` or `scripture_ref` is laid out.
//!
//! Each script's text uses its reading font (#138; by default Ezra SIL for Hebrew, at
//! `hebrew_scale`, and Gentium Plus for everything else), falling back per cluster to the
//! bundled face for characters the chosen font lacks. Verse and chapter numbers follow the
//! Latin font.

use std::ops::Range;

use std::sync::Arc;

use obs_fonts::{Face, FaceId, FaceSet, ReadingScript, face};
use obs_model::{Direction, NoteKind, ParaStyle, Span, SpanKind, TextUnit, UnitKind};
use unicode_segmentation::UnicodeSegmentation;

use crate::paragraph::{Align, InlineObject, Paragraph, Run, RunSource};
use crate::style::{ColorRole, RunStyle, TextSettings, VerseNumbers};

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

/// Style flags accumulated from the spans over a piece of text.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Flags {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) small_caps: bool,
    pub(crate) role: Option<ColorRole>,
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
    // made whole-book layout slow. The same pass counts each chapter's Footnotes, for the
    // letters of the block's markers.
    let mut local: Vec<(usize, &Span)> = Vec::new();
    let mut footnotes: Vec<(usize, usize)> = Vec::new();
    let mut chapter = None;
    let mut in_chapter = 0;
    for (i, s) in unit.spans.iter().enumerate() {
        match &s.kind {
            SpanKind::Verse { verse, .. } if chapter != Some((verse.book, verse.chapter)) => {
                chapter = Some((verse.book, verse.chapter));
                in_chapter = 0;
            }
            SpanKind::NoteRef {
                note_kind: NoteKind::Footnote,
                ..
            } => {
                if range.contains(&(s.start as usize)) || s.start as usize == range.end {
                    footnotes.push((i, in_chapter));
                }
                in_chapter += 1;
            }
            _ => {}
        }
        if s.start as usize <= range.end && s.end as usize >= range.start {
            local.push((i, s));
        }
    }

    let mut cuts: Vec<usize> = vec![range.start, range.end];
    for (_, s) in &local {
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
        let objects = Objects {
            spans: &local,
            footnotes: &footnotes,
            rtl: direction == Direction::Rtl,
        };
        push_objects(unit, &objects, a, &range, settings, body, &mut runs);
        if b <= a {
            continue;
        }
        let mut flags = base;
        for (_, s) in &local {
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
                    // A Strong's number in a Lexicon or Dictionary entry is a link to its
                    // Word Study (#55); Tagged words in a Bible stay plain text.
                    SpanKind::TaggedWord { .. } if unit.kind == UnitKind::Entry => {
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
        interlinear: None,
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

/// The label of a chapter's `n`th Footnote marker, from 0: a … z, then aa, ab … (#208).
pub fn footnote_label(n: usize) -> String {
    let mut n = n + 1;
    let mut letters = Vec::new();
    while n > 0 {
        n -= 1;
        letters.push(char::from(b'a' + (n % 26) as u8));
        n /= 26;
    }
    letters.iter().rev().collect()
}

/// The spans touching a block, and its Footnotes' places in their chapters (span index,
/// ordinal).
struct Objects<'a> {
    spans: &'a [(usize, &'a Span)],
    footnotes: &'a [(usize, usize)],
    /// A right-to-left paragraph: a letter marker ends with a right-to-left mark, so that it
    /// does not join the verse number after it in one left-to-right run.
    rtl: bool,
}

/// Whether a zero-width Scripture reference or Cross-reference sits at byte `a`: its `↗`
/// stands for a cross-reference note there.
fn reference_marker_at(spans: &[(usize, &Span)], a: usize) -> bool {
    spans.iter().any(|(_, s)| {
        s.start as usize == a
            && s.start == s.end
            && matches!(
                s.kind,
                SpanKind::ScriptureRef { .. } | SpanKind::Xref { .. }
            )
    })
}

/// Verse numbers and Footnote markers anchored at byte `a`.
fn push_objects(
    unit: &TextUnit,
    objects: &Objects<'_>,
    a: usize,
    block: &Range<usize>,
    settings: &TextSettings,
    body: f32,
    runs: &mut Vec<Run>,
) {
    let marker = |text: String, face: FaceId, object: InlineObject| Run {
        text,
        style: RunStyle {
            face,
            size: body * 0.7,
            small_caps: false,
            raise: body * 0.35,
            role: match object {
                InlineObject::NoteMarker { .. } => ColorRole::NoteMarker,
                _ => ColorRole::Link,
            },
        },
        source: RunSource::Object { anchor: a, object },
    };
    for &(span, s) in objects.spans {
        let start = s.start as usize;
        if start != a {
            continue;
        }
        match &s.kind {
            SpanKind::ScriptureRef { .. } | SpanKind::Xref { .. }
                if s.start == s.end && settings.footnote_markers && owns_marker(unit, block, a) =>
            {
                runs.push(marker(
                    "↗".into(),
                    FaceId::Regular,
                    InlineObject::ScriptureReference { span },
                ));
            }
            SpanKind::NoteRef { note, note_kind }
                if settings.footnote_markers && owns_marker(unit, block, a) =>
            {
                let (text, face) = match note_kind {
                    NoteKind::Footnote => {
                        let n = objects
                            .footnotes
                            .iter()
                            .find(|(i, _)| *i == span)
                            .map_or(0, |(_, n)| *n);
                        let mut label = footnote_label(n);
                        if objects.rtl {
                            label.push('\u{200F}');
                        }
                        (label, FaceId::Italic)
                    }
                    NoteKind::CrossReference if reference_marker_at(objects.spans, a) => continue,
                    NoteKind::CrossReference => ("‡".to_string(), FaceId::Regular),
                };
                runs.push(marker(
                    text,
                    face,
                    InlineObject::NoteMarker {
                        note: note.clone(),
                        kind: *note_kind,
                    },
                ));
            }
            SpanKind::Verse { verse, .. }
                if verse.verse > 0
                    && block.contains(&a)
                    && (settings.verse_numbers != VerseNumbers::Hidden || verse.verse == 1) =>
            {
                // Numbers follow the Latin reading font (bold), when it has the digits.
                let numbers = FaceChooser::new(settings, true, false);
                let number_face = |t: &str| numbers.face_for(ReadingScript::Latin, t);
                let (text, style) = if verse.verse == 1 {
                    (
                        format!("{}\u{00A0}", verse.chapter),
                        RunStyle {
                            face: number_face(&verse.chapter.to_string()),
                            size: body * 1.3,
                            small_caps: false,
                            raise: 0.0,
                            role: ColorRole::Accent,
                        },
                    )
                } else if settings.verse_numbers == VerseNumbers::Inline {
                    (
                        format!("{}\u{00A0}", verse.verse),
                        RunStyle {
                            face: number_face(&verse.verse.to_string()),
                            size: body * 0.8,
                            small_caps: false,
                            raise: 0.0,
                            role: ColorRole::VerseNumber,
                        },
                    )
                } else {
                    (
                        format!("{}\u{202F}", verse.verse),
                        RunStyle {
                            face: number_face(&verse.verse.to_string()),
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

/// Synthetic small capitals (#138): with a reading font that has no `smcp`, lowercase letters
/// of small-caps text are drawn as capitals at this share of the size.
pub const SYNTHETIC_SMALL_CAPS: f32 = 0.78;

/// Characters that draw nothing (joiners, direction marks, variation selectors, the
/// combining grapheme joiner): shaping hides them, so a face need not have them.
fn is_default_ignorable(c: char) -> bool {
    matches!(c as u32, 0x034F | 0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2064 | 0xFE00..=0xFE0F | 0xFEFF)
}

/// Picks the face for each cluster of a text in one style: the chosen reading font of the
/// cluster's script when it has every character, else the bundled face (per-glyph fallback,
/// #138).
struct FaceChooser {
    chosen: [FaceId; 3],
    /// The chosen faces that are not bundled, loaded once per text.
    loaded: [Option<Arc<Face>>; 3],
    bundled: [FaceId; 3],
}

impl FaceChooser {
    fn new(settings: &TextSettings, bold: bool, italic: bool) -> Self {
        let chosen = ReadingScript::ALL.map(|s| settings.fonts.get(s).pick(bold, italic));
        Self {
            chosen,
            loaded: chosen.map(|id| (!id.is_bundled()).then(|| face(id).ok()).flatten()),
            bundled: ReadingScript::ALL.map(|s| FaceSet::bundled(s).pick(bold, italic)),
        }
    }

    fn all_bundled(&self) -> bool {
        self.chosen.iter().all(|id| id.is_bundled())
    }

    fn slot(script: ReadingScript) -> usize {
        script as usize
    }

    /// Whether `id` can draw every visible character of `cluster`.
    fn covers(&self, id: FaceId, cluster: &str) -> bool {
        if id.is_bundled() {
            return true;
        }
        let Some(f) = self.loaded.iter().flatten().find(|f| f.id == id) else {
            return false;
        };
        cluster
            .chars()
            .all(|c| is_default_ignorable(c) || f.covers(c))
    }

    fn face_for(&self, script: ReadingScript, cluster: &str) -> FaceId {
        let i = Self::slot(script);
        let chosen = self.chosen[i];
        if self.covers(chosen, cluster) {
            chosen
        } else {
            self.bundled[i]
        }
    }
}

/// The script a cluster is drawn as: its first character with one. Maqaf and sof pasuq go with
/// the text around them, like spaces and punctuation.
fn cluster_script(cluster: &str) -> Option<ReadingScript> {
    cluster
        .chars()
        .filter(|&c| c != '\u{05BE}' && c != '\u{05C3}' && !c.is_ascii_digit())
        .find_map(ReadingScript::of_char)
}

/// Split `text` into runs by script and face: each script gets its reading font (Ezra SIL and
/// Gentium Plus by default), and a cluster the chosen font lacks falls back to the bundled face.
/// Neutral clusters (spaces, punctuation, digits) stay with the run before them, or, at the
/// start, with the first cluster that has a script. Every cluster, neutral or not, is drawn
/// only in a face that covers it.
pub(crate) fn push_text(
    runs: &mut Vec<Run>,
    text: &str,
    unit_start: usize,
    flags: Flags,
    body: f32,
    settings: &TextSettings,
) {
    let chooser = FaceChooser::new(settings, flags.bold, flags.italic);
    let style_for = |face: FaceId, script: ReadingScript| {
        let hebrew = script == ReadingScript::Hebrew;
        RunStyle {
            face,
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
    // With only bundled faces every character is drawn where it is; a chosen face is checked
    // for whole grapheme clusters, so a letter and its marks stay in one face.
    let clusters = || -> Box<dyn Iterator<Item = (usize, &str)>> {
        if chooser.all_bundled() {
            Box::new(
                text.char_indices()
                    .map(|(i, c)| (i, &text[i..i + c.len_utf8()])),
            )
        } else {
            Box::new(text.grapheme_indices(true))
        }
    };
    // Leading neutral clusters go with the first cluster that has a script, in its face when
    // that face has them. With no such cluster, such as a maqaf alone between two Tagged
    // words, they take the script the characters belong to (Gentium Plus has no maqaf).
    let first = clusters().find_map(|(_, cluster)| {
        cluster_script(cluster).map(|script| (chooser.face_for(script, cluster), script))
    });
    let first_script = first.map(|(_, script)| script).unwrap_or_else(|| {
        text.chars()
            .find_map(ReadingScript::of_char)
            .unwrap_or(ReadingScript::Latin)
    });
    let mut start = 0;
    let mut current: Option<(FaceId, ReadingScript)> = None;
    for (i, cluster) in clusters() {
        let next = match (cluster_script(cluster), current) {
            (Some(script), _) => (chooser.face_for(script, cluster), script),
            (None, Some((face, script))) if chooser.covers(face, cluster) => (face, script),
            (None, Some((_, script))) => (chooser.face_for(script, cluster), script),
            (None, None) => match first {
                Some((face, script)) if chooser.covers(face, cluster) => (face, script),
                _ => (chooser.face_for(first_script, cluster), first_script),
            },
        };
        let Some(cur) = current else {
            current = Some(next);
            continue;
        };
        // Latin and Greek in one face stay one run; Hebrew is sized apart.
        let hebrew = |(_, script): (FaceId, ReadingScript)| script == ReadingScript::Hebrew;
        if next.0 != cur.0 || hebrew(next) != hebrew(cur) {
            push_run(
                runs,
                &text[start..i],
                unit_start + start,
                style_for(cur.0, cur.1),
            );
            start = i;
        }
        current = Some(next);
    }
    if let Some((face, script)) = current {
        push_run(
            runs,
            &text[start..],
            unit_start + start,
            style_for(face, script),
        );
    }
}

/// Push one run. Small-caps text in a face without `smcp` becomes synthetic small caps: ASCII
/// lowercase letters drawn as capitals at [`SYNTHETIC_SMALL_CAPS`] of the size (byte offsets
/// are kept, as only ASCII changes case).
fn push_run(runs: &mut Vec<Run>, text: &str, unit_start: usize, style: RunStyle) {
    let real = !style.small_caps || face(style.face).is_ok_and(|f| f.has_small_caps());
    if real {
        runs.push(Run {
            text: text.to_string(),
            style,
            source: RunSource::Text { unit_start },
        });
        return;
    }
    let mut start = 0;
    let mut lower: Option<bool> = None;
    let push = |runs: &mut Vec<Run>, a: usize, b: usize, lower: bool| {
        let piece = &text[a..b];
        runs.push(Run {
            text: if lower {
                piece.to_ascii_uppercase()
            } else {
                piece.to_string()
            },
            style: RunStyle {
                size: if lower {
                    style.size * SYNTHETIC_SMALL_CAPS
                } else {
                    style.size
                },
                small_caps: false,
                ..style
            },
            source: RunSource::Text {
                unit_start: unit_start + a,
            },
        });
    };
    for (i, c) in text.char_indices() {
        let is_lower = c.is_ascii_lowercase();
        if lower.is_some_and(|l| l != is_lower) {
            push(runs, start, i, !is_lower);
            start = i;
        }
        lower = Some(is_lower);
    }
    if start < text.len() {
        push(runs, start, text.len(), lower.unwrap_or(false));
    }
}

/// Whether `text` reads right to left as a paragraph of its own: its first strong character
/// is right-to-left (UAX #9 rules P2-P3), as for a line of copied Hebrew text.
pub fn starts_rtl(text: &str) -> bool {
    unicode_bidi::get_base_direction(text) == unicode_bidi::Direction::Rtl
}

/// A one-off paragraph of plain text in `base`, with the byte ranges in `styled` drawn in
/// their own colour role (a Note body's Bible references as links, #64). Byte positions map
/// back to `text` (as unit positions), so hit tests and range rects work on them.
pub fn styled_snippet_paragraph(
    text: &str,
    base: ColorRole,
    styled: &[(std::ops::Range<usize>, ColorRole)],
    rtl: bool,
    settings: &TextSettings,
) -> Paragraph {
    let mut para = snippet_paragraph("", base, rtl, settings);
    let flags = |role| Flags {
        role: Some(role),
        ..Flags::default()
    };
    let mut at = 0;
    let mut ranges: Vec<&(std::ops::Range<usize>, ColorRole)> = styled.iter().collect();
    ranges.sort_by_key(|(r, _)| r.start);
    for (range, role) in ranges {
        let (start, end) = (range.start.max(at), range.end.min(text.len()));
        let (Some(before), Some(inside)) = (text.get(at..start), text.get(start..end)) else {
            continue;
        };
        if start >= end {
            continue;
        }
        push_text(
            &mut para.runs,
            before,
            at,
            flags(base),
            settings.size,
            settings,
        );
        push_text(
            &mut para.runs,
            inside,
            start,
            flags(*role),
            settings.size,
            settings,
        );
        at = end;
    }
    if let Some(rest) = text.get(at..) {
        push_text(
            &mut para.runs,
            rest,
            at,
            flags(base),
            settings.size,
            settings,
        );
    }
    para
}

/// How a byte range of a formatted snippet is drawn (Note formatting, #186).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SnippetStyle {
    pub bold: bool,
    pub italic: bool,
    /// Its own colour role (a link), else the snippet's.
    pub role: Option<ColorRole>,
}

/// A one-off paragraph of text in `base` at `scale` times the text size, with the byte ranges
/// in `styled` bold, italic or in their own colour role (a Note's formatting, its links and
/// Bible references). Ranges may overlap, such as a reference in bold text: each byte takes
/// every style over it. Byte positions map back to `text`, as in
/// [`styled_snippet_paragraph`].
pub fn formatted_snippet_paragraph(
    text: &str,
    base: ColorRole,
    styled: &[(Range<usize>, SnippetStyle)],
    scale: f32,
    settings: &TextSettings,
) -> Paragraph {
    let mut para = snippet_paragraph("", base, false, settings);
    let mut cuts: Vec<usize> = vec![0, text.len()];
    for (r, _) in styled {
        cuts.extend([r.start.min(text.len()), r.end.min(text.len())]);
    }
    cuts.retain(|&c| text.is_char_boundary(c));
    cuts.sort_unstable();
    cuts.dedup();
    for pair in cuts.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let mut flags = Flags {
            role: Some(base),
            ..Flags::default()
        };
        for (_, style) in styled.iter().filter(|(r, _)| r.start <= a && b <= r.end) {
            flags.bold |= style.bold;
            flags.italic |= style.italic;
            if style.role.is_some() {
                flags.role = style.role;
            }
        }
        push_text(
            &mut para.runs,
            &text[a..b],
            a,
            flags,
            settings.size * scale,
            settings,
        );
    }
    para
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
        interlinear: None,
    }
}
