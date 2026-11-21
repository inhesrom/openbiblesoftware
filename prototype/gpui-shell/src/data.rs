//! PROTOTYPE — throwaway. A Bible read from its `.obr` (read-only) into display paragraphs:
//! verse numbers inserted as text, style runs flattened, verses indexed for linking.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use obs_layout::{BlockKind, unit_blocks};
use obs_model::{Direction, ParaStyle, SpanKind, UnitKind};
use obs_scripture::{Verse, VerseMap};
use obs_store::{ObrReader, ResourceStore};

/// Style flags of one run of a paragraph's display text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    pub italic: bool,
    pub bold: bool,
    pub woj: bool,
    pub small_caps: bool,
    pub verse_number: bool,
    pub chapter_number: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Prose,
    Poetry(u8),
    Heading,
    PsalmTitle,
}

#[derive(Debug)]
pub struct Para {
    pub text: gpui::SharedString,
    pub kind: Kind,
    /// Runs covering `text` exactly, in order: (byte length, flags).
    pub runs: Vec<(usize, Flags)>,
    /// Verses starting in this paragraph: (display byte offset of the verse's label, verse).
    pub verses: Vec<(usize, Verse)>,
}

#[derive(Debug)]
pub struct Bible {
    #[allow(dead_code)]
    pub id: String,
    pub abbrev: String,
    pub title: String,
    pub rtl: bool,
    pub paras: Vec<Para>,
    /// Native verse → paragraph index and display byte offset.
    pub verse_at: BTreeMap<Verse, (usize, usize)>,
    pub verse_map: Option<VerseMap>,
    /// The units (with all their spans: Tagged words, notes, references) and Verses, kept as
    /// the egui app's `ReadingDoc` keeps them, so memory compares like for like. Unused here.
    #[allow(dead_code)]
    pub units: Vec<obs_model::TextUnit>,
    #[allow(dead_code)]
    pub verses: Vec<obs_model::BibleVerse>,
}

/// `$OBS_LIBRARY`, else `$XDG_DATA_HOME/OpenBible/library`, else `~/.local/share/OpenBible/library`.
pub fn library_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("OBS_LIBRARY") {
        return d.into();
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    data.join("OpenBible/library")
}

const SUPERSCRIPT: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

fn superscript(n: u16) -> String {
    n.to_string()
        .chars()
        .map(|c| SUPERSCRIPT[c.to_digit(10).unwrap_or(0) as usize])
        .collect()
}

fn span_flags(kind: &SpanKind) -> Option<Flags> {
    let mut f = Flags::default();
    match kind {
        SpanKind::Added | SpanKind::Italic | SpanKind::Emphasis | SpanKind::Selah => {
            f.italic = true
        }
        SpanKind::Bold => f.bold = true,
        SpanKind::Woj => f.woj = true,
        SpanKind::DivineName => f.small_caps = true,
        _ => return None,
    }
    Some(f)
}

fn merge(a: &mut Flags, b: Flags) {
    a.italic |= b.italic;
    a.bold |= b.bold;
    a.woj |= b.woj;
    a.small_caps |= b.small_caps;
    a.verse_number |= b.verse_number;
    a.chapter_number |= b.chapter_number;
}

