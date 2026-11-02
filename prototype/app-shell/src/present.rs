//! PROTOTYPE: text unit + standoff spans -> styled runs for the layout (ADR 0004 §6).

use crate::data::{Resource, SpanKind, UnitKind};
use crate::text::{FontId, Para, Run, Src, Style};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSettings {
    pub size: f32,
    pub hebrew_scale: f32,
    pub line_spacing: f32,
    pub red_letter: bool,
    pub verse_numbers: bool,
}

impl Default for TextSettings {
    fn default() -> Self {
        Self { size: 17.0, hebrew_scale: 1.3, line_spacing: 1.25, red_letter: true, verse_numbers: true }
    }
}

impl TextSettings {
    pub fn key(&self) -> u64 {
        (self.size * 10.0) as u64 * 1_000_000
            + (self.hebrew_scale * 100.0) as u64 * 1000
            + (self.line_spacing * 100.0) as u64
            + if self.verse_numbers { 7 } else { 0 }
    }
}

pub const INK: [u8; 4] = [32, 38, 41, 255];
pub const ACCENT: [u8; 4] = [45, 78, 134, 255];
pub const VERSE: [u8; 4] = [128, 94, 32, 255];
pub const LINK: [u8; 4] = [38, 91, 145, 255];
pub const NOTE: [u8; 4] = [45, 116, 105, 255];

/// Synthetic object in a laid block.
#[derive(Clone, Debug)]
pub enum Obj {
    Verse(crate::refs::VRef),
    Note(String),
}

pub fn is_hebrew(c: char) -> bool {
    matches!(c as u32, 0x0591..=0x05F4 | 0xFB1D..=0xFB4F)
}

/// Split text into runs by script so Hebrew gets the Hebrew face (font fallback).
fn push_text(runs: &mut Vec<Run>, text: &str, unit_off: usize, style: Style, settings: &TextSettings) {
    let mut start = 0;
    let mut cur_heb: Option<bool> = None;
    let flush = |runs: &mut Vec<Run>, s: usize, e: usize, heb: bool| {
        if e > s {
            let mut st = style;
            if heb {
                st.font = FontId::Hebrew;
                st.size = style.size * settings.hebrew_scale;
                st.smcp = false;
            }
            runs.push(Run { text: text[s..e].to_string(), style: st, src: Src::Text(unit_off + s) });
        }
    };
    for (i, c) in text.char_indices() {
        let heb = if is_hebrew(c) {
            Some(true)
        } else if c.is_alphanumeric() {
            Some(false)
        } else {
            None // neutral: stays with the current run
        };
        if let (Some(h), Some(ch)) = (heb, cur_heb) {
            if h != ch {
                flush(runs, start, i, ch);
                start = i;
            }
        }
        if heb.is_some() {
            cur_heb = heb;
        }
    }
    flush(runs, start, text.len(), cur_heb.unwrap_or(false));
}

