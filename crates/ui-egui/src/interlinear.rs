//! Interlinear lines in a Bible Tab (#143): what the lines under each Tagged word read, from
//! the word's own tagging and the library's Lexicons, ready for the layout workers.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};

use obs_engine::word_info::lexicon_excerpt;
use obs_engine::{InterlinearKind, InterlinearLine, LexiconWord, cell_gloss, cell_texts};
use obs_layout::{
    BlockKind, BlockSource, Interlinear, Paragraph, TextSettings, add_interlinear, block_paragraph,
    tagged_words,
};
use obs_model::{Key, MorphemePart};
use obs_scripture::StrongsNumber;

use crate::doc::ReadingDoc;
use crate::selection::DocPos;

/// The lines a Bible Tab shows and where their words are looked up. Shared by the Tab's view
/// and its layout workers.
pub struct InterlinearSource {
    pub kind: InterlinearKind,
    /// The lines shown, top to bottom (never empty).
    pub lines: Vec<InterlinearLine>,
    /// The library's Lexicons, in Preferred order.
    lexicons: Vec<Arc<ReadingDoc>>,
    /// Lexicon answers by number, filled as words are laid out.
    looked_up: Mutex<HashMap<StrongsNumber, Option<LexiconWord>>>,
    key: u64,
}

impl std::fmt::Debug for InterlinearSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InterlinearSource")
            .field("kind", &self.kind)
            .field("lines", &self.lines)
            .field("lexicons", &self.lexicons.len())
            .finish()
    }
}

impl InterlinearSource {
    /// `lines` (top to bottom) of a Bible of `kind`, looked up in `lexicons` (Preferred
    /// order). `None` without lines.
    pub fn new(
        kind: InterlinearKind,
        lines: Vec<InterlinearLine>,
        lexicons: Vec<Arc<ReadingDoc>>,
    ) -> Option<Self> {
        if lines.is_empty() {
            return None;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (kind as u8, &lines).hash(&mut h);
        for l in &lexicons {
            (&l.manifest.resource_id, l.manifest.revision).hash(&mut h);
        }
        Some(Self {
            key: h.finish(),
            kind,
            lines,
            lexicons,
            looked_up: Mutex::new(HashMap::new()),
        })
    }

    /// Changes whenever the lines, or what they are looked up in, change.
    pub fn key(&self) -> u64 {
        self.key
    }

    /// Whether these are `lines` of a `kind` Bible looked up in the Lexicons `lexicons`
    /// (resource ids, Preferred order).
    pub fn is_for(
        &self,
        kind: InterlinearKind,
        lines: &[InterlinearLine],
        lexicons: &[String],
    ) -> bool {
        self.kind == kind
            && self.lines == lines
            && self.lexicons.len() == lexicons.len()
            && self
                .lexicons
                .iter()
                .zip(lexicons)
                .all(|(d, id)| d.manifest.resource_id == *id)
    }

    /// Whether `lines` of a `kind` Bible need the Lexicons: an English Bible's words carry
    /// only their Strong's numbers, so their Lemma (and its transliteration) and Gloss are
    /// looked up.
    pub fn needs_lexicons(kind: InterlinearKind, lines: &[InterlinearLine]) -> bool {
        kind == InterlinearKind::Reverse
            && lines.iter().any(|l| {
                matches!(
                    l,
                    InterlinearLine::Lemma
                        | InterlinearLine::Transliteration
                        | InterlinearLine::Gloss
                )
            })
    }

    /// The text of each line under a word.
    pub fn cells(&self, written: &str, parts: &[MorphemePart]) -> Vec<String> {
        cell_texts(&self.lines, self.kind, written, parts, &|n| {
            self.lexicon_word(n)
        })
    }

    /// The Lemma (the first Lexicon headword) and Gloss the Lexicons give a number.
    fn lexicon_word(&self, number: StrongsNumber) -> Option<LexiconWord> {
        if let Some(found) = self
            .looked_up
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&number)
        {
            return found.clone();
        }
        let mut lemma = None;
        let mut excerpts = Vec::new();
        for doc in &self.lexicons {
            let Some(entry) = doc.navigation.lookup(&Key::StrongsNumber(number)) else {
                continue;
            };
            let Some((unit, start)) = doc.entry_position(entry) else {
                continue;
            };
            let Some(unit) = doc.units.get(unit) else {
                continue;
            };
            if lemma.is_none() {
                lemma = unit.title.clone().filter(|t| !t.trim().is_empty());
            }
            let end = entry.range.map_or(unit.text.len(), |(_, e)| e as usize);
            if let Some(excerpt) = unit.text.get(start..end).and_then(lexicon_excerpt) {
                excerpts.push(excerpt);
            }
        }
        let gloss = cell_gloss(excerpts.iter().map(String::as_str));
        let found = (lemma.is_some() || gloss.is_some()).then_some(LexiconWord { lemma, gloss });
        self.looked_up
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(number, found.clone());
        found
    }

    /// The Tagged words a document range `[a, b)` reaches into, as copied: one row each,
    /// the word as written then its lines, separated by tabs (blank lines are left out).
    pub fn copy_rows(&self, doc: &ReadingDoc, a: DocPos, b: DocPos) -> Vec<String> {
        let mut rows = Vec::new();
        if a >= b {
            return rows;
        }
        for u in a.unit..=b.unit.min(doc.units.len().saturating_sub(1)) {
            let unit = &doc.units[u];
            let start = if u == a.unit { a.byte } else { 0 };
            let end = if u == b.unit { b.byte } else { usize::MAX };
            let whole = BlockSource {
                kind: BlockKind::Plain,
                range: 0..unit.text.len(),
            };
            for (range, parts) in tagged_words(unit, &whole) {
                if range.end <= start || range.start >= end {
                    continue;
                }
                let written = unit.text.get(range).unwrap_or_default().trim();
                let mut row = written.to_string();
                for cell in self.cells(written, parts) {
                    if !cell.trim().is_empty() {
                        row.push('\t');
                        row.push_str(cell.trim());
                    }
                }
                rows.push(row);
            }
        }
        rows
    }

    /// Block `i` of `doc` as a paragraph with its interlinear lines, or (`estimate`) with
    /// room for them only, for a height estimate that looks nothing up.
    pub fn paragraph(
        &self,
        doc: &ReadingDoc,
        i: usize,
        settings: &TextSettings,
        estimate: bool,
    ) -> Option<Paragraph> {
        let b = doc.blocks.get(i)?;
        let unit = doc.units.get(b.unit)?;
        let mut p = block_paragraph(unit, &b.source, doc.direction(), settings);
        if estimate {
            // Headings have no Tagged words; any other block may.
            if !matches!(b.source.kind, BlockKind::Heading(_)) {
                p.interlinear = Some(Interlinear::rows_only(self.lines.len(), settings));
            }
        } else {
            add_interlinear(
                &mut p,
                unit,
                &b.source,
                settings,
                self.lines.len(),
                &mut |written, parts| self.cells(written, parts),
            );
        }
        Some(p)
    }
}