pub fn load(path: &Path) -> Result<Bible, String> {
    let reader = ObrReader::open(path).map_err(|e| e.to_string())?;
    let m = reader.manifest().clone();
    let verses = reader.verses().map_err(|e| e.to_string())?;
    let verse_map = reader.verse_map().map_err(|e| e.to_string())?;
    let mut by_unit: BTreeMap<String, Vec<(usize, Verse)>> = BTreeMap::new();
    for v in &verses {
        by_unit
            .entry(v.unit.as_str().to_string())
            .or_default()
            .push((v.start as usize, v.verse));
    }
    let mut paras: Vec<Para> = Vec::new();
    let mut verse_at = BTreeMap::new();
    let mut units = Vec::new();
    for id in reader.unit_ids().map_err(|e| e.to_string())? {
        let Some(unit) = reader.unit(&id).map_err(|e| e.to_string())? else {
            continue;
        };
        if unit.kind == UnitKind::Note {
            continue;
        }
        let text = unit.text.as_str();
        // Trimmed, non-empty blocks.
        let blocks: Vec<(BlockKind, Range<usize>)> = unit_blocks(&unit)
            .into_iter()
            .filter(|b| !matches!(b.kind, BlockKind::Paragraph(ParaStyle::Blank)))
            .filter_map(|b| {
                let s = &text[b.range.clone()];
                let lead = s.len() - s.trim_start().len();
                let trail = s.len() - s.trim_end().len();
                let r = b.range.start + lead..b.range.end - trail;
                (r.start < r.end).then_some((b.kind, r))
            })
            .collect();
        let unit_verses = by_unit.get(id.as_str()).cloned().unwrap_or_default();
        // Each verse goes to the block its start falls in, else the next block.
        let mut per_block: Vec<Vec<(usize, Verse)>> = vec![Vec::new(); blocks.len()];
        for (start, v) in unit_verses {
            let ix = blocks.partition_point(|(_, r)| r.start <= start);
            let (bi, at) = match ix.checked_sub(1) {
                Some(i) if start < blocks[i].1.end => (i, start),
                _ if ix < blocks.len() => (ix, blocks[ix].1.start),
                Some(i) => (i, blocks[i].1.end),
                None => continue,
            };
            per_block[bi].push((at, v));
        }
        let style_spans: Vec<(Range<usize>, Flags)> = unit
            .spans
            .iter()
            .filter_map(|s| Some((s.start as usize..s.end as usize, span_flags(&s.kind)?)))
            .collect();
        for ((kind, range), vs) in blocks.into_iter().zip(per_block) {
            let kind = match kind {
                BlockKind::Paragraph(ParaStyle::Poetry1) => Kind::Poetry(1),
                BlockKind::Paragraph(ParaStyle::Poetry2) => Kind::Poetry(2),
                BlockKind::Paragraph(ParaStyle::Poetry3) => Kind::Poetry(3),
                BlockKind::Paragraph(_) | BlockKind::Plain => Kind::Prose,
                BlockKind::Heading(_) => Kind::Heading,
                BlockKind::PsalmTitle => Kind::PsalmTitle,
            };
            // Labels inserted before unit offsets: (unit offset, label, flags).
            let labels: Vec<(usize, String, Flags, Verse)> = vs
                .into_iter()
                .map(|(at, v)| {
                    if v.verse <= 1 {
                        let f = Flags {
                            chapter_number: true,
                            ..Flags::default()
                        };
                        (at, format!("{} ", v.chapter), f, v)
                    } else {
                        let f = Flags {
                            verse_number: true,
                            ..Flags::default()
                        };
                        (at, superscript(v.verse), f, v)
                    }
                })
                .collect();
            // Display text and boundaries.
            let mut out = String::new();
            let mut segs: Vec<(Range<usize>, Flags)> = Vec::new(); // display ranges with fixed flags
            let mut para_verses = Vec::new();
            let mut pos = range.start;
            // map unit offset → display offset for text pieces
            let mut pieces: Vec<(usize, usize)> = Vec::new(); // (unit start, display start)
            for (at, label, f, v) in &labels {
                let at = (*at).clamp(pos, range.end);
                if at > pos {
                    pieces.push((pos, out.len()));
                    out.push_str(&text[pos..at]);
                    pos = at;
                }
                para_verses.push((out.len(), *v));
                let s = out.len();
                out.push_str(label);
                segs.push((s..out.len(), *f));
            }
            if range.end > pos {
                pieces.push((pos, out.len()));
                out.push_str(&text[pos..range.end]);
            }
            // Text pieces' runs from style spans.
            let mut runs_raw: Vec<(Range<usize>, Flags)> = segs;
            for (i, &(ustart, dstart)) in pieces.iter().enumerate() {
                let uend = pieces
                    .get(i + 1)
                    .map(|&(u, _)| u)
                    .unwrap_or(range.end)
                    .min(range.end);
                // boundaries within [ustart, uend)
                let mut cuts = vec![ustart, uend];
                for (r, _) in &style_spans {
                    for c in [r.start, r.end] {
                        if c > ustart && c < uend && text.is_char_boundary(c) {
                            cuts.push(c);
                        }
                    }
                }
                cuts.sort_unstable();
                cuts.dedup();
                for w in cuts.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let mut f = Flags::default();
                    for (r, sf) in &style_spans {
                        if r.start <= a && r.end >= b && r.start < r.end {
                            merge(&mut f, *sf);
                        }
                    }
                    runs_raw.push((dstart + (a - ustart)..dstart + (b - ustart), f));
                }
            }
            runs_raw.sort_by_key(|(r, _)| r.start);
            let mut runs: Vec<(usize, Flags)> = Vec::new();
            for (r, f) in runs_raw {
                if r.is_empty() {
                    continue;
                }
                match runs.last_mut() {
                    Some((len, lf)) if *lf == f => *len += r.len(),
                    _ => runs.push((r.len(), f)),
                }
            }
            let mut out = out.replace(['\n', '\r', '\t'], " ");
            // Small capitals (the divine name): "LORD" is drawn as "Lord" with `smcp`.
            if runs.iter().any(|(_, f)| f.small_caps) {
                let mut s = String::with_capacity(out.len());
                let mut at = 0;
                for (len, f) in &runs {
                    let piece = &out[at..at + len];
                    if f.small_caps {
                        let mut first = true;
                        for c in piece.chars() {
                            if c.is_whitespace() {
                                first = true;
                                s.push(c);
                            } else if first {
                                first = false;
                                s.push(c);
                            } else {
                                s.push(c.to_ascii_lowercase());
                            }
                        }
                    } else {
                        s.push_str(piece);
                    }
                    at += len;
                }
                out = s;
            }
            debug_assert_eq!(runs.iter().map(|r| r.0).sum::<usize>(), out.len());
            let pi = paras.len();
            for &(off, v) in &para_verses {
                verse_at.entry(v).or_insert((pi, off));
            }
            paras.push(Para {
                text: out.into(),
                kind,
                runs,
                verses: para_verses,
            });
        }
        units.push(unit);
    }
    Ok(Bible {
        id: m.resource_id.clone(),
        abbrev: m.abbreviation.clone(),
        title: m.title.clone(),
        rtl: m.direction == Direction::Rtl,
        paras,
        verse_at,
        verse_map,
        units,
        verses,
    })
}