pub fn unit_para(res: &Resource, ui: usize, s: &TextSettings, objs: &mut Vec<Obj>) -> Para {
    let u = &res.units[ui];
    let size = s.size;
    let base = Style { font: FontId::Regular, size, color: INK, smcp: false, raise: 0.0 };
    let (base, space_before, space_after, center, indent) = match u.kind {
        UnitKind::Chapter => (
            Style { font: FontId::Bold, size: size * 1.6, color: ACCENT, ..base },
            size * 1.2,
            size * 0.5,
            false,
            0.0,
        ),
        UnitKind::H2 => (Style { font: FontId::Bold, size: size * 1.12, color: ACCENT, ..base }, size * 1.0, size * 0.2, false, 0.0),
        UnitKind::H3 => (Style { font: FontId::Bold, size, ..base }, size * 0.6, size * 0.2, false, 0.0),
        UnitKind::Q => (base, 0.0, size * 0.2, false, size * 1.5),
        UnitKind::P => (base, 0.0, size * 0.55, false, 0.0),
    };
    let mut runs: Vec<Run> = Vec::new();
    // boundaries
    let mut cuts: Vec<usize> = vec![0, u.text.len()];
    for sp in &u.spans {
        cuts.push(sp.start);
        cuts.push(sp.end);
    }
    cuts.sort();
    cuts.dedup();
    for w in cuts.windows(2).map(|w| (w[0], w[1])).chain(std::iter::once((u.text.len(), u.text.len()))) {
        let (a, b) = w;
        // objects anchored at a: note markers first (they end the previous text), then verse numbers
        for sp in u.spans.iter().filter(|sp| sp.start == a && sp.end == a) {
            if let SpanKind::Note(t) = &sp.kind {
                let id = objs.len() as u32;
                objs.push(Obj::Note(t.clone()));
                runs.push(Run {
                    text: "†".into(),
                    style: Style { font: FontId::Regular, size: size * 0.7, color: NOTE, smcp: false, raise: size * 0.35 },
                    src: Src::Object { anchor: a, id },
                });
            }
        }
        for sp in u.spans.iter().filter(|sp| sp.start == a && sp.end > a) {
            match &sp.kind {
                SpanKind::Verse { std, label, .. } if s.verse_numbers && res.std_index.get(std) == Some(&(ui, a)) => {
                    let id = objs.len() as u32;
                    objs.push(Obj::Verse(*std));
                    let lead = "";
                    runs.push(Run {
                        text: format!("{lead}{label}\u{202F}"),
                        style: Style { font: FontId::Bold, size: size * 0.62, color: VERSE, smcp: false, raise: size * 0.38 },
                        src: Src::Object { anchor: a, id },
                    });
                }
                _ => {}
            }
        }
        if b <= a {
            continue;
        }
        let mut st = base;
        for sp in &u.spans {
            if sp.start <= a && sp.end >= b && sp.end > sp.start {
                match &sp.kind {
                    SpanKind::Added => st.font = FontId::Italic,
                    SpanKind::Italic => st.font = FontId::Italic,
                    SpanKind::Bold => st.font = FontId::Bold,
                    SpanKind::DivineName => st.smcp = true,
                    SpanKind::Ref(_) => st.color = LINK,
                    _ => {}
                }
            }
        }
        if st.smcp {
            // divine name: "LORD" -> L + small-capped "ord" (ASCII lowercasing keeps byte offsets)
            let t = &u.text[a..b];
            let mut out = String::with_capacity(t.len());
            for (i, c) in t.char_indices() {
                let first = i == 0 || !t[..i].chars().last().map(|p| p.is_alphabetic()).unwrap_or(false);
                out.push(if first { c } else { c.to_ascii_lowercase() });
            }
            push_text(&mut runs, &out, a, st, s);
        } else {
            push_text(&mut runs, &u.text[a..b], a, st, s);
        }
    }
    if runs.is_empty() {
        runs.push(Run { text: " ".into(), style: base, src: Src::Text(0) });
    }
    Para { runs, rtl: res.rtl, line_spacing: s.line_spacing, indent, space_before, space_after, center }
}

/// Plain text snippet (inline-snippet mode), e.g. a cross-ref preview or a Hebrew lemma.
pub fn snippet_para(text: &str, size: f32, color: [u8; 4], rtl: bool, s: &TextSettings) -> Para {
    let mut runs = Vec::new();
    push_text(&mut runs, text, 0, Style { font: FontId::Regular, size, color, smcp: false, raise: 0.0 }, s);
    if runs.is_empty() {
        runs.push(Run { text: " ".into(), style: Style { font: FontId::Regular, size, color, smcp: false, raise: 0.0 }, src: Src::Text(0) });
    }
    Para { runs, rtl, line_spacing: 1.15, indent: 0.0, space_before: 0.0, space_after: 0.0, center: false }
}
