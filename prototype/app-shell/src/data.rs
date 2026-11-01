//! PROTOTYPE: in-memory Resources loaded from the JSON extracts written by prep_data.py.
//! Shape loosely mirrors ADR 0003 (text units + standoff spans).

use crate::refs::VRef;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitKind {
    Chapter,
    P,
    Q,
    H2,
    H3,
}

#[derive(Clone, Debug)]
pub enum SpanKind {
    /// Verse span: Standard ref, label shown (native numbering), native ref for Bibles with their own numbering.
    Verse { std: VRef, label: String, native: VRef },
    Chapter,
    /// English word tagged with a Strong's number.
    Strong(String),
    /// Hebrew word, index into `Resource::words`.
    HebWord(usize),
    Added,
    DivineName,
    Note(String),
    Bold,
    Italic,
    /// Scripture reference inside a commentary (OSIS).
    Ref(String),
    /// Commentary section key (Passage) — start..end.
    Key(VRef, VRef),
}

#[derive(Clone, Debug)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: SpanKind,
}

#[derive(Clone, Debug)]
pub struct Unit {
    pub kind: UnitKind,
    pub text: String,
    pub spans: Vec<Span>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct HebWordInfo {
    pub heb: String,
    pub translit: String,
    pub gloss: String,
    pub strongs: String,
    pub lemma: String,
    pub lemma_strong: String,
    pub lemma_gloss: String,
    pub morph: String,
    pub morph_en: String,
    #[serde(rename = "ref")]
    pub reference: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResKind {
    Bible,
    Commentary,
}

pub struct Resource {
    pub id: String,
    pub abbrev: String,
    pub title: String,
    pub kind: ResKind,
    pub rtl: bool,
    pub attribution: String,
    pub units: Vec<Unit>,
    pub words: Vec<HebWordInfo>,
    /// sorted by (unit, byte)
    pub verse_pos: Vec<(usize, usize, VRef)>,
    pub std_index: HashMap<VRef, (usize, usize)>,
    pub native_to_std: HashMap<VRef, VRef>,
    pub std_to_native: HashMap<VRef, VRef>,
    /// commentary sections: (start, end, heading unit)
    pub keys: Vec<(VRef, VRef, usize)>,
}

#[derive(Deserialize)]
struct RawRes {
    id: String,
    abbrev: String,
    title: String,
    #[serde(rename = "type")]
    kind: String,
    dir: String,
    attribution: String,
    units: Vec<RawUnit>,
    #[serde(default)]
    words: Vec<HebWordInfo>,
}

#[derive(Deserialize)]
struct RawUnit {
    kind: String,
    text: String,
    spans: Vec<(usize, usize, String, Value)>,
}

impl Resource {
    pub fn load(path: &Path) -> Resource {
        let raw: RawRes = serde_json::from_slice(&std::fs::read(path).unwrap_or_else(|e| {
            panic!("missing {}: {e}. Run `python3 prep_data.py` first.", path.display())
        }))
        .expect("bad json");
        let mut units = Vec::with_capacity(raw.units.len());
        for u in raw.units {
            let kind = match u.kind.as_str() {
                "chapter" => UnitKind::Chapter,
                "q" => UnitKind::Q,
                "h2" => UnitKind::H2,
                "h3" => UnitKind::H3,
                _ => UnitKind::P,
            };
            let mut spans = Vec::new();
            for (s, e, k, d) in u.spans {
                let gs = |key: &str| d.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string();
                let kind = match k.as_str() {
                    "verse" => {
                        let std = VRef::from_osis(&gs("ref")).unwrap();
                        let native = d.get("native").and_then(|x| x.as_str()).and_then(VRef::from_osis).unwrap_or(std);
                        SpanKind::Verse { std, label: gs("label"), native }
                    }
                    "chapter" => SpanKind::Chapter,
                    "w" => match d.get("i").and_then(|x| x.as_u64()) {
                        Some(i) => SpanKind::HebWord(i as usize),
                        None => SpanKind::Strong(gs("s")),
                    },
                    "add" => SpanKind::Added,
                    "nd" => SpanKind::DivineName,
                    "note" => SpanKind::Note(gs("text")),
                    "b" => SpanKind::Bold,
                    "i" => SpanKind::Italic,
                    "ref" => SpanKind::Ref(gs("ref")),
                    "key" => {
                        let a = VRef::from_osis(&gs("ref")).unwrap();
                        let b = VRef::from_osis(&gs("end")).unwrap_or(a);
                        SpanKind::Key(a, b)
                    }
                    _ => continue,
                };
                spans.push(Span { start: s, end: e, kind });
            }
            spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
            units.push(Unit { kind, text: u.text, spans });
        }
        let mut verse_pos = Vec::new();
        let mut std_index = HashMap::new();
        let mut native_to_std = HashMap::new();
        let mut std_to_native = HashMap::new();
        let mut keys = Vec::new();
        for (ui, u) in units.iter().enumerate() {
            for s in &u.spans {
                match &s.kind {
                    SpanKind::Verse { std, native, .. } => {
                        // a verse may be split over paragraphs: first occurrence wins
                        if !std_index.contains_key(std) {
                            verse_pos.push((ui, s.start, *std));
                            std_index.insert(*std, (ui, s.start));
                            native_to_std.insert(*native, *std);
                            std_to_native.insert(*std, *native);
                        }
                    }
                    SpanKind::Key(a, b) => keys.push((*a, *b, ui)),
                    _ => {}
                }
            }
        }
        verse_pos.sort();
        Resource {
            id: raw.id,
            abbrev: raw.abbrev,
            title: raw.title,
            kind: if raw.kind == "bible" { ResKind::Bible } else { ResKind::Commentary },
            rtl: raw.dir == "rtl",
            attribution: raw.attribution,
            units,
            words: raw.words,
            verse_pos,
            std_index,
            native_to_std,
            std_to_native,
            keys,
        }
    }

    /// Verse containing (unit, byte): last verse start <= pos.
    pub fn verse_at(&self, unit: usize, byte: usize) -> Option<VRef> {
        if self.units.get(unit).map(|u| u.kind) == Some(UnitKind::Chapter) {
            return self.verse_pos.iter().find(|(u, _, _)| *u > unit).map(|x| x.2);
        }
        let i = self.verse_pos.partition_point(|(u, b, _)| (*u, *b) <= (unit, byte));
        if i == 0 { self.verse_pos.first().map(|x| x.2) } else { Some(self.verse_pos[i - 1].2) }
    }

    /// Commentary section key at a unit.
    pub fn key_at(&self, unit: usize) -> Option<VRef> {
        let i = self.keys.partition_point(|(_, _, u)| *u <= unit);
        if i == 0 { self.keys.first().map(|k| k.0) } else { Some(self.keys[i - 1].0) }
    }

    /// Where to scroll for a Standard-versification location.
    pub fn locate(&self, r: VRef) -> Option<(usize, usize)> {
        match self.kind {
            ResKind::Bible => self.std_index.get(&r).copied().or_else(|| {
                // nearest following verse (e.g. absent verse)
                let i = self.verse_pos.partition_point(|(_, _, v)| *v < r);
                self.verse_pos.get(i).map(|x| (x.0, x.1))
            }),
            ResKind::Commentary => {
                let i = self.keys.partition_point(|(a, _, _)| *a <= r);
                if i == 0 { None } else { Some((self.keys[i - 1].2, 0)) }
            }
        }
    }

    /// Range of (unit, byte) covering a verse.
    pub fn verse_range(&self, r: VRef) -> Option<((usize, usize), (usize, usize))> {
        let i = self.verse_pos.iter().position(|x| x.2 == r)?;
        let a = (self.verse_pos[i].0, self.verse_pos[i].1);
        // end = end of the last span with this std ref
        let mut end = a;
        for (ui, u) in self.units.iter().enumerate().skip(a.0).take(4) {
            for s in &u.spans {
                if let SpanKind::Verse { std, .. } = s.kind {
                    if std == r {
                        end = (ui, s.end);
                    }
                }
            }
        }
        Some((a, end))
    }
}

pub struct XRefs {
    pub refs: HashMap<VRef, Vec<(String, u32)>>,
    pub kjv_text: HashMap<String, String>,
    pub attribution: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LexEntry {
    pub strong: String,
    pub lemma: String,
    pub gloss: String,
    pub count: u32,
}

pub struct Library {
    pub kjv: Resource,
    pub tahot: Resource,
    pub jfb: Resource,
    pub mhcc: Resource,
    pub xrefs: XRefs,
    pub lexicon: BTreeMap<String, LexEntry>,
}

impl Library {
    pub fn load(dir: &Path) -> Library {
        let xr: Value = serde_json::from_slice(&std::fs::read(dir.join("xref.json")).expect("xref.json")).unwrap();
        let mut refs = HashMap::new();
        for (k, v) in xr["refs"].as_object().unwrap() {
            let list = v
                .as_array()
                .unwrap()
                .iter()
                .map(|x| (x[0].as_str().unwrap().to_string(), x[1].as_u64().unwrap() as u32))
                .collect();
            if let Some(r) = VRef::from_osis(k) {
                refs.insert(r, list);
            }
        }
        let kjv_text = xr["kjv_text"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
            .collect();
        let lexicon = serde_json::from_slice(&std::fs::read(dir.join("lexicon.json")).expect("lexicon.json")).unwrap();
        Library {
            kjv: Resource::load(&dir.join("kjv.json")),
            tahot: Resource::load(&dir.join("tahot.json")),
            jfb: Resource::load(&dir.join("jfb.json")),
            mhcc: Resource::load(&dir.join("mhcc.json")),
            xrefs: XRefs { refs, kjv_text, attribution: xr["attribution"].as_str().unwrap_or("").into() },
            lexicon,
        }
    }

    pub fn get(&self, id: ResId) -> &Resource {
        match id {
            ResId::Kjv => &self.kjv,
            ResId::Tahot => &self.tahot,
            ResId::Jfb => &self.jfb,
            ResId::Mhcc => &self.mhcc,
        }
    }

    /// Plain KJV text for a verse in Genesis or a cross-ref target.
    pub fn kjv_plain(&self, osis: &str) -> Option<String> {
        let first = osis.split('-').next()?;
        if let Some(t) = self.xrefs.kjv_text.get(first) {
            return Some(t.clone());
        }
        let r = VRef::from_osis(first)?;
        let ((ua, ba), (ub, bb)) = self.kjv.verse_range(r)?;
        Some(slice_units(&self.kjv, (ua, ba), (ub, bb)))
    }
}

pub fn slice_units(res: &Resource, a: (usize, usize), b: (usize, usize)) -> String {
    let mut out = String::new();
    for ui in a.0..=b.0 {
        let t = &res.units[ui].text;
        let s = if ui == a.0 { a.1 } else { 0 };
        let e = if ui == b.0 { b.1 } else { t.len() };
        if s <= e && e <= t.len() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&t[s..e]);
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResId {
    Kjv,
    Tahot,
    Jfb,
    Mhcc,
}
