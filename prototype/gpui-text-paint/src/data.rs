//! PROTOTYPE — throwaway (#372). A Bible read from its `.obr` (read-only) the way the egui
//! app's `ReadingDoc` reads it: its reading units, their blocks (`obs_layout::unit_blocks`),
//! its Verses and Verse map. Paragraphs are built with `obs_layout::block_paragraph`, as the
//! reading pane builds them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use obs_layout::{BlockSource, Paragraph, TextSettings, block_paragraph, unit_blocks};
use obs_model::{BibleVerse, Direction, TextUnit, UnitKind};
use obs_scripture::{Verse, VerseMap};
use obs_store::{ObrReader, ResourceStore};

/// One block of the document: which unit, and which part of it.
#[derive(Debug, Clone)]
pub struct DocBlock {
    pub unit: usize,
    pub source: BlockSource,
}

#[derive(Debug)]
pub struct Bible {
    pub abbrev: String,
    pub title: String,
    pub direction: Direction,
    pub units: Vec<TextUnit>,
    pub blocks: Vec<DocBlock>,
    /// Native verse → (unit index, unit byte where it starts).
    pub verse_at: BTreeMap<Verse, (usize, usize)>,
    /// Per unit: (start byte, verse), sorted, for the verse at a unit byte.
    pub unit_verses: Vec<Vec<(usize, Verse)>>,
    pub verse_map: Option<VerseMap>,
}

/// `$OBS_LIBRARY`, else `$XDG_DATA_HOME/OpenBible/library`, else `~/.local/share/OpenBible/library`.
pub fn library_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("OBS_LIBRARY") {
        return d.into();
    }
    let data = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
    });
    data.join("OpenBible/library")
}

pub fn load(path: &Path) -> Result<Bible, String> {
    let reader = ObrReader::open(path).map_err(|e| e.to_string())?;
    let m = reader.manifest().clone();
    let mut units = Vec::new();
    for id in reader.unit_ids().map_err(|e| e.to_string())? {
        if let Some(u) = reader.unit(&id).map_err(|e| e.to_string())?
            && u.kind != UnitKind::Note
        {
            units.push(u);
        }
    }
    let mut blocks = Vec::new();
    for (i, u) in units.iter().enumerate() {
        for source in unit_blocks(u) {
            blocks.push(DocBlock { unit: i, source });
        }
    }
    let index: std::collections::HashMap<&str, usize> =
        units.iter().enumerate().map(|(i, u)| (u.id.as_str(), i)).collect();
    let verses: Vec<BibleVerse> = reader.verses().map_err(|e| e.to_string())?;
    let mut verse_at = BTreeMap::new();
    let mut unit_verses = vec![Vec::new(); units.len()];
    for v in &verses {
        if let Some(&u) = index.get(v.unit.as_str()) {
            verse_at.insert(v.verse, (u, v.start as usize));
            unit_verses[u].push((v.start as usize, v.verse));
        }
    }
    for list in &mut unit_verses {
        list.sort_by_key(|(s, _)| *s);
    }
    Ok(Bible {
        abbrev: m.abbreviation.clone(),
        title: m.title.clone(),
        direction: m.direction,
        units,
        blocks,
        verse_at,
        unit_verses,
        verse_map: reader.verse_map().map_err(|e| e.to_string())?,
    })
}

impl Bible {
    #[allow(dead_code)]
    pub fn rtl(&self) -> bool {
        self.direction == Direction::Rtl
    }

    /// Block `i` as a paragraph, as the reading pane builds it (no interlinear lines, no Notes).
    pub fn paragraph(&self, i: usize, settings: &TextSettings) -> Option<Paragraph> {
        let b = self.blocks.get(i)?;
        Some(block_paragraph(self.units.get(b.unit)?, &b.source, self.direction, settings))
    }

    /// The block holding unit byte `byte` of unit `unit` (or the next block of that unit).
    pub fn block_at(&self, unit: usize, byte: usize) -> Option<usize> {
        let first = self.blocks.partition_point(|b| b.unit < unit);
        let mut best = None;
        for (i, b) in self.blocks.iter().enumerate().skip(first) {
            if b.unit != unit {
                break;
            }
            if b.source.range.start <= byte {
                best = Some(i);
            }
            if b.source.range.contains(&byte) {
                return Some(i);
            }
            if b.source.range.start > byte {
                return best.or(Some(i));
            }
        }
        best
    }

    /// The Standard verse a native verse maps to.
    pub fn to_standard(&self, v: Verse) -> Verse {
        match &self.verse_map {
            Some(m) => m.to_standard(v).map(|p| p.start()).unwrap_or(v),
            None => v,
        }
    }

    /// Where a Standard verse starts in this Bible: (unit, byte). The verse before it in the
    /// same chapter if this Bible lacks it.
    pub fn locate_standard(&self, s: Verse) -> Option<(usize, usize)> {
        let native = match &self.verse_map {
            Some(m) => m.first_native(s).unwrap_or(s),
            None => s,
        };
        if let Some(&at) = self.verse_at.get(&native) {
            return Some(at);
        }
        self.verse_at
            .range(..=native)
            .next_back()
            .filter(|(v, _)| v.book == native.book && v.chapter == native.chapter)
            .map(|(_, &at)| at)
    }

    /// The verse at unit byte `byte` of unit `unit`: the last one starting at or before it.
    pub fn verse_at_unit(&self, unit: usize, byte: usize) -> Option<Verse> {
        let list = self.unit_verses.get(unit)?;
        let i = list.partition_point(|(s, _)| *s <= byte);
        list.get(i.checked_sub(1)?).map(|(_, v)| *v).or_else(|| list.first().map(|(_, v)| *v))
    }
}