impl Bible {
    /// The Standard verse a native verse maps to.
    pub fn to_standard(&self, v: Verse) -> Verse {
        match &self.verse_map {
            Some(m) => m.to_standard(v).map(|p| p.start()).unwrap_or(v),
            None => v,
        }
    }

    /// Where a Standard verse shows in this Bible: paragraph and display byte offset.
    pub fn locate_standard(&self, s: Verse) -> Option<(usize, usize)> {
        let native = match &self.verse_map {
            Some(m) => m.first_native(s).unwrap_or(s),
            None => s,
        };
        if let Some(&at) = self.verse_at.get(&native) {
            return Some(at);
        }
        // The verse before it in this Bible, if in the same book (verse ranges, gaps).
        self.verse_at
            .range(..=native)
            .next_back()
            .filter(|(v, _)| v.book == native.book && v.chapter == native.chapter)
            .map(|(_, &at)| at)
    }

    /// The verse at paragraph `pi`, display byte `off` (the last verse starting at or before it).
    pub fn verse_at_offset(&self, pi: usize, off: usize) -> Option<Verse> {
        let mut p = pi.min(self.paras.len().saturating_sub(1));
        if let Some((_, v)) = self
            .paras
            .get(p)?
            .verses
            .iter()
            .rev()
            .find(|(o, _)| *o <= off)
        {
            return Some(*v);
        }
        while p > 0 {
            p -= 1;
            if let Some((_, v)) = self.paras[p].verses.last() {
                return Some(*v);
            }
        }
        self.paras
            .iter()
            .find_map(|p| p.verses.first().map(|(_, v)| *v))
    }
}
